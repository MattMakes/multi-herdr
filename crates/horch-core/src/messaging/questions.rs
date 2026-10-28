//! Open questions (#70): a `QUESTION:` or `BLOCKED:` a role sent to the
//! orchestrator, with no later orchestrator `tell` to that role.
//!
//! The record lives per workspace under the state folder, next to the mail,
//! so a fleet with no task-ledger run has it too. One file per role holds
//! its newest question; an answer marks it answered. A record is kept for
//! the acked-mail retention (7 days) after its last change.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// How long a record is kept after its last change: the acked-mail retention.
const RETENTION_S: i64 = 7 * 24 * 60 * 60;

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
            dir: state_root.join("questions").join(file_safe(workspace)),
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

    /// The open question of `role`, if it has one.
    pub fn open_for(&self, role: &str) -> Result<Option<OpenQuestion>> {
        Ok(read(&self.path(role))?
            .filter(|r| r.answered_at.is_none() && !expired(r))
            .map(|r| r.question))
    }

    fn path(&self, role: &str) -> PathBuf {
        self.dir.join(format!("{}.json", file_safe(role)))
    }

    /// Open `role`'s question. A question while one is open keeps the first
    /// one's time: the role has waited since then.
    fn ask(&self, role: &str, line: &str) -> Result<()> {
        self.prune()?;
        let now = crate::clock::now_stamp();
        let asked_at = self
            .open_for(role)?
            .map(|q| q.asked_at)
            .unwrap_or_else(|| now.clone());
        self.write(&Record {
            question: OpenQuestion {
                role: role.to_string(),
                text: line.to_string(),
                asked_at,
            },
            answered_at: None,
        })
    }

    fn answer(&self, role: &str) -> Result<()> {
        let Some(mut r) = read(&self.path(role))? else {
            return Ok(());
        };
        if r.answered_at.is_some() {
            return Ok(());
        }
        r.answered_at = Some(crate::clock::now_stamp());
        self.write(&r)
    }

    fn write(&self, r: &Record) -> Result<()> {
        std::fs::create_dir_all(&self.dir)
            .with_context(|| format!("creating {}", self.dir.display()))?;
        let path = self.path(&r.question.role);
        let temp = path.with_extension(format!("json.{}.tmp", std::process::id()));
        std::fs::write(&temp, serde_json::to_vec(r)?)
            .with_context(|| format!("writing {}", temp.display()))?;
        std::fs::rename(&temp, &path)
            .with_context(|| format!("moving {} to {}", temp.display(), path.display()))
    }

    /// Remove the records past the retention.
    fn prune(&self) -> Result<()> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Ok(());
        };
        for entry in entries {
            let path = entry?.path();
            if path.extension().is_some_and(|x| x == "json")
                && read(&path)?.is_some_and(|r| expired(&r))
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

fn read(path: &Path) -> Result<Option<Record>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes).ok()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

fn expired(r: &Record) -> bool {
    let (Some(at), now) = (crate::clock::parse(r.last_change()), crate::clock::now()) else {
        return true;
    };
    (now - at).num_seconds() > RETENTION_S
}

fn file_safe(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
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
}
