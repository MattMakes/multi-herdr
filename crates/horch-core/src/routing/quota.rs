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
pub const POOL_GOOGLE: &str = "google";
pub(crate) const POOL_LOCAL: &str = "local";
pub(crate) const POOL_UNKNOWN: &str = "unknown";

pub const POOLS: [&str; 5] = [POOL_CLAUDE, POOL_CODEX, POOL_ZEN, POOL_GOOGLE, POOL_LOCAL];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Window {
    pub name: String,
    pub minutes: u64,
    #[serde(default)]
    pub scope_model: Option<String>,
    pub used: f64,
    #[serde(default)]
    pub resets_at: Option<String>,
}

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

    pub fn label(&self) -> String {
        match &self.scope_model {
            Some(s) => format!("{}:{s}", self.name),
            None => self.name.clone(),
        }
    }

    fn resets(&self) -> Option<DateTime<Utc>> {
        self.resets_at.as_deref().and_then(clock::parse)
    }

    pub fn used_at(&self, now: DateTime<Utc>) -> f64 {
        match self.resets() {
            Some(r) if r <= now => 0.0,
            _ => self.used,
        }
    }

    pub(crate) fn left_h(&self, now: DateTime<Utc>) -> Option<f64> {
        let r = self.resets()?;
        (r > now).then(|| (r - now).num_seconds() as f64 / 3600.0)
    }

    pub fn start(&self) -> Option<DateTime<Utc>> {
        Some(self.resets()? - Duration::minutes(self.minutes as i64))
    }

    pub(crate) fn pace(&self, now: DateTime<Utc>) -> Option<f64> {
        let left = self.left_h(now)?;
        let elapsed = (1.0 - left / (self.minutes as f64 / 60.0)).clamp(0.01, 1.0);
        Some(self.used_at(now) / elapsed)
    }

    pub fn headroom_per_h(&self, now: DateTime<Utc>) -> f64 {
        let left = self.left_h(now).unwrap_or(0.0);
        (1.0 - self.used_at(now)).max(0.0) / left.max(1.0)
    }
}

pub fn pool_for(agent: &str, model: &str) -> &'static str {
    let m = model.to_ascii_lowercase();
    let provider = m.split_once('/').map(|(p, _)| p);
    match agent {
        "claude" => POOL_CLAUDE,
        "codex" => POOL_CODEX,
        "antigravity" => POOL_GOOGLE,
        "opencode" if provider == Some("opencode") => POOL_ZEN,
        "pi" | "prime" => provider_pool(provider),
        _ => POOL_UNKNOWN,
    }
}

fn provider_pool(provider: Option<&str>) -> &'static str {
    match provider {
        Some("ollama") => POOL_LOCAL,
        Some("anthropic") => POOL_CLAUDE,
        Some("openai") | Some("openai-codex") => POOL_CODEX,
        Some("opencode") => POOL_ZEN,
        _ => POOL_UNKNOWN,
    }
}

pub(crate) fn model_family(model: &str) -> Option<&'static str> {
    let m = model.to_ascii_lowercase();
    [
        "opus", "sonnet", "haiku", "fable", "astra", "sol", "terra", "luna", "argon", "gemini",
        "gpt-oss",
    ]
    .into_iter()
    .find(|f| m.contains(f))
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PoolReading {
    #[serde(default)]
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default)]
    pub observed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness_version: Option<String>,
    #[serde(default)]
    pub windows: Vec<Window>,
    #[serde(default)]
    pub refusal_seen_at: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models: Option<Vec<String>>,
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
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub harnesses: BTreeMap<String, HarnessHealth>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessHealth {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub probed_at: Option<String>,
}

impl Default for QuotaFile {
    fn default() -> Self {
        QuotaFile {
            schema: 1,
            written_at: None,
            pools: BTreeMap::new(),
            harnesses: BTreeMap::new(),
        }
    }
}

pub(crate) fn quota_path(state_root: &Path) -> PathBuf {
    crate::telemetry::dir(state_root).join("quota.json")
}

impl QuotaFile {
    pub(crate) fn apply_signals(&mut self, signals: &[QuotaSignal], policy: &Policy) {
        for s in signals {
            match s {
                QuotaSignal::Refusal { pool, at, what } => self
                    .pools
                    .entry(pool.clone())
                    .or_default()
                    .apply_refusal(pool, at, what, policy),
                QuotaSignal::Snapshot {
                    pool,
                    at,
                    windows,
                    ordinary_usage_allowed,
                } => self.pools.entry(pool.clone()).or_default().apply_snapshot(
                    at,
                    windows,
                    *ordinary_usage_allowed,
                ),
            }
        }
    }
}

