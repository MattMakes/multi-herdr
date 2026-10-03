//! Usage limits: the pools a teammate draws from, how full each one is, and
//! what state that puts it in (design section 11).
//!
//! Every reading is normalized on the way in: a window is named by its
//! duration, never by its position; `used` is a fraction 0-1; times are
//! RFC 3339 UTC (QUO-04). Account ids, emails and dollar amounts never leave
//! the parsers (TEL-11).
//!
//! Limits are read through each harness's own client, so horch never holds a
//! credential (D2):
//!
//! | pool | probe |
//! |---|---|
//! | claude | `claude -p --input-format stream-json` + one `get_usage` control request |
//! | codex | `codex app-server`: `initialize`, `initialized`, `account/rateLimits/read` |
//! | opencode-zen | none exists; a `FreeUsageLimitError` starts a cooldown |
//! | local | `pi --version`, and the model in `ollama list` |

//!
//! This module is pure. File I/O lives in [`crate::routing::snapshot`] and the
//! probes in [`crate::routing::quota_probe`].

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::clock;
use crate::routing::policy::Policy;
use crate::telemetry::QuotaSignal;

pub const POOL_CLAUDE: &str = "claude";
pub const POOL_CODEX: &str = "codex";
pub const POOL_ZEN: &str = "opencode-zen";
/// The operator's Google account, which every Antigravity model draws on,
/// whoever made the model. No probe reads it yet, so it assesses `unknown`.
pub const POOL_GOOGLE: &str = "google";
pub(crate) const POOL_LOCAL: &str = "local";
pub(crate) const POOL_UNKNOWN: &str = "unknown";

/// The pools, in display order.
pub const POOLS: [&str; 5] = [POOL_CLAUDE, POOL_CODEX, POOL_ZEN, POOL_GOOGLE, POOL_LOCAL];

/// One limit window, normalized (QUO-04).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Window {
    /// `5h`, `7d`, or `<n>m` for any other duration.
    pub name: String,
    pub minutes: u64,
    /// The model family a scoped window limits (`opus`, `sonnet`, `fable`).
    #[serde(default)]
    pub scope_model: Option<String>,
    /// Fraction used, 0-1. A reading above 1 is kept: it means over.
    pub used: f64,
    #[serde(default)]
    pub resets_at: Option<String>,
}

/// A window's name from its duration (QUO-04).
pub fn window_name(minutes: u64) -> String {
    match minutes {
        300 => "5h".into(),
        10_080 => "7d".into(),
        n => format!("{n}m"),
    }
}

impl Window {
    pub fn new(minutes: u64, used: f64, resets_at: Option<String>, scope: Option<String>) -> Self {
        Window {
            name: window_name(minutes),
            minutes,
            scope_model: scope.map(|s| s.to_ascii_lowercase()),
            used,
            resets_at,
        }
    }

    /// `name`, with the scope when there is one: `7d`, `7d:fable`.
    pub fn label(&self) -> String {
        match &self.scope_model {
            Some(s) => format!("{}:{s}", self.name),
            None => self.name.clone(),
        }
    }

    fn resets(&self) -> Option<DateTime<Utc>> {
        self.resets_at.as_deref().and_then(clock::parse)
    }

    /// `used`, or 0 once the window has reset (section 11.4).
    pub fn used_at(&self, now: DateTime<Utc>) -> f64 {
        match self.resets() {
            Some(r) if r <= now => 0.0,
            _ => self.used,
        }
    }

    /// Hours until reset, if known and in the future.
    pub(crate) fn left_h(&self, now: DateTime<Utc>) -> Option<f64> {
        let r = self.resets()?;
        (r > now).then(|| (r - now).num_seconds() as f64 / 3600.0)
    }

    /// When the current window started: `resets_at - minutes`.
    pub fn start(&self) -> Option<DateTime<Utc>> {
        Some(self.resets()? - Duration::minutes(self.minutes as i64))
    }

    /// `used / elapsed`, elapsed clamped to [0.01, 1] (section 11.4).
    pub(crate) fn pace(&self, now: DateTime<Utc>) -> Option<f64> {
        let left = self.left_h(now)?;
        let elapsed = (1.0 - left / (self.minutes as f64 / 60.0)).clamp(0.01, 1.0);
        Some(self.used_at(now) / elapsed)
    }

    /// `(1 - used) / max(left_h, 1)`.
    pub fn headroom_per_h(&self, now: DateTime<Utc>) -> f64 {
        let left = self.left_h(now).unwrap_or(0.0);
        (1.0 - self.used_at(now)).max(0.0) / left.max(1.0)
    }
}

