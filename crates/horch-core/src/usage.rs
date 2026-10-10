//! What a fleet run actually cost, read back from each harness's own
//! transcripts (ImCesar/cezaar#49, extended past Claude).
//!
//! Attribution is exact: the ledger holds each worker's session id, and every
//! harness names its transcript after that id. Nothing is matched by time.
//!
//! | harness | transcript | tokens |
//! |---------|------------|--------|
//! | claude  | `~/.claude/projects/*/<sid>.jsonl` | assistant `message.usage`, streamed several times per `message.id` |
//! | codex   | `<codex home>/sessions/**/rollout-*-<sid>.jsonl` | `token_usage_record` per response; before 0.157, `token_count` totals |
//! | pi      | `~/.pi/agent/sessions/**/*<sid>*.jsonl` | assistant `usage` per message |
//! | prime   | the session file path the ledger stores | as pi |
//! | opencode| `opencode.db`, via the `sqlite3` CLI | completed assistant messages |
//!
//! The token readers themselves live in `telemetry::readers`, shared with the
//! telemetry collector; this module keeps the prices and the skill scan.
//!
//! Prices are per million tokens and dated. `--pricing <file.json>`
//! overrides any row without a rebuild.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod money;

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

/// The compiled-in price table, as of 2026-09-24. The rows added on
/// 2026-10-04 cite their sources in a comment on each row.
pub fn builtin_prices() -> BTreeMap<String, Price> {
    let mut m = BTreeMap::new();
    // Anthropic, from cezaar#49 (platform.claude.com pricing).
    m.insert("claude-fable-5-1".into(), Price::new(10.0, 50.0, 0.25));
    m.insert("claude-opus-5-5".into(), Price::new(4.0, 20.0, 0.20));
    m.insert("claude-opus-5".into(), Price::new(5.0, 25.0, 0.50));
    // Sonnet 5.5, from platform.claude.com/docs/en/about-claude/pricing
    // (read 2026-10-04): $2 / $10, cache hits $0.20, writes 1.25x and 2x.
    m.insert("claude-sonnet-5-5".into(), Price::new(2.0, 10.0, 0.20));
    m.insert("claude-sonnet-5".into(), Price::new(2.0, 10.0, 0.20));
    m.insert("claude-haiku-4-5".into(), Price::new(1.0, 5.0, 0.10));
    // OpenAI. Astra's cache write is published ($12.50, i.e. 1.25x).
    m.insert("gpt-6-astra".into(), Price::new(10.0, 50.0, 1.0));
    // Sol: promotional until 2026-11-21, then $5 / $30.
    m.insert("gpt-5.6-sol".into(), Price::new(4.0, 20.0, 0.40));
    // Terra and Luna: sources disagree; the lower figure. UNVERIFIED.
    m.insert("gpt-5.6-terra".into(), Price::new(2.0, 12.0, 0.20));
    m.insert("gpt-5.6-luna".into(), Price::new(0.20, 1.20, 0.02));
    // Google, from ai.google.dev/gemini-api/docs/pricing (read 2026-10-04),
    // paid tier, prompts up to 200k tokens: $2 / $12, cached $0.20. Over 200k
    // it is $4 / $18 / $0.40; one row cannot hold both, so long prompts are
    // under-priced. Gemini bills no cache write, only hourly storage, so a
    // write prices as plain input.
    let gemini_pro = Price {
        cache_write_5m: Some(2.0),
        cache_write_1h: Some(2.0),
        ..Price::new(2.0, 12.0, 0.20)
    };
    // agy names a model once per thinking level (`agy models`, 2026-10-05).
    for id in [
        "gemini-3.1-pro-preview",
        "gemini-3.1-pro-high",
        "gemini-3.1-pro-low",
    ] {
        m.insert(id.into(), gemini_pro);
    }
    // 3.8 Flash: promotional until 2026-12-31, then $1.50 / $7.50, cached
    // $0.15. One price at every prompt length; no cache write, as above.
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
    // No row for `codex-auto-review`, on purpose. It is Codex's approval
    // reviewer (`approvals_reviewer = "auto_review"`; a rollout with
    // `thread_source: guardian_review` names it). Codex's models cache lists
    // it hidden, and OpenAI publishes no per-token price for it
    // (learn.chatgpt.com/docs/pricing, read 2026-10-06). Its events are
    // listed as unpriced, never counted as $0.
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
pub(crate) fn canonical_model(model: &str) -> String {
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
        // Claude Code resolves `sonnet` to Sonnet 5.5: its transcripts name
        // claude-sonnet-5-5 (seen 2026-10-04).
        "sonnet" => "claude-sonnet-5-5".into(),
        "haiku" => "claude-haiku-4-5".into(),
        _ => m,
    }
}

