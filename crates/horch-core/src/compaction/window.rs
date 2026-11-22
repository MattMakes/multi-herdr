use std::collections::BTreeMap;

use crate::execution::legacy::{LedgerWindow, LedgerWindowSource};
use crate::harness::capabilities::TriggerRule;

pub const BASE_THRESHOLD: u64 = 300_000;

pub const HEADROOM_FLOOR: u64 = 20_000;

pub const OPENCODE_TRIGGERS: &[(&str, u64)] = &[
    ("opencode/big-pickle", 140_000),
    ("opencode/nemotron-3.5-lightning-free", 230_144),
    ("opencode/nemotron-3-ultra-free", 968_000),
];

pub const MODEL_WINDOWS: &[(&str, u64)] = &[
    ("anthropic/claude-opus-5-5", 1_000_000),
    ("anthropic/claude-opus-5", 1_000_000),
    ("ollama/qwen3.8", 262_144),
];

const CLAUDE_SMALL_WINDOW: u64 = 200_000;
const CLAUDE_WINDOW: u64 = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowSource {
    Operator,
    Fleet,
    Harness,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorWindow {
    pub tokens: Option<u64>,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowDecision {
    pub tokens: Option<u64>,
    pub source: WindowSource,
    pub detail: String,
    pub applied: bool,
}

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
    let family = crate::harness::models::family_of(model);
    let found = [teammate, model, family.as_str()]
        .into_iter()
        .filter(|name| !name.is_empty())
        .map(|name| format!("{harness}/{name}"))
        .find_map(|key| {
            let n = *windows.get(&key)?;
            Some((n, format!("context-policy windows {key}")))
        });
    found
}

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

pub fn threshold(base: u64, native: Option<u64>) -> u64 {
    native.map_or(base, |n| base.min(n / 5 * 4 + n % 5 * 4 / 5))
}

pub fn headroom(native: u64, threshold: u64) -> u64 {
    native.saturating_sub(threshold)
}

fn lookup(table: &[(&str, u64)], model: &str) -> Option<u64> {
    table.iter().find(|(m, _)| *m == model).map(|(_, n)| *n)
}

impl From<&WindowDecision> for LedgerWindow {
    fn from(d: &WindowDecision) -> LedgerWindow {
        LedgerWindow {
            tokens: d.tokens,
            source: match d.source {
                WindowSource::Operator => LedgerWindowSource::Operator,
                WindowSource::Fleet => LedgerWindowSource::Fleet,
                WindowSource::Harness => LedgerWindowSource::Harness,
            },
            detail: d.detail.clone(),
            applied: d.applied,
        }
    }
}

impl WindowDecision {
    pub fn from_ledger(w: &LedgerWindow) -> Option<WindowDecision> {
        let source = match w.source {
            LedgerWindowSource::Operator => WindowSource::Operator,
            LedgerWindowSource::Fleet => WindowSource::Fleet,
            LedgerWindowSource::Harness => WindowSource::Harness,
            LedgerWindowSource::Unknown => return None,
        };
        Some(WindowDecision {
            tokens: w.tokens,
            source,
            detail: w.detail.clone(),
            applied: w.applied,
        })
    }
}
