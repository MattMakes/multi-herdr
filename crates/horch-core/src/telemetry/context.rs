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

use super::readers::{first_n, locate, n, sqlite_json, Located, Unreadable};
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
    let path = match locate(loc, agent, sid)? {
        // The main transcript; subagent files have their own context.
        Located::Files(files) => files.into_iter().next(),
        Located::OpenCode(db) => return read_opencode(&db, &loc.sqlite3, sid),
    }
    .ok_or_else(|| Unreadable::NotRead(format!("no {agent} context reader")))?;
    let failed = |e: std::io::Error| Unreadable::Failed(format!("reading {}: {e}", path.display()));

    let marks = update_marks(&path, cache_dir, agent).map_err(failed)?;
    let tail = scan_tail(&path, agent).map_err(failed)?;
    let last = marks.iter().max_by_key(|m| m.offset).cloned();
    let pending = match (&last, &tail.usable) {
        (Some(m), Some(u)) => m.offset > u.offset,
        (Some(_), None) => true,
        (None, _) => false,
    };
    let (tokens, provisional) = if pending {
        // Claude writes the post figure into its marker; Codex writes a new
        // token_count at once, and pi and Prime a new response, so they have
        // none.
        let post = last
            .as_ref()
            .and_then(|m| m.post_tokens)
            .filter(|_| agent == "claude");
        (post, post.is_some())
    } else {
        (tail.usable.as_ref().map(|u| u.tokens), false)
    };
    let (window, model) = match tail.usable {
        Some(u) => (u.window, u.model),
        None => (None, None),
    };
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
        let skip = if from == 0 {
            0
        } else {
            match buf.iter().position(|b| *b == b'\n') {
                Some(i) => i + 1,
                None => buf.len(),
            }
        };
        let usable = newest_usable(&buf[skip..], from + skip as u64, agent);
        if usable.is_some() || from == 0 {
            return Ok(Tail { usable, bytes_read });
        }
        window = window.saturating_mul(2);
    }
}

/// The newest usable line of whole lines `buf`, which starts at byte `base`.
fn newest_usable(buf: &[u8], base: u64, agent: &str) -> Option<Usable> {
    let mut end = buf.len();
    let mut found: Option<Usable> = None;
    while end > 0 {
        let line_end = if buf[end - 1] == b'\n' { end - 1 } else { end };
        let start = buf[..line_end]
            .iter()
            .rposition(|b| *b == b'\n')
            .map_or(0, |i| i + 1);
        end = start;
        let Ok(v) = serde_json::from_slice::<Value>(&buf[start..line_end]) else {
            continue;
        };
        match (&mut found, agent) {
            (None, "claude") => {
                found = claude_usable(&v).map(|(tokens, model)| Usable {
                    offset: base + start as u64,
                    tokens,
                    window: None,
                    model,
                });
                if found.is_some() {
                    return found;
                }
            }
            (None, "pi" | "prime") => {
                if let Some((tokens, model)) = pi_usable(&v) {
                    return Some(Usable {
                        offset: base + start as u64,
                        tokens,
                        window: None,
                        model,
                    });
                }
            }
            (None, _) => {
                found = codex_usable(&v).map(|(tokens, window)| Usable {
                    offset: base + start as u64,
                    tokens,
                    window,
                    model: None,
                });
            }
            // Codex names its model in the turn context before the usage;
            // look for it in the bytes already read.
            (Some(u), _) => {
                if v.get("type").and_then(Value::as_str) == Some("turn_context") {
                    u.model = v
                        .pointer("/payload/model")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    return found;
                }
            }
        }
    }
    found
}

/// A Claude main-chain response with usage: its context and its model.
///
/// The context is the input side plus the output of the last `message`
/// iteration when the line has iterations (an advisor call is not the
/// context), else of the line's usage.
fn claude_usable(v: &Value) -> Option<(u64, Option<String>)> {
    if v.get("type").and_then(Value::as_str) != Some("assistant")
        || v.get("isSidechain").and_then(Value::as_bool) == Some(true)
    {
        return None;
    }
    let model = v.pointer("/message/model").and_then(Value::as_str);
    if model == Some("<synthetic>") {
        return None;
    }
    let usage = v.pointer("/message/usage").filter(|u| u.is_object())?;
    let input = |u: &Value| {
        n(u, "/input_tokens")
            + n(u, "/cache_creation_input_tokens")
            + n(u, "/cache_read_input_tokens")
    };
    if input(usage) == 0 {
        return None;
    }
    let last_message = usage
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
        .filter(|it| input(it) > 0);
    let u = last_message.unwrap_or(usage);
    Some((input(u) + n(u, "/output_tokens"), model.map(str::to_owned)))
}

