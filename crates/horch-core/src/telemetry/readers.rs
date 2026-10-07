//! One incremental reader per harness (design section 7.1).
//!
//! A reader turns new input into [`Observation`]s: usage events, and limit
//! signals for the quota module. It copies usage fields only; no message
//! text, tool input or tool output ever leaves it (TEL-11).
//!
//! Every event carries an id that is stable across re-reads, so reading a
//! file twice (a restart, a rotation, `horch cost` from offset 0) yields the
//! same events, and the store drops the repeats.
//!
//! | agent | input | event id |
//! |---|---|---|
//! | claude | `<projects>/*/<sid>.jsonl` + `<sid>/subagents/**/*.jsonl` | `message.id`; a later repeat with more tokens adds a correction `<id>+<n>` |
//! | codex | `rollout-*-<sid>.jsonl` | `token_usage_record.response_id`; before 0.157, `tc:<total_tokens>` |
//! | pi, prime | the session `.jsonl` | the line's `id`, else `@<byte offset>` |
//! | opencode | `opencode.db` via `sqlite3 -readonly -json` | `message.id`; a changed message adds `<id>+<n>` |

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::cursor::{poll_lines, Cursor, Seen};
use super::{Observation, QuotaSignal, RawUsage, TokenClasses};
use crate::clock;
use crate::routing::quota::Window;
use crate::usage::{find_codex_rollout, find_pi_session, Locations};

/// The cursors of every input, keyed by `input_key`.
pub type Cursors = BTreeMap<String, Cursor>;

/// Why a record yields no events (section 7.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unreadable {
    NoSessionId,
    NoTranscript(String),
    NotRead(String),
    Failed(String),
}

impl std::fmt::Display for Unreadable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unreadable::NoSessionId => f.write_str("no session id yet"),
            // The detail names machine paths; the one-line reason does not,
            // so the screen and its goldens stay the same on every machine.
            Unreadable::NoTranscript(_) => f.write_str("no transcript found"),
            Unreadable::NotRead(why) => f.write_str(why),
            Unreadable::Failed(why) => f.write_str(why),
        }
    }
}

/// A record's inputs and what reading them produced.
#[derive(Debug, Default)]
pub struct Polled {
    pub observations: Vec<Observation>,
    /// The files read (the main transcript first).
    pub files: Vec<PathBuf>,
}

/// The cursor key of one input.
pub(crate) fn input_key(agent: &str, session_id: &str, path: &Path) -> String {
    format!("{agent}|{session_id}|{}", path.display())
}

/// Read everything new for one record. `record_model` names the model when a
/// line does not (pi tool results, Codex before its first turn context).
pub fn poll_record(
    loc: &Locations,
    agent: &str,
    session_id: Option<&str>,
    cursors: &mut Cursors,
) -> Result<Polled, Unreadable> {
    let Some(sid) = session_id.filter(|s| !s.is_empty()) else {
        return Err(Unreadable::NoSessionId);
    };
    let mut polled = Polled::default();
    match agent {
        "claude" => {
            let main = crate::usage::find_claude_transcript(&loc.claude_projects, sid).ok_or_else(
                || {
                    Unreadable::NoTranscript(format!(
                        "{sid}.jsonl under {}",
                        loc.claude_projects.display()
                    ))
                },
            )?;
            let subs = claude_subagent_files(&main);
            poll_file(
                agent,
                sid,
                &main,
                cursors,
                &mut polled,
                |c, at, line, out| claude_line(c, at, line, false, out),
            )?;
            for sub in subs {
                poll_file(
                    agent,
                    sid,
                    &sub,
                    cursors,
                    &mut polled,
                    |c, at, line, out| claude_line(c, at, line, true, out),
                )?;
            }
        }
        "codex" => {
            let path = find_codex_rollout(&loc.codex_sessions, sid).ok_or_else(|| {
                Unreadable::NoTranscript(format!(
                    "rollout-*-{sid}.jsonl under {}",
                    loc.codex_sessions.display()
                ))
            })?;
            poll_file(agent, sid, &path, cursors, &mut polled, codex_line)?;
        }
        "pi" | "prime" => {
            let path = find_pi_session(&loc.pi_sessions, sid)
                .ok_or_else(|| Unreadable::NoTranscript(format!("a session file for {sid}")))?;
            poll_file(agent, sid, &path, cursors, &mut polled, pi_line)?;
        }
        "opencode" => {
            let key = input_key(agent, sid, &loc.opencode_db);
            let cursor = cursors.entry(key).or_default();
            opencode_poll(
                &loc.opencode_db,
                &loc.sqlite3,
                sid,
                cursor,
                &mut polled.observations,
            )?;
            polled.files.push(loc.opencode_db.clone());
        }
        "none" => return Err(Unreadable::NotRead("agent none is not read".into())),
        // "antigravity" lands here: agy 1.2.17 keeps no local usage that
        // horch can read (U-60). Its state files are binary protobuf, and the
        // conversation_summaries.db table has no token columns. Only `-p
        // --output-format json` prints per-run tokens, and horch runs the TUI.
        // Seen live: docs/live-checks/harnesses.md.
        other => return Err(Unreadable::NotRead(format!("unknown agent '{other}'"))),
    }
    Ok(polled)
}

