//! Open questions (#70): a `QUESTION:` or `BLOCKED:` a role sent to the
//! orchestrator, with no later orchestrator `tell` to that role.
//!
//! The record lives per workspace under the state folder, next to the mail,
//! so a fleet with no task-ledger run has it too. One file per role holds
//! its newest question; an answer marks it answered. A record is kept for
//! the acked-mail retention (7 days) after its last change.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

/// How long a record is kept after its last change: the acked-mail retention.
const RETENTION_S: i64 = 7 * 24 * 60 * 60;

/// A record file's mode: it holds a role's message text.
const RECORD_MODE: u32 = 0o600;

/// How long a change waits for another process's change to the record.
const LOCK_WAIT: Duration = Duration::from_secs(5);
/// A lock this old is left over from a process that died mid-change.
const LOCK_STALE: Duration = Duration::from_secs(30);

/// The keywords that open a question.
const KEYWORDS: [&str; 2] = ["QUESTION:", "BLOCKED:"];

/// 1 open question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenQuestion {
    pub role: String,
    /// The line the role sent, tag included.
    pub text: String,
    /// When the role asked: a ledger stamp from [`crate::clock`].
    pub asked_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Record {
    #[serde(flatten)]
    question: OpenQuestion,
    answered_at: Option<String>,
}

impl Record {
    fn last_change(&self) -> &str {
        self.answered_at
            .as_deref()
            .unwrap_or(&self.question.asked_at)
    }
}

/// The open-question record of 1 workspace.
#[derive(Debug, Clone)]
pub struct Questions {
    dir: PathBuf,
}

impl Questions {
    pub fn under(state_root: &Path, workspace: &str) -> Self {
        Self {
            dir: state_root.join("questions").join(file_name(workspace)),
        }
    }

    /// Record 1 `tell` line from `sender` (`None`: the orchestrator) to
    /// `to`. A role's question to the orchestrator opens a question; an
    /// orchestrator line to a role answers that role's question.
    pub fn record_tell(&self, sender: Option<&str>, to: &str, line: &str) -> Result<()> {
        let from_orchestrator = sender.is_none_or(|s| s == "orchestrator");
        if from_orchestrator && to != "orchestrator" {
            return self.answer(to);
        }
        match sender {
            Some(role) if to == "orchestrator" && role != "horch" && is_question(line, role) => {
                self.ask(role, line)
            }
            _ => Ok(()),
        }
    }