impl PoolReading {
    fn apply_refusal(&mut self, pool: &str, at: &str, what: &str, policy: &Policy) {
        if self.refusal_seen_at.as_deref().is_none_or(|prev| at > prev) {
            self.refusal_seen_at = Some(at.to_string());
            self.refusal_window =
                matches!(what, "5h" | "7d" | "7d:opus" | "7d:sonnet" | "7d:fable")
                    .then(|| what.to_string());
        }
        if let Some(t) = (pool == POOL_ZEN).then(|| clock::parse(at)).flatten() {
            let until = clock::stamp(t + Duration::minutes(policy.opencode_cooldown_min));
            if self
                .cooling_until
                .as_deref()
                .is_none_or(|prev| until.as_str() > prev)
            {
                self.cooling_until = Some(until);
            }
            self.no_signal = false;
        }
    }

    fn apply_snapshot(
        &mut self,
        at: &str,
        windows: &[Window],
        ordinary_usage_allowed: Option<bool>,
    ) {
        if self.observed_at.as_deref().is_none_or(|prev| at > prev) {
            self.windows = windows.to_vec();
            self.observed_at = Some(at.to_string());
            self.source = Some("rollout".into());
            if ordinary_usage_allowed.is_some() {
                self.ordinary_usage_allowed = ordinary_usage_allowed;
            }
        }
    }
}

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

    pub fn blocks(self) -> bool {
        matches!(self, State::Broken | State::Exhausted | State::Cooling)
    }
}

impl std::fmt::Display for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Assessment {
    pub pool: String,
    pub state: State,
    pub reason: String,
    pub headroom_per_h: Option<f64>,
    pub worst: Option<Window>,
}

impl Assessment {
    fn new(
        pool: &str,
        state: State,
        reason: String,
        worst: Option<Window>,
        headroom_per_h: Option<f64>,
    ) -> Assessment {
        Assessment {
            pool: pool.to_string(),
            state,
            reason,
            headroom_per_h,
            worst,
        }
    }
}

pub fn pct(used: f64) -> String {
    format!("{:.0}%", used * 100.0)
}

pub fn short_time(stamp: &str) -> String {
    clock::parse(stamp)
        .map(|t| {
            let t = t + Duration::seconds(30);
            t.format("%a %H:%MZ").to_string()
        })
        .unwrap_or_else(|| stamp.to_string())
}

#[derive(Debug, Clone)]
pub struct QuotaView {
    pub file: QuotaFile,
    pub now: DateTime<Utc>,
    pub policy: Policy,
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

    pub fn assess(&self, agent: &str, model: &str) -> Assessment {
        let pool = pool_for(agent, model);
        if let Some(e) = self
            .file
            .harnesses
            .get(agent)
            .and_then(|h| h.error.as_ref())
        {
            return Assessment {
                pool: pool.to_string(),
                state: State::Broken,
                reason: e.clone(),
                headroom_per_h: None,
                worst: None,
            };
        }
        let local_model =
            (pool == POOL_LOCAL).then(|| model.split_once('/').map(|(_, m)| m).unwrap_or(model));
        self.assess_pool(pool, model_family(model), local_model)
    }

    pub fn assess_shown(&self, pool: &str) -> Assessment {
        let scopes: Vec<&str> = self
            .file
            .pools
            .get(pool)
            .map(|r| r.windows.iter().map(|w| w.scope_model.as_deref()).collect())
            .and_then(|s: Option<Vec<&str>>| s)
            .unwrap_or_default();
        scopes
            .iter()
            .map(|s| self.assess_pool(pool, Some(s), None))
            .max_by_key(|a| a.state)
            .unwrap_or_else(|| self.assess_pool(pool, None, None))
    }

    pub fn assess_pool(
        &self,
        pool: &str,
        scope: Option<&str>,
        local_model: Option<&str>,
    ) -> Assessment {
        match self.precheck(pool, local_model) {
            Ok(r) => self.assess_reading(pool, scope, r),
            Err((state, reason)) => Assessment::new(pool, state, reason, None, None),
        }
    }

    fn precheck(
        &self,
        pool: &str,
        local_model: Option<&str>,
    ) -> Result<&PoolReading, (State, String)> {
        if pool == POOL_UNKNOWN {
            return Err((State::Unknown, "no known pool for this model".into()));
        }
        let Some(r) = self.file.pools.get(pool) else {
            return Err((State::Unknown, "no reading".into()));
        };
        match local_broken(pool, r, local_model) {
            Some(why) => Err((State::Broken, why)),
            None => Ok(r),
        }
    }

    fn assess_reading(&self, pool: &str, scope: Option<&str>, r: &PoolReading) -> Assessment {
        let rules = Rules::new(self, pool, scope, r);
        rules
            .full_window()
            .or_else(|| rules.refused())
            .or_else(|| rules.usage_barred())
            .or_else(|| rules.keyless_pool())
            .unwrap_or_else(|| rules.soft_limit(scope))
    }
}

fn local_broken(pool: &str, r: &PoolReading, local_model: Option<&str>) -> Option<String> {
    if pool != POOL_LOCAL {
        return None;
    }
    let missing = local_model
        .zip(r.models.as_ref())
        .filter(|(want, models)| {
            !models
                .iter()
                .any(|m| m == want || m.strip_suffix(":latest") == Some(want))
        })
        .map(|(want, _)| format!("model {want} is not in ollama list"));
    r.error
        .clone()
        .or(missing)
        .or_else(|| (r.state == "broken").then(|| "broken".into()))
}