fn poll_file(
    agent: &str,
    sid: &str,
    path: &Path,
    cursors: &mut Cursors,
    polled: &mut Polled,
    mut line_fn: impl FnMut(&mut Cursor, u64, &str, &mut Vec<Observation>),
) -> Result<(), Unreadable> {
    let cursor = cursors.entry(input_key(agent, sid, path)).or_default();
    let out = &mut polled.observations;
    poll_lines(path, cursor, |c, at, line| line_fn(c, at, line, out))
        .map_err(|e| Unreadable::Failed(format!("reading {}: {e}", path.display())))?;
    polled.files.push(path.to_path_buf());
    Ok(())
}

/// Every `*.jsonl` under `<sid>/subagents/`, nested directories included
/// ([S] C1: a flat glob misses nested ones). Sorted, for a stable order.
pub(crate) fn claude_subagent_files(main: &Path) -> Vec<PathBuf> {
    let dir = main.with_extension("").join("subagents");
    let mut out = Vec::new();
    collect_jsonl(&dir, &mut out);
    out.sort();
    out
}

fn collect_jsonl(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            out.push(path);
        }
    }
}

// ─── helpers ────────────────────────────────────────────────────────────────

fn n(v: &Value, pointer: &str) -> u64 {
    v.pointer(pointer).and_then(Value::as_u64).unwrap_or(0)
}

fn first_n(v: &Value, keys: &[&str]) -> u64 {
    keys.iter()
        .find_map(|k| v.get(*k).and_then(Value::as_u64))
        .unwrap_or(0)
}

/// An RFC 3339 transcript time as a ledger stamp; the raw string if it does
/// not parse (a reader never drops an event over its timestamp).
fn ts(raw: Option<&str>) -> String {
    raw.and_then(clock::parse)
        .map(clock::stamp)
        .unwrap_or_else(|| raw.unwrap_or_default().to_string())
}

/// Emit `tokens` for `id`, or only what grew since the last time `id` was
/// emitted, as a correction. Returns the event to push, if any.
fn emit_or_correct(
    seen: Option<&Seen>,
    id: &str,
    tokens: TokenClasses,
) -> Option<(String, TokenClasses, bool, Seen)> {
    match seen {
        None => Some((
            id.to_string(),
            tokens,
            false,
            Seen {
                id: id.to_string(),
                tokens,
                corrections: 0,
            },
        )),
        Some(prev) => {
            let grown = tokens.minus(&prev.tokens);
            if grown.is_zero() {
                return None;
            }
            let n = prev.corrections + 1;
            let mut total = prev.tokens;
            total.add(&grown);
            Some((
                format!("{id}+{n}"),
                grown,
                true,
                Seen {
                    id: id.to_string(),
                    tokens: total,
                    corrections: n,
                },
            ))
        }
    }
}

