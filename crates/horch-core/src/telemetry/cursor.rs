//! Where each reader stopped, so a restart resumes instead of re-reading.
//!
//! A cursor belongs to one input: a transcript file, or one OpenCode session
//! in the database. Only complete lines (ending in `\n`) are consumed; a
//! partial last line waits for the next tick. When the file's identity
//! changes (a new inode: a rewrite or rotation) or it shrinks below the
//! offset, the cursor starts over at 0 with fresh reader state. Re-reading is
//! safe because every event has a stable id and the store drops repeats.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::TokenClasses;

/// How many recent Claude message ids a cursor remembers. Repeats of one id
/// are adjacent (one line per content block), so a handful is plenty.
pub(crate) const RECENT_IDS: usize = 8;

/// One message id a reader has emitted, with what it has emitted so far.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seen {
    pub id: String,
    pub tokens: TokenClasses,
    /// Corrections emitted for this id, which numbers the next one.
    #[serde(default)]
    pub corrections: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Cursor {
    /// The input: a file path, or the OpenCode database.
    #[serde(default)]
    pub path: String,
    /// The session the input belongs to. The cursor's key in
    /// `cursors.json` names it too, but a session id can hold a `|`.
    #[serde(default)]
    pub agent: String,
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub dev: u64,
    #[serde(default)]
    pub ino: u64,
    #[serde(default)]
    pub offset: u64,
    /// Ticks in a row with no new input.
    #[serde(default)]
    pub quiet_ticks: u32,

    // Claude: the last few message ids (section 7.1).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent: Vec<Seen>,

    // Codex: the latest turn context, and which counting mode the file is in.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// `Some(true)`: count `token_usage_record` lines; `Some(false)`: count
    /// differences of `token_count` totals; `None`: not decided yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub records_mode: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_total: Option<TokenClasses>,

    // OpenCode: the newest `time_updated` seen, and what each message has
    // emitted so far (a message can be updated after it completes).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub since_ms: i64,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub emitted: BTreeMap<String, Seen>,
}

fn is_zero(n: &i64) -> bool {
    *n == 0
}

/// A file's identity: `(dev, ino)` on Unix. Stable Rust has no file index on
/// Windows, so there the creation time stands in for the inode.
pub(crate) fn file_identity(meta: &std::fs::Metadata) -> (u64, u64) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (meta.dev(), meta.ino())
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        (0, meta.creation_time())
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = meta;
        (0, 0)
    }
}

impl Cursor {
    /// Remember `tokens` emitted for `id`, keeping the last [`RECENT_IDS`].
    pub(crate) fn remember(&mut self, seen: Seen) {
        self.recent.retain(|s| s.id != seen.id);
        self.recent.push(seen);
        if self.recent.len() > RECENT_IDS {
            let extra = self.recent.len() - RECENT_IDS;
            self.recent.drain(..extra);
        }
    }

    pub(crate) fn recalled(&self, id: &str) -> Option<&Seen> {
        self.recent.iter().find(|s| s.id == id)
    }

    /// Fill the agent, session id and path that a cursor saved before they
    /// were fields leaves empty, from its key `agent|session id|path`. The
    /// agent ends at the first `|`. A known path ends the key; else the path
    /// starts after the last `|`.
    pub(crate) fn fill_from_key(&mut self, key: &str) {
        let Some((agent, rest)) = key.split_once('|') else {
            return;
        };
        if self.agent.is_empty() {
            self.agent = agent.to_string();
        }
        let tail = format!("|{}", self.path);
        let session = match rest.strip_suffix(&tail) {
            Some(session) if !self.path.is_empty() => session,
            _ => {
                let Some((session, path)) = rest.rsplit_once('|') else {
                    return;
                };
                if self.path.is_empty() {
                    self.path = path.to_string();
                }
                session
            }
        };
        if self.session_id.is_empty() {
            self.session_id = session.to_string();
        }
    }
}

/// Feed every new complete line of `path` to `f`, with the byte offset the
/// line starts at, and advance the cursor past it. Returns whether any line
/// was consumed. A cursor for a different file identity, or past the end of
/// a shrunken file, is reset first.
pub(crate) fn poll_lines(
    path: &Path,
    cursor: &mut Cursor,
    mut f: impl FnMut(&mut Cursor, u64, &str),
) -> std::io::Result<bool> {
    poll_lines_until(path, cursor, |c, at, line| {
        f(c, at, line);
        true
    })
}

