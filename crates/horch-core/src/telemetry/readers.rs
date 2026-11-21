use std::borrow::Cow;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use super::cursor::{poll_lines_until, Cursor, Seen};
use super::{Observation, QuotaSignal, RawUsage, TokenClasses};
use crate::clock;
use crate::routing::quota::Window;
use crate::usage::{find_codex_rollout, find_pi_session, Locations};

pub type Cursors = BTreeMap<String, Cursor>;

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
            Unreadable::NoTranscript(_) => f.write_str("no transcript found"),
            Unreadable::NotRead(why) => f.write_str(why),
            Unreadable::Failed(why) => f.write_str(why),
        }
    }
}

#[derive(Debug, Default)]
pub struct Polled {
    pub observations: Vec<Observation>,
    pub files: Vec<PathBuf>,
    pub more: bool,
}

pub(crate) fn input_key(agent: &str, session_id: &str, path: &Path) -> String {
    format!("{agent}|{session_id}|{}", path.display())
}

fn cursor_of<'c>(cursors: &'c mut Cursors, agent: &str, sid: &str, path: &Path) -> &'c mut Cursor {
    let cursor = cursors.entry(input_key(agent, sid, path)).or_default();
    if cursor.agent.is_empty() || cursor.session_id.is_empty() {
        cursor.agent = agent.to_string();
        cursor.session_id = sid.to_string();
    }
    if cursor.path.is_empty() {
        cursor.path = path.to_string_lossy().into_owned();
    }
    cursor
}

pub(crate) enum Located {
    Files(Vec<PathBuf>),
    OpenCode(PathBuf),
}

pub(crate) fn locate(loc: &Locations, agent: &str, sid: &str) -> Result<Located, Unreadable> {
    match agent {
        "claude" => locate_claude(loc, sid),
        "codex" => locate_codex(loc, sid),
        "pi" | "prime" => {
            let path = find_pi_session(&loc.pi_sessions, sid)
                .ok_or_else(|| Unreadable::NoTranscript(format!("a session file for {sid}")))?;
            Ok(Located::Files(vec![path]))
        }
        "opencode" => Ok(Located::OpenCode(loc.opencode_db.clone())),
        "none" => Err(Unreadable::NotRead("agent none is not read".into())),
        other => Err(Unreadable::NotRead(format!("unknown agent '{other}'"))),
    }
}

fn locate_claude(loc: &Locations, sid: &str) -> Result<Located, Unreadable> {
    let main =
        crate::usage::find_claude_transcript(&loc.claude_projects, sid).ok_or_else(|| {
            Unreadable::NoTranscript(format!(
                "{sid}.jsonl under {}",
                loc.claude_projects.display()
            ))
        })?;
    let mut files = vec![main.clone()];
    files.extend(claude_subagent_files(&main));
    Ok(Located::Files(files))
}

fn locate_codex(loc: &Locations, sid: &str) -> Result<Located, Unreadable> {
    let path = find_codex_rollout(&loc.codex_sessions, sid).ok_or_else(|| {
        Unreadable::NoTranscript(format!(
            "rollout-*-{sid}.jsonl under {}",
            loc.codex_sessions.display()
        ))
    })?;
    Ok(Located::Files(vec![path]))
}

pub fn poll_record(
    loc: &Locations,
    agent: &str,
    session_id: Option<&str>,
    cursors: &mut Cursors,
) -> Result<Polled, Unreadable> {
    let Some(sid) = session_id.filter(|s| !s.is_empty()) else {
        return Err(Unreadable::NoSessionId);
    };
    let located = locate(loc, agent, sid)?;
    poll_located(loc, agent, sid, located, cursors, usize::MAX)
}

pub(crate) fn poll_located(
    loc: &Locations,
    agent: &str,
    sid: &str,
    located: Located,
    cursors: &mut Cursors,
    limit: usize,
) -> Result<Polled, Unreadable> {
    let mut polled = Polled::default();
    match located {
        Located::OpenCode(db) => {
            let cursor = cursor_of(cursors, agent, sid, &db);
            opencode_poll(&db, &loc.sqlite3, sid, cursor, &mut polled.observations)?;
            polled.files.push(db);
        }
        Located::Files(files) => poll_files(agent, sid, &files, cursors, &mut polled, limit)?,
    }
    Ok(polled)
}

fn poll_files(
    agent: &str,
    sid: &str,
    files: &[PathBuf],
    cursors: &mut Cursors,
    polled: &mut Polled,
    limit: usize,
) -> Result<(), Unreadable> {
    for (i, path) in files.iter().enumerate() {
        if polled.observations.len() >= limit {
            polled.more = true;
            break;
        }
        let limit = limit - polled.observations.len();
        let subagent = i > 0;
        poll_file(agent, sid, path, subagent, cursors, polled, limit)?;
    }
    Ok(())
}