// ─── claude ─────────────────────────────────────────────────────────────────

/// Which window a Claude 429 text names ([Q] 1c), as a short label. The text
/// itself is never kept.
fn claude_refusal_label(line: &Value) -> String {
    let text = line
        .pointer("/message/content/0/text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    for (needle, label) in [
        ("session limit", "5h"),
        ("weekly limit", "7d"),
        ("opus limit", "7d:opus"),
        ("sonnet limit", "7d:sonnet"),
        ("fable limit", "7d:fable"),
        ("usage credit limit", "credits"),
    ] {
        if text.contains(needle) {
            return label.to_string();
        }
    }
    "unknown".to_string()
}

/// One Claude transcript line (TEL-04).
pub(crate) fn claude_line(
    c: &mut Cursor,
    _at: u64,
    line: &str,
    subagent: bool,
    out: &mut Vec<Observation>,
) {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return;
    };
    if v.get("type").and_then(Value::as_str) != Some("assistant") {
        return;
    }
    let when = ts(v.get("timestamp").and_then(Value::as_str));
    let model = v
        .pointer("/message/model")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if model == "<synthetic>" {
        if v.get("error").and_then(Value::as_str) == Some("rate_limit") {
            out.push(Observation::Quota(QuotaSignal::Refusal {
                pool: "claude".into(),
                at: when,
                what: claude_refusal_label(&v),
            }));
        }
        return;
    }
    let (Some(id), Some(u)) = (
        v.pointer("/message/id").and_then(Value::as_str),
        v.pointer("/message/usage").filter(|u| u.is_object()),
    ) else {
        return;
    };
    let written = first_n(u, &["cache_creation_input_tokens"]);
    // The TTL split when present; a missing 5m count is what the 1h count
    // leaves of the total written.
    let (cw5m, cw1h) = match u.get("cache_creation").filter(|c| c.is_object()) {
        Some(cc) => {
            let one_hour = n(cc, "/ephemeral_1h_input_tokens");
            let five = cc
                .get("ephemeral_5m_input_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(written.saturating_sub(one_hour));
            (five, one_hour)
        }
        None => (written, 0),
    };
    let tokens = TokenClasses {
        input: first_n(u, &["input_tokens"]),
        cache_write_5m: cw5m,
        cache_write_1h: cw1h,
        cache_read: first_n(u, &["cache_read_input_tokens"]),
        output: first_n(u, &["output_tokens"]),
        reasoning: n(u, "/output_tokens_details/thinking_tokens"),
    };
    let effort = v.get("effort").and_then(Value::as_str).map(str::to_owned);
    if let Some((event_id, tokens, delta, seen)) = emit_or_correct(c.recalled(id), id, tokens) {
        c.remember(seen);
        out.push(Observation::Usage(RawUsage {
            ts: when,
            event_id,
            model: model.to_string(),
            effort,
            tokens,
            subagent,
            delta,
            tool_nested: false,
            harness_cost: None,
        }));
    }
}

// ─── codex ──────────────────────────────────────────────────────────────────

/// The first Codex version known to write `token_usage_record` lines
/// ([S] X1: the type exists in 0.158.0; the operator saw it in 0.157.1).
const CODEX_RECORDS_SINCE: (u64, u64, u64) = (0, 157, 0);

fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let core = v.trim().trim_start_matches('v');
    let core = core.split(['-', '+', ' ']).next()?;
    let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
    Some((
        parts.next()??,
        parts.next()??,
        parts.next().flatten().unwrap_or(0),
    ))
}

/// The first value under `key`, depth first.
fn find_key<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Object(map) => map
            .get(key)
            .or_else(|| map.values().find_map(|x| find_key(x, key))),
        Value::Array(list) => list.iter().find_map(|x| find_key(x, key)),
        _ => None,
    }
}