/// [`poll_lines`], stopping after the line for which `f` returns false. The
/// cursor then points at the start of the first unread line, so the next
/// call resumes there: no line is read twice and none is skipped (NFR-02:
/// the collector's first read of a large history goes in batches).
pub(crate) fn poll_lines_until(
    path: &Path,
    cursor: &mut Cursor,
    f: impl FnMut(&mut Cursor, u64, &str) -> bool,
) -> std::io::Result<bool> {
    let file = std::fs::File::open(path)?;
    let meta = file.metadata()?;
    let (dev, ino) = file_identity(&meta);
    let path_str = path.to_string_lossy().into_owned();
    if cursor.dev != dev
        || cursor.ino != ino
        || meta.len() < cursor.offset
        || cursor.path != path_str
    {
        *cursor = Cursor {
            path: path_str,
            agent: std::mem::take(&mut cursor.agent),
            session_id: std::mem::take(&mut cursor.session_id),
            dev,
            ino,
            ..Cursor::default()
        };
    }
    let mut reader = BufReader::with_capacity(1 << 20, file);
    reader.seek(SeekFrom::Start(cursor.offset))?;
    let mut buf = LINE.take();
    let read = read_lines(&mut reader, &mut buf, cursor, f);
    LINE.set(buf);
    let consumed_any = read?;
    if consumed_any {
        cursor.quiet_ticks = 0;
    } else {
        cursor.quiet_ticks = cursor.quiet_ticks.saturating_add(1);
    }
    Ok(consumed_any)
}

thread_local! {
    /// The line buffer of [`poll_lines_until`], kept from call to call
    /// (NFR-02 peak RSS). A transcript line can be megabytes long; a new
    /// buffer per call grew by doubling each time, and macOS kept every
    /// freed block of more than 1 MB resident in its cache.
    static LINE: std::cell::Cell<Vec<u8>> = const { std::cell::Cell::new(Vec::new()) };
}