fn read_line(
    agent: &str,
    subagent: bool,
    c: &mut Cursor,
    at: u64,
    line: &str,
    out: &mut Vec<Observation>,
) {
    match agent {
        "claude" => claude_line(c, at, line, subagent, out),
        "codex" => codex_line(c, at, line, out),
        _ => pi_line(c, at, line, out),
    }
}

fn poll_file(
    agent: &str,
    sid: &str,
    path: &Path,
    subagent: bool,
    cursors: &mut Cursors,
    polled: &mut Polled,
    limit: usize,
) -> Result<(), Unreadable> {
    let cursor = cursor_of(cursors, agent, sid, path);
    let out = &mut polled.observations;
    let (start, mut full) = (out.len(), false);
    poll_lines_until(path, cursor, |c, at, line| {
        read_line(agent, subagent, c, at, line, out);
        full = out.len() - start >= limit;
        !full
    })
    .map_err(|e| Unreadable::Failed(format!("reading {}: {e}", path.display())))?;
    polled.more |= full;
    polled.files.push(path.to_path_buf());
    Ok(())
}

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

pub(crate) fn n(v: &Value, pointer: &str) -> u64 {
    v.pointer(pointer).and_then(Value::as_u64).unwrap_or(0)
}

pub(crate) fn first_n(v: &Value, keys: &[&str]) -> u64 {
    keys.iter()
        .find_map(|k| v.get(*k).and_then(Value::as_u64))
        .unwrap_or(0)
}

pub(crate) fn ts(raw: Option<&str>) -> String {
    raw.and_then(clock::parse)
        .map(clock::stamp)
        .unwrap_or_else(|| raw.unwrap_or_default().to_string())
}

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

fn skim_type(line: &str) -> Option<Option<Cow<'_, str>>> {
    #[derive(Deserialize)]
    struct Head<'a> {
        #[serde(rename = "type", borrow, default)]
        kind: Option<Cow<'a, str>>,
    }
    serde_json::from_str::<Head>(line).ok().map(|h| h.kind)
}

fn skims_as_other(line: &str, kinds: &[&str]) -> bool {
    skim_type(line).is_some_and(|t| !t.is_some_and(|t| kinds.contains(&&*t)))
}

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

pub(crate) fn claude_line(
    c: &mut Cursor,
    at: u64,
    line: &str,
    subagent: bool,
    out: &mut Vec<Observation>,
) {
    if !skims_as_other(line, &["assistant"]) {
        claude_whole(c, at, line, subagent, out);
    }
}

fn claude_whole(c: &mut Cursor, _at: u64, line: &str, subagent: bool, out: &mut Vec<Observation>) {
    if let Some(v) = claude_assistant(line) {
        out.extend(claude_observation(c, &v, subagent));
    }
}

fn claude_assistant(line: &str) -> Option<Value> {
    serde_json::from_str::<Value>(line)
        .ok()
        .filter(|v| v.get("type").and_then(Value::as_str) == Some("assistant"))
}

fn claude_observation(c: &mut Cursor, v: &Value, subagent: bool) -> Option<Observation> {
    let when = ts(v.get("timestamp").and_then(Value::as_str));
    let model = v
        .pointer("/message/model")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if model == "<synthetic>" {
        return claude_refusal(v, when);
    }
    let id = v.pointer("/message/id").and_then(Value::as_str)?;
    let u = v.pointer("/message/usage").filter(|u| u.is_object())?;
    let (cw5m, cw1h) = cache_write_split(u, first_n(u, &["cache_creation_input_tokens"]));
    let tokens = TokenClasses {
        input: first_n(u, &["input_tokens"]),
        cache_write_5m: cw5m,
        cache_write_1h: cw1h,
        cache_read: first_n(u, &["cache_read_input_tokens"]),
        output: first_n(u, &["output_tokens"]),
        reasoning: n(u, "/output_tokens_details/thinking_tokens"),
    };
    let effort = v.get("effort").and_then(Value::as_str).map(str::to_owned);
    let (event_id, tokens, delta, seen) = emit_or_correct(c.recalled(id), id, tokens)?;
    c.remember(seen);
    Some(Observation::Usage(RawUsage {
        ts: when,
        event_id,
        model: model.to_string(),
        effort,
        tokens,
        subagent,
        delta,
        tool_nested: false,
        harness_cost: None,
    }))
}

