pub mod collect;
pub mod context;
pub(crate) mod cursor;
pub mod lock;
pub mod readers;
pub mod start;
pub mod store;
pub mod timing;

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::routing::quota::Window;
use crate::usage::{self, Price, Tokens};

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

    pub fn total(&self) -> u64 {
        self.input + self.cache_write_5m + self.cache_write_1h + self.cache_read + self.output
    }

    pub fn fresh(&self) -> u64 {
        self.input + self.cache_write_5m + self.cache_write_1h
    }

    pub fn cache_hit(&self) -> Option<f64> {
        let denom = self.cache_read + self.fresh();
        (denom > 0).then(|| self.cache_read as f64 / denom as f64)
    }

    pub fn priced(&self) -> Tokens {
        Tokens {
            input: self.input,
            cache_write_5m: self.cache_write_5m,
            cache_write_1h: self.cache_write_1h,
            cache_read: self.cache_read,
            output: self.output,
        }
    }

    pub(crate) fn is_zero(&self) -> bool {
        self.total() == 0 && self.reasoning == 0
    }

    pub(crate) fn minus(&self, earlier: &TokenClasses) -> TokenClasses {
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawUsage {
    pub ts: String,
    pub event_id: String,
    pub model: String,
    #[serde(default)]
    pub effort: Option<String>,
    pub tokens: TokenClasses,
    #[serde(default)]
    pub subagent: bool,
    #[serde(default)]
    pub delta: bool,
    #[serde(default)]
    pub tool_nested: bool,
    #[serde(default)]
    pub harness_cost: Option<f64>,
}

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
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub idle: bool,
    pub tokens: TokenClasses,
    pub cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness_cost: Option<f64>,
}

impl Event {
    pub fn key(&self) -> (String, String, String) {
        (
            self.agent.clone(),
            self.session_id.clone(),
            self.event_id.clone(),
        )
    }

    pub fn price_if_unset(&mut self, prices: &BTreeMap<String, Price>) {
        if self.cost_usd.is_none() {
            self.cost_usd = usage::cost_of(prices, &self.model, &self.tokens.priced());
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum QuotaSignal {
    Refusal {
        pool: String,
        at: String,
        what: String,
    },
    Snapshot {
        pool: String,
        at: String,
        windows: Vec<Window>,
        #[serde(default)]
        ordinary_usage_allowed: Option<bool>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Observation {
    Usage(RawUsage),
    Quota(QuotaSignal),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unread {
    pub record_id: String,
    pub role: String,
    pub agent: String,
    pub reason: String,
}

pub fn dir(state_root: &std::path::Path) -> PathBuf {
    state_root.join("telemetry")
}

pub(crate) fn plan_slug(task: &str) -> Option<String> {
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
