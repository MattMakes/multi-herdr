//! What a fleet run actually cost, read back from each harness's own
//! transcripts (ImCesar/cezaar#49, extended past Claude).
//!
//! Attribution is exact: the ledger holds each worker's session id, and every
//! harness names its transcript after that id. Nothing is matched by time.
//!
//! | harness | transcript | tokens |
//! |---------|------------|--------|
//! | claude  | `~/.claude/projects/*/<sid>.jsonl` | assistant `message.usage`, streamed several times per `message.id` |
//! | codex   | `<codex home>/sessions/**/rollout-*-<sid>.jsonl` | running `total_token_usage` in `token_count` events |
//! | pi      | `~/.pi/agent/sessions/**/*<sid>*.jsonl` | assistant `usage` per message |
//! | prime   | the session file path the ledger stores | as pi |
//! | opencode| SQLite; not read | reported as not priced |
//!
//! Prices are per million tokens and dated; see
//! `ai_docs/reports/model-guide-2026-09.md`. `--pricing <file.json>`
//! overrides any row without a rebuild.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Token counts in the one shape every harness is converted into.
///
/// `input` is fresh (uncached) input only. Codex reports cached tokens inside
/// its input count, so its reader subtracts them; Claude and pi report them
/// separately already.
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

/// One session's usage, split by model because a session can switch models.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub by_model: BTreeMap<String, Tokens>,
    /// Model API calls (distinct assistant messages, or turns for codex).
    pub calls: u64,
    /// Skills this session loaded, with how many times.
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

/// USD per million tokens.
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

    /// Dollars for `t` at this price. Cache writes default to the Anthropic
    /// multipliers, 1.25x input for the 5-minute TTL and 2x for the 1-hour
    /// one; a harness without TTLs reports all writes as 5-minute.
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

/// The compiled-in price table, as of 2026-09-24. Sources and the unverified
/// rows are in `ai_docs/reports/model-guide-2026-09.md`.
pub fn builtin_prices() -> BTreeMap<String, Price> {
    let mut m = BTreeMap::new();
    // Anthropic, from cezaar#49 (platform.claude.com pricing).
    m.insert("claude-fable-5-1".into(), Price::new(10.0, 50.0, 0.25));
    m.insert("claude-opus-5-5".into(), Price::new(4.0, 20.0, 0.20));
    m.insert("claude-opus-5".into(), Price::new(5.0, 25.0, 0.50));
    m.insert("claude-sonnet-5".into(), Price::new(2.0, 10.0, 0.20));
    m.insert("claude-haiku-4-5".into(), Price::new(1.0, 5.0, 0.10));
    // OpenAI. Astra's cache write is published ($12.50, i.e. 1.25x).
    m.insert("gpt-6-astra".into(), Price::new(10.0, 50.0, 1.0));
    // Sol: promotional until 2026-11-21, then $5 / $30.
    m.insert("gpt-5.6-sol".into(), Price::new(4.0, 20.0, 0.40));
    // Terra and Luna: sources disagree; the lower figure. UNVERIFIED.
    m.insert("gpt-5.6-terra".into(), Price::new(2.0, 12.0, 0.20));
    m.insert("gpt-5.6-luna".into(), Price::new(0.20, 1.20, 0.02));
    m
}

/// Merge a `--pricing` JSON file (`{"<model>": {input, output, cache_read,
/// cache_write_5m?, cache_write_1h?}}`) over the built-in table.
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

/// The price-table key for a model as a transcript or teammate names it:
/// provider prefix, `[1m]` suffix and date stamp removed, Claude aliases
/// resolved to the model they currently point at.
pub fn canonical_model(model: &str) -> String {
    let mut m = model.trim().to_ascii_lowercase();
    if let Some(rest) = m.strip_suffix("[1m]") {
        m = rest.to_string();
    }
    for prefix in ["anthropic/", "openai/"] {
        if let Some(rest) = m.strip_prefix(prefix) {
            m = rest.to_string();
        }
    }
    // claude-haiku-4-5-20251001 -> claude-haiku-4-5
    if let Some((head, tail)) = m.rsplit_once('-') {
        if tail.len() == 8 && tail.bytes().all(|b| b.is_ascii_digit()) {
            m = head.to_string();
        }
    }
    match m.as_str() {
        "fable" => "claude-fable-5-1".into(),
        "opus" => "claude-opus-5-5".into(),
        "sonnet" => "claude-sonnet-5".into(),
        "haiku" => "claude-haiku-4-5".into(),
        _ => m,
    }
}