// ─── pools ──────────────────────────────────────────────────────────────────

/// A teammate's pool, from its agent and model (section 11.1). Prime runs
/// Anthropic models on a credential horch cannot see, so it is counted
/// against the claude pool: the conservative choice.
pub fn pool_for(agent: &str, model: &str) -> &'static str {
    let m = model.to_ascii_lowercase();
    let provider = m.split_once('/').map(|(p, _)| p);
    match agent {
        "claude" => POOL_CLAUDE,
        "codex" => POOL_CODEX,
        "antigravity" => POOL_GOOGLE,
        "opencode" if provider == Some("opencode") => POOL_ZEN,
        "pi" | "prime" => match provider {
            Some("ollama") => POOL_LOCAL,
            Some("anthropic") => POOL_CLAUDE,
            Some("openai") | Some("openai-codex") => POOL_CODEX,
            Some("opencode") => POOL_ZEN,
            _ => POOL_UNKNOWN,
        },
        _ => POOL_UNKNOWN,
    }
}

/// The model family a scoped window can limit, from a model name.
pub(crate) fn model_family(model: &str) -> Option<&'static str> {
    let m = model.to_ascii_lowercase();
    [
        "opus", "sonnet", "haiku", "fable", "astra", "sol", "terra", "luna",
    ]
    .into_iter()
    .find(|f| m.contains(f))
}

// ─── quota.json ─────────────────────────────────────────────────────────────

/// What horch last knew about one pool (section 11.3).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PoolReading {
    /// The state when this was written, for display. Deciders re-assess.
    #[serde(default)]
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default)]
    pub observed_at: Option<String>,
    /// `get_usage`, `app-server`, `rollout`, `refusal`, `health`, `none`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness_version: Option<String>,
    #[serde(default)]
    pub windows: Vec<Window>,
    #[serde(default)]
    pub refusal_seen_at: Option<String>,
    /// Which window the refusal named (`5h`, `7d`, `7d:fable`), if it said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal_window: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ordinary_usage_allowed: Option<bool>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub no_signal: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cooling_until: Option<String>,
    /// Local pool: the models `ollama list` reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models: Option<Vec<String>>,
    /// When a probe last ran, successful or not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub probed_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuotaFile {
    pub schema: u32,
    #[serde(default)]
    pub written_at: Option<String>,
    #[serde(default)]
    pub pools: BTreeMap<String, PoolReading>,
}

impl Default for QuotaFile {
    fn default() -> Self {
        QuotaFile {
            schema: 1,
            written_at: None,
            pools: BTreeMap::new(),
        }
    }
}

pub(crate) fn quota_path(state_root: &Path) -> PathBuf {
    crate::telemetry::dir(state_root).join("quota.json")
}

impl QuotaFile {
    /// Fold transcript signals in (QUO-03, QUO-06): refusals, and Codex
    /// snapshots that are newer than what the file holds.
    pub(crate) fn apply_signals(&mut self, signals: &[QuotaSignal], policy: &Policy) {
        for s in signals {
            match s {
                QuotaSignal::Refusal { pool, at, what } => {
                    let r = self.pools.entry(pool.clone()).or_default();
                    if r.refusal_seen_at
                        .as_deref()
                        .is_none_or(|prev| at.as_str() > prev)
                    {
                        r.refusal_seen_at = Some(at.clone());
                        r.refusal_window = match what.as_str() {
                            "5h" | "7d" | "7d:opus" | "7d:sonnet" | "7d:fable" => {
                                Some(what.clone())
                            }
                            _ => None,
                        };
                    }
                    if pool == POOL_ZEN {
                        if let Some(t) = clock::parse(at) {
                            let until =
                                clock::stamp(t + Duration::minutes(policy.opencode_cooldown_min));
                            if r.cooling_until
                                .as_deref()
                                .is_none_or(|prev| until.as_str() > prev)
                            {
                                r.cooling_until = Some(until);
                            }
                            r.no_signal = false;
                        }
                    }
                }
                QuotaSignal::Snapshot {
                    pool,
                    at,
                    windows,
                    ordinary_usage_allowed,
                } => {
                    let r = self.pools.entry(pool.clone()).or_default();
                    let newer = r
                        .observed_at
                        .as_deref()
                        .is_none_or(|prev| at.as_str() > prev);
                    if newer {
                        r.windows = windows.clone();
                        r.observed_at = Some(at.clone());
                        r.source = Some("rollout".into());
                        if ordinary_usage_allowed.is_some() {
                            r.ordinary_usage_allowed = *ordinary_usage_allowed;
                        }
                    }
                }
            }
        }
    }
}