/// A Codex `token_count` with `info`: the last response's total and the
/// model window.
fn codex_usable(v: &Value) -> Option<(u64, Option<u64>)> {
    if v.get("type").and_then(Value::as_str) != Some("event_msg")
        || v.pointer("/payload/type").and_then(Value::as_str) != Some("token_count")
    {
        return None;
    }
    let info = v.pointer("/payload/info").filter(|i| i.is_object())?;
    let last = info.get("last_token_usage").filter(|u| u.is_object())?;
    let window = info.get("model_context_window").and_then(Value::as_u64);
    Some((first_n(last, &["total_tokens"]), window))
}

/// A pi or Prime response that did not fail: its context and its model.
///
/// The context is `totalTokens` when it is above 0, else the sum of the
/// token classes. A `toolResult` line is not a response.
fn pi_usable(v: &Value) -> Option<(u64, Option<String>)> {
    let message = v.get("message")?;
    if v.get("type").and_then(Value::as_str) != Some("message")
        || message.get("role").and_then(Value::as_str) != Some("assistant")
        || matches!(
            message.get("stopReason").and_then(Value::as_str),
            Some("aborted" | "error")
        )
    {
        return None;
    }
    let usage = message.get("usage").filter(|u| u.is_object())?;
    let tokens = match n(usage, "/totalTokens") {
        0 => {
            n(usage, "/input")
                + n(usage, "/output")
                + n(usage, "/cacheRead")
                + n(usage, "/cacheWrite")
        }
        total => total,
    };
    let model = message.get("model").and_then(Value::as_str);
    (tokens > 0).then(|| (tokens, model.map(str::to_owned)))
}

// ─── opencode ───────────────────────────────────────────────────────────────

/// The current context of 1 OpenCode session, from its assistant rows in
/// `db`. The newest summary row is the last compaction; its `offset` is the
/// row's `time_updated` (ms), as rows have no byte offset.
fn read_opencode(db: &Path, sqlite3: &Path, sid: &str) -> Result<Reading, Unreadable> {
    // The id goes into the SQL text: `ses_` and letters and digits only.
    let id_ok = sid
        .strip_prefix("ses_")
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_alphanumeric()));
    if !id_ok {
        return Err(Unreadable::Failed(format!(
            "unexpected opencode session id '{sid}'"
        )));
    }
    if !db.is_file() {
        return Err(Unreadable::NoTranscript(format!(
            "no opencode database at {}",
            db.display()
        )));
    }
    let query = format!(
        "SELECT time_updated, data FROM message WHERE session_id = '{sid}' \
         AND json_extract(data, '$.role') = 'assistant' ORDER BY time_updated, id;"
    );
    let rows = sqlite_json(sqlite3, db, &query).map_err(|e| match e {
        // The cost reader says "sqlite3 not found"; this reason names the fix.
        Unreadable::NotRead(_) => Unreadable::NotRead("sqlite3 not on PATH".into()),
        other => other,
    })?;
    let mut usable: Option<Usable> = None;
    let mut marks: Vec<CompactionMark> = Vec::new();
    for row in rows {
        let updated = row.get("time_updated").and_then(Value::as_u64).unwrap_or(0);
        let data: Value = match row.get("data") {
            Some(Value::String(s)) => serde_json::from_str(s).unwrap_or(Value::Null),
            Some(v) => v.clone(),
            None => Value::Null,
        };
        if data.get("summary").and_then(Value::as_bool) == Some(true) {
            let at = data
                .pointer("/time/created")
                .and_then(Value::as_i64)
                .and_then(clock::from_epoch_ms)
                .map(clock::stamp)
                .unwrap_or_default();
            marks.push(CompactionMark {
                at,
                offset: updated,
                trigger: None,
                pre_tokens: usable.as_ref().map(|u| u.tokens),
                post_tokens: None,
            });
            continue;
        }
        if data.get("finish").is_none_or(Value::is_null) {
            continue;
        }
        let t = data.get("tokens").unwrap_or(&Value::Null);
        let tokens = match n(t, "/total") {
            0 => {
                n(t, "/input")
                    + n(t, "/output")
                    + n(t, "/reasoning")
                    + n(t, "/cache/read")
                    + n(t, "/cache/write")
            }
            total => total,
        };
        if tokens == 0 {
            continue;
        }
        let model = match (
            data.get("providerID").and_then(Value::as_str),
            data.get("modelID").and_then(Value::as_str),
        ) {
            (Some(p), Some(m)) => Some(format!("{p}/{m}")),
            (None, Some(m)) => Some(m.to_string()),
            _ => None,
        };
        usable = Some(Usable {
            offset: updated,
            tokens,
            window: None,
            model,
        });
    }
    let last = marks.last().cloned();
    let pending = match (&last, &usable) {
        (Some(m), Some(u)) => m.offset > u.offset,
        (Some(_), None) => true,
        (None, _) => false,
    };
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
    let mut cache = std::fs::read(&cache_path)
        .ok()
        .and_then(|b| serde_json::from_slice::<MarkerCache>(&b).ok())
        .filter(|c| c.v == CACHE_VERSION && c.path == key && c.scanned_to <= len)
        .unwrap_or(MarkerCache {
            v: CACHE_VERSION,
            path: key,
            scanned_to: 0,
            marks: Vec::new(),
        });
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
        // Best effort: a cache that cannot be written costs 1 re-scan.
        if let Ok(bytes) = serde_json::to_vec(&cache) {
            if let Some(dir) = cache_path.parent() {
                if std::fs::create_dir_all(dir).is_ok() {
                    let _ = crate::fsx::write_atomic(&cache_path, &bytes, 0o600);
                }
            }
        }
    }
    Ok(cache.marks)
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
        file.seek(SeekFrom::Start(pos))?;
        let want = SEARCH_CHUNK.min((len - pos) as usize);
        file.read_exact(&mut buf[..want])?;
        let chunk = &buf[..want];
        if let Some(i) = chunk.iter().rposition(|b| *b == b'\n') {
            last_newline = Some(pos + i as u64);
        }
        let mut i = 0;
        while i + needle.len() <= chunk.len() {
            let at = pos + i as u64;
            if at >= checked_to && chunk[i..i + needle.len()] == *needle {
                let (start, line) = line_around(file, at, len)?;
                checked_to = start + line.len() as u64;
                if let Some(mark) = confirm(&line, start, agent) {
                    if !marks.iter().any(|m| m.offset == start) {
                        marks.push(mark);
                    }
                }
            }
            i += 1;
        }
        if want < needle.len() || pos + want as u64 >= len {
            break;
        }
        // Overlap by the needle length, so a needle across 2 chunks is found.
        pos += (want - (needle.len() - 1)) as u64;
    }
    marks.sort_by_key(|m| m.offset);
    Ok(last_newline.map(|i| i + 1))
}