/// Models that cost nothing: local Ollama and OpenCode's free tier.
pub fn is_free(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    m.starts_with("ollama/")
        || (m.starts_with("opencode/") && (m.ends_with("-free") || m == "opencode/big-pickle"))
}

/// The price for `model`, or `None` when the table does not know it. A free
/// model prices at zero rather than unknown.
pub fn price_for(prices: &BTreeMap<String, Price>, model: &str) -> Option<Price> {
    if is_free(model) {
        return Some(Price::new(0.0, 0.0, 0.0));
    }
    prices
        .get(model)
        .or_else(|| prices.get(&canonical_model(model)))
        .copied()
}

// ─── transcript discovery ───────────────────────────────────────────────────

/// `~/.claude/projects/<any>/<sid>.jsonl`. Searched across project
/// directories instead of re-deriving Claude's cwd slug.
pub fn find_claude_transcript(home: &Path, session_id: &str) -> Option<PathBuf> {
    let projects = home.join(".claude/projects");
    let file = format!("{session_id}.jsonl");
    for entry in std::fs::read_dir(projects).ok()?.flatten() {
        let candidate = entry.path().join(&file);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// A codex rollout named after `session_id`, anywhere under `sessions_dir`.
pub fn find_codex_rollout(sessions_dir: &Path, session_id: &str) -> Option<PathBuf> {
    find_file(sessions_dir, &|name| {
        name.starts_with("rollout-") && name.ends_with(&format!("{session_id}.jsonl"))
    })
}

/// A pi session file whose name carries `session_id`. `session_id` may also
/// be a path already (the Prime case).
pub fn find_pi_session(sessions_dir: &Path, session_id: &str) -> Option<PathBuf> {
    let direct = PathBuf::from(session_id);
    if direct.is_file() {
        return Some(direct);
    }
    find_file(sessions_dir, &|name| {
        name.ends_with(".jsonl") && name.contains(session_id)
    })
}

fn find_file(dir: &Path, matches: &dyn Fn(&str) -> bool) -> Option<PathBuf> {
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_file(&path, matches) {
                return Some(found);
            }
        } else if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(matches)
        {
            return Some(path);
        }
    }
    None
}

// ─── readers ────────────────────────────────────────────────────────────────

fn u(v: &Value, keys: &[&str]) -> u64 {
    keys.iter()
        .find_map(|k| v.get(*k).and_then(Value::as_u64))
        .unwrap_or(0)
}

/// A Claude Code transcript.
///
/// Streaming writes several records for one API response, each carrying the
/// same `message.id` and usage that only grows, so the last record per id is
/// the response. `<synthetic>` records are Claude Code's own, not API calls.
/// Skill loads are `Skill` tool calls, counted once per `tool_use` id.
pub fn read_claude(text: &str) -> Usage {
    let mut last: BTreeMap<String, (String, Tokens)> = BTreeMap::new();
    let mut tool_uses: BTreeMap<String, String> = BTreeMap::new();
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
        if let Some(content) = message.get("content").and_then(Value::as_array) {
            for block in content {
                if block.get("type").and_then(Value::as_str) == Some("tool_use")
                    && block.get("name").and_then(Value::as_str) == Some("Skill")
                {
                    let id = block.get("id").and_then(Value::as_str).unwrap_or_default();
                    if let Some(skill) = block.pointer("/input/skill").and_then(Value::as_str) {
                        tool_uses.insert(id.to_string(), skill.to_string());
                    }
                }
            }
        }
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
    for skill in tool_uses.into_values() {
        *usage.skills.entry(skill).or_default() += 1;
    }
    usage
}