// ─── states ─────────────────────────────────────────────────────────────────

/// A pool state (section 11.4), worst first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Broken,
    Exhausted,
    Cooling,
    Unknown,
    Tight,
    Ok,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Broken => "broken",
            State::Exhausted => "exhausted",
            State::Cooling => "cooling",
            State::Unknown => "unknown",
            State::Tight => "tight",
            State::Ok => "ok",
        }
    }

    /// A pool a spawn cannot use.
    pub fn blocks(self) -> bool {
        matches!(self, State::Broken | State::Exhausted | State::Cooling)
    }
}

impl std::fmt::Display for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A pool's state for one model scope, and why.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Assessment {
    pub pool: String,
    pub state: State,
    pub reason: String,
    /// The binding window's headroom, per hour. `None` without windows.
    pub headroom_per_h: Option<f64>,
    /// The window that decided, e.g. `7d 100%, resets 2026-10-02T13:59:59Z`.
    pub worst: Option<Window>,
}

/// `100%` from 1.0.
pub fn pct(used: f64) -> String {
    format!("{:.0}%", used * 100.0)
}

/// `Thu 14:00Z`: a short reset time for NOTE lines and the screen.
pub fn short_time(stamp: &str) -> String {
    clock::parse(stamp)
        .map(|t| {
            // Round to the minute the way people read a reset.
            let t = t + Duration::seconds(30);
            t.format("%a %H:%MZ").to_string()
        })
        .unwrap_or_else(|| stamp.to_string())
}

/// Readings plus everything needed to judge them. Pure: time comes in.
#[derive(Debug, Clone)]
pub struct QuotaView {
    pub file: QuotaFile,
    pub now: DateTime<Utc>,
    pub policy: Policy,
    /// Readings came from `HORCH_QUOTA_FILE`: authoritative, never stale.
    pub from_override: bool,
}

impl QuotaView {
    pub fn new(file: QuotaFile, now: DateTime<Utc>, policy: Policy, from_override: bool) -> Self {
        QuotaView {
            file,
            now,
            policy,
            from_override,
        }
    }

    /// The state of the pool a teammate draws from, for its model.
    pub fn assess(&self, agent: &str, model: &str) -> Assessment {
        let pool = pool_for(agent, model);
        let local_model =
            (pool == POOL_LOCAL).then(|| model.split_once('/').map(|(_, m)| m).unwrap_or(model));
        self.assess_pool(pool, model_family(model), local_model)
    }

