//! Fleet telemetry: where the tokens go, per pane, live.
//!
//! Every ledger record on the machine (orchestrators included) is read from
//! its harness's own transcript, incrementally, into an append-only event
//! store. One collector per state root does the reading; everything else reads
//! the files it writes. See `ai_docs/designs/2026-09-28-fleet-telemetry-design.md`.
//!
//! | module | job |
//! |---|---|
//! | [`readers`] | one incremental reader per harness |
//! | [`cursor`] | where each reader stopped, and file identity |
//! | [`store`] | the event files, the dedupe index, rollups |
//! | [`collect`] | one tick: ledgers -> readers -> store -> snapshot |
//! | [`lock`] | one collector per state root |

pub mod collect;
pub mod cursor;
pub mod lock;
pub mod readers;
pub mod store;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::quota::Window;
use crate::usage::Tokens;

/// Token counts, one field per class (TEL-03).
///
/// `input` is fresh input only. `reasoning` is already inside `output` for
/// every harness; it is reported for information and never priced.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenClasses {
    #[serde(default)]
    pub input: u64,
    #[serde(default)]
    pub cache_write_5m: u64,
    #[serde(default)]
    pub cache_write_1h: u64,
    #[serde(default)]
    pub cache_read: u64,
    #[serde(default)]
    pub output: u64,
    #[serde(default)]
    pub reasoning: u64,
}

impl TokenClasses {
    pub fn add(&mut self, o: &TokenClasses) {
        self.input += o.input;
        self.cache_write_5m += o.cache_write_5m;
        self.cache_write_1h += o.cache_write_1h;
        self.cache_read += o.cache_read;
        self.output += o.output;
        self.reasoning += o.reasoning;
    }

    /// Every billed token. `reasoning` is inside `output`, so not added again.
    pub fn total(&self) -> u64 {
        self.input + self.cache_write_5m + self.cache_write_1h + self.cache_read + self.output
    }

    /// Input the cache did not serve: fresh input plus cache writes.
    pub fn fresh(&self) -> u64 {
        self.input + self.cache_write_5m + self.cache_write_1h
    }

    /// `cache_read / (cache_read + fresh)`, or `None` with no input at all.
    pub fn cache_hit(&self) -> Option<f64> {
        let denom = self.cache_read + self.fresh();
        (denom > 0).then(|| self.cache_read as f64 / denom as f64)
    }

    /// The pricing shape. Reasoning is dropped: it is priced as output.
    pub fn priced(&self) -> Tokens {
        Tokens {
            input: self.input,
            cache_write_5m: self.cache_write_5m,
            cache_write_1h: self.cache_write_1h,
            cache_read: self.cache_read,
            output: self.output,
        }
    }

    pub fn is_zero(&self) -> bool {
        self.total() == 0 && self.reasoning == 0
    }

    /// Per-class `self - earlier`, saturating. For corrections.
    pub fn minus(&self, earlier: &TokenClasses) -> TokenClasses {
        TokenClasses {
            input: self.input.saturating_sub(earlier.input),
            cache_write_5m: self.cache_write_5m.saturating_sub(earlier.cache_write_5m),
            cache_write_1h: self.cache_write_1h.saturating_sub(earlier.cache_write_1h),
            cache_read: self.cache_read.saturating_sub(earlier.cache_read),
            output: self.output.saturating_sub(earlier.output),
            reasoning: self.reasoning.saturating_sub(earlier.reasoning),
        }
    }
}

/// One model response, as a reader found it: no ledger attributes yet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawUsage {
    /// When the harness wrote it (RFC 3339, UTC, seconds).
    pub ts: String,
    /// The dedupe key within the session: `message.id`, `response_id`, ...
    pub event_id: String,
    /// The model the transcript names. Empty when it names none.
    pub model: String,
    #[serde(default)]
    pub effort: Option<String>,
    pub tokens: TokenClasses,
    /// A Claude subagent's call, attributed to the parent record.
    #[serde(default)]
    pub subagent: bool,
    /// A correction: `tokens` is the difference from an earlier event.
    #[serde(default)]
    pub delta: bool,
    /// pi/Prime usage carried on a tool result message.
    #[serde(default)]
    pub tool_nested: bool,
    /// What the harness itself says the call cost (OpenCode). Shown, never summed.
    #[serde(default)]
    pub harness_cost: Option<f64>,
}

