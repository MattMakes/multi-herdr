use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::readers::{
    check_opencode, first_n, locate, n, row_data, sqlite_json, Located, Unreadable,
};
use crate::clock;
use crate::usage::Locations;

const TAIL_WINDOW: u64 = 256 << 10;

const SEARCH_CHUNK: usize = 1 << 20;

const CACHE_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionMark {
    pub at: String,
    pub offset: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reading {
    pub tokens: Option<u64>,
    pub provisional: bool,
    pub pending: bool,
    pub window: Option<u64>,
    pub model: Option<String>,
    pub last_compaction: Option<CompactionMark>,
    pub marks: usize,
    pub marks_at: Vec<String>,
    pub transcript: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Usable {
    pub offset: u64,
    pub tokens: u64,
    pub window: Option<u64>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tail {
    pub usable: Option<Usable>,
    pub bytes_read: u64,
}

pub fn read_current(
    loc: &Locations,
    cache_dir: &Path,
    agent: &str,
    session_id: Option<&str>,
) -> Result<Reading, Unreadable> {
    let Some(sid) = session_id.filter(|s| !s.is_empty()) else {
        return Err(Unreadable::NoSessionId);
    };
    if agent == "antigravity" {
        return Err(Unreadable::NotRead(format!("no {agent} context reader")));
    }
    match locate(loc, agent, sid)? {
        Located::Files(files) => files
            .into_iter()
            .next()
            .ok_or_else(|| Unreadable::NotRead(format!("no {agent} context reader")))
            .and_then(|path| read_transcript(path, cache_dir, agent)),
        Located::OpenCode(db) => read_opencode(&db, &loc.sqlite3, sid),
    }
}

fn read_transcript(path: PathBuf, cache_dir: &Path, agent: &str) -> Result<Reading, Unreadable> {
    let failed = |e: std::io::Error| Unreadable::Failed(format!("reading {}: {e}", path.display()));
    let marks = update_marks(&path, cache_dir, agent).map_err(failed)?;
    let tail = scan_tail(&path, agent).map_err(failed)?;
    let last = marks.iter().max_by_key(|m| m.offset).cloned();
    let pending = is_pending(last.as_ref(), tail.usable.as_ref());
    let (tokens, provisional) = current_tokens(pending, last.as_ref(), &tail, agent);
    let (window, model) = tail.usable.map_or((None, None), |u| (u.window, u.model));
    Ok(Reading {
        tokens,
        provisional,
        pending,
        window,
        model,
        last_compaction: last,
        marks: marks.len(),
        marks_at: marks.iter().map(|m| m.at.clone()).collect(),
        transcript: path,
    })
}

fn is_pending(last: Option<&CompactionMark>, usable: Option<&Usable>) -> bool {
    last.is_some_and(|m| usable.is_none_or(|u| m.offset > u.offset))
}

fn current_tokens(
    pending: bool,
    last: Option<&CompactionMark>,
    tail: &Tail,
    agent: &str,
) -> (Option<u64>, bool) {
    if !pending {
        return (tail.usable.as_ref().map(|u| u.tokens), false);
    }
    let post = last
        .and_then(|m| m.post_tokens)
        .filter(|_| agent == "claude");
    (post, post.is_some())
}

pub fn scan_tail(path: &Path, agent: &str) -> std::io::Result<Tail> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let mut window = TAIL_WINDOW;
    let mut bytes_read = 0;
    loop {
        let from = len.saturating_sub(window);
        let mut buf = Vec::with_capacity((len - from) as usize);
        file.seek(SeekFrom::Start(from))?;
        (&mut file).take(len - from).read_to_end(&mut buf)?;
        bytes_read += buf.len() as u64;
        let skip = first_whole_line(&buf, from);
        let usable = newest_usable(&buf[skip..], from + skip as u64, agent);
        if usable.is_some() || from == 0 {
            return Ok(Tail { usable, bytes_read });
        }
        window = window.saturating_mul(2);
    }
}

fn first_whole_line(buf: &[u8], from: u64) -> usize {
    if from == 0 {
        return 0;
    }
    buf.iter()
        .position(|b| *b == b'\n')
        .map_or(buf.len(), |i| i + 1)
}

fn lines_rev(buf: &[u8]) -> impl Iterator<Item = (usize, &[u8])> {
    let mut end = buf.len();
    std::iter::from_fn(move || {
        if end == 0 {
            return None;
        }
        let line_end = if buf[end - 1] == b'\n' { end - 1 } else { end };
        let start = buf[..line_end]
            .iter()
            .rposition(|b| *b == b'\n')
            .map_or(0, |i| i + 1);
        end = start;
        Some((start, &buf[start..line_end]))
    })
}

fn newest_usable(buf: &[u8], base: u64, agent: &str) -> Option<Usable> {
    let parse = usable_parser(agent);
    let mut values = lines_rev(buf)
        .filter_map(|(start, line)| Some((start, serde_json::from_slice::<Value>(line).ok()?)));
    let mut found = values.find_map(|(start, v)| parse(&v, base + start as u64))?;
    if reads_turn_context(agent) {
        found.model = codex_model_before(values.map(|(_, v)| v));
    }
    Some(found)
}

fn usable_parser(agent: &str) -> fn(&Value, u64) -> Option<Usable> {
    match agent {
        "claude" => claude_usable,
        "pi" | "prime" => pi_usable,
        _ => codex_usable,
    }
}

fn reads_turn_context(agent: &str) -> bool {
    !matches!(agent, "claude" | "pi" | "prime")
}

fn codex_model_before(mut values: impl Iterator<Item = Value>) -> Option<String> {
    values
        .find(|v| v.get("type").and_then(Value::as_str) == Some("turn_context"))
        .and_then(|v| {
            v.pointer("/payload/model")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
}

fn claude_usable(v: &Value, offset: u64) -> Option<Usable> {
    let model = v.pointer("/message/model").and_then(Value::as_str);
    let usage = Some(v)
        .filter(|v| is_main_assistant(v))
        .filter(|_| model != Some("<synthetic>"))
        .and_then(|v| v.pointer("/message/usage"))
        .filter(|u| u.is_object())
        .filter(|u| claude_input(u) > 0)?;
    let last_message = last_message_iteration(usage);
    let u = last_message.unwrap_or(usage);
    Some(Usable {
        offset,
        tokens: claude_input(u) + n(u, "/output_tokens"),
        window: None,
        model: model.map(str::to_owned),
    })
}

fn last_message_iteration(usage: &Value) -> Option<&Value> {
    usage
        .get("iterations")
        .and_then(Value::as_array)
        .and_then(|its| {
            its.iter().rev().find(|it| {
                matches!(
                    it.get("type").and_then(Value::as_str),
                    None | Some("message")
                )
            })
        })
        .filter(|it| claude_input(it) > 0)
}

fn is_main_assistant(v: &Value) -> bool {
    v.get("type").and_then(Value::as_str) == Some("assistant")
        && v.get("isSidechain").and_then(Value::as_bool) != Some(true)
}

fn claude_input(u: &Value) -> u64 {
    n(u, "/input_tokens") + n(u, "/cache_creation_input_tokens") + n(u, "/cache_read_input_tokens")
}

fn codex_usable(v: &Value, offset: u64) -> Option<Usable> {
    if v.get("type").and_then(Value::as_str) != Some("event_msg")
        || v.pointer("/payload/type").and_then(Value::as_str) != Some("token_count")
    {
        return None;
    }
    let info = v.pointer("/payload/info").filter(|i| i.is_object())?;
    let last = info.get("last_token_usage").filter(|u| u.is_object())?;
    let window = info.get("model_context_window").and_then(Value::as_u64);
    Some(Usable {
        offset,
        tokens: first_n(last, &["total_tokens"]),
        window,
        model: None,
    })
}

fn pi_usable(v: &Value, offset: u64) -> Option<Usable> {
    let message = pi_response(v)?;
    let usage = message.get("usage").filter(|u| u.is_object())?;
    let tokens = pi_tokens(usage);
    let model = message.get("model").and_then(Value::as_str);
    (tokens > 0).then(|| Usable {
        offset,
        tokens,
        window: None,
        model: model.map(str::to_owned),
    })
}

fn pi_response(v: &Value) -> Option<&Value> {
    let message = v.get("message")?;
    let failed = matches!(
        message.get("stopReason").and_then(Value::as_str),
        Some("aborted" | "error")
    );
    (v.get("type").and_then(Value::as_str) == Some("message")
        && message.get("role").and_then(Value::as_str) == Some("assistant")
        && !failed)
        .then_some(message)
}

fn pi_tokens(usage: &Value) -> u64 {
    match n(usage, "/totalTokens") {
        0 => {
            n(usage, "/input")
                + n(usage, "/output")
                + n(usage, "/cacheRead")
                + n(usage, "/cacheWrite")
        }
        total => total,
    }
}

fn read_opencode(db: &Path, sqlite3: &Path, sid: &str) -> Result<Reading, Unreadable> {
    check_opencode(db, sid, is_opencode_id)?;
    let rows = opencode_rows(db, sqlite3, sid)?;
    let (usable, marks) = fold_rows(&rows);
    let last = marks.last().cloned();
    let pending = is_pending(last.as_ref(), usable.as_ref());
    let (tokens, model) = match usable {
        Some(u) => ((!pending).then_some(u.tokens), u.model),
        None => (None, None),
    };
    Ok(Reading {
        tokens,
        provisional: false,
        pending,
        window: None,
        model,
        last_compaction: last,
        marks: marks.len(),
        marks_at: marks.iter().map(|m| m.at.clone()).collect(),
        transcript: db.to_path_buf(),
    })
}

fn is_opencode_id(sid: &str) -> bool {
    sid.strip_prefix("ses_")
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_alphanumeric()))
}

fn opencode_rows(db: &Path, sqlite3: &Path, sid: &str) -> Result<Vec<Value>, Unreadable> {
    let query = format!(
        "SELECT time_updated, data FROM message WHERE session_id = '{sid}' \
         AND json_extract(data, '$.role') = 'assistant' ORDER BY time_updated, id;"
    );
    sqlite_json(sqlite3, db, &query).map_err(|e| match e {
        Unreadable::NotRead(_) => Unreadable::NotRead("sqlite3 not on PATH".into()),
        other => other,
    })
}

fn fold_rows(rows: &[Value]) -> (Option<Usable>, Vec<CompactionMark>) {
    let mut usable: Option<Usable> = None;
    let mut marks = Vec::new();
    for row in rows {
        match opencode_row(row, usable.as_ref()) {
            Some(Row::Mark(mark)) => marks.push(mark),
            Some(Row::Usable(u)) => usable = Some(u),
            None => {}
        }
    }
    (usable, marks)
}

enum Row {
    Mark(CompactionMark),
    Usable(Usable),
}

fn opencode_row(row: &Value, usable: Option<&Usable>) -> Option<Row> {
    let updated = row.get("time_updated").and_then(Value::as_u64).unwrap_or(0);
    let data = row_data(row);
    if data.get("summary").and_then(Value::as_bool) == Some(true) {
        let at = data
            .pointer("/time/created")
            .and_then(Value::as_i64)
            .and_then(clock::from_epoch_ms)
            .map(clock::stamp)
            .unwrap_or_default();
        return Some(Row::Mark(CompactionMark {
            at,
            offset: updated,
            trigger: None,
            pre_tokens: usable.map(|u| u.tokens),
            post_tokens: None,
        }));
    }
    let finished = data.get("finish").is_some_and(|f| !f.is_null());
    let tokens = opencode_tokens(data.get("tokens").unwrap_or(&Value::Null));
    (finished && tokens > 0).then(|| {
        Row::Usable(Usable {
            offset: updated,
            tokens,
            window: None,
            model: opencode_model(&data),
        })
    })
}

fn opencode_tokens(t: &Value) -> u64 {
    match n(t, "/total") {
        0 => {
            n(t, "/input")
                + n(t, "/output")
                + n(t, "/reasoning")
                + n(t, "/cache/read")
                + n(t, "/cache/write")
        }
        total => total,
    }
}

fn opencode_model(data: &Value) -> Option<String> {
    let provider = data.get("providerID").and_then(Value::as_str);
    let model = data.get("modelID").and_then(Value::as_str)?;
    Some(match provider {
        Some(p) => format!("{p}/{model}"),
        None => model.to_string(),
    })
}

#[derive(Debug, Serialize, Deserialize)]
struct MarkerCache {
    v: u32,
    path: String,
    scanned_to: u64,
    marks: Vec<CompactionMark>,
}

fn cache_file(cache_dir: &Path, transcript: &str) -> PathBuf {
    let hash = Sha256::digest(transcript.as_bytes());
    let hex: String = hash[..8].iter().map(|b| format!("{b:02x}")).collect();
    cache_dir.join("markers").join(format!("{hex}.json"))
}

fn needle(agent: &str) -> &'static [u8] {
    match agent {
        "claude" => br#""subtype":"compact_boundary""#,
        "pi" | "prime" => br#""type":"compaction""#,
        _ => br#""type":"compacted""#,
    }
}

fn update_marks(
    path: &Path,
    cache_dir: &Path,
    agent: &str,
) -> std::io::Result<Vec<CompactionMark>> {
    let key = path.to_string_lossy().into_owned();
    let cache_path = cache_file(cache_dir, &key);
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let mut cache = load_cache(&cache_path, key, len);
    if cache.scanned_to == len {
        return Ok(cache.marks);
    }
    let needle = needle(agent);
    let from = cache.scanned_to.saturating_sub(needle.len() as u64 - 1);
    let scanned_to = search(&mut file, from, len, needle, agent, &mut cache.marks)?
        .unwrap_or(cache.scanned_to)
        .max(cache.scanned_to);
    if scanned_to != cache.scanned_to {
        cache.scanned_to = scanned_to;
        save_cache(&cache_path, &cache);
    }
    Ok(cache.marks)
}

fn load_cache(cache_path: &Path, key: String, len: u64) -> MarkerCache {
    std::fs::read(cache_path)
        .ok()
        .and_then(|b| serde_json::from_slice::<MarkerCache>(&b).ok())
        .filter(|c| c.v == CACHE_VERSION && c.path == key && c.scanned_to <= len)
        .unwrap_or(MarkerCache {
            v: CACHE_VERSION,
            path: key,
            scanned_to: 0,
            marks: Vec::new(),
        })
}

fn save_cache(cache_path: &Path, cache: &MarkerCache) {
    let Ok(bytes) = serde_json::to_vec(cache) else {
        return;
    };
    let Some(dir) = cache_path.parent() else {
        return;
    };
    if std::fs::create_dir_all(dir).is_ok() {
        let _ = crate::fsx::write_atomic(cache_path, &bytes, 0o600);
    }
}

fn search(
    file: &mut File,
    from: u64,
    len: u64,
    needle: &[u8],
    agent: &str,
    marks: &mut Vec<CompactionMark>,
) -> std::io::Result<Option<u64>> {
    let mut buf = vec![0u8; SEARCH_CHUNK];
    let mut pos = from;
    let mut last_newline = None;
    let mut checked_to = from;
    while pos < len {
        let want = SEARCH_CHUNK.min((len - pos) as usize);
        read_at(file, pos, &mut buf[..want])?;
        let chunk = &buf[..want];
        if let Some(i) = chunk.iter().rposition(|b| *b == b'\n') {
            last_newline = Some(pos + i as u64);
        }
        scan_chunk(file, chunk, pos, len, needle, agent, &mut checked_to, marks)?;
        if pos + want as u64 >= len {
            break;
        }
        pos += (want - (needle.len() - 1)) as u64;
    }
    marks.sort_by_key(|m| m.offset);
    Ok(last_newline.map(|i| i + 1))
}

#[allow(clippy::too_many_arguments)]
fn scan_chunk(
    file: &mut File,
    chunk: &[u8],
    pos: u64,
    len: u64,
    needle: &[u8],
    agent: &str,
    checked_to: &mut u64,
    marks: &mut Vec<CompactionMark>,
) -> std::io::Result<()> {
    for (i, window) in chunk.windows(needle.len()).enumerate() {
        let at = pos + i as u64;
        if at < *checked_to || window != needle {
            continue;
        }
        let (start, line) = line_around(file, at, len)?;
        *checked_to = start + line.len() as u64;
        if let Some(mark) = confirm(&line, start, agent).filter(|_| !has_mark(marks, start)) {
            marks.push(mark);
        }
    }
    Ok(())
}

fn read_at(file: &mut File, pos: u64, buf: &mut [u8]) -> std::io::Result<()> {
    file.seek(SeekFrom::Start(pos))?;
    file.read_exact(buf)
}

fn has_mark(marks: &[CompactionMark], offset: u64) -> bool {
    marks.iter().any(|m| m.offset == offset)
}

fn line_around(file: &mut File, at: u64, len: u64) -> std::io::Result<(u64, Vec<u8>)> {
    let mut buf = vec![0u8; LINE_STEP as usize];
    let start = line_start(file, at, &mut buf)?;
    let line = line_from(file, start, len, &mut buf)?;
    Ok((start, line))
}

const LINE_STEP: u64 = 64 << 10;

fn line_start(file: &mut File, at: u64, buf: &mut [u8]) -> std::io::Result<u64> {
    let mut start = at;
    while start > 0 {
        let from = start.saturating_sub(LINE_STEP);
        let want = (start - from) as usize;
        read_at(file, from, &mut buf[..want])?;
        if let Some(i) = buf[..want].iter().rposition(|b| *b == b'\n') {
            return Ok(from + i as u64 + 1);
        }
        start = from;
    }
    Ok(start)
}

fn line_from(file: &mut File, start: u64, len: u64, buf: &mut [u8]) -> std::io::Result<Vec<u8>> {
    let mut line = Vec::new();
    let mut pos = start;
    while pos < len {
        let want = LINE_STEP.min(len - pos) as usize;
        read_at(file, pos, &mut buf[..want])?;
        let end = buf[..want].iter().position(|b| *b == b'\n');
        line.extend_from_slice(&buf[..end.unwrap_or(want)]);
        if end.is_some() {
            break;
        }
        pos += want as u64;
    }
    Ok(line)
}

fn confirm(line: &[u8], offset: u64, agent: &str) -> Option<CompactionMark> {
    let v: Value = serde_json::from_slice(line).ok()?;
    let (kind, subtype, fields) = marker_kind(agent);
    let top = |key: &str| v.get(key).and_then(Value::as_str);
    if top("type") != Some(kind) || subtype.is_some_and(|s| top("subtype") != Some(s)) {
        return None;
    }
    let (trigger, pre_tokens, post_tokens) = fields(&v);
    Some(CompactionMark {
        at: top("timestamp").unwrap_or_default().to_string(),
        offset,
        trigger,
        pre_tokens,
        post_tokens,
    })
}

fn marker_kind(agent: &str) -> (&'static str, Option<&'static str>, MarkerFields) {
    match agent {
        "claude" => ("system", Some("compact_boundary"), |v| {
            let meta = v.get("compactMetadata").unwrap_or(&Value::Null);
            (
                meta.get("trigger")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                meta.get("preTokens").and_then(Value::as_u64),
                meta.get("postTokens").and_then(Value::as_u64),
            )
        }),
        "pi" | "prime" => ("compaction", None, |v| {
            (None, v.get("tokensBefore").and_then(Value::as_u64), None)
        }),
        _ => ("compacted", None, |v| {
            let pre = v.pointer("/payload/latest_token_usage_record/usage/total_tokens");
            (None, pre.and_then(Value::as_u64), None)
        }),
    }
}

type MarkerFields = fn(&Value) -> (Option<String>, Option<u64>, Option<u64>);
