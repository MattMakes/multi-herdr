//! The current context of one session (design section 6.6, CTX-02 to CTX-04).
//!
//! Current context is the harness's own comparison number from the newest
//! completed response, never a cumulative total (CTX-02). It is read from
//! the tail of the main transcript only, so a 160 MB transcript costs 1
//! window of 256 KiB (CTX-26).
//!
//! Compaction markers can be anywhere in the file. A small cache per
//! transcript, `<cache_dir>/markers/<16 hex>.json`, keeps the markers found
//! and the byte where the search stopped, so each read searches only the new
//! bytes. The cache is best effort: a read never fails, and never loses its
//! reading, because the cache cannot be written (a sandboxed Codex worker).
//!
//! | agent | usable line (the newest wins) | marker |
//! |---|---|---|
//! | claude | a main-chain `assistant` line with usage | `system` / `compact_boundary` |
//! | codex | an `event_msg` `token_count` with `info` | `compacted` |
//! | pi, prime | an assistant `message` that did not fail, with usage | `compaction` |
//! | opencode | the newest finished assistant row that is not a summary | a summary row |
//!
//! OpenCode keeps its sessions in a database, not a file: each read queries
//! it through `sqlite3`, and the summary rows are the markers, so it uses no
//! marker cache.

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

/// The first tail window. It doubles until it holds a usable line.
const TAIL_WINDOW: u64 = 256 << 10;

/// The size of 1 read of the marker search.
const SEARCH_CHUNK: usize = 1 << 20;

/// The marker cache format.
const CACHE_VERSION: u32 = 1;

/// 1 compaction marker in a transcript.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionMark {
    /// The marker's timestamp, as written.
    pub at: String,
    /// The byte offset of the marker line (the compaction job's proof).
    pub offset: u64,
    /// Claude only: `manual` or `auto`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_tokens: Option<u64>,
}

/// The current context of 1 session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reading {
    /// None: unknown, or pending without a figure. Never 0 for "unknown".
    pub tokens: Option<u64>,
    /// `tokens` come from a marker (Claude `postTokens`), not a response.
    pub provisional: bool,
    /// A marker is newer than the last usable response.
    pub pending: bool,
    /// The transcript's model window (Codex).
    pub window: Option<u64>,
    /// The transcript's model, if it names one.
    pub model: Option<String>,
    pub last_compaction: Option<CompactionMark>,
    /// The markers in the file.
    pub marks: usize,
    /// The time of each marker in the file, as written, oldest first
    /// (CTX-25: `compaction::policy::count_compactions`).
    pub marks_at: Vec<String>,
    pub transcript: PathBuf,
}

/// The newest usable line of a transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Usable {
    /// The byte offset of the line.
    pub offset: u64,
    pub tokens: u64,
    pub window: Option<u64>,
    pub model: Option<String>,
}

/// What a tail scan found, and what it cost.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tail {
    pub usable: Option<Usable>,
    /// The bytes read from the file, every window included.
    pub bytes_read: u64,
}

/// The current context of one session. `cache_dir` is `<state_root>/context`.
pub fn read_current(
    loc: &Locations,
    cache_dir: &Path,
    agent: &str,
    session_id: Option<&str>,
) -> Result<Reading, Unreadable> {
    let Some(sid) = session_id.filter(|s| !s.is_empty()) else {
        return Err(Unreadable::NoSessionId);
    };
    match locate(loc, agent, sid)? {
        // The main transcript; subagent files have their own context.
        Located::Files(files) => files
            .into_iter()
            .next()
            .ok_or_else(|| Unreadable::NotRead(format!("no {agent} context reader")))
            .and_then(|path| read_transcript(path, cache_dir, agent)),
        Located::OpenCode(db) => read_opencode(&db, &loc.sqlite3, sid),
    }
}

/// The current context of the transcript file `path`.
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

/// A marker is newer than the last usable response, or there is a marker and
/// no response.
fn is_pending(last: Option<&CompactionMark>, usable: Option<&Usable>) -> bool {
    last.is_some_and(|m| usable.is_none_or(|u| m.offset > u.offset))
}

/// The context figure, and whether it is provisional. While a compaction is
/// pending, Claude writes the post figure into its marker; Codex writes a new
/// token_count at once, and pi and Prime a new response, so they have none.
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

// ─── tail scan ──────────────────────────────────────────────────────────────