/// One stored event line (section 8.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub ts: String,
    pub project: Option<String>,
    pub record_id: String,
    pub session_id: String,
    pub event_id: String,
    pub role: String,
    pub teammate: String,
    pub via: Option<String>,
    pub kind: String,
    pub agent: String,
    pub model: String,
    pub effort: Option<String>,
    pub phase: Option<String>,
    pub plan: Option<String>,
    #[serde(default)]
    pub subagent: bool,
    #[serde(default)]
    pub delta: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub tool_nested: bool,
    /// The record was idle when this call happened (section 9.1).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub idle: bool,
    pub tokens: TokenClasses,
    /// List price; `None` when the price table does not know the model.
    pub cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness_cost: Option<f64>,
}

impl Event {
    /// The store's dedupe key (section 8.2).
    pub fn key(&self) -> (String, String, String) {
        (
            self.agent.clone(),
            self.session_id.clone(),
            self.event_id.clone(),
        )
    }
}

/// A limit signal found inside a transcript, handed to the quota module.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum QuotaSignal {
    /// The harness refused a request for a limit: a Claude 429 line, a Codex
    /// `usage_limit_exceeded`, an OpenCode `FreeUsageLimitError`. Carries no
    /// message text (TEL-11).
    Refusal {
        pool: String,
        at: String,
        what: String,
    },
    /// A limits snapshot a harness wrote on its own (Codex `rate_limits`).
    Snapshot {
        pool: String,
        at: String,
        windows: Vec<Window>,
        #[serde(default)]
        ordinary_usage_allowed: Option<bool>,
    },
}

/// What a reader hands back.
#[derive(Debug, Clone, PartialEq)]
pub enum Observation {
    Usage(RawUsage),
    Quota(QuotaSignal),
}

/// Why a record contributes no events (section 7.3). Never counted as 0.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unread {
    pub record_id: String,
    pub role: String,
    pub agent: String,
    pub reason: String,
}

/// The telemetry directory under a state root.
pub fn dir(state_root: &std::path::Path) -> PathBuf {
    state_root.join("telemetry")
}

/// The plan slug in a task (section 6.1): the first
/// `ai_docs/plans/<slug>.md` it names, without the extension.
pub fn plan_slug(task: &str) -> Option<String> {
    // The regex `ai_docs/plans/([A-Za-z0-9._-]+)\.md`, by hand: take the run
    // of name characters after the prefix, and cut it at its LAST `.md`,
    // which is where a greedy match backtracks to.
    const NEEDLE: &str = "ai_docs/plans/";
    let mut rest = task;
    while let Some(at) = rest.find(NEEDLE) {
        rest = &rest[at + NEEDLE.len()..];
        let run: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
            .collect();
        if let Some(cut) = run.rfind(".md").filter(|&i| i > 0) {
            return Some(run[..cut].to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tel_08_plan_slug_parsed() {
        for (task, slug) in [
            (
                "Implement ai_docs/plans/golden-prompts-whitespace.md step 3",
                Some("golden-prompts-whitespace"),
            ),
            (
                "read /Users/x/p/ai_docs/plans/v1.2_fix.md, then build",
                Some("v1.2_fix"),
            ),
            (
                "first ai_docs/plans/a.md then ai_docs/plans/b.md",
                Some("a"),
            ),
            ("ai_docs/plans/.md is not a plan", None),
            ("ai_docs/plans/notes.txt", None),
            ("see ai_docs/plans/", None),
            ("no plan at all", None),
            ("(idle - awaiting assignment)", None),
            ("ai_docs/plans/x.md.", Some("x")),
        ] {
            assert_eq!(plan_slug(task).as_deref(), slug, "{task}");
        }
    }

    #[test]
    fn tel_03_reasoning_is_informational_and_never_priced() {
        let t = TokenClasses {
            input: 10,
            cache_write_5m: 1,
            cache_write_1h: 2,
            cache_read: 100,
            output: 50,
            reasoning: 20,
        };
        assert_eq!(t.total(), 163, "reasoning is inside output");
        assert_eq!(t.fresh(), 13);
        assert_eq!(t.priced().output, 50);
        let hit = t.cache_hit().unwrap();
        assert!((hit - 100.0 / 113.0).abs() < 1e-12);
        assert_eq!(TokenClasses::default().cache_hit(), None);
    }
}