struct Rules<'a> {
    view: &'a QuotaView,
    pool: &'a str,
    scope: Option<&'a str>,
    r: &'a PoolReading,
    windows: Vec<&'a Window>,
    worst: Option<&'a Window>,
    headroom: Option<f64>,
}

impl<'a> Rules<'a> {
    fn new(view: &'a QuotaView, pool: &'a str, scope: Option<&'a str>, r: &'a PoolReading) -> Self {
        let now = view.now;
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
        Rules {
            view,
            pool,
            scope,
            r,
            windows,
            worst,
            headroom,
        }
    }

    fn out(
        &self,
        state: State,
        reason: String,
        worst: Option<Window>,
        headroom: Option<f64>,
    ) -> Assessment {
        Assessment::new(self.pool, state, reason, worst, headroom)
    }

    fn describe(&self, w: &Window) -> String {
        let mut s = format!("{} {}", w.label(), pct(w.used_at(self.view.now)));
        if let Some(at) = &w.resets_at {
            s.push_str(&format!(", resets {at}"));
        }
        s
    }

    fn full_window(&self) -> Option<Assessment> {
        let limit = self.view.policy.exhausted_used;
        let w = self.worst.filter(|w| w.used_at(self.view.now) >= limit)?;
        Some(self.out(
            State::Exhausted,
            format!("{} (>= {limit})", self.describe(w)),
            Some(w.clone()),
            self.headroom,
        ))
    }

    fn refused(&self) -> Option<Assessment> {
        let seen = self
            .r
            .refusal_seen_at
            .as_deref()
            .and_then(clock::parse)
            .filter(|_| self.pool != POOL_ZEN)
            .filter(|_| refusal_covers(self.r, self.scope))
            .filter(|seen| refusal_in_force(self.r, &self.windows, *seen, self.view.now))?;
        Some(self.out(
            State::Exhausted,
            format!("refused at {}", clock::stamp(seen)),
            self.worst.cloned(),
            Some(0.0),
        ))
    }

    fn usage_barred(&self) -> Option<Assessment> {
        (self.r.ordinary_usage_allowed == Some(false)).then(|| {
            self.out(
                State::Exhausted,
                "ordinary usage not allowed".into(),
                self.worst.cloned(),
                Some(0.0),
            )
        })
    }

    fn keyless_pool(&self) -> Option<Assessment> {
        match self.pool {
            POOL_ZEN => Some(self.zen_state()),
            POOL_LOCAL => Some(self.out(State::Ok, "pi and ollama answer".into(), None, None)),
            _ => None,
        }
    }

    fn zen_state(&self) -> Assessment {
        let r = self.r;
        let until = r
            .cooling_until
            .as_deref()
            .and_then(clock::parse)
            .filter(|u| *u > self.view.now);
        match until {
            Some(until) => self.out(
                State::Cooling,
                format!("free limit hit; cooling until {}", clock::stamp(until)),
                None,
                None,
            ),
            None => self.out(
                State::Ok,
                if r.no_signal || r.cooling_until.is_none() {
                    "no signal (no quota endpoint exists)".into()
                } else {
                    "cooldown over".into()
                },
                None,
                None,
            ),
        }
    }

    fn soft_limit(&self, scope: Option<&str>) -> Assessment {
        let now = self.view.now;
        let fresh = self.view.from_override
            || self
                .r
                .observed_at
                .as_deref()
                .and_then(clock::parse)
                .is_some_and(|t| now - t < Duration::minutes(self.view.policy.stale_after_min));
        match self.worst {
            Some(w) if fresh => self.tight(w).unwrap_or_else(|| {
                self.out(State::Ok, self.describe(w), Some(w.clone()), self.headroom)
            }),
            _ => self.out(
                State::Unknown,
                self.unknown_reason(fresh, scope),
                self.worst.cloned(),
                self.headroom,
            ),
        }
    }

    fn unknown_reason(&self, fresh: bool, scope: Option<&str>) -> String {
        let r = self.r;
        match (&r.error, &r.observed_at) {
            (Some(e), _) => e.clone(),
            (None, Some(at)) if !fresh => format!("reading from {at} is stale"),
            (None, _) if !r.windows.is_empty() && self.windows.is_empty() => {
                format!("no matching window for {}", scope.unwrap_or("this pool"))
            }
            _ => "no reading".into(),
        }
    }

    fn tight(&self, worst: &Window) -> Option<Assessment> {
        let (now, p) = (self.view.now, &self.view.policy);
        if worst.used_at(now) >= p.tight_used {
            return Some(self.out(
                State::Tight,
                format!("{} (>= {})", self.describe(worst), p.tight_used),
                Some(worst.clone()),
                self.headroom,
            ));
        }
        self.windows.iter().find_map(|w| {
            let pace = w.pace(now).filter(|pace| *pace > p.pace_factor)?;
            (w.used_at(now) >= p.pace_min_used).then(|| {
                self.out(
                    State::Tight,
                    format!("{}, pace {pace:.1}x", self.describe(w)),
                    Some((*w).clone()),
                    self.headroom,
                )
            })
        })
    }
}