/// Find the newest usable line of `path` for `agent`. Reads the last 256 KiB,
/// drops the partial first line, and parses lines from the end; an invalid
/// line (one being written) is skipped. With no usable line and more file
/// before the window, the window doubles, up to the file size.
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
        // The first line may be partial. A whole line dropped here is read
        // again by the next, doubled window.
        let skip = first_whole_line(&buf, from);
        let usable = newest_usable(&buf[skip..], from + skip as u64, agent);
        if usable.is_some() || from == 0 {
            return Ok(Tail { usable, bytes_read });
        }
        window = window.saturating_mul(2);
    }
}

/// Where the first whole line of `buf` starts, `buf` being the file from
/// byte `from`: the first line is partial unless the buffer is the whole
/// file.
fn first_whole_line(buf: &[u8], from: u64) -> usize {
    if from == 0 {
        return 0;
    }
    buf.iter()
        .position(|b| *b == b'\n')
        .map_or(buf.len(), |i| i + 1)
}

/// The lines of `buf` from the last to the first, each with its start
/// offset in `buf` and without its newline.
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

/// The newest usable line of whole lines `buf`, which starts at byte `base`.
/// A line that does not parse (one being written) is skipped.
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

/// The parser of a usable line of `agent`, given the line's offset.
fn usable_parser(agent: &str) -> fn(&Value, u64) -> Option<Usable> {
    match agent {
        "claude" => claude_usable,
        "pi" | "prime" => pi_usable,
        _ => codex_usable,
    }
}

/// Codex names its model in the turn context before the usage; the other
/// agents name it on the usable line.
fn reads_turn_context(agent: &str) -> bool {
    !matches!(agent, "claude" | "pi" | "prime")
}

/// The model of the nearest turn context in `values`, newest first.
fn codex_model_before(mut values: impl Iterator<Item = Value>) -> Option<String> {
    values
        .find(|v| v.get("type").and_then(Value::as_str) == Some("turn_context"))
        .and_then(|v| {
            v.pointer("/payload/model")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
}

/// A Claude main-chain response with usage: its context and its model.
///
/// The context is the input side plus the output of the last `message`
/// iteration when the line has iterations (an advisor call is not the
/// context), else of the line's usage.
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

/// The last `message` iteration of `usage` that has input, if any.
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

/// An `assistant` line of the main chain, not of a sidechain.
fn is_main_assistant(v: &Value) -> bool {
    v.get("type").and_then(Value::as_str) == Some("assistant")
        && v.get("isSidechain").and_then(Value::as_bool) != Some(true)
}

/// The input side of a Claude usage object.
fn claude_input(u: &Value) -> u64 {
    n(u, "/input_tokens") + n(u, "/cache_creation_input_tokens") + n(u, "/cache_read_input_tokens")
}

/// A Codex `token_count` with `info`: the last response's total and the
/// model window.
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

/// A pi or Prime response that did not fail: its context and its model.
///
/// The context is `totalTokens` when it is above 0, else the sum of the
/// token classes. A `toolResult` line is not a response.
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

/// The `message` of a `message` line that is an assistant response not
/// ended by an abort or an error.
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

/// `totalTokens` when it is above 0, else the sum of the token classes.
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

// ─── opencode ───────────────────────────────────────────────────────────────

/// The current context of 1 OpenCode session, from its assistant rows in
/// `db`. The newest summary row is the last compaction; its `offset` is the
/// row's `time_updated` (ms), as rows have no byte offset.
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

/// The id goes into the SQL text: `ses_` and letters and digits only.
fn is_opencode_id(sid: &str) -> bool {
    sid.strip_prefix("ses_")
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_alphanumeric()))
}

/// The assistant rows of session `sid`, oldest first.
fn opencode_rows(db: &Path, sqlite3: &Path, sid: &str) -> Result<Vec<Value>, Unreadable> {
    let query = format!(
        "SELECT time_updated, data FROM message WHERE session_id = '{sid}' \
         AND json_extract(data, '$.role') = 'assistant' ORDER BY time_updated, id;"
    );
    sqlite_json(sqlite3, db, &query).map_err(|e| match e {
        // The cost reader says "sqlite3 not found"; this reason names the fix.
        Unreadable::NotRead(_) => Unreadable::NotRead("sqlite3 not on PATH".into()),
        other => other,
    })
}

