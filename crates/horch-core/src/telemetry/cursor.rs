use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::TokenClasses;

pub(crate) const RECENT_IDS: usize = 8;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seen {
    pub id: String,
    pub tokens: TokenClasses,
    #[serde(default)]
    pub corrections: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Cursor {
    #[serde(default)]
    pub path: String,
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
    #[serde(default)]
    pub quiet_ticks: u32,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent: Vec<Seen>,

    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub records_mode: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_total: Option<TokenClasses>,

    #[serde(default, skip_serializing_if = "is_zero")]
    pub since_ms: i64,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub emitted: BTreeMap<String, Seen>,
}

fn is_zero(n: &i64) -> bool {
    *n == 0
}

impl Cursor {
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

pub(crate) fn poll_lines_until(
    path: &Path,
    cursor: &mut Cursor,
    f: impl FnMut(&mut Cursor, u64, &str) -> bool,
) -> std::io::Result<bool> {
    let file = std::fs::File::open(path)?;
    let meta = file.metadata()?;
    let (dev, ino) = horch_platform::fs::file_identity_of(&file)?;
    let path_str = path.to_string_lossy().into_owned();
    reset_if_moved(cursor, dev, ino, meta.len(), path_str);
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
    static LINE: std::cell::Cell<Vec<u8>> = const { std::cell::Cell::new(Vec::new()) };
}

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

fn reset_if_moved(cursor: &mut Cursor, dev: u64, ino: u64, len: u64, path: String) {
    if cursor.dev != dev || cursor.ino != ino || len < cursor.offset || cursor.path != path {
        *cursor = Cursor {
            path,
            agent: std::mem::take(&mut cursor.agent),
            session_id: std::mem::take(&mut cursor.session_id),
            dev,
            ino,
            ..Cursor::default()
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn lines_of(path: &Path, cursor: &mut Cursor) -> Vec<(u64, String)> {
        let mut out = Vec::new();
        poll_lines_until(path, cursor, |_, at, l| {
            out.push((at, l.to_string()));
            true
        })
        .unwrap();
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
        std::fs::write(&p, "x\n").unwrap();
        assert_eq!(lines_of(&p, &mut c), vec![(0, "x".into())]);
        assert!(c.model.is_empty());
        let q = tmp.path().join("new.jsonl");
        std::fs::write(&q, "fresh\nlines\n").unwrap();
        std::fs::rename(&q, &p).unwrap();
        assert_eq!(lines_of(&p, &mut c).len(), 2);
    }

    #[test]
    fn tel_07_a_reset_keeps_the_owner_and_restarts_the_quiet_count() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("t.jsonl");
        std::fs::write(&p, "one\ntwo\n").unwrap();
        let mut c = Cursor {
            agent: "claude".into(),
            session_id: "s-1".into(),
            ..Cursor::default()
        };
        assert_eq!(lines_of(&p, &mut c).len(), 2);
        let (dev, ino) = (c.dev, c.ino);
        assert_eq!(c.path, p.to_string_lossy());

        c.model = "kept while the file is the same".into();
        c.quiet_ticks = 4;
        assert_eq!(lines_of(&p, &mut c), vec![]);
        assert_eq!((c.offset, c.quiet_ticks), (8, 5));
        assert_eq!(c.model, "kept while the file is the same");

        c.path = "/elsewhere".into();
        assert_eq!(lines_of(&p, &mut c).len(), 2);
        assert_eq!(c.path, p.to_string_lossy());
        assert_eq!((c.dev, c.ino), (dev, ino));
        assert_eq!((c.agent.as_str(), c.session_id.as_str()), ("claude", "s-1"));
        assert!(c.model.is_empty());
        assert_eq!((c.offset, c.quiet_ticks), (8, 0));

        c.quiet_ticks = 5;
        std::fs::write(&p, "").unwrap();
        assert_eq!(lines_of(&p, &mut c), vec![]);
        assert_eq!((c.offset, c.quiet_ticks), (0, 1));
        assert_eq!((c.agent.as_str(), c.session_id.as_str()), ("claude", "s-1"));
    }

    #[test]
    fn nfr_04_file_identity() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a");
        let b = tmp.path().join("b");
        std::fs::write(&a, "1").unwrap();
        std::fs::write(&b, "2").unwrap();
        let id = |p: &Path| horch_platform::fs::file_identity_of(&std::fs::File::open(p).unwrap());
        let ia = id(&a).unwrap();
        let ib = id(&b).unwrap();
        assert_eq!(ia, id(&a).unwrap(), "stable");
        assert_ne!(ia, ib, "two files, two identities");
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