/// Models that cost nothing: local Ollama and OpenCode's free tier.
pub(crate) fn is_free(model: &str) -> bool {
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

/// Dollars for `tokens` on `model`, or `None` when the table does not know
/// the model. The one pricing function: `horch cost`, the collector and the
/// telemetry readers that price a stored event later all call it.
pub fn cost_of(prices: &BTreeMap<String, Price>, model: &str, tokens: &Tokens) -> Option<f64> {
    price_for(prices, model).map(|p| p.cost(tokens))
}

// ─── transcript discovery ───────────────────────────────────────────────────

/// `~/.claude/projects/<any>/<sid>.jsonl`. Searched across project
/// directories instead of re-deriving Claude's cwd slug.
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

/// A codex rollout named after `session_id`, anywhere under `sessions_dir`.
pub(crate) fn find_codex_rollout(sessions_dir: &Path, session_id: &str) -> Option<PathBuf> {
    find_file(sessions_dir, &|name| {
        name.starts_with("rollout-") && name.ends_with(&format!("{session_id}.jsonl"))
    })
}

/// A pi session file whose name carries `session_id`. `session_id` may also
/// be a path already (the Prime case).
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
//
// Only tests call the whole-file token readers below: `read_session_events`
// takes its tokens from the telemetry readers. The skill scan is live code.

#[cfg(test)]
fn u(v: &Value, keys: &[&str]) -> u64 {
    keys.iter()
        .find_map(|k| v.get(*k).and_then(Value::as_u64))
        .unwrap_or(0)
}

#[cfg(test)]
/// A Claude Code transcript.
///
/// Streaming writes several records for one API response, each carrying the
/// same `message.id` and usage that only grows, so the last record per id is
/// the response. `<synthetic>` records are Claude Code's own, not API calls.
/// Skill loads are `Skill` tool calls, counted once per `tool_use` id.
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
/// Skill loads per skill name, over the whole text.
fn count_skills(agent: &str, text: &str) -> BTreeMap<String, u64> {
    let mut out = BTreeMap::new();
    for load in skill_loads_in(agent, text) {
        *out.entry(load.skill).or_default() += 1;
    }
    out
}

/// The time a transcript line was written, as a ledger stamp: its
/// `timestamp` string, or pi's `message.timestamp` in epoch milliseconds.
/// Empty when the line has neither.
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

/// Every skill load in one transcript of `agent`, with the time of its line.
///
/// Claude: `Skill` tool calls, once per `tool_use` id. Codex: shell or tool
/// calls that read a `skills/<name>/SKILL.md`. pi and Prime: assistant
/// messages that read one.
pub(crate) fn skill_loads_in(agent: &str, text: &str) -> Vec<SkillLoad> {
    let mut out = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for line in text.lines() {
        let Ok(record) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let ts = line_ts(&record);
        let mut push = |skill: String| {
            out.push(SkillLoad {
                ts: ts.clone(),
                skill,
            })
        };
        match agent {
            "claude" => {
                if record.get("type").and_then(Value::as_str) != Some("assistant") {
                    continue;
                }
                let Some(content) = record.pointer("/message/content").and_then(Value::as_array)
                else {
                    continue;
                };
                for block in content {
                    if block.get("type").and_then(Value::as_str) == Some("tool_use")
                        && block.get("name").and_then(Value::as_str) == Some("Skill")
                    {
                        let id = block.get("id").and_then(Value::as_str).unwrap_or_default();
                        if let Some(skill) = block.pointer("/input/skill").and_then(Value::as_str) {
                            if seen.insert(id.to_string()) {
                                push(skill.to_string());
                            }
                        }
                    }
                }
            }
            "codex" => {
                let kind = record
                    .pointer("/payload/type")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if matches!(
                    kind,
                    "function_call" | "local_shell_call" | "custom_tool_call"
                ) {
                    skill_reads(line).into_iter().for_each(&mut push);
                }
            }
            "pi" | "prime" => {
                let message = record.get("message").unwrap_or(&record);
                if message.get("role").and_then(Value::as_str) == Some("assistant") {
                    skill_reads(line).into_iter().for_each(&mut push);
                }
            }
            _ => {}
        }
    }
    out
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

#[cfg(test)]
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

#[cfg(test)]
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
        // Each distinct running total is one more model response.
        usage.calls = turns.into_iter().filter(|n| *n > 0).count() as u64;
    }
    usage
}

#[cfg(test)]
/// A pi or Prime Agent session file: one JSON record per line, assistant
/// messages carrying `usage`. The field names are pi's (`input`, `output`,
/// `cacheRead`, `cacheWrite`), with the Anthropic spellings accepted too,
/// because they were not confirmed against a live file.
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

/// Where each harness keeps its sessions on this machine.
#[derive(Debug, Clone)]
pub struct Locations {
    pub home: PathBuf,
    /// `<home>/.claude/projects`: one directory per project slug.
    pub claude_projects: PathBuf,
    pub codex_sessions: PathBuf,
    pub pi_sessions: PathBuf,
    /// OpenCode's SQLite database. `$HORCH_OPENCODE_DB` overrides.
    pub opencode_db: PathBuf,
    /// The `sqlite3` CLI that reads `opencode_db`. `$HORCH_SQLITE3_BIN`
    /// overrides.
    pub sqlite3: PathBuf,
}