    /// The open questions, oldest first.
    pub fn open(&self) -> Result<Vec<OpenQuestion>> {
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e).with_context(|| format!("reading {}", self.dir.display())),
        };
        let mut out = Vec::new();
        for entry in entries {
            let path = entry?.path();
            if path.extension().is_none_or(|x| x != "json") {
                continue;
            }
            if let Some(r) = read(&path)? {
                if r.answered_at.is_none() && !expired(&r) {
                    out.push(r.question);
                }
            }
        }
        out.sort_by(|a, b| a.asked_at.cmp(&b.asked_at).then(a.role.cmp(&b.role)));
        Ok(out)
    }

    /// The open question of `role`, if it has one. A record that cannot be
    /// read, or that names another role, is an error.
    pub fn open_for(&self, role: &str) -> Result<Option<OpenQuestion>> {
        Ok(self
            .read_role(role)?
            .filter(|r| r.answered_at.is_none() && !expired(r))
            .map(|r| r.question))
    }

    fn path(&self, role: &str) -> PathBuf {
        self.dir.join(format!("{}.json", file_name(role)))
    }

    fn read_role(&self, role: &str) -> Result<Option<Record>> {
        let path = self.path(role);
        let r = read(&path)?;
        if let Some(r) = r.as_ref().filter(|r| r.question.role != role) {
            bail!(
                "{} holds the question of {}, not of {role}",
                path.display(),
                r.question.role
            );
        }
        Ok(r)
    }

    /// Run `change` while this process holds the workspace's record lock.
    fn locked(&self, change: impl FnOnce() -> Result<()>) -> Result<()> {
        std::fs::create_dir_all(&self.dir)
            .with_context(|| format!("creating {}", self.dir.display()))?;
        let _guard = crate::fsx::DirLock::acquire(&self.dir, "records", LOCK_STALE, LOCK_WAIT)
            .with_context(|| format!("locking {}", self.dir.display()))?;
        change()
    }

    /// Open `role`'s question. A question while one is open keeps the first
    /// one's time: the role has waited since then.
    fn ask(&self, role: &str, line: &str) -> Result<()> {
        self.locked(|| {
            self.prune()?;
            let asked_at = self
                .open_for(role)?
                .map(|q| q.asked_at)
                .unwrap_or_else(crate::clock::now_stamp);
            self.write(&Record {
                question: OpenQuestion {
                    role: role.to_string(),
                    text: line.to_string(),
                    asked_at,
                },
                answered_at: None,
            })
        })
    }

    fn answer(&self, role: &str) -> Result<()> {
        self.locked(|| {
            let Some(mut r) = self.read_role(role)? else {
                return Ok(());
            };
            if r.answered_at.is_some() {
                return Ok(());
            }
            r.answered_at = Some(crate::clock::now_stamp());
            self.write(&r)
        })
    }

    fn write(&self, r: &Record) -> Result<()> {
        std::fs::create_dir_all(&self.dir)
            .with_context(|| format!("creating {}", self.dir.display()))?;
        let path = self.path(&r.question.role);
        crate::fsx::write_atomic(&path, &serde_json::to_vec(r)?, RECORD_MODE)
            .with_context(|| format!("writing {}", path.display()))
    }

    /// Remove the records past the retention. A record that cannot be read
    /// stays: it may hold an open question.
    fn prune(&self) -> Result<()> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Ok(());
        };
        for entry in entries {
            let path = entry?.path();
            if path.extension().is_some_and(|x| x == "json")
                && read(&path).is_ok_and(|r| r.is_some_and(|r| expired(&r)))
            {
                std::fs::remove_file(&path)
                    .with_context(|| format!("removing {}", path.display()))?;
            }
        }
        Ok(())
    }
}

/// Whether `line` from `role` opens a question: after the `[role]` tag it
/// starts with `QUESTION:` or `BLOCKED:`.
pub fn is_question(line: &str, role: &str) -> bool {
    let s = line.trim_start();
    let s = s
        .strip_prefix(&format!("[{role}]"))
        .unwrap_or(s)
        .trim_start();
    KEYWORDS
        .iter()
        .any(|k| s.get(..k.len()).is_some_and(|h| h.eq_ignore_ascii_case(k)))
}

/// The record at `path`, or `None` when there is none. A record that does
/// not parse is an error, not an absent question.
fn read(path: &Path) -> Result<Option<Record>> {
    let bytes = std::fs::read(path);
    if bytes
        .as_ref()
        .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
    {
        return Ok(None);
    }
    let bytes = bytes.with_context(|| format!("reading {}", path.display()))?;
    let r = serde_json::from_slice(&bytes)
        .with_context(|| format!("{} is not a question record", path.display()))?;
    Ok(Some(r))
}

fn expired(r: &Record) -> bool {
    let (Some(at), now) = (crate::clock::parse(r.last_change()), crate::clock::now()) else {
        return true;
    };
    (now - at).num_seconds() > RETENTION_S
}