/// The newest usable response and the markers of `rows`, oldest first.
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

/// What 1 OpenCode row is.
enum Row {
    Mark(CompactionMark),
    Usable(Usable),
}

/// The meaning of 1 row: a summary row is a marker, a finished response
/// with tokens is usable, any other row is nothing. `usable` is the newest
/// response so far, the marker's `pre_tokens`.
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

/// The tokens of 1 response: `total` when it is above 0, else the sum of
/// the classes.
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

/// The model of 1 response: `provider/model`, or the model alone.
fn opencode_model(data: &Value) -> Option<String> {
    let provider = data.get("providerID").and_then(Value::as_str);
    let model = data.get("modelID").and_then(Value::as_str)?;
    Some(match provider {
        Some(p) => format!("{p}/{model}"),
        None => model.to_string(),
    })
}

// ─── markers ────────────────────────────────────────────────────────────────

/// The marker cache of 1 transcript.
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

/// Every marker of `path`: the cached ones, and those in the bytes after the
/// cache's `scanned_to`. Writes the cache back, best effort.
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

/// The marker cache at `cache_path` when it is this version's, of the
/// transcript `key`, and no further than `len`; else an empty one.
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

/// Write the marker cache back. Best effort: a cache that cannot be written
/// costs 1 re-scan.
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

/// Search `from..len` for `needle` with a plain byte search, and add each
/// hit whose whole line is a top-level marker to `marks`. Returns the end of
/// the last whole line, so a line being written is searched again next time;
/// None when the bytes hold no newline.
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
    // Hits before this offset are inside a line already checked.
    let mut checked_to = from;
    while pos < len {
        let want = SEARCH_CHUNK.min((len - pos) as usize);
        read_at(file, pos, &mut buf[..want])?;
        let chunk = &buf[..want];
        if let Some(i) = chunk.iter().rposition(|b| *b == b'\n') {
            last_newline = Some(pos + i as u64);
        }
        scan_chunk(file, chunk, pos, len, needle, agent, &mut checked_to, marks)?;
        // A short read is the last one, so `want` is above the overlap below.
        if pos + want as u64 >= len {
            break;
        }
        // Overlap by the needle length, so a needle across 2 chunks is found.
        pos += (want - (needle.len() - 1)) as u64;
    }
    marks.sort_by_key(|m| m.offset);
    Ok(last_newline.map(|i| i + 1))
}

/// Check each hit of `needle` in `chunk`, which starts at file offset `pos`,
/// and add each hit whose whole line is a marker to `marks`. A hit before
/// `checked_to` is inside a line already checked.
#[allow(clippy::too_many_arguments)] // scan state is split on purpose: file and chunk are read, the rest is updated
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

/// Fill `buf` from byte `pos` of `file`.
fn read_at(file: &mut File, pos: u64, buf: &mut [u8]) -> std::io::Result<()> {
    file.seek(SeekFrom::Start(pos))?;
    file.read_exact(buf)
}

/// `marks` holds the marker at `offset`.
fn has_mark(marks: &[CompactionMark], offset: u64) -> bool {
    marks.iter().any(|m| m.offset == offset)
}

/// The whole line holding byte `at`: its start offset and its bytes, without
/// the newline.
fn line_around(file: &mut File, at: u64, len: u64) -> std::io::Result<(u64, Vec<u8>)> {
    let mut buf = vec![0u8; LINE_STEP as usize];
    let start = line_start(file, at, &mut buf)?;
    let line = line_from(file, start, len, &mut buf)?;
    Ok((start, line))
}

/// The read step of [`line_around`].
const LINE_STEP: u64 = 64 << 10;

/// The start of the line holding byte `at`: the byte after the last newline
/// before it, or 0. `buf` is the read buffer, [`LINE_STEP`] bytes.
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

/// The bytes from `start` to the next newline or to `len`, without the
/// newline.
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

/// A marker, when the line's TOP-LEVEL fields say so. A needle inside a
/// string is escaped, and one in a nested object fails this check.
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

/// What a marker line of `agent` is: its `type`, its `subtype` (Claude),
/// and the reader of its trigger, `pre_tokens` and `post_tokens`.
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

/// Reads a marker line's trigger, `pre_tokens` and `post_tokens`.
type MarkerFields = fn(&Value) -> (Option<String>, Option<u64>, Option<u64>);