/// A Codex `TokenUsage` object as token classes. `input_tokens` includes
/// the cached tokens, so they are subtracted.
fn codex_tokens(u: &Value) -> TokenClasses {
    let input = first_n(u, &["input_tokens"]);
    let cached = first_n(u, &["cached_input_tokens"]).min(input);
    TokenClasses {
        input: input - cached,
        cache_write_5m: first_n(u, &["cache_write_input_tokens"]),
        cache_write_1h: 0,
        cache_read: cached,
        output: first_n(u, &["output_tokens"]),
        reasoning: first_n(u, &["reasoning_output_tokens"]),
    }
}

/// A Codex `rate_limits` snapshot as windows, by duration (QUO-04). Codex
/// reports 0-100 and epoch seconds.
pub(crate) fn codex_rollout_windows(rl: &Value) -> Vec<Window> {
    let mut out = Vec::new();
    for slot in ["primary", "secondary"] {
        let Some(w) = rl.get(slot).filter(|w| w.is_object()) else {
            continue;
        };
        let Some(minutes) = w.get("window_minutes").and_then(Value::as_u64) else {
            continue;
        };
        let used = w.get("used_percent").and_then(Value::as_f64).unwrap_or(0.0) / 100.0;
        let resets = w
            .get("resets_at")
            .and_then(Value::as_i64)
            .and_then(clock::from_epoch)
            .map(clock::stamp);
        out.push(Window::new(minutes, used, resets, None));
    }
    out
}

/// One Codex rollout line (TEL-05).
pub(crate) fn codex_line(c: &mut Cursor, at: u64, line: &str, out: &mut Vec<Observation>) {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return;
    };
    let when = ts(v.get("timestamp").and_then(Value::as_str));
    let kind = v.get("type").and_then(Value::as_str).unwrap_or_default();
    let payload = v.get("payload").unwrap_or(&Value::Null);
    match kind {
        "session_meta" => {
            if let Some(version) = payload.get("cli_version").and_then(Value::as_str) {
                if let Some(ver) = parse_version(version) {
                    c.records_mode = Some(ver >= CODEX_RECORDS_SINCE);
                }
            }
        }
        "turn_context" => {
            if let Some(m) = payload.get("model").and_then(Value::as_str) {
                c.model = m.to_string();
            }
            if let Some(e) = payload.get("effort").and_then(Value::as_str) {
                c.effort = Some(e.to_string());
            }
        }
        "token_usage_record" => {
            // A record settles the question for a file that never said its
            // version.
            if c.records_mode.is_none() {
                c.records_mode = Some(true);
            }
            if c.records_mode != Some(true) {
                return;
            }
            let Some(u) = payload.get("usage") else {
                return;
            };
            let event_id = payload
                .get("response_id")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| format!("rec@{at}"));
            out.push(Observation::Usage(RawUsage {
                ts: when,
                event_id,
                model: c.model.clone(),
                effort: c.effort.clone(),
                tokens: codex_tokens(u),
                subagent: false,
                delta: false,
                tool_nested: false,
                harness_cost: None,
            }));
        }
        "event_msg" => {
            let msg = payload
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if msg == "token_count" {
                if let Some(rl) = payload.get("rate_limits").filter(|r| r.is_object()) {
                    let windows = codex_rollout_windows(rl);
                    if !windows.is_empty() {
                        out.push(Observation::Quota(QuotaSignal::Snapshot {
                            pool: "codex".into(),
                            at: when.clone(),
                            windows,
                            ordinary_usage_allowed: None,
                        }));
                    }
                }
                if c.records_mode != Some(true) {
                    codex_fallback(c, payload, &when, out);
                }
            }
            if find_key(payload, "codex_error_info").and_then(Value::as_str)
                == Some("usage_limit_exceeded")
            {
                out.push(Observation::Quota(QuotaSignal::Refusal {
                    pool: "codex".into(),
                    at: when,
                    what: "usage_limit_exceeded".into(),
                }));
            }
        }
        _ => {}
    }
}