impl Locations {
    /// Every location for the context's home, with the environment overrides
    /// it read applied, and the context's `sqlite3`.
    pub fn from_context(ctx: &crate::runtime::RuntimeContext) -> Self {
        Self {
            sqlite3: ctx.bins.harness.sqlite3.clone(),
            ..Self::under_home(&ctx.paths.home, &ctx.inherited)
        }
    }

    /// Every location for a given home directory, with the overrides in
    /// `inherited` applied: `CODEX_HOME`, `PI_CODING_AGENT_SESSION_DIR`,
    /// `HORCH_OPENCODE_DB` and `XDG_DATA_HOME`. Tests pass a temp dir.
    /// `sqlite3` is the plain program name.
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

/// Why a session has no usage row.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Missing {
    /// The ledger never learned the session id.
    NoSessionId,
    /// The id is known but no transcript file was found for it.
    NoTranscript,
    /// This harness's usage is not read (the smoke fake), or cannot be here
    /// (no `sqlite3` for OpenCode).
    NotRead,
    /// The transcript exists but could not be read.
    Failed(String),
}

/// A half-open time range of ledger stamps (`2026-10-07T09:00:00Z`): calls
/// at or after `since` and before `until`. `None` leaves that side open, so
/// the default span is everything.
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

    /// Whether a call at `ts` is in the span. Stamps compare as strings, as
    /// `horch usage --since` compares them, so the 2 commands agree (TEL-10).
    pub fn contains(&self, ts: &str) -> bool {
        self.since.as_deref().is_none_or(|s| ts >= s)
            && self.until.as_deref().is_none_or(|u| ts < u)
    }

    pub fn is_all(&self) -> bool {
        self.since.is_none() && self.until.is_none()
    }
}

/// One skill load and when the transcript says it happened ("" when the
/// line has no time).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillLoad {
    pub ts: String,
    pub skill: String,
}

/// One session read once: every usage event and every skill load, each with
/// its own time, so one read serves any number of spans.
#[derive(Debug, Clone)]
pub struct Session {
    /// The main transcript.
    pub path: PathBuf,
    pub events: Vec<crate::telemetry::RawUsage>,
    pub skill_loads: Vec<SkillLoad>,
}

impl Session {
    /// The usage of the calls and skill loads in `span`. A session with no
    /// call in the span has an empty `by_model`.
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

/// Find and read one session for `agent`: its usage events and skill loads.
///
/// Tokens come from the telemetry readers, from offset 0 with throwaway
/// cursors, so `horch cost` and `horch usage` count the same events
/// (TEL-10): Codex's final response (`token_usage_record`) and Claude
/// subagent transcripts included. Skill loads are found by scanning the
/// transcript text.
pub fn read_session_events(
    loc: &Locations,
    agent: &str,
    session_id: Option<&str>,
) -> std::result::Result<Session, Missing> {
    use crate::telemetry::readers::{read_whole, Unreadable};
    let (events, files) = read_whole(loc, agent, session_id).map_err(|e| match e {
        Unreadable::NoSessionId => Missing::NoSessionId,
        Unreadable::NoTranscript(_) => Missing::NoTranscript,
        Unreadable::NotRead(_) => Missing::NotRead,
        Unreadable::Failed(why) => Missing::Failed(why),
    })?;
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

/// Find and read one session's whole usage for `agent`:
/// [`read_session_events`] over the unbounded span.
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
        assert_eq!(
            usage.skills.get("horch:tdd"),
            Some(&1),
            "{:?}",
            usage.skills
        );
        assert_eq!(usage.skills.len(), 1, "a repeated record is one load");

        let prices = builtin_prices();
        let cost = price_for(&prices, "claude-opus-5-5").unwrap().cost(&t);
        // 15*4 + 1000*5 + 400*8 + 50_000*0.2 + 350*20, per million.
        let expected =
            (15.0 * 4.0 + 1000.0 * 5.0 + 400.0 * 8.0 + 50_000.0 * 0.2 + 350.0 * 20.0) / 1_000_000.0;
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

    /// Every model a built-in teammate runs on prices, or is free or local
    /// on purpose ([`is_free`]). A teammate without a model runs its
    /// harness's default, which no teammate file names; one with no agent
    /// (`smoke`) runs no model.
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
    /// A Claude line at `at` with one call and, optionally, one skill load.
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

    /// A span counts each call and each skill load by its own time: a
    /// session that started 40 days ago shows only its last day in a 24h
    /// span, and all of it with no span.
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
        // The reader leaves a last line without a newline for the next poll.
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

        // Before the span's end only: a call at the end itself is out.
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