    /// The state of `pool` for a model family scope (section 11.4).
    pub fn assess_pool(
        &self,
        pool: &str,
        scope: Option<&str>,
        local_model: Option<&str>,
    ) -> Assessment {
        let now = self.now;
        let p = &self.policy;
        let out = |state: State, reason: String, worst: Option<Window>, headroom: Option<f64>| {
            Assessment {
                pool: pool.to_string(),
                state,
                reason,
                headroom_per_h: headroom,
                worst,
            }
        };
        if pool == POOL_UNKNOWN {
            return out(
                State::Unknown,
                "no known pool for this model".into(),
                None,
                None,
            );
        }
        let Some(r) = self.file.pools.get(pool) else {
            return out(State::Unknown, "no reading".into(), None, None);
        };

        // broken: the local health check failed.
        if pool == POOL_LOCAL {
            if let Some(e) = &r.error {
                return out(State::Broken, e.clone(), None, None);
            }
            if let (Some(want), Some(models)) = (local_model, &r.models) {
                let have = models
                    .iter()
                    .any(|m| m == want || m.strip_suffix(":latest") == Some(want));
                if !have {
                    return out(
                        State::Broken,
                        format!("model {want} is not in ollama list"),
                        None,
                        None,
                    );
                }
            }
            if r.state == "broken" {
                return out(
                    State::Broken,
                    r.error.clone().unwrap_or_else(|| "broken".into()),
                    None,
                    None,
                );
            }
        }

        let windows: Vec<&Window> = r
            .windows
            .iter()
            .filter(|w| w.scope_model.is_none() || w.scope_model.as_deref() == scope)
            .collect();
        let worst = windows.iter().copied().max_by(|a, b| {
            a.used_at(now)
                .partial_cmp(&b.used_at(now))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let headroom = windows
            .iter()
            .map(|w| w.headroom_per_h(now))
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let describe = |w: &Window| {
            let mut s = format!("{} {}", w.label(), pct(w.used_at(now)));
            if let Some(at) = &w.resets_at {
                s.push_str(&format!(", resets {at}"));
            }
            s
        };

        // exhausted
        if let Some(w) = worst.filter(|w| w.used_at(now) >= p.exhausted_used) {
            return out(
                State::Exhausted,
                format!("{} (>= {})", describe(w), p.exhausted_used),
                Some(w.clone()),
                headroom,
            );
        }
        // A Zen refusal starts a cooldown instead (below).
        if let Some(seen) = r
            .refusal_seen_at
            .as_deref()
            .and_then(clock::parse)
            .filter(|_| pool != POOL_ZEN)
        {
            if refusal_in_force(r, &windows, seen, now) {
                return out(
                    State::Exhausted,
                    format!("refused at {}", clock::stamp(seen)),
                    worst.cloned(),
                    Some(0.0),
                );
            }
        }
        if r.ordinary_usage_allowed == Some(false) {
            return out(
                State::Exhausted,
                "ordinary usage not allowed".into(),
                worst.cloned(),
                Some(0.0),
            );
        }

        // cooling
        if pool == POOL_ZEN {
            if let Some(until) = r.cooling_until.as_deref().and_then(clock::parse) {
                if until > now {
                    return out(
                        State::Cooling,
                        format!("free limit hit; cooling until {}", clock::stamp(until)),
                        None,
                        None,
                    );
                }
            }
            return out(
                State::Ok,
                if r.no_signal || r.cooling_until.is_none() {
                    "no signal (no quota endpoint exists)".into()
                } else {
                    "cooldown over".into()
                },
                None,
                None,
            );
        }
        if pool == POOL_LOCAL {
            return out(State::Ok, "pi and ollama answer".into(), None, None);
        }

        // unknown
        let fresh = self.from_override
            || r.observed_at
                .as_deref()
                .and_then(clock::parse)
                .is_some_and(|t| now - t < Duration::minutes(p.stale_after_min));
        if !fresh || windows.is_empty() {
            let why = match (&r.error, &r.observed_at) {
                (Some(e), _) => e.clone(),
                (None, Some(at)) if !fresh => format!("reading from {at} is stale"),
                _ => "no reading".into(),
            };
            return out(State::Unknown, why, worst.cloned(), headroom);
        }

        // tight
        let w = worst.expect("windows is not empty");
        if w.used_at(now) >= p.tight_used {
            return out(
                State::Tight,
                format!("{} (>= {})", describe(w), p.tight_used),
                Some(w.clone()),
                headroom,
            );
        }
        for w in &windows {
            if let Some(pace) = w.pace(now) {
                if w.used_at(now) >= p.pace_min_used && pace > p.pace_factor {
                    return out(
                        State::Tight,
                        format!("{}, pace {pace:.1}x", describe(w)),
                        Some((*w).clone()),
                        headroom,
                    );
                }
            }
        }
        out(State::Ok, describe(w), Some(w.clone()), headroom)
    }
}

/// A refusal counts until the window it hit resets. Unnamed, it is held to
/// the shortest window it could be (it expires soonest); with no windows at
/// all, for 5 hours.
fn refusal_in_force(
    r: &PoolReading,
    windows: &[&Window],
    seen: DateTime<Utc>,
    now: DateTime<Utc>,
) -> bool {
    let named = r
        .refusal_window
        .as_deref()
        .and_then(|label| windows.iter().find(|w| w.label() == label));
    let window = named.or_else(|| windows.iter().min_by_key(|w| w.minutes));
    match window.and_then(|w| Some((w.start()?, w.resets()?))) {
        Some((start, resets)) => seen >= start && resets > now,
        None => now - seen < Duration::hours(5),
    }
}

/// What a successful probe read.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Probed {
    pub windows: Vec<Window>,
    pub ordinary_usage_allowed: Option<bool>,
}

/// The first object under `v` (depth first) that has `key`.
fn object_with<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Object(map) => {
            if map.contains_key(key) {
                return Some(v);
            }
            map.values().find_map(|x| object_with(x, key))
        }
        Value::Array(list) => list.iter().find_map(|x| object_with(x, key)),
        _ => None,
    }
}