/// Before `token_usage_record`: the difference of consecutive running
/// totals. Each total is written twice; the repeat differs by nothing.
fn codex_fallback(c: &mut Cursor, payload: &Value, when: &str, out: &mut Vec<Observation>) {
    let Some(total) = find_key(payload, "total_token_usage").filter(|t| t.is_object()) else {
        return;
    };
    let now = codex_tokens(total);
    let grown = match &c.last_total {
        Some(prev) => now.minus(prev),
        None => now,
    };
    let total_tokens = first_n(total, &["total_tokens"]);
    c.last_total = Some(now);
    if grown.is_zero() {
        return;
    }
    out.push(Observation::Usage(RawUsage {
        ts: when.to_string(),
        event_id: format!("tc:{total_tokens}"),
        model: c.model.clone(),
        effort: c.effort.clone(),
        tokens: grown,
        subagent: false,
        delta: false,
        tool_nested: false,
        harness_cost: None,
    }));
}

// ─── pi and prime ───────────────────────────────────────────────────────────

/// One pi or Prime Agent session line.
///
/// `cacheWrite` includes `cacheWrite1h` in pi 0.87 ([S] pi), so the 1h part
/// is split out. Anthropic spellings are accepted too.
pub(crate) fn pi_line(_c: &mut Cursor, at: u64, line: &str, out: &mut Vec<Observation>) {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return;
    };
    let message = v.get("message").unwrap_or(&v);
    let role = message
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !matches!(role, "assistant" | "toolResult") {
        return;
    }
    let Some(u) = message.get("usage").filter(|u| u.is_object()) else {
        return;
    };
    let written = first_n(u, &["cacheWrite", "cache_creation_input_tokens"]);
    let one_hour = first_n(u, &["cacheWrite1h"]).min(written);
    let tokens = TokenClasses {
        input: first_n(u, &["input", "input_tokens"]),
        cache_write_5m: written - one_hour,
        cache_write_1h: one_hour,
        cache_read: first_n(u, &["cacheRead", "cache_read_input_tokens"]),
        output: first_n(u, &["output", "output_tokens"]),
        reasoning: first_n(u, &["reasoning"]),
    };
    if tokens.is_zero() {
        return;
    }
    let model = message
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let model = match message.get("provider").and_then(Value::as_str) {
        Some(p) if !model.is_empty() && !model.contains('/') => format!("{p}/{model}"),
        _ => model.to_string(),
    };
    let when = match v.get("timestamp").and_then(Value::as_str) {
        Some(t) => ts(Some(t)),
        None => message
            .get("timestamp")
            .and_then(Value::as_i64)
            .and_then(clock::from_epoch_ms)
            .map(clock::stamp)
            .unwrap_or_default(),
    };
    let event_id = v
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("@{at}"));
    out.push(Observation::Usage(RawUsage {
        ts: when,
        event_id,
        model,
        effort: None,
        tokens,
        subagent: false,
        delta: false,
        tool_nested: role == "toolResult",
        harness_cost: None,
    }));
}

// ─── opencode ───────────────────────────────────────────────────────────────