/// The whole line holding byte `at`: its start offset and its bytes, without
/// the newline.
fn line_around(file: &mut File, at: u64, len: u64) -> std::io::Result<(u64, Vec<u8>)> {
    const STEP: u64 = 64 << 10;
    let mut start = at;
    let mut buf = vec![0u8; STEP as usize];
    'back: while start > 0 {
        let from = start.saturating_sub(STEP);
        let want = (start - from) as usize;
        file.seek(SeekFrom::Start(from))?;
        file.read_exact(&mut buf[..want])?;
        match buf[..want].iter().rposition(|b| *b == b'\n') {
            Some(i) => {
                start = from + i as u64 + 1;
                break 'back;
            }
            None => start = from,
        }
    }
    let mut line = Vec::new();
    let mut pos = start;
    while pos < len {
        let want = STEP.min(len - pos) as usize;
        file.seek(SeekFrom::Start(pos))?;
        file.read_exact(&mut buf[..want])?;
        match buf[..want].iter().position(|b| *b == b'\n') {
            Some(i) => {
                line.extend_from_slice(&buf[..i]);
                return Ok((start, line));
            }
            None => line.extend_from_slice(&buf[..want]),
        }
        pos += want as u64;
    }
    Ok((start, line))
}

/// A marker, when the line's TOP-LEVEL fields say so. A needle inside a
/// string is escaped, and one in a nested object fails this check.
fn confirm(line: &[u8], offset: u64, agent: &str) -> Option<CompactionMark> {
    let v: Value = serde_json::from_slice(line).ok()?;
    let at = v
        .get("timestamp")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let kind = v.get("type").and_then(Value::as_str);
    match agent {
        "claude" => {
            if kind != Some("system")
                || v.get("subtype").and_then(Value::as_str) != Some("compact_boundary")
            {
                return None;
            }
            let meta = v.get("compactMetadata").unwrap_or(&Value::Null);
            Some(CompactionMark {
                at,
                offset,
                trigger: meta
                    .get("trigger")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                pre_tokens: meta.get("preTokens").and_then(Value::as_u64),
                post_tokens: meta.get("postTokens").and_then(Value::as_u64),
            })
        }
        "pi" | "prime" => {
            if kind != Some("compaction") {
                return None;
            }
            Some(CompactionMark {
                at,
                offset,
                trigger: None,
                pre_tokens: v.get("tokensBefore").and_then(Value::as_u64),
                post_tokens: None,
            })
        }
        _ => {
            if kind != Some("compacted") {
                return None;
            }
            Some(CompactionMark {
                at,
                offset,
                trigger: None,
                pre_tokens: v
                    .pointer("/payload/latest_token_usage_record/usage/total_tokens")
                    .and_then(Value::as_u64),
                post_tokens: None,
            })
        }
    }
}