/// A file name for `s` that no other string has: ASCII letters, digits and
/// `-` stay, and every other char (`_` too) becomes `_<hex code>_`.
fn file_name(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_alphanumeric() || c == '-' {
            out.push(c);
        } else {
            out.push_str(&format!("_{:x}_", c as u32));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(dir: &Path) -> Questions {
        Questions::under(dir, "w1")
    }

    #[test]
    fn a_question_to_the_orchestrator_is_open_until_the_orchestrator_tells_that_role() {
        let root = tempfile::tempdir().unwrap();
        let q = record(root.path());
        q.record_tell(
            Some("opus-1"),
            "orchestrator",
            "[opus-1] QUESTION: May I edit x?",
        )
        .unwrap();
        q.record_tell(
            Some("sonnet-1"),
            "orchestrator",
            "[sonnet-1] BLOCKED: no access",
        )
        .unwrap();
        let open = q.open().unwrap();
        assert_eq!(
            open.iter().map(|o| o.role.as_str()).collect::<Vec<_>>(),
            ["opus-1", "sonnet-1"]
        );
        assert_eq!(open[0].text, "[opus-1] QUESTION: May I edit x?");

        // A tell to another role answers nothing; the orchestrator's tell to
        // opus-1 answers opus-1 only.
        q.record_tell(None, "codex-1", "go on").unwrap();
        assert_eq!(q.open().unwrap().len(), 2);
        q.record_tell(Some("orchestrator"), "opus-1", "[orchestrator] Yes.")
            .unwrap();
        assert!(q.open_for("opus-1").unwrap().is_none());
        assert!(q.open_for("sonnet-1").unwrap().is_some());
    }

    #[test]
    fn other_lines_open_no_question() {
        let root = tempfile::tempdir().unwrap();
        let q = record(root.path());
        q.record_tell(Some("opus-1"), "orchestrator", "[opus-1] DONE: x")
            .unwrap();
        q.record_tell(
            Some("opus-1"),
            "orchestrator",
            "[opus-1] NOTE: QUESTION: later",
        )
        .unwrap();
        q.record_tell(Some("horch"), "orchestrator", "[horch] QUESTION: x")
            .unwrap();
        q.record_tell(Some("opus-1"), "sonnet-1", "[opus-1] QUESTION: x")
            .unwrap();
        assert!(q.open().unwrap().is_empty());
        assert!(is_question("[opus-1]   question: lower case", "opus-1"));
    }

    #[test]
    fn a_second_question_keeps_the_first_time_and_a_new_one_after_an_answer_opens_again() {
        let root = tempfile::tempdir().unwrap();
        let q = record(root.path());
        q.record_tell(Some("opus-1"), "orchestrator", "[opus-1] QUESTION: a")
            .unwrap();
        let first = q.open_for("opus-1").unwrap().unwrap();
        // Back-date it, as if it was asked an hour ago.
        let mut r = read(&q.path("opus-1")).unwrap().unwrap();
        r.question.asked_at = crate::clock::stamp(
            crate::clock::parse(&first.asked_at).unwrap() - chrono::Duration::hours(1),
        );
        q.write(&r).unwrap();
        q.record_tell(Some("opus-1"), "orchestrator", "[opus-1] QUESTION: b")
            .unwrap();
        let second = q.open_for("opus-1").unwrap().unwrap();
        assert_eq!(second.asked_at, r.question.asked_at);
        assert_eq!(second.text, "[opus-1] QUESTION: b");

        q.record_tell(None, "opus-1", "answer").unwrap();
        q.record_tell(Some("opus-1"), "orchestrator", "[opus-1] QUESTION: c")
            .unwrap();
        let third = q.open_for("opus-1").unwrap().unwrap();
        assert_ne!(third.asked_at, r.question.asked_at);
    }

    #[test]
    fn a_record_older_than_seven_days_is_dropped() {
        let root = tempfile::tempdir().unwrap();
        let q = record(root.path());
        let old = crate::clock::stamp(crate::clock::now() - chrono::Duration::days(8));
        q.write(&Record {
            question: OpenQuestion {
                role: "opus-1".into(),
                text: "[opus-1] QUESTION: old".into(),
                asked_at: old,
            },
            answered_at: None,
        })
        .unwrap();
        assert!(q.open().unwrap().is_empty());
        q.record_tell(Some("sonnet-1"), "orchestrator", "[sonnet-1] QUESTION: new")
            .unwrap();
        assert!(!q.path("opus-1").exists());
        assert_eq!(q.open().unwrap().len(), 1);
    }

    #[test]
    fn workspaces_do_not_share_questions() {
        let root = tempfile::tempdir().unwrap();
        Questions::under(root.path(), "w1")
            .record_tell(Some("opus-1"), "orchestrator", "[opus-1] QUESTION: x")
            .unwrap();
        assert!(Questions::under(root.path(), "w2")
            .open()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn a_record_that_does_not_parse_is_an_error_and_is_not_pruned() {
        let root = tempfile::tempdir().unwrap();
        let q = record(root.path());
        std::fs::create_dir_all(&q.dir).unwrap();
        std::fs::write(q.path("opus-1"), "{not json").unwrap();
        let e = q.open_for("opus-1").unwrap_err();
        assert!(
            format!("{e:#}").contains("is not a question record"),
            "{e:#}"
        );
        q.record_tell(Some("sonnet-1"), "orchestrator", "[sonnet-1] QUESTION: x")
            .unwrap();
        assert!(q.path("opus-1").exists(), "a corrupt record stays");
    }

    #[test]
    fn roles_that_differ_only_in_punctuation_have_their_own_records() {
        let root = tempfile::tempdir().unwrap();
        let q = record(root.path());
        for role in ["a_b", "a.b", "a+b"] {
            q.record_tell(Some(role), "orchestrator", &format!("[{role}] QUESTION: x"))
                .unwrap();
        }
        q.record_tell(None, "a.b", "answer").unwrap();
        assert!(q.open_for("a_b").unwrap().is_some());
        assert!(q.open_for("a.b").unwrap().is_none());
        assert!(q.open_for("a+b").unwrap().is_some());
        assert_eq!(file_name("a_b"), "a_5f_b");
    }

    #[test]
    fn a_record_that_names_another_role_is_an_error() {
        let root = tempfile::tempdir().unwrap();
        let q = record(root.path());
        q.record_tell(Some("opus-1"), "orchestrator", "[opus-1] QUESTION: x")
            .unwrap();
        std::fs::rename(q.path("opus-1"), q.path("opus-2")).unwrap();
        assert!(q.open_for("opus-2").is_err());
    }

    #[test]
    fn an_answer_keeps_the_record_7_days_from_the_answer() {
        let root = tempfile::tempdir().unwrap();
        let q = record(root.path());
        let days_ago =
            |d: i64| crate::clock::stamp(crate::clock::now() - chrono::Duration::days(d));
        let answered = |at: String| Record {
            question: OpenQuestion {
                role: "opus-1".into(),
                text: "[opus-1] QUESTION: x".into(),
                asked_at: days_ago(10),
            },
            answered_at: Some(at),
        };
        q.write(&answered(days_ago(6))).unwrap();
        q.record_tell(Some("sonnet-1"), "orchestrator", "[sonnet-1] QUESTION: y")
            .unwrap();
        assert!(q.path("opus-1").exists(), "answered 6 days ago: kept");
        q.write(&answered(days_ago(8))).unwrap();
        q.record_tell(Some("sonnet-1"), "orchestrator", "[sonnet-1] QUESTION: y")
            .unwrap();
        assert!(!q.path("opus-1").exists(), "answered 8 days ago: pruned");
    }

    #[test]
    fn concurrent_questions_and_answers_lose_no_record() {
        let root = tempfile::tempdir().unwrap();
        let q = std::sync::Arc::new(record(root.path()));
        let threads: Vec<_> = (0..2)
            .map(|t| {
                let q = std::sync::Arc::clone(&q);
                std::thread::spawn(move || {
                    for i in 0..6 {
                        let role = format!("r{t}-{i}");
                        q.record_tell(
                            Some(&role),
                            "orchestrator",
                            &format!("[{role}] QUESTION: x"),
                        )
                        .unwrap();
                        if i % 2 == 0 {
                            q.record_tell(None, &role, "answer").unwrap();
                        }
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(q.open().unwrap().len(), 6);
    }
}