fn claude_refusal(v: &Value, when: String) -> Option<Observation> {
    (v.get("error").and_then(Value::as_str) == Some("rate_limit")).then(|| {
        Observation::Quota(QuotaSignal::Refusal {
            pool: "claude".into(),
            at: when,
            what: claude_refusal_label(v),
        })
    })
}

fn cache_write_split(u: &Value, written: u64) -> (u64, u64) {
    let cc = u.get("cache_creation").unwrap_or(&Value::Null);
    let one_hour = n(cc, "/ephemeral_1h_input_tokens");
    let five = cc
        .get("ephemeral_5m_input_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(written.saturating_sub(one_hour));
    (five, one_hour)
}

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

fn find_key<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Object(map) => map
            .get(key)
            .or_else(|| map.values().find_map(|x| find_key(x, key))),
        Value::Array(list) => list.iter().find_map(|x| find_key(x, key)),
        _ => None,
    }
}

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

pub(crate) fn codex_rollout_windows(rl: &Value) -> Vec<Window> {
    ["primary", "secondary"]
        .iter()
        .filter_map(|slot| rl.get(slot).and_then(rollout_window))
        .collect()
}

fn rollout_window(w: &Value) -> Option<Window> {
    let minutes = w.get("window_minutes")?.as_u64()?;
    let used = w.get("used_percent").and_then(Value::as_f64).unwrap_or(0.0) / 100.0;
    let resets = w
        .get("resets_at")
        .and_then(Value::as_i64)
        .and_then(clock::from_epoch)
        .map(clock::stamp);
    Some(Window::new(minutes, used, resets, None))
}

const CODEX_KINDS: &[&str] = &[
    "session_meta",
    "turn_context",
    "token_usage_record",
    "event_msg",
];

pub(crate) fn codex_line(c: &mut Cursor, at: u64, line: &str, out: &mut Vec<Observation>) {
    if !skims_as_other(line, CODEX_KINDS) {
        codex_whole(c, at, line, out);
    }
}

fn codex_whole(c: &mut Cursor, at: u64, line: &str, out: &mut Vec<Observation>) {
    let Ok(v) = serde_json::from_str::<Value>(line) else {
        return;
    };
    let when = ts(v.get("timestamp").and_then(Value::as_str));
    let kind = v.get("type").and_then(Value::as_str).unwrap_or_default();
    let payload = v.get("payload").unwrap_or(&Value::Null);
    match kind {
        "session_meta" => codex_session_meta(c, payload),
        "turn_context" => codex_turn_context(c, payload),
        "token_usage_record" => codex_usage_record(c, payload, at, when, out),
        "event_msg" => codex_event_msg(c, payload, &when, out),
        _ => {}
    }
}

fn codex_session_meta(c: &mut Cursor, payload: &Value) {
    let version = payload
        .get("cli_version")
        .and_then(Value::as_str)
        .and_then(parse_version);
    if let Some(ver) = version {
        c.records_mode = Some(ver >= CODEX_RECORDS_SINCE);
    }
}

fn codex_turn_context(c: &mut Cursor, payload: &Value) {
    if let Some(m) = payload.get("model").and_then(Value::as_str) {
        c.model = m.to_string();
    }
    if let Some(e) = payload.get("effort").and_then(Value::as_str) {
        c.effort = Some(e.to_string());
    }
}