/// Skill names in text that reads a `skills/<name>/SKILL.md` file.
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

/// The first value under `key`, searched depth-first through `v`. A record
/// carries at most one running total, so first and last are the same.
fn find_key<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Object(map) => map
            .get(key)
            .or_else(|| map.values().find_map(|x| find_key(x, key))),
        Value::Array(list) => list.iter().find_map(|x| find_key(x, key)),
        _ => None,
    }
}

/// A Codex rollout.
///
/// `token_count` events carry a running `total_token_usage` and are written
/// twice each, so the last one is the session. `cached_input_tokens` is part
/// of `input_tokens` and reasoning is part of `output_tokens`, so neither is
/// added again. Newer rollouts nest the same fields elsewhere
/// (getagentseal/codeburn#1380), hence the key search. Usage from
/// auto-compaction is missing from the totals (openai/codex#47003), so a
/// compacted session reads low. A session that switched models is priced
/// entirely as its last model; the totals are not split per turn. Skill loads
/// are shell or tool calls that read a `skills/<name>/SKILL.md`.
pub fn read_codex(text: &str) -> Usage {
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
        let kind = record
            .pointer("/payload/type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if matches!(kind, "function_call" | "local_shell_call" | "custom_tool_call") {
            for skill in skill_reads(line) {
                *usage.skills.entry(skill).or_default() += 1;
            }
        }
    }
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
        // Each distinct running total is one more model response.
        usage.calls = turns.into_iter().filter(|n| *n > 0).count() as u64;
    }
    usage
}

/// A pi or Prime Agent session file: one JSON record per line, assistant
/// messages carrying `usage`. The field names are pi's (`input`, `output`,
/// `cacheRead`, `cacheWrite`), with the Anthropic spellings accepted too,
/// because they were not confirmed against a live file.
pub fn read_pi(text: &str) -> Usage {
    let mut usage = Usage::default();
    for line in text.lines() {
        let Ok(record) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let message = record.get("message").unwrap_or(&record);
        if message.get("role").and_then(Value::as_str) == Some("assistant") {
            for skill in skill_reads(line) {
                *usage.skills.entry(skill).or_default() += 1;
            }
        }
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
    usage
}

/// Where each harness keeps its sessions on this machine.
#[derive(Debug, Clone)]
pub struct Locations {
    pub home: PathBuf,
    pub codex_sessions: PathBuf,
    pub pi_sessions: PathBuf,
}

impl Locations {
    pub fn from_env() -> Self {
        let home = crate::agent::home_dir();
        let codex_sessions = crate::codex::codex_home(&home).join("sessions");
        let pi_sessions = std::env::var_os("PI_CODING_AGENT_SESSION_DIR")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".pi/agent/sessions"));
        Locations {
            home,
            codex_sessions,
            pi_sessions,
        }
    }
}

/// Why a session has no usage row.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Missing {
    /// The ledger never learned the session id.
    NoSessionId,
    /// The id is known but no transcript file was found for it.
    NoTranscript,
    /// This harness's usage is not read (opencode, or the smoke fake).
    NotRead,
}

