use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tokens {
    pub input: u64,
    pub cache_write_5m: u64,
    pub cache_write_1h: u64,
    pub cache_read: u64,
    pub output: u64,
}

impl Tokens {
    pub fn add(&mut self, other: &Tokens) {
        self.input += other.input;
        self.cache_write_5m += other.cache_write_5m;
        self.cache_write_1h += other.cache_write_1h;
        self.cache_read += other.cache_read;
        self.output += other.output;
    }

    pub fn total(&self) -> u64 {
        self.input + self.cache_write_5m + self.cache_write_1h + self.cache_read + self.output
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub by_model: BTreeMap<String, Tokens>,
    pub calls: u64,
    pub skills: BTreeMap<String, u64>,
}

impl Usage {
    pub fn tokens(&self) -> Tokens {
        let mut t = Tokens::default();
        for v in self.by_model.values() {
            t.add(v);
        }
        t
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Price {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    #[serde(default)]
    pub cache_write_5m: Option<f64>,
    #[serde(default)]
    pub cache_write_1h: Option<f64>,
}

impl Price {
    const fn new(input: f64, output: f64, cache_read: f64) -> Self {
        Price {
            input,
            output,
            cache_read,
            cache_write_5m: None,
            cache_write_1h: None,
        }
    }

    pub fn cost(&self, t: &Tokens) -> f64 {
        let w5 = self.cache_write_5m.unwrap_or(self.input * 1.25);
        let w1h = self.cache_write_1h.unwrap_or(self.input * 2.0);
        (t.input as f64 * self.input
            + t.cache_write_5m as f64 * w5
            + t.cache_write_1h as f64 * w1h
            + t.cache_read as f64 * self.cache_read
            + t.output as f64 * self.output)
            / 1_000_000.0
    }
}

pub fn builtin_prices() -> BTreeMap<String, Price> {
    let mut m = BTreeMap::new();
    m.insert("claude-fable-5-2".into(), Price::new(8.0, 40.0, 0.20));
    m.insert("claude-fable-5-1".into(), Price::new(10.0, 50.0, 0.25));
    m.insert("claude-opus-5-5".into(), Price::new(4.0, 20.0, 0.20));
    m.insert("claude-opus-5".into(), Price::new(5.0, 25.0, 0.50));
    m.insert("claude-sonnet-5-5".into(), Price::new(2.0, 10.0, 0.20));
    m.insert("claude-sonnet-5".into(), Price::new(2.0, 10.0, 0.20));
    m.insert("claude-haiku-4-5".into(), Price::new(1.0, 5.0, 0.10));
    m.insert("gpt-6-astra".into(), Price::new(10.0, 50.0, 1.0));
    m.insert("gpt-5.6-sol".into(), Price::new(4.0, 20.0, 0.40));
    m.insert("gpt-5.6-terra".into(), Price::new(2.0, 12.0, 0.20));
    m.insert("gpt-5.6-luna".into(), Price::new(0.20, 1.20, 0.02));
    let gemini_pro = Price {
        cache_write_5m: Some(2.0),
        cache_write_1h: Some(2.0),
        ..Price::new(2.0, 12.0, 0.20)
    };
    for id in [
        "gemini-3.1-pro-preview",
        "gemini-3.1-pro-high",
        "gemini-3.1-pro-low",
    ] {
        m.insert(id.into(), gemini_pro);
    }
    let gemini_flash = Price {
        cache_write_5m: Some(0.75),
        cache_write_1h: Some(0.75),
        ..Price::new(0.75, 3.75, 0.075)
    };
    for id in [
        "gemini-3.8-flash",
        "gemini-3.8-flash-high",
        "gemini-3.8-flash-medium",
        "gemini-3.8-flash-low",
    ] {
        m.insert(id.into(), gemini_flash);
    }
    let gemini_argon = Price {
        cache_write_5m: Some(2.0),
        cache_write_1h: Some(2.0),
        ..Price::new(2.0, 10.0, 0.10)
    };
    for id in [
        "gemini-4-argon",
        "gemini-4-argon-high",
        "gemini-4-argon-medium",
        "gemini-4-argon-low",
    ] {
        m.insert(id.into(), gemini_argon);
    }
    let gemini_neon = Price {
        cache_write_5m: Some(3.0),
        cache_write_1h: Some(3.0),
        ..Price::new(3.0, 15.0, 0.15)
    };
    for id in [
        "gemini-4-neon",
        "gemini-4-neon-xhigh",
        "gemini-4-neon-high",
        "gemini-4-neon-medium",
        "gemini-4-neon-low",
    ] {
        m.insert(id.into(), gemini_neon);
    }
    m
}

pub fn load_prices(path: Option<&Path>) -> Result<BTreeMap<String, Price>> {
    let mut prices = builtin_prices();
    if let Some(path) = path {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading pricing file {}", path.display()))?;
        let extra: BTreeMap<String, Price> = serde_json::from_str(&text)
            .with_context(|| format!("parsing pricing file {}", path.display()))?;
        prices.extend(extra);
    }
    Ok(prices)
}

pub(crate) fn canonical_model(model: &str) -> String {
    let lowered = model.trim().to_ascii_lowercase();
    let m = strip_model_noise(&lowered);
    let alias = MODEL_ALIASES.iter().find(|(name, _)| *name == m);
    alias.map_or(m, |(_, key)| key).to_string()
}

const MODEL_ALIASES: [(&str, &str); 4] = [
    ("fable", "claude-fable-5-2"),
    ("opus", "claude-opus-5-5"),
    ("sonnet", "claude-sonnet-5-5"),
    ("haiku", "claude-haiku-4-5"),
];

fn strip_model_noise(m: &str) -> &str {
    let m = m.strip_suffix("[1m]").unwrap_or(m);
    let m = ["anthropic/", "openai/"]
        .iter()
        .find_map(|prefix| m.strip_prefix(prefix))
        .unwrap_or(m);
    match m.rsplit_once('-') {
        Some((head, tail)) if tail.len() == 8 && tail.as_bytes().iter().all(u8::is_ascii_digit) => {
            head
        }
        _ => m,
    }
}

pub(crate) fn is_free(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    m.starts_with("ollama/")
        || (m.starts_with("opencode/") && (m.ends_with("-free") || m == "opencode/big-pickle"))
}

pub fn price_for(prices: &BTreeMap<String, Price>, model: &str) -> Option<Price> {
    if is_free(model) {
        return Some(Price::new(0.0, 0.0, 0.0));
    }
    prices
        .get(model)
        .or_else(|| prices.get(&canonical_model(model)))
        .copied()
}

pub fn cost_of(prices: &BTreeMap<String, Price>, model: &str, tokens: &Tokens) -> Option<f64> {
    price_for(prices, model).map(|p| p.cost(tokens))
}

pub(crate) fn find_claude_transcript(projects: &Path, session_id: &str) -> Option<PathBuf> {
    let file = format!("{session_id}.jsonl");
    for entry in std::fs::read_dir(projects).ok()?.flatten() {
        let candidate = entry.path().join(&file);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub(crate) fn find_codex_rollout(sessions_dir: &Path, session_id: &str) -> Option<PathBuf> {
    find_file(sessions_dir, &|name| {
        name.starts_with("rollout-") && name.ends_with(&format!("{session_id}.jsonl"))
    })
}

pub(crate) fn find_pi_session(sessions_dir: &Path, session_id: &str) -> Option<PathBuf> {
    let direct = PathBuf::from(session_id);
    if direct.is_file() {
        return Some(direct);
    }
    find_file(sessions_dir, &|name| {
        name.ends_with(".jsonl") && name.contains(session_id)
    })
}

fn find_file(dir: &Path, matches: &dyn Fn(&str) -> bool) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    entries.flatten().map(|e| e.path()).find_map(|path| {
        let name = path.file_name().and_then(|n| n.to_str());
        match path.is_dir() {
            true => find_file(&path, matches),
            false => name.is_some_and(matches).then_some(path),
        }
    })
}

#[cfg(test)]
fn u(v: &Value, keys: &[&str]) -> u64 {
    keys.iter()
        .find_map(|k| v.get(*k).and_then(Value::as_u64))
        .unwrap_or(0)
}

#[cfg(test)]
pub(crate) fn read_claude(text: &str) -> Usage {
    let mut last: BTreeMap<String, (String, Tokens)> = BTreeMap::new();
    for line in text.lines() {
        let Ok(record) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if record.get("type").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(message) = record.get("message") else {
            continue;
        };
        let model = message.get("model").and_then(Value::as_str).unwrap_or("");
        if model == "<synthetic>" {
            continue;
        }
        let (Some(id), Some(usage)) = (
            message.get("id").and_then(Value::as_str),
            message.get("usage"),
        ) else {
            continue;
        };
        let written = u(usage, &["cache_creation_input_tokens"]);
        let one_hour = usage
            .pointer("/cache_creation/ephemeral_1h_input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .min(written);
        let tokens = Tokens {
            input: u(usage, &["input_tokens"]),
            cache_write_5m: written - one_hour,
            cache_write_1h: one_hour,
            cache_read: u(usage, &["cache_read_input_tokens"]),
            output: u(usage, &["output_tokens"]),
        };
        last.insert(id.to_string(), (model.to_string(), tokens));
    }
    let mut usage = Usage {
        calls: last.len() as u64,
        ..Usage::default()
    };
    for (model, tokens) in last.into_values() {
        usage.by_model.entry(model).or_default().add(&tokens);
    }
    usage.skills = count_skills("claude", text);
    usage
}

#[cfg(test)]
fn count_skills(agent: &str, text: &str) -> BTreeMap<String, u64> {
    let mut out = BTreeMap::new();
    for load in skill_loads_in(agent, text) {
        *out.entry(load.skill).or_default() += 1;
    }
    out
}

fn line_ts(record: &Value) -> String {
    match record.get("timestamp").and_then(Value::as_str) {
        Some(t) => crate::telemetry::readers::ts(Some(t)),
        None => record
            .pointer("/message/timestamp")
            .and_then(Value::as_i64)
            .and_then(crate::clock::from_epoch_ms)
            .map(crate::clock::stamp)
            .unwrap_or_default(),
    }
}

pub(crate) fn skill_loads_in(agent: &str, text: &str) -> Vec<SkillLoad> {
    let read = skill_reader(agent);
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for line in text.lines() {
        let Ok(record) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let ts = line_ts(&record);
        for skill in read(&record, line, &mut seen) {
            out.push(SkillLoad {
                ts: ts.clone(),
                skill,
            });
        }
    }
    out
}

type SkillReader = fn(&Value, &str, &mut BTreeSet<String>) -> Vec<String>;

fn skill_reader(agent: &str) -> SkillReader {
    match agent {
        "claude" => claude_skills,
        "codex" => codex_skills,
        "pi" | "prime" => pi_skills,
        _ => no_skills,
    }
}

fn no_skills(_: &Value, _: &str, _: &mut BTreeSet<String>) -> Vec<String> {
    Vec::new()
}

fn claude_skills(record: &Value, _line: &str, seen: &mut BTreeSet<String>) -> Vec<String> {
    let mut out = Vec::new();
    if record.get("type").and_then(Value::as_str) != Some("assistant") {
        return out;
    }
    let blocks = record.pointer("/message/content").and_then(Value::as_array);
    for block in blocks.into_iter().flatten() {
        let id = block.get("id").and_then(Value::as_str).unwrap_or_default();
        if let Some(skill) = skill_call(block).filter(|_| seen.insert(id.to_string())) {
            out.push(skill.to_string());
        }
    }
    out
}

fn skill_call(block: &Value) -> Option<&str> {
    let tool = block.get("name").and_then(Value::as_str);
    let call = block.get("type").and_then(Value::as_str) == Some("tool_use");
    let skill = block.pointer("/input/skill").and_then(Value::as_str);
    if call && tool == Some("Skill") {
        skill
    } else {
        None
    }
}

fn codex_skills(record: &Value, line: &str, _seen: &mut BTreeSet<String>) -> Vec<String> {
    let kind = record.pointer("/payload/type").and_then(Value::as_str);
    match kind {
        Some("function_call" | "local_shell_call" | "custom_tool_call") => skill_reads(line),
        _ => Vec::new(),
    }
}

fn pi_skills(record: &Value, line: &str, _seen: &mut BTreeSet<String>) -> Vec<String> {
    let message = record.get("message").unwrap_or(record);
    match message.get("role").and_then(Value::as_str) {
        Some("assistant") => skill_reads(line),
        _ => Vec::new(),
    }
}

fn skill_reads(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("/SKILL.md") {
        let before = &rest[..at];
        let name: String = before
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_' || *c == ':')
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        if !name.is_empty() && before[..before.len() - name.len()].ends_with("skills/") {
            out.push(name);
        }
        rest = &rest[at + "/SKILL.md".len()..];
    }
    out
}

#[cfg(test)]
fn find_key<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Object(map) => map
            .get(key)
            .or_else(|| map.values().find_map(|x| find_key(x, key))),
        Value::Array(list) => list.iter().find_map(|x| find_key(x, key)),
        _ => None,
    }
}

#[cfg(test)]
pub(crate) fn read_codex(text: &str) -> Usage {
    let mut total: Option<Value> = None;
    let mut model = String::new();
    let mut turns: BTreeSet<u64> = BTreeSet::new();
    let mut usage = Usage::default();
    for line in text.lines() {
        let Ok(record) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let Some(m) = record
            .pointer("/payload/model")
            .or_else(|| record.get("model"))
            .and_then(Value::as_str)
        {
            model = m.to_string();
        }
        if let Some(t) = find_key(&record, "total_token_usage") {
            turns.insert(u(t, &["total_tokens"]));
            total = Some(t.clone());
        }
    }
    usage.skills = count_skills("codex", text);
    if let Some(t) = total {
        let input = u(&t, &["input_tokens"]);
        let cached = u(&t, &["cached_input_tokens"]).min(input);
        usage.by_model.insert(
            model,
            Tokens {
                input: input - cached,
                cache_read: cached,
                output: u(&t, &["output_tokens"]),
                ..Tokens::default()
            },
        );
        usage.calls = turns.into_iter().filter(|n| *n > 0).count() as u64;
    }
    usage
}

#[cfg(test)]
pub(crate) fn read_pi(text: &str) -> Usage {
    let mut usage = Usage::default();
    for line in text.lines() {
        let Ok(record) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let message = record.get("message").unwrap_or(&record);
        let Some(u_) = message.get("usage").filter(|v| v.is_object()) else {
            continue;
        };
        let model = message
            .get("model")
            .or_else(|| record.get("model"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let provider = message
            .get("provider")
            .and_then(Value::as_str)
            .map(|p| format!("{p}/"))
            .unwrap_or_default();
        let model = if model.contains('/') || provider.is_empty() {
            model.to_string()
        } else {
            format!("{provider}{model}")
        };
        let tokens = Tokens {
            input: u(u_, &["input", "input_tokens"]),
            cache_write_5m: u(u_, &["cacheWrite", "cache_creation_input_tokens"]),
            cache_write_1h: 0,
            cache_read: u(u_, &["cacheRead", "cache_read_input_tokens"]),
            output: u(u_, &["output", "output_tokens"]),
        };
        if tokens.total() == 0 {
            continue;
        }
        usage.calls += 1;
        usage.by_model.entry(model).or_default().add(&tokens);
    }
    usage.skills = count_skills("pi", text);
    usage
}

#[derive(Debug, Clone)]
pub struct Locations {
    pub home: PathBuf,
    pub claude_projects: PathBuf,
    pub codex_sessions: PathBuf,
    pub pi_sessions: PathBuf,
    pub opencode_db: PathBuf,
    pub sqlite3: PathBuf,
}

impl Locations {
    pub fn from_context(ctx: &crate::runtime::RuntimeContext) -> Self {
        Self {
            sqlite3: ctx.bins.harness.sqlite3.clone(),
            ..Self::under_home(&ctx.paths.home, &ctx.inherited)
        }
    }

    pub fn under_home(home: &Path, inherited: &crate::runtime::Inherited) -> Self {
        let codex_sessions =
            crate::harness::codex::codex_home(home, inherited.codex_home.as_deref())
                .join("sessions");
        let pi_sessions = inherited
            .pi_session_dir
            .clone()
            .unwrap_or_else(|| home.join(".pi/agent/sessions"));
        let opencode_db = inherited.opencode_db.clone().unwrap_or_else(|| {
            inherited
                .xdg_data_home
                .clone()
                .unwrap_or_else(|| home.join(".local/share"))
                .join("opencode/opencode.db")
        });
        Locations {
            home: home.to_path_buf(),
            claude_projects: home.join(".claude/projects"),
            codex_sessions,
            pi_sessions,
            opencode_db,
            sqlite3: PathBuf::from("sqlite3"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Missing {
    NoSessionId,
    NoTranscript,
    NotRead,
    Failed(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Span {
    pub since: Option<String>,
    pub until: Option<String>,
}

impl Span {
    pub fn new(since: Option<&str>, until: Option<&str>) -> Self {
        Span {
            since: since.map(str::to_string),
            until: until.map(str::to_string),
        }
    }

    pub fn contains(&self, ts: &str) -> bool {
        self.since.as_deref().is_none_or(|s| ts >= s)
            && self.until.as_deref().is_none_or(|u| ts < u)
    }

    pub fn is_all(&self) -> bool {
        self.since.is_none() && self.until.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillLoad {
    pub ts: String,
    pub skill: String,
}

#[derive(Debug, Clone)]
pub struct Session {
    pub path: PathBuf,
    pub events: Vec<crate::telemetry::RawUsage>,
    pub skill_loads: Vec<SkillLoad>,
}

impl Session {
    pub fn usage(&self, span: &Span) -> Usage {
        let mut usage = Usage::default();
        for e in self.events.iter().filter(|e| span.contains(&e.ts)) {
            usage
                .by_model
                .entry(e.model.clone())
                .or_default()
                .add(&e.tokens.priced());
            if !e.delta {
                usage.calls += 1;
            }
        }
        for load in self.skill_loads.iter().filter(|l| span.contains(&l.ts)) {
            *usage.skills.entry(load.skill.clone()).or_default() += 1;
        }
        usage
    }
}

pub fn has_transcript(kind: crate::harness::HarnessKind) -> bool {
    use crate::harness::HarnessKind as K;
    matches!(kind, K::Claude | K::Codex | K::OpenCode | K::Pi | K::Prime)
}

pub fn read_session_events(
    loc: &Locations,
    agent: &str,
    session_id: Option<&str>,
) -> std::result::Result<Session, Missing> {
    let (events, files) =
        crate::telemetry::readers::read_whole(loc, agent, session_id).map_err(missing_for)?;
    let mut skill_loads = Vec::new();
    for file in &files {
        if let Ok(text) = std::fs::read_to_string(file) {
            skill_loads.extend(skill_loads_in(agent, &text));
        }
    }
    let path = files.into_iter().next().ok_or(Missing::NoTranscript)?;
    Ok(Session {
        path,
        events,
        skill_loads,
    })
}

fn missing_for(why: crate::telemetry::readers::Unreadable) -> Missing {
    use crate::telemetry::readers::Unreadable;
    match why {
        Unreadable::NoSessionId => Missing::NoSessionId,
        Unreadable::NoTranscript(_) => Missing::NoTranscript,
        Unreadable::NotRead(_) => Missing::NotRead,
        Unreadable::Failed(why) => Missing::Failed(why),
    }
}

pub fn read_session(
    loc: &Locations,
    agent: &str,
    session_id: Option<&str>,
) -> std::result::Result<(PathBuf, Usage), Missing> {
    let session = read_session_events(loc, agent, session_id)?;
    let usage = session.usage(&Span::default());
    Ok((session.path, usage))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/usage")
            .join(name);
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    #[test]
    fn claude_dedups_streamed_messages_and_splits_cache_ttls() {
        let usage = read_claude(&fixture("claude.jsonl"));
        assert_eq!(usage.calls, 2, "two message ids, four records");
        let t = usage.by_model["claude-opus-5-5"];
        assert_eq!(
            t,
            Tokens {
                input: 10 + 5,
                cache_write_5m: 1000,
                cache_write_1h: 400,
                cache_read: 20_000 + 30_000,
                output: 300 + 50,
            }
        );
        assert!(!usage.by_model.contains_key("<synthetic>"));
        assert_eq!(
            usage.skills.get("horch:tdd"),
            Some(&1),
            "{:?}",
            usage.skills
        );
        assert_eq!(usage.skills.len(), 1, "a repeated record is one load");

        let prices = builtin_prices();
        let cost = price_for(&prices, "claude-opus-5-5").unwrap().cost(&t);
        let expected =
            (15.0 * 4.0 + 1000.0 * 5.0 + 400.0 * 8.0 + 50_000.0 * 0.2 + 350.0 * 20.0) / 1_000_000.0;
        assert!((cost - expected).abs() < 1e-12, "{cost} vs {expected}");
    }

    #[test]
    fn codex_takes_the_last_running_total_and_splits_cached_input() {
        let usage = read_codex(&fixture("codex.jsonl"));
        assert_eq!(
            usage.by_model["gpt-5.6-sol"],
            Tokens {
                input: 3000 - 2000,
                cache_read: 2000,
                output: 700,
                ..Tokens::default()
            }
        );
        assert_eq!(usage.calls, 2);
        assert_eq!(
            usage.skills.get("code-review"),
            Some(&1),
            "{:?}",
            usage.skills
        );
        assert!(
            !usage.skills.contains_key("tdd"),
            "a SKILL.md path in the prompt is not a load"
        );
        let newer = read_codex(&fixture("codex-new.jsonl"));
        assert_eq!(newer.tokens().output, 90, "{newer:?}");
    }

    #[test]
    fn pi_reads_both_usage_spellings() {
        let usage = read_pi(&fixture("pi.jsonl"));
        assert_eq!(usage.calls, 2);
        let t = usage.tokens();
        assert_eq!(
            (t.input, t.output, t.cache_read, t.cache_write_5m),
            (150, 60, 500, 40)
        );
        assert!(usage.by_model.contains_key("ollama/qwen3.8"), "{usage:?}");
        assert_eq!(usage.skills.get("debug"), Some(&1));
    }

    #[test]
    fn every_builtin_teammate_model_has_a_price_or_is_free() {
        let prices = builtin_prices();
        let roster = crate::roster::Roster::builtin().unwrap();
        let mut unpriced = Vec::new();
        for name in roster.names() {
            let t = roster.require(name).unwrap();
            if t.agent == crate::harness::HarnessKind::None {
                continue;
            }
            let Some(model) = t.model.as_deref() else {
                continue;
            };
            if price_for(&prices, model).is_none() {
                unpriced.push(format!("{name}: {model}"));
            }
        }
        assert!(unpriced.is_empty(), "no price: {unpriced:?}");
    }

    #[test]
    fn fable_alias_prices_as_fable_5_2_and_fable_5_1_keeps_its_row() {
        let prices = builtin_prices();
        let fable = price_for(&prices, "fable").unwrap();
        assert_eq!(
            (fable.input, fable.output, fable.cache_read),
            (8.0, 40.0, 0.20)
        );
        let million = Tokens {
            cache_write_5m: 1_000_000,
            cache_write_1h: 1_000_000,
            ..Tokens::default()
        };
        assert!((fable.cost(&million) - (10.0 + 16.0)).abs() < 1e-9);
        let old = price_for(&prices, "claude-fable-5-1").unwrap();
        assert_eq!((old.input, old.output, old.cache_read), (10.0, 50.0, 0.25));
    }

    #[test]
    fn gemini_4_neon_has_a_price_and_gemini_4_argon_keeps_its_row() {
        let prices = builtin_prices();
        let million = Tokens {
            cache_write_5m: 1_000_000,
            cache_write_1h: 1_000_000,
            ..Tokens::default()
        };
        for id in [
            "gemini-4-neon",
            "gemini-4-neon-low",
            "gemini-4-neon-medium",
            "gemini-4-neon-high",
            "gemini-4-neon-xhigh",
        ] {
            let neon = price_for(&prices, id).unwrap();
            assert_eq!(
                (neon.input, neon.output, neon.cache_read),
                (3.0, 15.0, 0.15),
                "{id}"
            );
            assert!((neon.cost(&million) - 6.0).abs() < 1e-9, "{id}");
        }
        for id in ["gemini-4-argon", "gemini-4-argon-high"] {
            let argon = price_for(&prices, id).unwrap();
            assert_eq!(
                (argon.input, argon.output, argon.cache_read),
                (2.0, 10.0, 0.10),
                "{id}"
            );
        }
    }

    #[test]
    fn models_are_canonicalised_and_free_or_unknown_ones_are_told_apart() {
        for (raw, key) in [
            ("claude-haiku-4-5-20251001", "claude-haiku-4-5"),
            ("anthropic/claude-opus-5-5", "claude-opus-5-5"),
            ("claude-opus-5-5[1m]", "claude-opus-5-5"),
            ("opus", "claude-opus-5-5"),
            ("sonnet", "claude-sonnet-5-5"),
            ("GPT-5.6-Sol", "gpt-5.6-sol"),
        ] {
            assert_eq!(canonical_model(raw), key, "{raw}");
        }
        let prices = builtin_prices();
        assert_eq!(
            price_for(&prices, "ollama/qwen3.8").unwrap().cost(&Tokens {
                input: 1_000_000,
                ..Tokens::default()
            }),
            0.0
        );
        assert_eq!(
            price_for(&prices, "opencode/big-pickle").unwrap().input,
            0.0
        );
        assert!(
            price_for(&prices, "mystery-model-9").is_none(),
            "unknown is not free"
        );
    }

    #[test]
    fn canonical_model_strips_each_noise_form_and_resolves_every_alias() {
        for (raw, key) in [
            ("  Claude-Opus-5-5  ", "claude-opus-5-5"),
            ("openai/gpt-5.6-sol", "gpt-5.6-sol"),
            (
                "anthropic/claude-haiku-4-5-20251001[1m]",
                "claude-haiku-4-5",
            ),
            ("fable", "claude-fable-5-2"),
            ("haiku", "claude-haiku-4-5"),
            ("claude-opus-5-5-2025101", "claude-opus-5-5-2025101"),
            ("claude-opus-5-5-2025101x", "claude-opus-5-5-2025101x"),
            ("gpt-5.6-sol", "gpt-5.6-sol"),
            ("nodashname", "nodashname"),
        ] {
            assert_eq!(canonical_model(raw), key, "{raw}");
        }
    }

    #[test]
    fn find_file_walks_sub_directories_and_applies_the_matcher() {
        let tmp = tempfile::tempdir().unwrap();
        let deep = tmp.path().join("a/b");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::create_dir_all(tmp.path().join("empty")).unwrap();
        std::fs::write(tmp.path().join("a/skip.txt"), "").unwrap();
        std::fs::write(deep.join("want.jsonl"), "").unwrap();
        let found = find_file(tmp.path(), &|n| n.ends_with(".jsonl"));
        assert_eq!(found, Some(deep.join("want.jsonl")));
        assert_eq!(find_file(tmp.path(), &|n| n == "nope"), None);
        assert_eq!(find_file(&tmp.path().join("missing"), &|_| true), None);
    }

    #[test]
    fn skill_loads_in_reads_each_harness_format() {
        let skill = |id: &str, name: &str| {
            format!(
                r#"{{"type":"tool_use","id":"{id}","name":"Skill","input":{{"skill":"{name}"}}}}"#
            )
        };
        let claude = [
            format!(
                r#"{{"type":"assistant","timestamp":"2026-10-06T12:00:00Z","message":{{"content":[{},{},{{"type":"tool_use","id":"o","name":"Bash","input":{{"skill":"no"}}}}]}}}}"#,
                skill("t1", "one"),
                skill("t1", "one-again")
            ),
            "not json".to_string(),
            format!(
                r#"{{"type":"user","message":{{"content":[{}]}}}}"#,
                skill("t2", "user-line")
            ),
            r#"{"type":"assistant","message":{"content":"text"}}"#.to_string(),
            format!(
                r#"{{"type":"assistant","message":{{"content":[{}]}}}}"#,
                skill("t1", "repeat-id")
            ),
            format!(
                r#"{{"type":"assistant","message":{{"content":[{},{{"type":"tool_use","name":"Skill","input":{{}}}}]}}}}"#,
                skill("t3", "three")
            ),
        ]
        .join("\n");
        let names = |loads: Vec<SkillLoad>| loads.into_iter().map(|l| l.skill).collect::<Vec<_>>();
        let loads = skill_loads_in("claude", &claude);
        assert_eq!(loads[0].ts, "2026-10-06T12:00:00Z");
        assert_eq!(names(loads), ["one", "three"]);

        let read = "cat /h/skills/tdd/SKILL.md";
        let codex = [
            format!(r#"{{"payload":{{"type":"function_call","arguments":"{read}"}}}}"#),
            format!(r#"{{"payload":{{"type":"local_shell_call","command":"{read}"}}}}"#),
            format!(r#"{{"payload":{{"type":"custom_tool_call","input":"{read}"}}}}"#),
            format!(r#"{{"payload":{{"type":"message","text":"{read}"}}}}"#),
            format!(r#"{{"text":"{read}"}}"#),
        ]
        .join("\n");
        assert_eq!(names(skill_loads_in("codex", &codex)), ["tdd"; 3]);

        let pi = [
            format!(r#"{{"message":{{"role":"assistant","content":"{read}"}}}}"#),
            format!(r#"{{"message":{{"role":"user","content":"{read}"}}}}"#),
            format!(r#"{{"role":"assistant","content":"{read}"}}"#),
            format!(r#"{{"role":"user","content":"{read}"}}"#),
        ]
        .join("\n");
        assert_eq!(names(skill_loads_in("pi", &pi)), ["tdd"; 2]);
        assert_eq!(names(skill_loads_in("prime", &pi)), ["tdd"; 2]);
        assert!(skill_loads_in("opencode", &pi).is_empty());
    }

    #[test]
    fn has_transcript_agrees_with_the_telemetry_locator() {
        use crate::harness::HarnessKind;
        use crate::telemetry::readers::{locate, Unreadable};
        let home = tempfile::tempdir().unwrap();
        let loc = Locations::under_home(home.path(), &crate::runtime::Inherited::default());
        for &kind in HarnessKind::ALL {
            let refused = matches!(
                locate(&loc, kind.as_str(), "sid"),
                Err(Unreadable::NotRead(why)) if why != "sqlite3 not found"
            );
            assert_eq!(has_transcript(kind), !refused, "{}", kind.as_str());
        }
    }

    #[test]
    fn transcripts_are_found_by_session_id_per_harness() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let claude = home.join(".claude/projects/-Users-me-proj");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::write(claude.join("sid-c.jsonl"), fixture("claude.jsonl")).unwrap();
        let codex = home.join(".codex/sessions/2026/09/24");
        std::fs::create_dir_all(&codex).unwrap();
        std::fs::write(
            codex.join("rollout-2026-09-24T00-00-00-sid-x.jsonl"),
            fixture("codex.jsonl"),
        )
        .unwrap();
        let pi = home.join(".pi/agent/sessions/--proj--");
        std::fs::create_dir_all(&pi).unwrap();
        std::fs::write(pi.join("2026-09-24_sid-p.jsonl"), fixture("pi.jsonl")).unwrap();
        let loc = Locations {
            home: home.to_path_buf(),
            claude_projects: home.join(".claude/projects"),
            codex_sessions: home.join(".codex/sessions"),
            pi_sessions: home.join(".pi/agent/sessions"),
            opencode_db: home.join(".local/share/opencode/opencode.db"),
            sqlite3: PathBuf::from("sqlite3"),
        };

        assert!(read_session(&loc, "claude", Some("sid-c")).is_ok());
        assert!(read_session(&loc, "codex", Some("sid-x")).is_ok());
        assert!(read_session(&loc, "pi", Some("sid-p")).is_ok());
        let prime_path = pi.join("2026-09-24_sid-p.jsonl");
        assert!(read_session(&loc, "prime", prime_path.to_str()).is_ok());
        assert_eq!(
            read_session(&loc, "claude", Some("gone")).unwrap_err(),
            Missing::NoTranscript
        );
        assert_eq!(
            read_session(&loc, "claude", None).unwrap_err(),
            Missing::NoSessionId
        );
        assert_eq!(
            read_session(&loc, "opencode", Some("ses_1")).unwrap_err(),
            Missing::NoTranscript
        );
        assert_eq!(
            read_session(&loc, "none", Some("x")).unwrap_err(),
            Missing::NotRead
        );
    }

    #[test]
    fn a_session_that_cannot_be_read_is_missing_for_the_readers_reason() {
        let home = tempfile::tempdir().unwrap();
        let loc = Locations::under_home(home.path(), &crate::runtime::Inherited::default());
        let missing =
            |agent: &str, id: Option<&str>| read_session_events(&loc, agent, id).unwrap_err();
        assert_eq!(missing("claude", None), Missing::NoSessionId);
        assert_eq!(missing("claude", Some("gone")), Missing::NoTranscript);
        assert_eq!(missing("none", Some("x")), Missing::NotRead);
        assert_eq!(
            missing("opencode", Some("not safe")),
            Missing::Failed("unexpected opencode session id 'not safe'".into())
        );
    }

    #[test]
    fn a_session_is_its_main_transcript_with_the_subagent_ones() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".claude/projects/-proj");
        let subagents = dir.join("sid-s/subagents");
        std::fs::create_dir_all(&subagents).unwrap();
        let line = |at: &str, id: &str, skill: &str| claude_line(at, id, 100, Some(skill)) + "\n";
        std::fs::write(
            dir.join("sid-s.jsonl"),
            line("2026-10-06T12:00:00Z", "m1", "horch:tdd"),
        )
        .unwrap();
        std::fs::write(
            subagents.join("agent-a.jsonl"),
            line("2026-10-06T12:05:00Z", "m2", "horch:check"),
        )
        .unwrap();
        let loc = Locations::under_home(tmp.path(), &crate::runtime::Inherited::default());
        let session = read_session_events(&loc, "claude", Some("sid-s")).unwrap();
        assert_eq!(session.path, dir.join("sid-s.jsonl"));
        assert_eq!(session.events.len(), 2);
        let loads: Vec<(&str, &str)> = session
            .skill_loads
            .iter()
            .map(|l| (l.ts.as_str(), l.skill.as_str()))
            .collect();
        assert_eq!(
            loads,
            [
                ("2026-10-06T12:00:00Z", "horch:tdd"),
                ("2026-10-06T12:05:00Z", "horch:check")
            ]
        );
    }

    #[test]
    fn a_transcript_that_is_not_text_gives_its_usage_but_no_skill_loads() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join(".claude/projects/-proj");
        std::fs::create_dir_all(&dir).unwrap();
        let mut bytes =
            (claude_line("2026-10-06T12:00:00Z", "m1", 100, Some("horch:tdd")) + "\n").into_bytes();
        bytes.extend_from_slice(b"\xff\xfe not text\n");
        std::fs::write(dir.join("sid-b.jsonl"), bytes).unwrap();
        let loc = Locations::under_home(tmp.path(), &crate::runtime::Inherited::default());
        let session = read_session_events(&loc, "claude", Some("sid-b")).unwrap();
        assert_eq!(session.events.len(), 1);
        assert!(session.skill_loads.is_empty(), "{:?}", session.skill_loads);
    }

    #[test]
    fn a_pricing_file_overrides_one_row_and_keeps_the_rest() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("p.json");
        std::fs::write(
            &path,
            r#"{"gpt-5.6-sol": {"input": 5, "output": 30, "cache_read": 0.5}}"#,
        )
        .unwrap();
        let prices = load_prices(Some(&path)).unwrap();
        assert_eq!(prices["gpt-5.6-sol"].output, 30.0);
        assert_eq!(prices["claude-opus-5-5"].input, 4.0);
        assert!(load_prices(Some(&tmp.path().join("missing.json"))).is_err());
    }
    fn claude_line(at: &str, id: &str, out: u64, skill: Option<&str>) -> String {
        let content = match skill {
            Some(s) => format!(
                r#"[{{"type":"tool_use","id":"tu-{id}","name":"Skill","input":{{"skill":"{s}"}}}}]"#
            ),
            None => r#"[{"type":"text","text":"ok"}]"#.to_string(),
        };
        format!(
            r#"{{"type":"assistant","timestamp":"{at}","message":{{"id":"{id}","model":"claude-opus-5-5","role":"assistant","content":{content},"usage":{{"input_tokens":1,"cache_read_input_tokens":10,"output_tokens":{out}}}}}}}"#
        )
    }

    #[test]
    fn tel_13_a_span_counts_calls_and_skill_loads_by_their_own_time() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let dir = home.join(".claude/projects/-proj");
        std::fs::create_dir_all(&dir).unwrap();
        let text = [
            claude_line("2026-08-28T10:00:00Z", "m1", 100, Some("horch:tdd")),
            claude_line("2026-09-20T10:00:00Z", "m2", 200, None),
            claude_line("2026-10-06T12:00:00Z", "m3", 300, Some("horch:check")),
            claude_line("2026-10-07T09:00:00Z", "m4", 400, None),
        ]
        .join("\n");
        std::fs::write(dir.join("sid-old.jsonl"), text + "\n").unwrap();
        let loc = Locations::under_home(home, &crate::runtime::Inherited::default());
        let session = read_session_events(&loc, "claude", Some("sid-old")).unwrap();

        let day = Span::new(Some("2026-10-06T10:00:00Z"), Some("2026-10-07T10:00:00Z"));
        let u = session.usage(&day);
        assert_eq!(u.calls, 2);
        assert_eq!(u.tokens().output, 700);
        assert_eq!(u.skills.keys().collect::<Vec<_>>(), ["horch:check"]);

        let week = Span::new(Some("2026-09-30T10:00:00Z"), Some("2026-10-07T10:00:00Z"));
        assert_eq!(session.usage(&week).calls, 2);
        let month = Span::new(Some("2026-09-07T10:00:00Z"), Some("2026-10-07T10:00:00Z"));
        assert_eq!(session.usage(&month).calls, 3);

        let until = Span::new(None, Some("2026-10-07T09:00:00Z"));
        assert_eq!(session.usage(&until).tokens().output, 600);

        let all = session.usage(&Span::default());
        assert_eq!(all.calls, 4);
        assert_eq!(all.skills.len(), 2);
        let (_, whole) = read_session(&loc, "claude", Some("sid-old")).unwrap();
        assert_eq!(whole, all, "read_session is the unbounded span");

        let none = Span::new(Some("2026-10-08T00:00:00Z"), None);
        assert!(session.usage(&none).by_model.is_empty());
    }
}