fn refusal_in_force(
    r: &PoolReading,
    windows: &[&Window],
    seen: DateTime<Utc>,
    now: DateTime<Utc>,
) -> bool {
    match refused_window(r, windows).and_then(|w| Some((w.start()?, w.resets()?))) {
        Some((start, resets)) => seen >= start && resets > now,
        None => now - seen < Duration::hours(5),
    }
}

fn refusal_covers(r: &PoolReading, scope: Option<&str>) -> bool {
    match r.refusal_window.as_deref().and_then(|l| l.split_once(':')) {
        Some((_, named)) => scope == Some(named),
        None => true,
    }
}

fn refused_window<'a>(r: &PoolReading, windows: &[&'a Window]) -> Option<&'a Window> {
    let label = r.refusal_window.as_deref();
    let named = label.and_then(|label| windows.iter().find(|w| w.label() == label));
    let longest = label.map_or(u64::MAX, named_minutes);
    named
        .or_else(|| {
            windows
                .iter()
                .filter(|w| w.minutes <= longest)
                .min_by_key(|w| w.minutes)
        })
        .copied()
}

fn named_minutes(label: &str) -> u64 {
    match label.split(':').next() {
        Some("5h") => 300,
        _ => 10_080,
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Probed {
    pub windows: Vec<Window>,
    pub ordinary_usage_allowed: Option<bool>,
}

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

fn reset_stamp(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(s) => clock::parse(s).map(clock::stamp),
        Value::Number(n) => n.as_i64().and_then(clock::from_epoch).map(clock::stamp),
        _ => None,
    }
}

pub fn parse_claude_usage(reply: &Value) -> Result<Probed, String> {
    let rl = claude_rate_limits(reply)?;
    let mut out = fixed_windows(rl);
    let scoped = model_windows(rl, &out);
    out.extend(scoped);
    let rows = row_windows(rl, &out);
    out.extend(rows);
    if out.is_empty() {
        return Err("get_usage reply has no windows".into());
    }
    Ok(Probed {
        windows: out,
        ordinary_usage_allowed: None,
    })
}

fn claude_rate_limits(reply: &Value) -> Result<&Value, String> {
    if reply.pointer("/response/subtype").and_then(Value::as_str) == Some("error") {
        let e = reply
            .pointer("/response/error")
            .and_then(Value::as_str)
            .unwrap_or("error");
        return Err(format!("get_usage error: {e}"));
    }
    let holder = object_with(reply, "rate_limits").ok_or("get_usage reply has no rate_limits")?;
    if holder.get("rate_limits_available") == Some(&Value::Bool(false)) {
        return Err("rate limits are not available for this login".into());
    }
    Ok(holder.get("rate_limits").unwrap_or(&Value::Null))
}

fn pct_of(w: &Value, key: &str) -> Option<f64> {
    w.get(key).and_then(Value::as_f64).map(|u| u / 100.0)
}

fn has_scope(windows: &[Window], s: &str) -> bool {
    windows.iter().any(|w| w.scope_model.as_deref() == Some(s))
}

fn fixed_windows(rl: &Value) -> Vec<Window> {
    [
        ("five_hour", 300u64, None),
        ("seven_day", 10_080, None),
        ("seven_day_opus", 10_080, Some("opus")),
        ("seven_day_sonnet", 10_080, Some("sonnet")),
    ]
    .into_iter()
    .filter_map(|(key, minutes, scope)| {
        let w = rl.get(key).filter(|w| w.is_object())?;
        let used = pct_of(w, "utilization")?;
        Some(Window::new(
            minutes,
            used,
            reset_stamp(w.get("resets_at")),
            scope.map(str::to_owned),
        ))
    })
    .collect()
}

fn model_windows(rl: &Value, have: &[Window]) -> Vec<Window> {
    let mut out: Vec<Window> = Vec::new();
    let rows = rl.get("model_scoped").and_then(Value::as_array);
    for m in rows.into_iter().flatten() {
        let name = m.get("display_name").and_then(Value::as_str);
        let (Some(name), Some(used)) = (name, pct_of(m, "utilization")) else {
            continue;
        };
        let scope = name.to_ascii_lowercase();
        if !has_scope(have, &scope) && !has_scope(&out, &scope) {
            out.push(Window::new(
                10_080,
                used,
                reset_stamp(m.get("resets_at")),
                Some(scope),
            ));
        }
    }
    out
}

fn row_windows(rl: &Value, have: &[Window]) -> Vec<Window> {
    let mut out: Vec<Window> = Vec::new();
    let rows = rl.get("limits").and_then(Value::as_array);
    for row in rows.into_iter().flatten() {
        let all: Vec<Window> = have.iter().chain(out.iter()).cloned().collect();
        out.extend(row_window(row, &all));
    }
    out
}

fn row_window(row: &Value, all: &[Window]) -> Option<Window> {
    let used = pct_of(row, "percent")?;
    let resets = reset_stamp(row.get("resets_at"));
    let (minutes, scope) = match row.get("kind").and_then(Value::as_str) {
        Some("weekly_scoped") => (10_080, Some(row_scope(row)?)),
        Some("weekly_all") => (10_080, None),
        Some("session") => (300, None),
        _ => return None,
    };
    (!window_held(all, minutes, scope.as_deref()))
        .then(|| Window::new(minutes, used, resets, scope))
}

fn window_held(all: &[Window], minutes: u64, scope: Option<&str>) -> bool {
    match scope {
        Some(s) => has_scope(all, s),
        None if minutes == 300 => all.iter().any(|w| w.minutes == 300),
        None => all
            .iter()
            .any(|w| w.minutes == minutes && w.scope_model.is_none()),
    }
}

fn row_scope(row: &Value) -> Option<String> {
    let name = row
        .pointer("/scope/model/display_name")
        .and_then(Value::as_str)?;
    Some(name.to_ascii_lowercase())
}

pub fn parse_codex_limits(reply: &Value) -> Result<Probed, String> {
    codex_error(reply)?;
    let result = reply.get("result").ok_or("no result in the reply")?;
    let snap = result
        .get("rateLimits")
        .filter(|s| s.is_object())
        .or_else(|| result.pointer("/rateLimitsByLimitId/codex"))
        .ok_or("no rateLimits in the reply")?;
    let windows: Vec<Window> = ["primary", "secondary"]
        .iter()
        .filter_map(|slot| codex_window(snap, slot))
        .collect();
    let allowed = result.get("ordinaryUsageAllowed").and_then(Value::as_bool);
    if windows.is_empty() && allowed.is_none() {
        return Err("account/rateLimits/read reply has no windows".into());
    }
    Ok(Probed {
        windows,
        ordinary_usage_allowed: allowed,
    })
}

fn codex_error(reply: &Value) -> Result<(), String> {
    match reply.get("error") {
        Some(e) => {
            let msg = e.get("message").and_then(Value::as_str).unwrap_or("error");
            Err(format!("account/rateLimits/read error: {msg}"))
        }
        None => Ok(()),
    }
}

fn codex_window(snap: &Value, slot: &str) -> Option<Window> {
    let w = snap.get(slot).filter(|w| w.is_object())?;
    let mins = w.get("windowDurationMins").and_then(Value::as_u64)?;
    let used = w.get("usedPercent").and_then(Value::as_f64)?;
    Some(Window::new(
        mins,
        used / 100.0,
        reset_stamp(w.get("resetsAt")),
        None,
    ))
}

pub fn parse_agy_usage(reply: &Value) -> Result<Probed, String> {
    let buckets = agy_buckets(reply)?;
    let own_argon = buckets
        .iter()
        .any(|b| agy_id(b).starts_with("gemini-argon-"));
    let windows: Vec<Window> = buckets
        .iter()
        .flat_map(|b| bucket_windows(b, own_argon))
        .collect();
    if windows.is_empty() {
        return Err("agy /usage reply has no windows".into());
    }
    Ok(Probed {
        windows,
        ordinary_usage_allowed: None,
    })
}

fn agy_buckets(reply: &Value) -> Result<Vec<&Value>, String> {
    let status = reply.get("status").and_then(Value::as_str).unwrap_or("");
    if status != "SUCCESS" {
        let why = reply
            .get("response")
            .and_then(Value::as_str)
            .and_then(|r| r.lines().find(|l| !l.trim().is_empty()))
            .unwrap_or("no status");
        return Err(format!("agy /usage status {status:?}: {why}"));
    }
    let groups = reply
        .pointer("/command/data/groups")
        .and_then(Value::as_array)
        .ok_or("agy /usage reply has no command.data.groups")?;
    Ok(groups
        .iter()
        .filter_map(|g| g.get("buckets").and_then(Value::as_array))
        .flatten()
        .collect())
}

fn agy_id(b: &Value) -> &str {
    b.get("id").and_then(Value::as_str).unwrap_or("")
}

fn bucket_windows(b: &Value, own_argon: bool) -> Vec<Window> {
    let Some((minutes, used)) = agy_bucket(b) else {
        return Vec::new();
    };
    let resets = reset_stamp(b.get("reset_time"));
    let scopes = agy_scopes(agy_id(b), own_argon);
    if scopes.is_empty() {
        return vec![Window::new(minutes, used, resets, None)];
    }
    scopes
        .iter()
        .map(|s| Window::new(minutes, used, resets.clone(), Some(s.to_string())))
        .collect()
}

fn agy_bucket(b: &Value) -> Option<(u64, f64)> {
    let minutes = match b.get("window").and_then(Value::as_str) {
        Some("weekly") => 10_080,
        Some("5h") => 300,
        _ => return None,
    };
    let left = b.get("remaining_fraction").and_then(Value::as_f64)?;
    Some((minutes, 1.0 - left))
}

fn agy_scopes(id: &str, own_argon: bool) -> &'static [&'static str] {
    if id.starts_with("gemini-argon-") {
        &["argon"]
    } else if id.starts_with("gemini-") {
        if own_argon {
            &["gemini"]
        } else {
            &["gemini", "argon"]
        }
    } else if id.starts_with("3p-") {
        &["opus", "sonnet", "gpt-oss"]
    } else {
        &[]
    }
}