/// The loop of [`poll_lines_until`]; whether a line was consumed.
fn read_lines(
    reader: &mut impl BufRead,
    buf: &mut Vec<u8>,
    cursor: &mut Cursor,
    mut f: impl FnMut(&mut Cursor, u64, &str) -> bool,
) -> std::io::Result<bool> {
    let mut consumed_any = false;
    loop {
        buf.clear();
        let n = reader.read_until(b'\n', buf)?;
        if n == 0 || buf.last() != Some(&b'\n') {
            // End of file, or a partial line still being written.
            break;
        }
        let at = cursor.offset;
        cursor.offset += n as u64;
        consumed_any = true;
        let text = String::from_utf8_lossy(&buf[..n - 1]);
        let line = text.trim_end_matches('\r');
        if !line.trim().is_empty() && !f(cursor, at, line) {
            break;
        }
    }
    Ok(consumed_any)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn lines_of(path: &Path, cursor: &mut Cursor) -> Vec<(u64, String)> {
        let mut out = Vec::new();
        poll_lines(path, cursor, |_, at, l| out.push((at, l.to_string()))).unwrap();
        out
    }

    #[test]
    fn a_partial_line_waits_and_offsets_are_line_starts() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("t.jsonl");
        std::fs::write(&p, "a\nbb\ncc").unwrap();
        let mut c = Cursor::default();
        assert_eq!(
            lines_of(&p, &mut c),
            vec![(0, "a".into()), (2, "bb".into())]
        );
        assert_eq!(c.offset, 5);
        let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        f.write_all(b"c\n").unwrap();
        assert_eq!(lines_of(&p, &mut c), vec![(5, "ccc".into())]);
        assert_eq!(lines_of(&p, &mut c), vec![]);
        assert_eq!(c.quiet_ticks, 1);
    }

    /// NFR-02: a file read in batches of at most 3 lines gives every line
    /// once, in order, in 3 batches; after each batch the cursor is at the
    /// start of the first unread line. A blank line counts toward no batch.
    #[test]
    fn nfr_02_a_file_read_in_3_batches_gives_every_line_once() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("t.jsonl");
        std::fs::write(&p, "a\nbb\n\nccc\nd\nee\nf\ng\n").unwrap();
        let whole = lines_of(&p, &mut Cursor::default());
        let mut c = Cursor::default();
        let (mut batches, mut read) = (Vec::new(), Vec::new());
        loop {
            let mut batch = Vec::new();
            poll_lines_until(&p, &mut c, |_, at, l| {
                batch.push((at, l.to_string()));
                batch.len() < 3
            })
            .unwrap();
            if batch.is_empty() {
                break;
            }
            batches.push((batch.len(), c.offset));
            read.extend(batch);
        }
        assert_eq!(read, whole);
        assert_eq!(whole.len(), 7);
        // "ccc" ends at 10, "f" at 17, "g" at 19: the next unread line starts there.
        assert_eq!(batches, vec![(3, 10), (3, 17), (1, 19)]);
    }

    #[test]
    fn tel_07_rotation_resets() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("t.jsonl");
        std::fs::write(&p, "one\ntwo\n").unwrap();
        let mut c = Cursor::default();
        assert_eq!(lines_of(&p, &mut c).len(), 2);
        c.model = "kept only for this file".into();
        // Truncated below the offset: start over, with fresh reader state.
        std::fs::write(&p, "x\n").unwrap();
        assert_eq!(lines_of(&p, &mut c), vec![(0, "x".into())]);
        assert!(c.model.is_empty());
        // Replaced by a new file (new inode): start over too.
        let q = tmp.path().join("new.jsonl");
        std::fs::write(&q, "fresh\nlines\n").unwrap();
        std::fs::rename(&q, &p).unwrap();
        assert_eq!(lines_of(&p, &mut c).len(), 2);
    }

    #[test]
    fn nfr_04_file_identity() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a");
        let b = tmp.path().join("b");
        std::fs::write(&a, "1").unwrap();
        std::fs::write(&b, "2").unwrap();
        let ia = file_identity(&std::fs::metadata(&a).unwrap());
        let ib = file_identity(&std::fs::metadata(&b).unwrap());
        assert_eq!(ia, file_identity(&std::fs::metadata(&a).unwrap()), "stable");
        #[cfg(unix)]
        assert_ne!(ia, ib, "two files, two identities");
        #[cfg(windows)]
        let _ = ib;
    }

    #[test]
    fn remembers_only_the_last_few_ids() {
        let mut c = Cursor::default();
        for n in 0..20 {
            c.remember(Seen {
                id: format!("m{n}"),
                ..Seen::default()
            });
        }
        assert_eq!(c.recent.len(), RECENT_IDS);
        assert!(c.recalled("m19").is_some());
        assert!(c.recalled("m0").is_none());
    }

    /// NFR-02 peak RSS: the line buffer is kept from call to call, so a
    /// long line grows it once; the lines and offsets of the next files are
    /// the same as from a fresh buffer.
    #[test]
    fn nfr_02_the_line_buffer_is_kept_and_reads_the_same() {
        let tmp = tempfile::tempdir().unwrap();
        let long = "x".repeat(3 << 20);
        let a = tmp.path().join("a.jsonl");
        std::fs::write(&a, format!("{long}\nshort\npartial")).unwrap();
        let b = tmp.path().join("b.jsonl");
        std::fs::write(&b, "b1\r\n\n  \nb2\n").unwrap();
        let mut c = Cursor::default();
        let got = lines_of(&a, &mut c);
        assert_eq!(got.len(), 2);
        assert_eq!((got[0].0, got[0].1.len()), (0, long.len()));
        assert_eq!(got[1], (long.len() as u64 + 1, "short".into()));
        assert_eq!(c.offset, long.len() as u64 + 7);
        let kept = LINE.take();
        assert!(kept.capacity() > long.len(), "the buffer is kept");
        let at = kept.as_ptr();
        LINE.set(kept);

        let mut c = Cursor::default();
        let got = lines_of(&b, &mut c);
        assert_eq!(got, vec![(0, "b1".into()), (8, "b2".into())]);
        let kept = LINE.take();
        assert_eq!(kept.as_ptr(), at, "the same buffer, no new block");
    }
}