/// An ISO string or epoch seconds, as a stamp.
fn reset_stamp(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(s) => clock::parse(s).map(clock::stamp),
        Value::Number(n) => n.as_i64().and_then(clock::from_epoch).map(clock::stamp),
        _ => None,
    }
}

/// Parse a `get_usage` reply ([Q] 1a). `utilization` is 0-100 and resets
/// are ISO. Only window fields are read; everything else is dropped.
pub(crate) fn parse_claude_usage(reply: &Value) -> Result<Probed, String> {
    if reply.pointer("/response/subtype").and_then(Value::as_str) == Some("error") {
        let e = reply
            .pointer("/response/error")
            .and_then(Value::as_str)
            .unwrap_or("error");
        return Err(format!("get_usage error: {e}"));
    }
    let Some(holder) = object_with(reply, "rate_limits") else {
        return Err("get_usage reply has no rate_limits".into());
    };
    if holder.get("rate_limits_available") == Some(&Value::Bool(false)) {
        return Err("rate limits are not available for this login".into());
    }
    let rl = holder.get("rate_limits").unwrap_or(&Value::Null);
    let mut out: Vec<Window> = Vec::new();
    let pct_of = |w: &Value, key: &str| w.get(key).and_then(Value::as_f64).map(|u| u / 100.0);
    for (key, minutes, scope) in [
        ("five_hour", 300u64, None),
        ("seven_day", 10_080, None),
        ("seven_day_opus", 10_080, Some("opus")),
        ("seven_day_sonnet", 10_080, Some("sonnet")),
    ] {
        let Some(w) = rl.get(key).filter(|w| w.is_object()) else {
            continue;
        };
        let Some(used) = pct_of(w, "utilization") else {
            continue;
        };
        out.push(Window::new(
            minutes,
            used,
            reset_stamp(w.get("resets_at")),
            scope.map(str::to_owned),
        ));
    }
    let has_scope =
        |out: &[Window], s: &str| out.iter().any(|w| w.scope_model.as_deref() == Some(s));
    for m in rl
        .get("model_scoped")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let (Some(name), Some(used)) = (
            m.get("display_name").and_then(Value::as_str),
            pct_of(m, "utilization"),
        ) else {
            continue;
        };
        let scope = name.to_ascii_lowercase();
        if !has_scope(&out, &scope) {
            out.push(Window::new(
                10_080,
                used,
                reset_stamp(m.get("resets_at")),
                Some(scope),
            ));
        }
    }
    // The endpoint's own rows, passed through when present. Classified by
    // `kind`, never by label.
    for row in rl
        .get("limits")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(used) = pct_of(row, "percent") else {
            continue;
        };
        let resets = reset_stamp(row.get("resets_at"));
        match row.get("kind").and_then(Value::as_str) {
            Some("weekly_scoped") => {
                let Some(name) = row
                    .pointer("/scope/model/display_name")
                    .and_then(Value::as_str)
                else {
                    continue;
                };
                let scope = name.to_ascii_lowercase();
                if !has_scope(&out, &scope) {
                    out.push(Window::new(10_080, used, resets, Some(scope)));
                }
            }
            Some("weekly_all")
                if !out
                    .iter()
                    .any(|w| w.minutes == 10_080 && w.scope_model.is_none()) =>
            {
                out.push(Window::new(10_080, used, resets, None));
            }
            Some("session") if !out.iter().any(|w| w.minutes == 300) => {
                out.push(Window::new(300, used, resets, None));
            }
            _ => {}
        }
    }
    if out.is_empty() {
        return Err("get_usage reply has no windows".into());
    }
    Ok(Probed {
        windows: out,
        ordinary_usage_allowed: None,
    })
}

/// Parse an `account/rateLimits/read` result ([Q] 2). `usedPercent` is
/// 0-100 and `resetsAt` epoch seconds; windows by `windowDurationMins`.
pub(crate) fn parse_codex_limits(reply: &Value) -> Result<Probed, String> {
    if let Some(e) = reply.get("error") {
        let msg = e.get("message").and_then(Value::as_str).unwrap_or("error");
        return Err(format!("account/rateLimits/read error: {msg}"));
    }
    let result = reply.get("result").ok_or("no result in the reply")?;
    let snap = result
        .get("rateLimits")
        .filter(|s| s.is_object())
        .or_else(|| result.pointer("/rateLimitsByLimitId/codex"))
        .ok_or("no rateLimits in the reply")?;
    let mut windows = Vec::new();
    for slot in ["primary", "secondary"] {
        let Some(w) = snap.get(slot).filter(|w| w.is_object()) else {
            continue;
        };
        let (Some(mins), Some(used)) = (
            w.get("windowDurationMins").and_then(Value::as_u64),
            w.get("usedPercent").and_then(Value::as_f64),
        ) else {
            continue;
        };
        windows.push(Window::new(
            mins,
            used / 100.0,
            reset_stamp(w.get("resetsAt")),
            None,
        ));
    }
    let allowed = result.get("ordinaryUsageAllowed").and_then(Value::as_bool);
    if windows.is_empty() && allowed.is_none() {
        return Err("account/rateLimits/read reply has no windows".into());
    }
    Ok(Probed {
        windows,
        ordinary_usage_allowed: allowed,
    })
}

