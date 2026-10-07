//! Native auto-compact windows, triggers and watch thresholds (CTX-05,
//! CTX-07, CTX-08). Pure: no I/O.
//!
//! The harness compacts itself at its native trigger. horch watches a lower
//! threshold, `min(base, floor(0.8 x native))`, so the fleet has headroom to
//! write a handoff before the harness compacts on its own.

use std::collections::BTreeMap;

use crate::harness::capabilities::TriggerRule;

/// The watch base when nothing lowers it (CTX-07).
pub const BASE_THRESHOLD: u64 = 300_000;

/// The least headroom between threshold and native trigger that the roster
/// check accepts (CTX-08).
pub const HEADROOM_FLOOR: u64 = 20_000;

/// OpenCode's fixed trigger per model ([`TriggerRule::PerModel`]).
pub const OPENCODE_TRIGGERS: &[(&str, u64)] = &[
    ("opencode/big-pickle", 140_000),
    ("opencode/nemotron-3.5-lightning-free", 230_144),
    ("opencode/nemotron-3-ultra-free", 968_000),
];

/// Model context windows of the pi family (pi, Prime), by model id.
pub const MODEL_WINDOWS: &[(&str, u64)] = &[
    ("anthropic/claude-opus-5-5", 1_000_000),
    ("anthropic/claude-opus-5", 1_000_000),
    ("ollama/qwen3.8", 262_144),
];

/// Claude's window for a model with a `haiku` name segment.
const CLAUDE_SMALL_WINDOW: u64 = 200_000;
/// Claude's window for every other model.
const CLAUDE_WINDOW: u64 = 1_000_000;

/// Where a launch's window came from.
///
/// Not persisted (review finding 4): the ledger owns its own type,
/// `execution::legacy::LedgerWindow`. No serde derives here, so a rename for
/// display reasons cannot change what the ledger stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowSource {
    Operator,
    Fleet,
    Harness,
}

/// A value an operator file sets. `tokens: None` = set, but horch cannot
/// compute the trigger from it (for example `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE`,
/// or a value that is not a number).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorWindow {
    pub tokens: Option<u64>,
    pub detail: String,
}

/// What a launch decided about the harness's own auto-compact setting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowDecision {
    /// The setting in tokens: Claude `CLAUDE_CODE_AUTO_COMPACT_WINDOW`, Codex
    /// `model_auto_compact_token_limit`, Prime/pi `contextWindow`. None = none.
    pub tokens: Option<u64>,
    pub source: WindowSource,
    /// 1 line: a file path, `context-policy windows <key>`,
    /// `teammate <name> compact_window`, or `harness default`.
    pub detail: String,
    /// True only when horch passed the value to the harness. The launch sets
    /// the final value from the built command (review finding 12).
    pub applied: bool,
}

/// The fleet default for a launch: the teammate's `compact_window`, else
/// `windows["<harness>/<teammate>"]`, else `windows["<harness>/<model>"]`.
pub fn fleet_window(
    windows: &BTreeMap<String, u64>,
    harness: &str,
    teammate: &str,
    model: &str,
    teammate_override: Option<u64>,
) -> Option<(u64, String)> {
    if let Some(n) = teammate_override {
        return Some((n, format!("teammate {teammate} compact_window")));
    }
    [teammate, model]
        .into_iter()
        .filter(|name| !name.is_empty())
        .map(|name| format!("{harness}/{name}"))
        .find_map(|key| {
            let n = *windows.get(&key)?;
            Some((n, format!("context-policy windows {key}")))
        })
}

/// The operator's value first; else the fleet default (applied); else the
/// harness default.
pub fn decide(operator: Option<OperatorWindow>, fleet: Option<(u64, String)>) -> WindowDecision {
    if let Some(op) = operator {
        return WindowDecision {
            tokens: op.tokens,
            source: WindowSource::Operator,
            detail: op.detail,
            applied: false,
        };
    }
    match fleet {
        Some((n, detail)) => WindowDecision {
            tokens: Some(n),
            source: WindowSource::Fleet,
            detail,
            applied: true,
        },
        None => WindowDecision {
            tokens: None,
            source: WindowSource::Harness,
            detail: "harness default".into(),
            applied: false,
        },
    }
}

/// The model's context window when the transcript does not say. Claude:
/// 200,000 for a model name with a `haiku` segment, else 1,000,000. pi and
/// Prime: [`MODEL_WINDOWS`]. Every other harness: None (Codex reads it from
/// the transcript).
pub fn model_window(harness: &str, model: &str, rule: TriggerRule) -> Option<u64> {
    if !matches!(rule, TriggerRule::WindowMinusReserve { .. }) {
        return None;
    }
    match harness {
        "claude" => {
            let haiku = model
                .split(|c: char| !c.is_ascii_alphanumeric())
                .any(|segment| segment.eq_ignore_ascii_case("haiku"));
            Some(if haiku {
                CLAUDE_SMALL_WINDOW
            } else {
                CLAUDE_WINDOW
            })
        }
        "pi" | "prime" => lookup(MODEL_WINDOWS, model),
        _ => None,
    }
}

/// The harness's own auto-compact trigger. None = unknown.
pub fn native_trigger(
    rule: TriggerRule,
    harness: &str,
    model: &str,
    setting: Option<u64>,
    transcript_window: Option<u64>,
) -> Option<u64> {
    match rule {
        TriggerRule::WindowMinusReserve { reserve } => {
            let w = model_window(harness, model, rule)?;
            Some(setting.unwrap_or(w).min(w).saturating_sub(reserve))
        }
        TriggerRule::CodexLimit => match transcript_window {
            Some(w) => Some(setting.unwrap_or(u64::MAX).min(w * 18 / 19)),
            None => setting,
        },
        TriggerRule::PerModel => lookup(OPENCODE_TRIGGERS, model),
        TriggerRule::Unknown => None,
    }
}

/// `min(base, floor(0.8 x native))`; `base` when native is unknown (CTX-07).
pub fn threshold(base: u64, native: Option<u64>) -> u64 {
    native.map_or(base, |n| base.min(n / 5 * 4 + n % 5 * 4 / 5))
}

/// `native - threshold` (CTX-08); 0 when the threshold is above native.
pub fn headroom(native: u64, threshold: u64) -> u64 {
    native.saturating_sub(threshold)
}

fn lookup(table: &[(&str, u64)], model: &str) -> Option<u64> {
    table.iter().find(|(m, _)| *m == model).map(|(_, n)| *n)
}