/// Find and read one session's transcript for `agent`.
pub fn read_session(
    loc: &Locations,
    agent: &str,
    session_id: Option<&str>,
) -> std::result::Result<(PathBuf, Usage), Missing> {
    let Some(sid) = session_id.filter(|s| !s.is_empty()) else {
        return Err(Missing::NoSessionId);
    };
    let (path, reader): (Option<PathBuf>, fn(&str) -> Usage) = match agent {
        "claude" => (find_claude_transcript(&loc.home, sid), read_claude),
        "codex" => (find_codex_rollout(&loc.codex_sessions, sid), read_codex),
        "pi" | "prime" => (find_pi_session(&loc.pi_sessions, sid), read_pi),
        _ => return Err(Missing::NotRead),
    };
    let path = path.ok_or(Missing::NoTranscript)?;
    let text = std::fs::read_to_string(&path).map_err(|_| Missing::NoTranscript)?;
    Ok((path, reader(&text)))
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

    /// Streaming repeats a message; only its last record counts, and a
    /// 1-hour cache write is split out from the 5-minute ones.
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
        assert_eq!(usage.skills.get("horch:tdd"), Some(&1), "{:?}", usage.skills);
        assert_eq!(usage.skills.len(), 1, "a repeated record is one load");

        let prices = builtin_prices();
        let cost = price_for(&prices, "claude-opus-5-5").unwrap().cost(&t);
        // 15*4 + 1000*5 + 400*8 + 50_000*0.2 + 350*20, per million.
        let expected = (15.0 * 4.0 + 1000.0 * 5.0 + 400.0 * 8.0 + 50_000.0 * 0.2 + 350.0 * 20.0)
            / 1_000_000.0;
        assert!((cost - expected).abs() < 1e-12, "{cost} vs {expected}");
    }

    /// Running totals, written twice: the last one is the session. Cached
    /// input is inside the input count and must not be charged twice.
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
        assert_eq!(usage.skills.get("code-review"), Some(&1), "{:?}", usage.skills);
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
        assert_eq!((t.input, t.output, t.cache_read, t.cache_write_5m), (150, 60, 500, 40));
        assert!(usage.by_model.contains_key("ollama/qwen3.8"), "{usage:?}");
        assert_eq!(usage.skills.get("debug"), Some(&1));
    }

    #[test]
    fn models_are_canonicalised_and_free_or_unknown_ones_are_told_apart() {
        for (raw, key) in [
            ("claude-haiku-4-5-20251001", "claude-haiku-4-5"),
            ("anthropic/claude-opus-5-5", "claude-opus-5-5"),
            ("claude-opus-5-5[1m]", "claude-opus-5-5"),
            ("opus", "claude-opus-5-5"),
            ("GPT-5.6-Sol", "gpt-5.6-sol"),
        ] {
            assert_eq!(canonical_model(raw), key, "{raw}");
        }
        let prices = builtin_prices();
        assert_eq!(price_for(&prices, "ollama/qwen3.8").unwrap().cost(&Tokens { input: 1_000_000, ..Tokens::default() }), 0.0);
        assert_eq!(price_for(&prices, "opencode/big-pickle").unwrap().input, 0.0);
        assert!(price_for(&prices, "mystery-model-9").is_none(), "unknown is not free");
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
            codex_sessions: home.join(".codex/sessions"),
            pi_sessions: home.join(".pi/agent/sessions"),
        };

        assert!(read_session(&loc, "claude", Some("sid-c")).is_ok());
        assert!(read_session(&loc, "codex", Some("sid-x")).is_ok());
        assert!(read_session(&loc, "pi", Some("sid-p")).is_ok());
        let prime_path = pi.join("2026-09-24_sid-p.jsonl");
        assert!(read_session(&loc, "prime", prime_path.to_str()).is_ok());
        assert_eq!(read_session(&loc, "claude", Some("gone")).unwrap_err(), Missing::NoTranscript);
        assert_eq!(read_session(&loc, "claude", None).unwrap_err(), Missing::NoSessionId);
        assert_eq!(read_session(&loc, "opencode", Some("ses_1")).unwrap_err(), Missing::NotRead);
    }

    #[test]
    fn a_pricing_file_overrides_one_row_and_keeps_the_rest() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("p.json");
        std::fs::write(&path, r#"{"gpt-5.6-sol": {"input": 5, "output": 30, "cache_read": 0.5}}"#).unwrap();
        let prices = load_prices(Some(&path)).unwrap();
        assert_eq!(prices["gpt-5.6-sol"].output, 30.0);
        assert_eq!(prices["claude-opus-5-5"].input, 4.0);
        assert!(load_prices(Some(&tmp.path().join("missing.json"))).is_err());
    }
}