pub(crate) fn last_probe(file: &QuotaFile) -> Option<DateTime<Utc>> {
    file.pools
        .values()
        .filter_map(|r| r.probed_at.as_deref().and_then(clock::parse))
        .max()
}

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
        let rl: Value = serde_json::json!({"primary":{"used_percent":10.0,"window_minutes":300,"resets_at":1789473600},
                                           "secondary":{"used_percent":45.0,"window_minutes":10080,"resets_at":1789920000}});
        let w = crate::telemetry::readers::codex_rollout_windows(&rl);
        assert_eq!((w[0].name.as_str(), w[1].name.as_str()), ("5h", "7d"));
        assert_eq!(window_name(60), "60m");
        let rl: Value = serde_json::json!({"limit_id":"codex","limit_name":null,
            "primary":{"used_percent":0.0,"window_minutes":10080,"resets_at":1791949554},
            "secondary":null,"credits":{"has_credits":false,"unlimited":false,"balance":"0"}});
        let w = crate::telemetry::readers::codex_rollout_windows(&rl);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].name, "7d");
        let p = parse_codex_limits(&serde_json::json!({"result":{"rateLimits":{
            "primary":{"usedPercent":43.0,"windowDurationMins":10080,"resetsAt":1791595230},
            "secondary":null}}}))
        .unwrap();
        assert_eq!(p.windows.len(), 1);
        assert_eq!(p.windows[0].name, "7d");
    }

    #[test]
    fn quo_04_units_normalized() {
        let p =
            parse_claude_usage(&last_json_line("probes/claude-get-usage-exhausted.jsonl")).unwrap();
        let seven = p
            .windows
            .iter()
            .find(|w| w.name == "7d" && w.scope_model.is_none())
            .unwrap();
        assert_eq!(seven.used, 1.0);
        assert_eq!(seven.resets_at.as_deref(), Some("2026-10-02T13:59:59Z"));
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
            ("all-exhausted", "codex", "gpt-6.2-sol", State::Exhausted),
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
                "gpt-6.2-sol",
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
        let half = "2026-10-02T06:00:00Z";
        assert_eq!(
            one_window(0.60, half).assess("claude", "opus").state,
            State::Ok
        );
        assert_eq!(
            one_window(0.63, half).assess("claude", "opus").state,
            State::Tight
        );
        assert_eq!(
            one_window(0.45, "2026-10-05T06:00:00Z")
                .assess("claude", "opus")
                .state,
            State::Ok
        );
        assert_eq!(
            one_window(1.0, "2026-09-28T12:00:00Z")
                .assess("claude", "opus")
                .state,
            State::Ok
        );
        let mut v = one_window(0.80, late);
        v.policy = Policy::parse(r#"{"tight_used": 0.75}"#).unwrap();
        assert_eq!(v.assess("claude", "opus").state, State::Tight);
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

    fn refusal_holds(seen: &str, named: Option<&str>, windows: Vec<Window>) -> bool {
        let mut file = QuotaFile::default();
        file.pools.insert(
            POOL_CLAUDE.into(),
            PoolReading {
                observed_at: Some("2026-09-28T17:55:00Z".into()),
                windows,
                refusal_seen_at: Some(seen.into()),
                refusal_window: named.map(str::to_string),
                ..Default::default()
            },
        );
        let a = QuotaView::new(file, now(), Policy::default(), false).assess("claude", "opus");
        if a.state == State::Exhausted {
            assert_eq!(a.reason, format!("refused at {seen}"));
            assert_eq!(a.headroom_per_h, Some(0.0));
        }
        a.state == State::Exhausted
    }

    fn five_h() -> Window {
        Window::new(300, 0.1, Some("2026-09-28T19:00:00Z".into()), None)
    }

    fn seven_d() -> Window {
        Window::new(10_080, 0.1, Some("2026-10-02T00:00:00Z".into()), None)
    }

    #[test]
    fn a_refusal_without_a_window_is_held_to_the_shortest_window() {
        let both = || vec![seven_d(), five_h()];
        assert!(refusal_holds("2026-09-28T17:00:00Z", None, both()));
        assert!(!refusal_holds("2026-09-28T13:00:00Z", None, both()));
        assert!(refusal_holds("2026-09-28T14:00:00Z", None, both()));
        assert!(!refusal_holds("2026-09-28T13:59:59Z", None, both()));
    }

    #[test]
    fn a_refusal_is_held_to_the_window_it_named() {
        let both = || vec![five_h(), seven_d()];
        assert!(refusal_holds("2026-09-28T13:00:00Z", Some("7d"), both()));
        assert!(!refusal_holds("2026-09-28T13:00:00Z", Some("5h"), both()));
        assert!(!refusal_holds(
            "2026-09-28T13:30:00Z",
            Some("7d:opus"),
            both()
        ));
        assert!(refusal_holds(
            "2026-09-28T17:00:00Z",
            Some("7d:opus"),
            both()
        ));
        let opus = Window::new(
            10_080,
            0.1,
            Some("2026-10-05T00:00:00Z".into()),
            Some("opus".into()),
        );
        let seen = "2026-09-28T13:00:00Z";
        assert!(refusal_holds(
            seen,
            Some("7d:opus"),
            vec![five_h(), opus.clone()]
        ));
        assert!(!refusal_holds(seen, Some("7d"), vec![five_h(), opus]));
    }

    #[test]
    fn a_refusal_ends_when_its_window_resets() {
        let resets = |at: &str| vec![Window::new(300, 0.1, Some(at.into()), None)];
        let seen = "2026-09-28T17:00:00Z";
        assert!(refusal_holds(seen, None, resets("2026-09-28T18:00:01Z")));
        assert!(!refusal_holds(seen, None, resets("2026-09-28T18:00:00Z")));
        assert!(!refusal_holds(seen, None, resets("2026-09-28T17:30:00Z")));
    }

    #[test]
    fn a_refusal_with_no_window_to_read_counts_for_5_hours() {
        let readings: [fn() -> Vec<Window>; 2] =
            [Vec::new, || vec![Window::new(300, 0.1, None, None)]];
        for windows in readings {
            assert!(refusal_holds("2026-09-28T13:00:01Z", None, windows()));
            assert!(!refusal_holds("2026-09-28T13:00:00Z", None, windows()));
            assert!(!refusal_holds("2026-09-28T12:00:00Z", None, windows()));
            assert!(refusal_holds("2026-09-28T17:59:00Z", Some("5h"), windows()));
        }
    }

    #[test]
    fn a_5h_refusal_with_only_a_7d_reading_counts_for_5_hours() {
        let windows = || vec![seven_d()];
        assert!(refusal_holds("2026-09-28T13:00:01Z", Some("5h"), windows()));
        assert!(!refusal_holds(
            "2026-09-28T13:00:00Z",
            Some("5h"),
            windows()
        ));
        assert!(!refusal_holds(
            "2026-09-27T12:00:00Z",
            Some("5h"),
            windows()
        ));
    }

    #[test]
    fn an_unnamed_refusal_with_no_reading_counts_for_5_hours() {
        assert!(refusal_holds("2026-09-28T13:00:01Z", None, Vec::new()));
        assert!(!refusal_holds("2026-09-28T13:00:00Z", None, Vec::new()));
    }

    #[test]
    fn a_5h_refusal_with_a_5h_reading_counts_until_the_5h_reset() {
        let resets_at = |at: &str| vec![Window::new(300, 0.1, Some(at.into()), None)];
        let seen = "2026-09-28T14:30:00Z";
        assert!(refusal_holds(
            seen,
            Some("5h"),
            resets_at("2026-09-28T19:00:00Z")
        ));
        assert!(!refusal_holds(
            seen,
            Some("5h"),
            resets_at("2026-09-28T18:00:00Z")
        ));
    }

    #[test]
    fn a_scoped_refusal_blocks_only_its_scope() {
        let mut file = QuotaFile::default();
        file.pools.insert(
            POOL_CLAUDE.into(),
            PoolReading {
                observed_at: Some("2026-09-28T17:55:00Z".into()),
                windows: vec![five_h(), seven_d()],
                refusal_seen_at: Some("2026-09-28T17:00:00Z".into()),
                refusal_window: Some("7d:fable".into()),
                ..Default::default()
            },
        );
        let v = QuotaView::new(file, now(), Policy::default(), false);
        assert_eq!(
            v.assess("claude", "claude-fable-5-2").state,
            State::Exhausted
        );
        assert_ne!(v.assess("claude", "opus").state, State::Exhausted);
        assert_ne!(v.assess_shown(POOL_CLAUDE).state, State::Exhausted);
    }

    #[test]
    fn a_refusal_is_held_to_the_first_of_two_equally_short_windows() {
        let opus = Window::new(
            10_080,
            0.1,
            Some("2026-10-05T00:00:00Z".into()),
            Some("opus".into()),
        );
        let seen = "2026-09-27T12:00:00Z";
        assert!(refusal_holds(seen, None, vec![seven_d(), opus.clone()]));
        assert!(!refusal_holds(seen, None, vec![opus, seven_d()]));
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
        assert_eq!(pool_for("codex", "gpt-6.2-sol"), POOL_CODEX);
        assert_eq!(pool_for("opencode", "opencode/big-pickle"), POOL_ZEN);
        assert_eq!(pool_for("antigravity", "gemini-3.8-flash"), POOL_GOOGLE);
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

    #[test]
    fn a_broken_harness_is_broken_whatever_its_pool() {
        let raw = std::fs::read_to_string(fixture("quota/all-ok.json")).unwrap();
        assert!(!raw.contains("harnesses"));
        let mut file = QuotaFile::read(&fixture("quota/all-ok.json")).unwrap();
        assert!(file.harnesses.is_empty());
        let v = QuotaView::new(file.clone(), now(), Policy::default(), true);
        assert_eq!(
            v.assess("pi", "anthropic/claude-sonnet-5-5").state,
            State::Ok
        );

        let why = "pi --version exited 1: Error [ERR_REQUIRE_ESM]: require() of ES Module";
        file.harnesses.insert(
            "pi".into(),
            HarnessHealth {
                version: Some("0.70.0".into()),
                error: Some(why.into()),
                probed_at: Some("2026-09-28T17:55:00Z".into()),
            },
        );
        let v = QuotaView::new(file.clone(), now(), Policy::default(), true);
        for model in ["anthropic/claude-sonnet-5-5", "ollama/qwen3.8"] {
            let a = v.assess("pi", model);
            assert_eq!(a.state, State::Broken, "pi on {model}");
            assert_eq!(a.reason, why);
        }
        assert_eq!(v.assess("claude", "claude-sonnet-5-5").state, State::Ok);
        let text = serde_json::to_string(&file).unwrap();
        let back: QuotaFile = serde_json::from_str(&text).unwrap();
        assert_eq!(back.harnesses, file.harnesses);
        assert!(!serde_json::to_string(&QuotaFile::default())
            .unwrap()
            .contains("harnesses"));

        file.harnesses.get_mut("pi").unwrap().error = None;
        let v = QuotaView::new(file, now(), Policy::default(), true);
        assert_eq!(
            v.assess("pi", "anthropic/claude-sonnet-5-5").state,
            State::Ok
        );
    }

    fn agy_usage(name: &str) -> Value {
        serde_json::from_str(
            &std::fs::read_to_string(fixture(&format!("probes/agy-usage-{name}.json"))).unwrap(),
        )
        .unwrap()
    }

    fn google_view(windows: Vec<Window>) -> QuotaView {
        let mut file = QuotaFile::default();
        file.pools.insert(
            POOL_GOOGLE.into(),
            PoolReading {
                windows,
                observed_at: Some(clock::stamp(now())),
                ..Default::default()
            },
        );
        QuotaView::new(file, now(), Policy::default(), false)
    }

    #[test]
    fn agy_usage_scopes_each_group_to_its_models() {
        let p = parse_agy_usage(&agy_usage("pro")).unwrap();
        let labels: Vec<String> = p.windows.iter().map(Window::label).collect();
        assert_eq!(
            labels,
            [
                "7d:gemini",
                "5h:gemini",
                "7d:opus",
                "7d:sonnet",
                "7d:gpt-oss",
                "5h:opus",
                "5h:sonnet",
                "5h:gpt-oss",
                "7d:argon",
            ]
        );
        let argon = p.windows.last().unwrap();
        assert_eq!(argon.used, 1.0);
        assert_eq!(argon.resets_at.as_deref(), Some("2026-10-27T18:48:25Z"));
        let v = google_view(p.windows);
        assert_eq!(
            v.assess("antigravity", "gemini-4-argon").state,
            State::Exhausted
        );
        assert_eq!(v.assess("antigravity", "gemini-3.8-flash").state, State::Ok);
        assert_eq!(v.assess("antigravity", "claude-opus-5-5").state, State::Ok);
        assert_eq!(v.assess_shown(POOL_GOOGLE).state, State::Ok);
        assert_eq!(v.assess_shown(POOL_CLAUDE).state, State::Unknown);
    }

    #[test]
    fn agy_usage_without_an_argon_group_limits_argon_by_gemini() {
        let p = parse_agy_usage(&agy_usage("ultra")).unwrap();
        let v = google_view(p.windows);
        for model in ["gemini-4-argon", "gemini-3.8-flash"] {
            assert_eq!(
                v.assess("antigravity", model).state,
                State::Exhausted,
                "{model}"
            );
        }
        assert_eq!(
            v.assess("antigravity", "claude-sonnet-5-5").state,
            State::Ok
        );
    }

    #[test]
    fn agy_usage_errors_are_errors() {
        let e = parse_agy_usage(&agy_usage("error")).unwrap_err();
        assert!(e.contains("You are not signed in"), "{e}");
        assert!(parse_agy_usage(&serde_json::json!({"status": "SUCCESS"})).is_err());
        assert!(parse_agy_usage(
            &serde_json::json!({"status": "SUCCESS", "command": {"data": {"groups": []}}})
        )
        .is_err());
        let odd = serde_json::json!({"status": "SUCCESS", "command": {"data": {"groups": [
            {"buckets": [{"id": "new-bucket", "window": "5h", "remaining_fraction": 0.5}]}]}}});
        assert_eq!(parse_agy_usage(&odd).unwrap().windows[0].scope_model, None);
    }

    #[test]
    fn agy_model_families() {
        assert_eq!(model_family("gemini-4-argon"), Some("argon"));
        assert_eq!(model_family("gemini-3.8-flash"), Some("gemini"));
        assert_eq!(model_family("gpt-oss-120b"), Some("gpt-oss"));
        assert_eq!(model_family("claude-opus-5-5"), Some("opus"));
    }
}