/// The newest time any pool was probed.
pub(crate) fn last_probe(file: &QuotaFile) -> Option<DateTime<Utc>> {
    file.pools
        .values()
        .filter_map(|r| r.probed_at.as_deref().and_then(clock::parse))
        .max()
}

/// Whether a probe is due (QUO-07): when the last probe is older than
/// `age_min`. A file override is never probed; the caller checks for one
/// first (it is a parameter, not an environment read, since A5).
pub(crate) fn probe_due(file: &QuotaFile, now: DateTime<Utc>, age_min: i64) -> bool {
    last_probe(file).is_none_or(|t| now - t >= Duration::minutes(age_min))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(rel: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/telemetry")
            .join(rel)
    }

    fn now() -> DateTime<Utc> {
        clock::parse("2026-09-28T18:00:00Z").unwrap()
    }

    fn view(name: &str) -> QuotaView {
        let file = QuotaFile::read(&fixture(&format!("quota/{name}.json"))).unwrap();
        QuotaView::new(file, now(), Policy::default(), true)
    }

    fn last_json_line(rel: &str) -> Value {
        let text = std::fs::read_to_string(fixture(rel)).unwrap();
        serde_json::from_str(text.lines().last().unwrap()).unwrap()
    }

    #[test]
    fn quo_04_windows_by_duration() {
        // The same limits in either slot read the same.
        let a = parse_codex_limits(
            &serde_json::from_str(
                &std::fs::read_to_string(fixture("probes/codex-ratelimits-5h-weekly.json"))
                    .unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        let b = parse_codex_limits(
            &serde_json::from_str(
                &std::fs::read_to_string(fixture("probes/codex-ratelimits-swapped.json")).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        let names = |p: &Probed| {
            let mut v: Vec<(String, u64)> = p
                .windows
                .iter()
                .map(|w| (w.name.clone(), (w.used * 100.0).round() as u64))
                .collect();
            v.sort();
            v
        };
        assert_eq!(names(&a), vec![("5h".into(), 20), ("7d".into(), 40)]);
        assert_eq!(names(&b), vec![("5h".into(), 20), ("7d".into(), 40)]);
        // The 09-11 rollout: primary is 5h, secondary 7d.
        let rl: Value = serde_json::json!({"primary":{"used_percent":10.0,"window_minutes":300,"resets_at":1789473600},
                                           "secondary":{"used_percent":45.0,"window_minutes":10080,"resets_at":1789920000}});
        let w = crate::telemetry::readers::codex_rollout_windows(&rl);
        assert_eq!((w[0].name.as_str(), w[1].name.as_str()), ("5h", "7d"));
        assert_eq!(window_name(60), "60m");
    }

    #[test]
    fn quo_04_units_normalized() {
        // get_usage: 0-100, ISO.
        let p =
            parse_claude_usage(&last_json_line("probes/claude-get-usage-exhausted.jsonl")).unwrap();
        let seven = p
            .windows
            .iter()
            .find(|w| w.name == "7d" && w.scope_model.is_none())
            .unwrap();
        assert_eq!(seven.used, 1.0);
        assert_eq!(seven.resets_at.as_deref(), Some("2026-10-02T13:59:59Z"));
        // Codex: 0-100, epoch seconds.
        let c = parse_codex_limits(
            &serde_json::from_str(
                &std::fs::read_to_string(fixture("probes/codex-ratelimits-weekly.json")).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        assert!((c.windows[0].used - 0.99).abs() < 1e-9);
        assert_eq!(
            c.windows[0].resets_at.as_deref(),
            Some("2026-10-03T19:15:49Z")
        );
        assert_eq!(c.ordinary_usage_allowed, Some(true));
        // A numeric reset is epoch seconds wherever it appears.
        assert_eq!(
            reset_stamp(Some(&Value::from(1_791_054_949))).as_deref(),
            Some("2026-10-03T19:15:49Z")
        );
    }

    #[test]
    fn quo_04_scoped_windows() {
        let p =
            parse_claude_usage(&last_json_line("probes/claude-get-usage-scoped.jsonl")).unwrap();
        let fable: Vec<&Window> = p
            .windows
            .iter()
            .filter(|w| w.scope_model.as_deref() == Some("fable"))
            .collect();
        assert_eq!(
            fable.len(),
            1,
            "model_scoped and limits[] name one window: {:?}",
            p.windows
        );
        assert!((fable[0].used - 0.99).abs() < 1e-9);
        let o = parse_claude_usage(&last_json_line("probes/claude-get-usage-opus-scoped.jsonl"))
            .unwrap();
        assert!(o
            .windows
            .iter()
            .any(|w| w.label() == "7d:opus" && (w.used - 0.97).abs() < 1e-9));
        // The scope applies only to its own family.
        let v = view("fable-scoped-exhausted");
        assert_eq!(v.assess("claude", "fable").state, State::Exhausted);
        assert_eq!(v.assess("claude", "opus").state, State::Ok);
    }

    #[test]
    fn tel_11_probe_parsers_keep_no_identity() {
        let p =
            parse_claude_usage(&last_json_line("probes/claude-get-usage-exhausted.jsonl")).unwrap();
        let text = format!("{:?}", p);
        assert!(
            !text.contains("SENTINEL") && !text.contains("sentinel@"),
            "{text}"
        );
        assert!(!text.contains("3750"), "no dollar amounts: {text}");
    }

    #[test]
    fn a_probe_error_or_empty_reply_is_never_ok() {
        assert!(
            parse_claude_usage(&last_json_line("probes/claude-get-usage-error.jsonl")).is_err()
        );
        assert!(parse_claude_usage(&serde_json::json!({"type":"control_response","response":{"subtype":"success","response":{"weird":1}}})).is_err());
        assert!(parse_codex_limits(
            &serde_json::json!({"id":2,"error":{"message":"not signed in"}})
        )
        .is_err());
        let mut file = QuotaFile::default();
        file.pools.insert(
            POOL_CLAUDE.into(),
            PoolReading {
                error: Some("parse".into()),
                ..Default::default()
            },
        );
        let v = QuotaView::new(file, now(), Policy::default(), false);
        assert_eq!(v.assess("claude", "opus").state, State::Unknown);
    }

    #[test]
    fn quo_05_state_table() {
        for (fixture_name, agent, model, state) in [
            ("all-exhausted", "claude", "opus", State::Exhausted),
            ("all-exhausted", "codex", "gpt-5.6-sol", State::Exhausted),
            ("all-exhausted", "pi", "ollama/qwen3.8", State::Broken),
            ("all-ok", "claude", "opus", State::Ok),
            ("all-ok", "opencode", "opencode/big-pickle", State::Ok),
            ("claude-tight-codex-ok", "claude", "sonnet", State::Tight),
            ("claude-pace-tight", "claude", "opus", State::Tight),
            ("claude-unknown-codex-ok", "claude", "opus", State::Unknown),
            ("claude-refused", "claude", "opus", State::Exhausted),
            (
                "codex-not-allowed",
                "codex",
                "gpt-5.6-sol",
                State::Exhausted,
            ),
            (
                "opencode-cooling",
                "opencode",
                "opencode/big-pickle",
                State::Cooling,
            ),
            ("local-broken", "pi", "ollama/qwen3.8", State::Broken),
            ("all-ok", "prime", "anthropic/claude-opus-5-5", State::Ok),
            (
                "all-exhausted",
                "prime",
                "anthropic/claude-opus-5-5",
                State::Exhausted,
            ),
        ] {
            let a = view(fixture_name).assess(agent, model);
            assert_eq!(
                a.state, state,
                "{fixture_name} {agent} {model}: {}",
                a.reason
            );
        }
    }

    fn one_window(used: f64, resets: &str) -> QuotaView {
        let mut file = QuotaFile::default();
        file.pools.insert(
            POOL_CLAUDE.into(),
            PoolReading {
                observed_at: Some("2026-09-28T17:55:00Z".into()),
                windows: vec![Window::new(10_080, used, Some(resets.into()), None)],
                ..Default::default()
            },
        );
        QuotaView::new(file, now(), Policy::default(), false)
    }

    #[test]
    fn quo_05_edges_and_overrides() {
        // Late in the window (pace low), so only the levels decide.
        let late = "2026-09-28T20:00:00Z";
        assert_eq!(
            one_window(0.849, late).assess("claude", "opus").state,
            State::Ok
        );
        assert_eq!(
            one_window(0.85, late).assess("claude", "opus").state,
            State::Tight
        );
        assert_eq!(
            one_window(0.979, late).assess("claude", "opus").state,
            State::Tight
        );
        assert_eq!(
            one_window(0.98, late).assess("claude", "opus").state,
            State::Exhausted
        );
        // Pace: 84h of 168h left is half elapsed. 0.6 / 0.5 = 1.2 (ok), 0.63 / 0.5 = 1.26 (tight).
        let half = "2026-10-02T06:00:00Z";
        assert_eq!(
            one_window(0.60, half).assess("claude", "opus").state,
            State::Ok
        );
        assert_eq!(
            one_window(0.63, half).assess("claude", "opus").state,
            State::Tight
        );
        // Below pace_min_used, pace does not matter.
        assert_eq!(
            one_window(0.45, "2026-10-05T06:00:00Z")
                .assess("claude", "opus")
                .state,
            State::Ok
        );
        // A reset in the past reads as 0.
        assert_eq!(
            one_window(1.0, "2026-09-28T12:00:00Z")
                .assess("claude", "opus")
                .state,
            State::Ok
        );
        // policy.json overrides every threshold.
        let mut v = one_window(0.80, late);
        v.policy = Policy::parse(r#"{"tight_used": 0.75}"#).unwrap();
        assert_eq!(v.assess("claude", "opus").state, State::Tight);
        // Stale reading.
        let mut v = one_window(0.1, late);
        v.now = clock::parse("2026-09-28T19:00:00Z").unwrap();
        assert_eq!(v.assess("claude", "opus").state, State::Unknown);
    }

    #[test]
    fn quo_06_opencode_cooldown() {
        let mut file = QuotaFile::default();
        file.apply_signals(
            &[QuotaSignal::Refusal {
                pool: POOL_ZEN.into(),
                at: "2026-09-28T17:30:00Z".into(),
                what: "FreeUsageLimitError".into(),
            }],
            &Policy::default(),
        );
        assert_eq!(
            file.pools[POOL_ZEN].cooling_until.as_deref(),
            Some("2026-09-28T18:30:00Z")
        );
        let v = QuotaView::new(file.clone(), now(), Policy::default(), false);
        assert_eq!(
            v.assess("opencode", "opencode/big-pickle").state,
            State::Cooling
        );
        let later = QuotaView::new(
            file,
            clock::parse("2026-09-28T18:31:00Z").unwrap(),
            Policy::default(),
            false,
        );
        assert_eq!(
            later.assess("opencode", "opencode/big-pickle").state,
            State::Ok
        );
    }

    #[test]
    fn quo_06_local_model_must_be_listed() {
        let mut file = QuotaFile::default();
        file.pools.insert(
            POOL_LOCAL.into(),
            PoolReading {
                models: Some(vec!["qwen3.8:latest".into()]),
                ..Default::default()
            },
        );
        let v = QuotaView::new(file, now(), Policy::default(), false);
        assert_eq!(v.assess("pi", "ollama/qwen3.8").state, State::Ok);
        assert_eq!(v.assess("pi", "ollama/llama9").state, State::Broken);
    }

    #[test]
    fn pools_follow_agent_and_provider() {
        assert_eq!(pool_for("claude", "opus"), POOL_CLAUDE);
        assert_eq!(pool_for("codex", "gpt-5.6-sol"), POOL_CODEX);
        assert_eq!(pool_for("opencode", "opencode/big-pickle"), POOL_ZEN);
        assert_eq!(pool_for("antigravity", "gemini-3-1-pro"), POOL_GOOGLE);
        assert_eq!(
            pool_for("antigravity", "claude-opus-4-6-thinking"),
            POOL_GOOGLE
        );
        assert_eq!(pool_for("pi", "ollama/qwen3.8"), POOL_LOCAL);
        assert_eq!(pool_for("prime", "anthropic/claude-opus-5-5"), POOL_CLAUDE);
        assert_eq!(pool_for("pi", "mystery/x"), POOL_UNKNOWN);
        assert_eq!(model_family("claude-opus-5-5"), Some("opus"));
        assert_eq!(short_time("2026-10-02T13:59:59Z"), "Fri 14:00Z");
    }
}