/// A session id safe to put in a SQL literal.
fn safe_id(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Read one OpenCode session's messages updated since the cursor (TEL-06).
///
/// Only completed assistant messages count. A message seen again with
/// different tokens yields a correction for the difference. The database is
/// opened read-only through the `sqlite3` CLI; horch links no SQLite.
pub(crate) fn opencode_poll(
    db: &Path,
    sqlite3: &Path,
    session_id: &str,
    c: &mut Cursor,
    out: &mut Vec<Observation>,
) -> Result<(), Unreadable> {
    if !safe_id(session_id) {
        return Err(Unreadable::Failed(format!(
            "unexpected opencode session id '{session_id}'"
        )));
    }
    if !db.is_file() {
        return Err(Unreadable::NoTranscript(format!(
            "no opencode database at {}",
            db.display()
        )));
    }
    let query = format!(
        "SELECT id, time_updated, data FROM message WHERE session_id = '{session_id}' \
         AND time_updated >= {} ORDER BY time_updated, id;",
        c.since_ms
    );
    let rows = sqlite_json(sqlite3, db, &query)?;
    let grew = !rows.is_empty();
    for row in rows {
        let id = row
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let updated = row.get("time_updated").and_then(Value::as_i64).unwrap_or(0);
        c.since_ms = c.since_ms.max(updated);
        let data: Value = match row.get("data") {
            Some(Value::String(s)) => serde_json::from_str(s).unwrap_or(Value::Null),
            Some(v) => v.clone(),
            None => Value::Null,
        };
        opencode_message(c, &id, &data, out);
    }
    c.quiet_ticks = if grew {
        0
    } else {
        c.quiet_ticks.saturating_add(1)
    };
    Ok(())
}

/// One OpenCode `message.data` value.
pub(crate) fn opencode_message(c: &mut Cursor, id: &str, data: &Value, out: &mut Vec<Observation>) {
    if data
        .get("error")
        .is_some_and(|e| e.to_string().contains("FreeUsageLimitError"))
    {
        let at = data
            .pointer("/time/completed")
            .or_else(|| data.pointer("/time/created"))
            .and_then(Value::as_i64)
            .and_then(clock::from_epoch_ms)
            .map(clock::stamp)
            .unwrap_or_default();
        out.push(Observation::Quota(QuotaSignal::Refusal {
            pool: "opencode-zen".into(),
            at,
            what: "FreeUsageLimitError".into(),
        }));
    }
    if data.get("role").and_then(Value::as_str) != Some("assistant") {
        return;
    }
    let Some(completed) = data.pointer("/time/completed").and_then(Value::as_i64) else {
        return;
    };
    let t = data.get("tokens").unwrap_or(&Value::Null);
    let tokens = TokenClasses {
        input: n(t, "/input"),
        cache_write_5m: n(t, "/cache/write"),
        cache_write_1h: 0,
        cache_read: n(t, "/cache/read"),
        output: n(t, "/output"),
        reasoning: n(t, "/reasoning"),
    };
    let model = match (
        data.get("providerID").and_then(Value::as_str),
        data.get("modelID").and_then(Value::as_str),
    ) {
        (Some(p), Some(m)) => format!("{p}/{m}"),
        (None, Some(m)) => m.to_string(),
        _ => String::new(),
    };
    let when = clock::from_epoch_ms(completed)
        .map(clock::stamp)
        .unwrap_or_default();
    let harness_cost = data.get("cost").and_then(Value::as_f64);
    let Some((event_id, tokens, delta, seen)) = emit_or_correct(c.emitted.get(id), id, tokens)
    else {
        return;
    };
    c.emitted.insert(id.to_string(), seen);
    out.push(Observation::Usage(RawUsage {
        ts: when,
        event_id,
        model,
        effort: None,
        tokens,
        subagent: false,
        delta,
        tool_nested: false,
        harness_cost,
    }));
}

/// Run a query through `sqlite3 -readonly -json`. No rows prints nothing.
fn sqlite_json(bin: &Path, db: &Path, query: &str) -> Result<Vec<Value>, Unreadable> {
    let output = std::process::Command::new(bin)
        .arg("-readonly")
        .arg("-json")
        .arg(db)
        .arg(query)
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Unreadable::NotRead("sqlite3 not found".into())
            } else {
                Unreadable::Failed(format!("running {}: {e}", bin.display()))
            }
        })?;
    if !output.status.success() {
        return Err(Unreadable::Failed(format!(
            "sqlite3 failed on {}: {}",
            db.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str(text.trim())
        .map_err(|e| Unreadable::Failed(format!("sqlite3 output from {}: {e}", db.display())))
}

/// Read one record from the start with throwaway cursors: what `horch cost`
/// does. Same readers as the collector, so the totals agree (TEL-10).
pub fn read_whole(
    loc: &Locations,
    agent: &str,
    session_id: Option<&str>,
) -> Result<(Vec<RawUsage>, Vec<PathBuf>), Unreadable> {
    let mut cursors = Cursors::new();
    let polled = poll_record(loc, agent, session_id, &mut cursors)?;
    let usage = polled
        .observations
        .into_iter()
        .filter_map(|o| match o {
            Observation::Usage(u) => Some(u),
            Observation::Quota(_) => None,
        })
        .collect();
    Ok((usage, polled.files))
}