fn codex_usage_record(
    c: &mut Cursor,
    payload: &Value,
    at: u64,
    when: String,
    out: &mut Vec<Observation>,
) {
    let records = *c.records_mode.get_or_insert(true);
    let Some(u) = payload.get("usage").filter(|_| records) else {
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

fn codex_event_msg(c: &mut Cursor, payload: &Value, when: &str, out: &mut Vec<Observation>) {
    let msg = payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if msg == "token_count" {
        out.extend(codex_quota_snapshot(payload, when));
        if c.records_mode != Some(true) {
            codex_fallback(c, payload, when, out);
        }
    }
    if find_key(payload, "codex_error_info").and_then(Value::as_str) == Some("usage_limit_exceeded")
    {
        out.push(Observation::Quota(QuotaSignal::Refusal {
            pool: "codex".into(),
            at: when.to_string(),
            what: "usage_limit_exceeded".into(),
        }));
    }
}

fn codex_quota_snapshot(payload: &Value, when: &str) -> Option<Observation> {
    let windows = codex_rollout_windows(payload.get("rate_limits")?);
    (!windows.is_empty()).then(|| {
        Observation::Quota(QuotaSignal::Snapshot {
            pool: "codex".into(),
            at: when.to_string(),
            windows,
            ordinary_usage_allowed: None,
        })
    })
}

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

pub(crate) fn pi_line(_c: &mut Cursor, at: u64, line: &str, out: &mut Vec<Observation>) {
    let Some((v, tokens)) = pi_usage(line) else {
        return;
    };
    let message = v.get("message").unwrap_or(&v);
    let role = message.get("role").and_then(Value::as_str);
    let event_id = v
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("@{at}"));
    out.push(Observation::Usage(RawUsage {
        ts: pi_when(&v, message),
        event_id,
        model: pi_model(message),
        effort: None,
        tokens,
        subagent: false,
        delta: false,
        tool_nested: role == Some("toolResult"),
        harness_cost: None,
    }));
}

fn pi_usage(line: &str) -> Option<(Value, TokenClasses)> {
    let v: Value = serde_json::from_str(line).ok()?;
    let message = v.get("message").unwrap_or(&v);
    let tokens = Some(message)
        .filter(|m| {
            matches!(
                m.get("role").and_then(Value::as_str),
                Some("assistant" | "toolResult")
            )
        })
        .and_then(|m| m.get("usage"))
        .map(pi_tokens)
        .filter(|t| !t.is_zero())?;
    Some((v, tokens))
}

fn pi_tokens(u: &Value) -> TokenClasses {
    let written = first_n(u, &["cacheWrite", "cache_creation_input_tokens"]);
    let one_hour = first_n(u, &["cacheWrite1h"]).min(written);
    TokenClasses {
        input: first_n(u, &["input", "input_tokens"]),
        cache_write_5m: written - one_hour,
        cache_write_1h: one_hour,
        cache_read: first_n(u, &["cacheRead", "cache_read_input_tokens"]),
        output: first_n(u, &["output", "output_tokens"]),
        reasoning: first_n(u, &["reasoning"]),
    }
}

fn pi_model(message: &Value) -> String {
    let model = message
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match message.get("provider").and_then(Value::as_str) {
        Some(p) if !model.is_empty() && !model.contains('/') => format!("{p}/{model}"),
        _ => model.to_string(),
    }
}

fn pi_when(v: &Value, message: &Value) -> String {
    match v.get("timestamp").and_then(Value::as_str) {
        Some(t) => ts(Some(t)),
        None => message
            .get("timestamp")
            .and_then(Value::as_i64)
            .and_then(clock::from_epoch_ms)
            .map(clock::stamp)
            .unwrap_or_default(),
    }
}

pub(crate) fn safe_id(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

pub(crate) fn check_opencode(
    db: &Path,
    session_id: &str,
    valid: fn(&str) -> bool,
) -> Result<(), Unreadable> {
    if !valid(session_id) {
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
    Ok(())
}

pub(crate) fn row_data(row: &Value) -> Value {
    match row.get("data") {
        Some(Value::String(s)) => serde_json::from_str(s).unwrap_or(Value::Null),
        other => other.cloned().unwrap_or(Value::Null),
    }
}

pub(crate) fn opencode_poll(
    db: &Path,
    sqlite3: &Path,
    session_id: &str,
    c: &mut Cursor,
    out: &mut Vec<Observation>,
) -> Result<(), Unreadable> {
    check_opencode(db, session_id, safe_id)?;
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
        opencode_message(c, &id, &row_data(&row), out);
    }
    c.quiet_ticks = if grew {
        0
    } else {
        c.quiet_ticks.saturating_add(1)
    };
    Ok(())
}

pub(crate) fn opencode_message(c: &mut Cursor, id: &str, data: &Value, out: &mut Vec<Observation>) {
    out.extend(opencode_refusal(data));
    out.extend(opencode_usage(c, id, data));
}

fn opencode_refusal(data: &Value) -> Option<Observation> {
    data.get("error")
        .filter(|e| e.to_string().contains("FreeUsageLimitError"))?;
    let at = data
        .pointer("/time/completed")
        .or_else(|| data.pointer("/time/created"))
        .and_then(Value::as_i64)
        .and_then(clock::from_epoch_ms)
        .map(clock::stamp)
        .unwrap_or_default();
    Some(Observation::Quota(QuotaSignal::Refusal {
        pool: "opencode-zen".into(),
        at,
        what: "FreeUsageLimitError".into(),
    }))
}

fn opencode_usage(c: &mut Cursor, id: &str, data: &Value) -> Option<Observation> {
    if data.get("role").and_then(Value::as_str) != Some("assistant") {
        return None;
    }
    let completed = data.pointer("/time/completed").and_then(Value::as_i64)?;
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
    let (event_id, tokens, delta, seen) = emit_or_correct(c.emitted.get(id), id, tokens)?;
    c.emitted.insert(id.to_string(), seen);
    Some(Observation::Usage(RawUsage {
        ts: when,
        event_id,
        model,
        effort: None,
        tokens,
        subagent: false,
        delta,
        tool_nested: false,
        harness_cost,
    }))
}

pub(crate) fn sqlite_json(bin: &Path, db: &Path, query: &str) -> Result<Vec<Value>, Unreadable> {
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
        Ok(Vec::new())
    } else {
        serde_json::from_str(text.trim())
            .map_err(|e| Unreadable::Failed(format!("sqlite3 output from {}: {e}", db.display())))
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn lines() -> Vec<String> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/telemetry");
        let mut files = Vec::new();
        let mut dirs = vec![root.join("claude"), root.join("codex")];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.extension().is_some_and(|e| e == "jsonl") {
                    files.push(path);
                }
            }
        }
        files.sort();
        let mut out: Vec<String> = files
            .iter()
            .flat_map(|f| {
                std::fs::read_to_string(f)
                    .unwrap()
                    .lines()
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .collect();
        assert!(out.len() > 20, "{} fixture lines", out.len());
        let assistant = out
            .iter()
            .find(|l| l.contains("\"assistant\""))
            .unwrap()
            .clone();
        let token_count = out
            .iter()
            .find(|l| l.contains("\"token_count\""))
            .unwrap()
            .clone();
        out.extend(
            [
                assistant.replacen("{", "{\"type\":\"user\",", 1),
                token_count.replacen("{", "{\"type\":\"response_item\",", 1),
                assistant.replace("\"assistant\"", "\"assist\\u0061nt\""),
                r#"{"type":7,"message":{"id":"m","usage":{"output_tokens":1}}}"#.into(),
                r#"{"message":{"id":"m","usage":{"output_tokens":1}}}"#.into(),
                r#"["assistant"]"#.into(),
                "not json".into(),
                r#"{"type":"user","message":{"content":"#.into(),
                format!(
                    r#"{{"type":"user","message":{{"content":"{}"}}}}"#,
                    "x".repeat(1 << 16)
                ),
            ]
            .map(String::from),
        );
        out
    }

    #[test]
    fn nfr_02_the_skim_reads_what_the_whole_parse_reads() {
        let mut skipped = 0;
        for subagent in [false, true] {
            let (mut fast, mut whole) = (Cursor::default(), Cursor::default());
            for (at, line) in lines().iter().enumerate() {
                let (mut a, mut b) = (Vec::new(), Vec::new());
                claude_line(&mut fast, at as u64, line, subagent, &mut a);
                claude_whole(&mut whole, at as u64, line, subagent, &mut b);
                assert_eq!(a, b, "{line}");
                assert_eq!(fast, whole, "{line}");
                skipped += usize::from(skims_as_other(line, &["assistant"]));
            }
        }
        let (mut fast, mut whole) = (Cursor::default(), Cursor::default());
        for (at, line) in lines().iter().enumerate() {
            let (mut a, mut b) = (Vec::new(), Vec::new());
            codex_line(&mut fast, at as u64, line, &mut a);
            codex_whole(&mut whole, at as u64, line, &mut b);
            assert_eq!(a, b, "{line}");
            assert_eq!(fast, whole, "{line}");
            skipped += usize::from(skims_as_other(line, CODEX_KINDS));
        }
        assert!(skipped > 10, "the skim skipped {skipped} lines");
    }
}

#[cfg(test)]
mod line_reader_tests {
    use super::*;

    fn run(
        read: impl Fn(&mut Cursor, u64, &str, &mut Vec<Observation>),
        lines: &[&str],
    ) -> (Vec<Observation>, Cursor) {
        let (mut c, mut out) = (Cursor::default(), Vec::new());
        for (at, line) in lines.iter().enumerate() {
            read(&mut c, at as u64 * 100, line, &mut out);
        }
        (out, c)
    }

    fn usages(obs: &[Observation]) -> Vec<&RawUsage> {
        obs.iter()
            .filter_map(|o| match o {
                Observation::Usage(u) => Some(u),
                Observation::Quota(_) => None,
            })
            .collect()
    }

    fn refusals(obs: &[Observation]) -> Vec<(String, String, String)> {
        obs.iter()
            .filter_map(|o| match o {
                Observation::Quota(QuotaSignal::Refusal { pool, at, what }) => {
                    Some((pool.clone(), at.clone(), what.clone()))
                }
                _ => None,
            })
            .collect()
    }

    fn claude(line: &str) -> Vec<Observation> {
        run(|c, at, l, o| claude_whole(c, at, l, false, o), &[line]).0
    }

    #[test]
    fn claude_whole_guards_and_the_cache_write_split() {
        for line in [
            r#"{"type":"user"}"#,
            "{",
            r#"{"type":"assistant","message":{"usage":{"output_tokens":1}}}"#,
            r#"{"type":"assistant","message":{"id":"a","usage":5}}"#,
        ] {
            assert!(claude(line).is_empty(), "{line}");
        }
        let a = r#"{"type":"assistant","timestamp":"2026-09-20T12:00:00Z","effort":"high","message":{"id":"a","model":"m","usage":{"input_tokens":1,"output_tokens":2,"cache_read_input_tokens":3,"cache_creation_input_tokens":10,"cache_creation":{"ephemeral_1h_input_tokens":4},"output_tokens_details":{"thinking_tokens":7}}}}"#;
        let obs = claude(a);
        let u = usages(&obs)[0];
        assert_eq!(
            (
                u.tokens.cache_write_5m,
                u.tokens.cache_write_1h,
                u.tokens.reasoning
            ),
            (6, 4, 7)
        );
        assert_eq!(
            (u.event_id.as_str(), u.model.as_str(), u.effort.as_deref()),
            ("a", "m", Some("high"))
        );
        let b = a.replace(r#""cache_creation":{"ephemeral_1h_input_tokens":4},"#, "");
        let obs = claude(&b);
        assert_eq!(
            (
                usages(&obs)[0].tokens.cache_write_5m,
                usages(&obs)[0].tokens.cache_write_1h
            ),
            (10, 0)
        );
        let c = a.replace(
            r#""ephemeral_1h_input_tokens":4"#,
            r#""ephemeral_1h_input_tokens":4,"ephemeral_5m_input_tokens":2"#,
        );
        assert_eq!(usages(&claude(&c))[0].tokens.cache_write_5m, 2);
    }

    #[test]
    fn claude_whole_synthetic_lines_only_refuse_on_rate_limit() {
        let line = |err: &str| {
            format!(
                r#"{{"type":"assistant","timestamp":"2026-09-20T12:00:00Z","error":"{err}","message":{{"id":"x","model":"<synthetic>","content":[{{"text":"You hit your weekly limit"}}],"usage":{{"output_tokens":1}}}}}}"#
            )
        };
        let obs = claude(&line("rate_limit"));
        assert!(usages(&obs).is_empty());
        assert_eq!(
            refusals(&obs),
            vec![("claude".into(), "2026-09-20T12:00:00Z".into(), "7d".into())]
        );
        assert!(claude(&line("other")).is_empty());
    }

    #[test]
    fn claude_whole_repeats_emit_only_the_growth() {
        let line = |out: u64| {
            format!(
                r#"{{"type":"assistant","message":{{"id":"a","model":"m","usage":{{"output_tokens":{out}}}}}}}"#
            )
        };
        let (a, b, c) = (line(5), line(5), line(8));
        let (obs, _) = run(|c, at, l, o| claude_whole(c, at, l, true, o), &[&a, &b, &c]);
        let u = usages(&obs);
        assert_eq!(u.len(), 2, "the repeat adds nothing");
        assert_eq!(
            (u[0].event_id.as_str(), u[0].delta, u[0].subagent),
            ("a", false, true)
        );
        assert_eq!(
            (u[1].event_id.as_str(), u[1].delta, u[1].tokens.output),
            ("a+1", true, 3)
        );
    }

    fn codex(lines: &[&str]) -> Vec<Observation> {
        run(codex_whole, lines).0
    }

    #[test]
    fn codex_whole_records_depend_on_the_version() {
        let record = r#"{"timestamp":"2026-09-20T12:00:00Z","type":"token_usage_record","payload":{"response_id":"r1","usage":{"input_tokens":10,"cached_input_tokens":4,"output_tokens":2,"reasoning_output_tokens":1}}}"#;
        let ctx = r#"{"type":"turn_context","payload":{"model":"gpt-x","effort":"low"}}"#;
        let meta =
            |v: &str| format!(r#"{{"type":"session_meta","payload":{{"cli_version":"{v}"}}}}"#);
        let obs = codex(&[ctx, record]);
        let u = usages(&obs)[0];
        assert_eq!(
            (u.event_id.as_str(), u.model.as_str(), u.effort.as_deref()),
            ("r1", "gpt-x", Some("low"))
        );
        assert_eq!(
            (
                u.tokens.input,
                u.tokens.cache_read,
                u.tokens.output,
                u.tokens.reasoning
            ),
            (6, 4, 2, 1)
        );
        assert!(usages(&codex(&[&meta("0.156.9"), record])).is_empty());
        assert_eq!(usages(&codex(&[&meta("v0.157.0"), record])).len(), 1);
        assert_eq!(usages(&codex(&[&meta("nope"), record])).len(), 1);
        let anon = record.replace(r#""response_id":"r1","#, "");
        let (obs, _) = run(codex_whole, &[ctx, &anon]);
        assert_eq!(usages(&obs)[0].event_id, "rec@100");
        let none = r#"{"type":"token_usage_record","payload":{}}"#;
        assert!(codex(&[none]).is_empty());
    }

    #[test]
    fn codex_whole_event_msg_snapshots_fallback_and_refusal() {
        let tc = |total: u64, rl: &str| {
            format!(
                r#"{{"timestamp":"2026-09-20T12:00:00Z","type":"event_msg","payload":{{"type":"token_count"{rl},"info":{{"total_token_usage":{{"input_tokens":{total},"total_tokens":{total}}}}}}}}}"#
            )
        };
        let rl = r#","rate_limits":{"primary":{"window_minutes":300,"used_percent":50,"resets_at":1790000000},"secondary":{"used_percent":9},"other":{"window_minutes":60}}"#;
        let (a, b) = (tc(10, rl), tc(10, ""));
        let obs = codex(&[&a]);
        let snap: Vec<_> = obs
            .iter()
            .filter_map(|o| match o {
                Observation::Quota(QuotaSignal::Snapshot { pool, windows, .. }) => {
                    Some((pool.clone(), windows.clone()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].0, "codex");
        assert_eq!(snap[0].1.len(), 1);
        assert_eq!((snap[0].1[0].minutes, snap[0].1[0].used), (300, 0.5));
        assert_eq!(
            snap[0].1[0].resets_at.as_deref(),
            Some("2026-09-21T14:13:20Z")
        );
        let ids: Vec<_> = usages(&codex(&[&a, &b, &tc(25, "")]))
            .iter()
            .map(|u| (u.event_id.clone(), u.tokens.input))
            .collect();
        assert_eq!(
            ids,
            vec![("tc:10".to_string(), 10), ("tc:25".to_string(), 15)]
        );
        let record = r#"{"type":"token_usage_record","payload":{"response_id":"r","usage":{"output_tokens":1}}}"#;
        assert_eq!(usages(&codex(&[record, &a])).len(), 1);
        let err = r#"{"timestamp":"2026-09-20T12:00:00Z","type":"event_msg","payload":{"type":"error","nested":{"codex_error_info":"usage_limit_exceeded"}}}"#;
        assert_eq!(
            refusals(&codex(&[err])),
            vec![(
                "codex".into(),
                "2026-09-20T12:00:00Z".into(),
                "usage_limit_exceeded".into()
            )]
        );
        let other = err.replace("usage_limit_exceeded", "other");
        assert!(codex(&[&other]).is_empty());
    }

    fn pi(line: &str) -> Vec<Observation> {
        run(pi_line, &[line]).0
    }

    #[test]
    fn pi_line_roles_models_times_and_ids() {
        let line = r#"{"id":"e1","timestamp":"2026-09-20T12:00:00Z","message":{"role":"assistant","provider":"acme","model":"m1","usage":{"input":5,"output":2,"cacheWrite":9,"cacheWrite1h":4,"cacheRead":1}}}"#;
        let obs = pi(line);
        let u = usages(&obs)[0];
        assert_eq!(
            (u.event_id.as_str(), u.model.as_str(), u.ts.as_str()),
            ("e1", "acme/m1", "2026-09-20T12:00:00Z")
        );
        assert_eq!(
            (
                u.tokens.cache_write_5m,
                u.tokens.cache_write_1h,
                u.tokens.cache_read
            ),
            (5, 4, 1)
        );
        assert!(!u.tool_nested);
        assert_eq!(
            usages(&pi(&line.replace(r#""m1""#, r#""x/m1""#)))[0].model,
            "x/m1"
        );
        assert_eq!(
            usages(&pi(&line.replace(r#""provider":"acme","#, "")))[0].model,
            "m1"
        );
        assert!(usages(&pi(&line.replace("assistant", "toolResult")))[0].tool_nested);
        assert!(pi(&line.replace("assistant", "user")).is_empty());
        assert!(pi(&line.replace(
            r#""input":5,"output":2,"cacheWrite":9,"cacheWrite1h":4,"cacheRead":1"#,
            r#""input":0"#
        ))
        .is_empty());
        assert!(pi("nope").is_empty());
        let bare = line
            .replace(r#""id":"e1","timestamp":"2026-09-20T12:00:00Z","#, "")
            .replace(
                r#""role":"assistant","#,
                r#""role":"assistant","timestamp":1790000000000,"#,
            );
        let (obs, _) = run(pi_line, &["x", &bare]);
        let u = usages(&obs)[0];
        assert_eq!(
            (u.event_id.as_str(), u.ts.as_str()),
            ("@100", "2026-09-21T14:13:20Z")
        );
    }

    fn opencode(data: &str) -> Vec<Observation> {
        let mut c = Cursor::default();
        let mut out = Vec::new();
        opencode_message(&mut c, "m1", &serde_json::from_str(data).unwrap(), &mut out);
        out
    }

    #[test]
    fn opencode_message_refusal_and_usage() {
        let done = r#"{"role":"assistant","time":{"created":1790000000000,"completed":1790000060000},"providerID":"p","modelID":"m","cost":0.5,"tokens":{"input":1,"output":2,"reasoning":3,"cache":{"read":4,"write":5}}}"#;
        let obs = opencode(done);
        let u = usages(&obs)[0];
        assert_eq!(
            (u.event_id.as_str(), u.model.as_str(), u.harness_cost),
            ("m1", "p/m", Some(0.5))
        );
        assert_eq!(
            (
                u.tokens.input,
                u.tokens.output,
                u.tokens.reasoning,
                u.tokens.cache_read,
                u.tokens.cache_write_5m
            ),
            (1, 2, 3, 4, 5)
        );
        assert_eq!(u.ts, "2026-09-21T14:14:20Z");
        assert!(opencode(&done.replace("assistant", "user")).is_empty());
        assert!(opencode(&done.replace(r#","completed":1790000060000"#, "")).is_empty());
        let refused =
            r#"{"error":{"name":"FreeUsageLimitError"},"time":{"created":1790000000000}}"#;
        assert_eq!(
            refusals(&opencode(refused)),
            vec![(
                "opencode-zen".into(),
                "2026-09-21T14:13:20Z".into(),
                "FreeUsageLimitError".into()
            )]
        );
        let mut c = Cursor::default();
        let mut out = Vec::new();
        let v: Value = serde_json::from_str(done).unwrap();
        opencode_message(&mut c, "m1", &v, &mut out);
        opencode_message(&mut c, "m1", &v, &mut out);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn sqlite_json_reads_rows_empty_output_and_failures() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("t.db");
        std::fs::write(&db, "").unwrap();
        let sqlite = Path::new("sqlite3");
        if std::process::Command::new(sqlite)
            .arg("-version")
            .output()
            .is_err()
        {
            return;
        }
        let rows = sqlite_json(sqlite, &db, "SELECT 1 AS a UNION SELECT 2;").unwrap();
        assert_eq!(rows.len(), 2);
        assert!(sqlite_json(sqlite, &db, "SELECT 1 WHERE 0;")
            .unwrap()
            .is_empty());
        assert!(
            matches!(sqlite_json(sqlite, &db, "SELEC;"), Err(Unreadable::Failed(m)) if m.contains("sqlite3 failed on"))
        );
        let missing = Path::new("/nonexistent/sqlite3-r-telemetry-core");
        assert_eq!(
            sqlite_json(missing, &db, "SELECT 1;"),
            Err(Unreadable::NotRead("sqlite3 not found".into()))
        );
    }

    #[test]
    fn locate_errors_name_what_was_looked_for() {
        let home = tempfile::tempdir().unwrap();
        let loc = Locations::under_home(home.path(), &crate::runtime::Inherited::default());
        for agent in ["claude", "codex", "pi", "prime"] {
            assert!(
                matches!(
                    locate(&loc, agent, "nope"),
                    Err(Unreadable::NoTranscript(_))
                ),
                "{agent}"
            );
        }
        assert!(matches!(
            locate(&loc, "opencode", "x"),
            Ok(Located::OpenCode(_))
        ));
        assert_eq!(
            locate(&loc, "none", "x").err(),
            Some(Unreadable::NotRead("agent none is not read".into()))
        );
        assert_eq!(
            locate(&loc, "agy", "x").err(),
            Some(Unreadable::NotRead("unknown agent 'agy'".into()))
        );
    }

    #[test]
    fn rollout_windows_skip_a_slot_without_a_duration() {
        let rl: Value = serde_json::from_str(r#"{"primary":{"window_minutes":300,"used_percent":25},"secondary":{"window_minutes":10080,"used_percent":50,"resets_at":1790000000},"other":{"window_minutes":1}}"#).unwrap();
        let w = codex_rollout_windows(&rl);
        let got: Vec<_> = w
            .iter()
            .map(|w| (w.name.as_str(), w.used, w.resets_at.clone()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("5h", 0.25, None),
                ("7d", 0.5, Some("2026-09-21T14:13:20Z".to_string()))
            ]
        );
        let none: Value =
            serde_json::from_str(r#"{"primary":5,"secondary":{"used_percent":1}}"#).unwrap();
        assert!(codex_rollout_windows(&none).is_empty());
    }
}
