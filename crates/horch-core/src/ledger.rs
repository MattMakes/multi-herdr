//! Persistent per-project session ledger.
//!
//! One JSON file per project dir, so "what has been worked on here, by which
//! session" survives pane close and workspace restarts - that is what lets the
//! orchestrator resume an old session id instead of starting cold. Records are
//! keyed by `record_id` (minted at spawn, always known) with `session_id` as a
//! separate field, because codex only reveals its session id after launch.
//!
//! The on-disk format, file path, slug rule and timestamp format are unchanged
//! from the bash implementation: existing ledgers stay readable and resumable.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

/// A worker session's lifecycle state.
pub const STATUS_WORKING: &str = "working";
pub const STATUS_DONE: &str = "done";

/// Placeholder task for a worker spawned with nothing assigned yet.
const IDLE_TASK: &str = "(idle - awaiting assignment)";

/// One entry in a record's history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub at: String,
    pub event: String,
    pub text: String,
}

/// One worker session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    pub record_id: String,
    /// `null` until known: codex only reveals its session id after launch.
    pub session_id: Option<String>,
    pub agent: String,
    /// The teammate's name. Still spelled `tier` on disk: ledgers written by
    /// earlier versions carry this key, and the built-in teammates kept the old
    /// tier ids as their names precisely so those records still resolve.
    pub tier: String,
    pub model: String,
    pub role: String,
    pub status: String,
    pub task: String,
    pub history: Vec<HistoryEntry>,
    pub created_at: String,
    pub updated_at: String,
}

impl Record {
    /// Records match on either id so callers can address a session by whichever
    /// one they hold.
    fn matches(&self, key: &str) -> bool {
        self.record_id == key || self.session_id.as_deref() == Some(key)
    }
}

/// A project's ledger file.
#[derive(Debug, Clone)]
pub struct Ledger {
    path: PathBuf,
}

/// Held for the duration of a read-modify-write. Released on drop.
struct LockGuard {
    path: PathBuf,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir(&self.path);
    }
}

/// UTC timestamp in the format the bash implementation wrote
/// (`date -u +%Y-%m-%dT%H:%M:%SZ`). Lexicographic order equals chronological
/// order, which is what the sorting relies on.
fn now() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Turn a project path into a filename, matching `tr -c 'A-Za-z0-9' '-'`.
///
/// Operates on bytes, exactly as `tr` does, so a multi-byte character becomes one
/// `-` per byte and slugs computed by the bash version still resolve.
pub fn slug(project: &str) -> String {
    project
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() {
                b as char
            } else {
                '-'
            }
        })
        .collect()
}

/// Root directory for ledger files: `$HORCH_STATE_DIR`, else
/// `${XDG_STATE_HOME:-$HOME/.local/state}/horch`.
pub fn state_root() -> PathBuf {
    if let Some(dir) = std::env::var_os("HORCH_STATE_DIR").filter(|v| !v.is_empty()) {
        return PathBuf::from(dir);
    }
    let base = std::env::var_os("XDG_STATE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".local").join("state"));
    base.join("horch")
}

/// The project dir a ledger belongs to: `$HORCH_PROJECT_DIR`, else the cwd.
pub fn project_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("HORCH_PROJECT_DIR").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    std::env::current_dir().context("resolving the current directory")
}

fn home_dir() -> PathBuf {
    let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(key)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

impl Ledger {
    /// The ledger for the ambient project, honouring `HORCH_STATE_DIR` and
    /// `HORCH_PROJECT_DIR`.
    pub fn open() -> Result<Self> {
        let project = project_dir()?;
        Ok(Self::for_project(state_root(), &project.to_string_lossy()))
    }

    /// The ledger for an explicit state root and project path.
    pub fn for_project(state_root: impl AsRef<Path>, project: &str) -> Self {
        Self {
            path: state_root
                .as_ref()
                .join(format!("{}.json", slug(project))),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// All records, oldest write order preserved. An absent or empty file reads
    /// as an empty ledger, matching the bash `[]` default.
    pub fn read(&self) -> Result<Vec<Record>> {
        let Ok(raw) = std::fs::read_to_string(&self.path) else {
            return Ok(Vec::new());
        };
        if raw.trim().is_empty() {
            return Ok(Vec::new());
        }
        serde_json::from_str(&raw)
            .with_context(|| format!("parsing ledger {}", self.path.display()))
    }

    /// Acquire the cross-process lock.
    ///
    /// `create_dir` is atomic on both platforms, which is why the bash mkdir
    /// spinlock ports directly (stock macOS has no `flock`). Panes can be killed
    /// mid-write, so a lock older than ~15s is broken rather than deadlocking the
    /// whole fleet.
    fn lock(&self) -> Result<LockGuard> {
        let root = self
            .path
            .parent()
            .context("ledger path has no parent directory")?;
        std::fs::create_dir_all(root)
            .with_context(|| format!("creating state dir {}", root.display()))?;
        let lock_path = self.path.with_extension("json.lock");
        for attempt in 0.. {
            match std::fs::create_dir(&lock_path) {
                Ok(()) => return Ok(LockGuard { path: lock_path }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if attempt >= 150 {
                        eprintln!("horch ledger: breaking stale lock {}", lock_path.display());
                        let _ = std::fs::remove_dir_all(&lock_path);
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(e) => {
                    return Err(e)
                        .with_context(|| format!("acquiring lock {}", lock_path.display()))
                }
            }
        }
        unreachable!()
    }

    /// Write via temp file + rename, so a killed process cannot leave a
    /// half-written ledger.
    fn write(&self, records: &[Record]) -> Result<()> {
        let tmp = self.path.with_extension(format!("json.tmp.{}", std::process::id()));
        let json = serde_json::to_string_pretty(records)?;
        std::fs::write(&tmp, json).with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, &self.path)
            .with_context(|| format!("replacing {}", self.path.display()))
    }

    /// Take the lock, mutate the records, write them back.
    fn update<T>(&self, f: impl FnOnce(&mut Vec<Record>) -> Result<T>) -> Result<T> {
        let _guard = self.lock()?;
        let mut records = self.read()?;
        let out = f(&mut records)?;
        self.write(&records)?;
        Ok(out)
    }

    fn require_key(records: &[Record], key: &str, path: &Path) -> Result<()> {
        if records.iter().any(|r| r.matches(key)) {
            return Ok(());
        }
        bail!("no record matches '{key}' in {}", path.display())
    }

    /// Record a freshly spawned session.
    #[allow(clippy::too_many_arguments)]
    pub fn add(
        &self,
        record_id: &str,
        agent: &str,
        tier: &str,
        model: &str,
        role: &str,
        session_id: Option<&str>,
        task: &str,
    ) -> Result<()> {
        let at = now();
        let record = Record {
            record_id: record_id.to_string(),
            session_id: session_id.filter(|s| !s.is_empty()).map(str::to_owned),
            agent: agent.to_string(),
            tier: tier.to_string(),
            model: model.to_string(),
            role: role.to_string(),
            status: STATUS_WORKING.to_string(),
            task: if task.is_empty() {
                IDLE_TASK.to_string()
            } else {
                task.to_string()
            },
            history: vec![HistoryEntry {
                at: at.clone(),
                event: "spawned".to_string(),
                text: if task.is_empty() {
                    format!("spawned idle as {role}")
                } else {
                    task.to_string()
                },
            }],
            created_at: at.clone(),
            updated_at: at,
        };
        self.update(|records| {
            records.push(record);
            Ok(())
        })
    }

    /// Attach a session id discovered after launch (the codex case).
    pub fn set_session(&self, key: &str, session_id: &str) -> Result<()> {
        let at = now();
        self.update(|records| {
            Self::require_key(records, key, &self.path)?;
            for r in records.iter_mut().filter(|r| r.matches(key)) {
                r.session_id = Some(session_id.to_string());
                r.updated_at = at.clone();
            }
            Ok(())
        })
    }

    /// True when any record already claims this session id. Used to stop two
    /// concurrently spawned codex workers from harvesting the same rollout file.
    pub fn has_session(&self, session_id: &str) -> Result<bool> {
        Ok(self
            .read()?
            .iter()
            .any(|r| r.session_id.as_deref() == Some(session_id)))
    }

    /// Re-open a finished session under `role`.
    pub fn resume(&self, key: &str, role: &str, task: &str) -> Result<()> {
        let at = now();
        self.update(|records| {
            Self::require_key(records, key, &self.path)?;
            for r in records.iter_mut().filter(|r| r.matches(key)) {
                r.status = STATUS_WORKING.to_string();
                r.role = role.to_string();
                r.updated_at = at.clone();
                if !task.is_empty() {
                    r.task = task.to_string();
                }
                r.history.push(HistoryEntry {
                    at: at.clone(),
                    event: "resumed".to_string(),
                    text: if task.is_empty() {
                        format!("resumed as {role}")
                    } else {
                        task.to_string()
                    },
                });
            }
            Ok(())
        })
    }

    /// Point a live worker at a new task.
    ///
    /// Targets the most recently updated `working` record for the role, so
    /// assigning updates what that worker is doing without minting a new session.
    pub fn assign(&self, role: &str, task: &str) -> Result<()> {
        let at = now();
        self.update(|records| {
            let key = records
                .iter()
                .filter(|r| r.role == role && r.status == STATUS_WORKING)
                .max_by(|a, b| a.updated_at.cmp(&b.updated_at))
                .map(|r| r.record_id.clone());
            let Some(key) = key else {
                bail!("no working session for role '{role}'")
            };
            for r in records.iter_mut().filter(|r| r.record_id == key) {
                r.task = task.to_string();
                r.updated_at = at.clone();
                r.history.push(HistoryEntry {
                    at: at.clone(),
                    event: "assigned".to_string(),
                    text: task.to_string(),
                });
            }
            Ok(())
        })
    }

    pub fn note(&self, key: &str, text: &str) -> Result<()> {
        self.append_event(key, "note", text)
    }

    /// Mark a session finished and record its handoff summary.
    pub fn done(&self, key: &str, summary: &str) -> Result<()> {
        let at = now();
        self.update(|records| {
            Self::require_key(records, key, &self.path)?;
            for r in records.iter_mut().filter(|r| r.matches(key)) {
                r.status = STATUS_DONE.to_string();
                r.updated_at = at.clone();
                r.history.push(HistoryEntry {
                    at: at.clone(),
                    event: "done".to_string(),
                    text: summary.to_string(),
                });
            }
            Ok(())
        })
    }

    fn append_event(&self, key: &str, event: &str, text: &str) -> Result<()> {
        let at = now();
        self.update(|records| {
            Self::require_key(records, key, &self.path)?;
            for r in records.iter_mut().filter(|r| r.matches(key)) {
                r.updated_at = at.clone();
                r.history.push(HistoryEntry {
                    at: at.clone(),
                    event: event.to_string(),
                    text: text.to_string(),
                });
            }
            Ok(())
        })
    }

    /// The newest record addressed by `key`.
    pub fn get(&self, key: &str) -> Result<Record> {
        self.read()?
            .into_iter()
            .filter(|r| r.matches(key))
            .next_back()
            .with_context(|| format!("no record matches {key}"))
    }

    /// Human-readable ledger, newest first.
    ///
    /// Written for the orchestrator LLM to read when deciding "fresh session or
    /// resume": every line it needs to weigh relevance and pick a resumable
    /// session id.
    pub fn render(&self) -> Result<String> {
        let mut records = self.read()?;
        if records.is_empty() {
            return Ok("no sessions recorded for this project yet\n".to_string());
        }
        records.sort_by(|a, b| a.updated_at.cmp(&b.updated_at));
        records.reverse();

        let mut out = String::new();
        for r in &records {
            let session = r.session_id.as_deref().unwrap_or("not-yet-known");
            out.push_str(&format!(
                "[{}] {} ({})  session={}  record={}\n",
                r.status, r.tier, r.role, session, r.record_id
            ));
            out.push_str(&format!(
                "  agent={} model={}  updated={}\n",
                r.agent, r.model, r.updated_at
            ));
            out.push_str(&format!("  task: {}\n", r.task));
            let notable: Vec<&HistoryEntry> = r
                .history
                .iter()
                .filter(|h| h.event == "done" || h.event == "note")
                .collect();
            for h in notable.iter().rev().take(3).rev() {
                out.push_str(&format!("  {} @ {}: {}\n", h.event, h.at, h.text));
            }
            out.push('\n');
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ledger() -> (tempfile::TempDir, Ledger) {
        let tmp = tempfile::tempdir().unwrap();
        let l = Ledger::for_project(tmp.path(), "/Users/a/proj");
        (tmp, l)
    }

    /// The slug rule decides which file an existing ledger lives in. Changing it
    /// would orphan every recorded session.
    #[test]
    fn slug_matches_tr_semantics() {
        assert_eq!(slug("/Users/a/proj"), "-Users-a-proj");
        assert_eq!(slug("/tmp/a.b-c_d"), "-tmp-a-b-c-d");
        assert_eq!(slug(r"C:\Users\a b\p"), "C--Users-a-b-p");
        // tr works on bytes, so one multi-byte char becomes one dash per byte.
        assert_eq!(slug("/é"), "---");
    }

    #[test]
    fn absent_ledger_reads_empty_and_renders_a_hint() {
        let (_t, l) = ledger();
        assert!(l.read().unwrap().is_empty());
        assert_eq!(l.render().unwrap(), "no sessions recorded for this project yet\n");
    }

    #[test]
    fn add_then_get_round_trips() {
        let (_t, l) = ledger();
        l.add("r1", "claude", "sonnet", "sonnet", "sonnet-1", Some("s1"), "fix auth")
            .unwrap();

        // Addressable by record id and by session id.
        for key in ["r1", "s1"] {
            let r = l.get(key).unwrap();
            assert_eq!(r.record_id, "r1");
            assert_eq!(r.status, STATUS_WORKING);
            assert_eq!(r.task, "fix auth");
            assert_eq!(r.history.len(), 1);
            assert_eq!(r.history[0].event, "spawned");
            assert_eq!(r.history[0].text, "fix auth");
        }
    }

    #[test]
    fn idle_spawn_gets_the_placeholder_task() {
        let (_t, l) = ledger();
        l.add("r1", "claude", "opus", "opus", "opus-1", None, "").unwrap();
        let r = l.get("r1").unwrap();
        assert_eq!(r.task, "(idle - awaiting assignment)");
        assert_eq!(r.history[0].text, "spawned idle as opus-1");
        assert_eq!(r.session_id, None);
    }

    /// An empty `--session-id` must persist as JSON null, not as `""`, or
    /// `has_session("")` would match and resume would offer a bogus handle.
    #[test]
    fn empty_session_id_becomes_null() {
        let (_t, l) = ledger();
        l.add("r1", "codex", "codex-sol", "gpt-5.6-sol", "codex-sol-1", Some(""), "")
            .unwrap();
        assert_eq!(l.get("r1").unwrap().session_id, None);
        assert!(!l.has_session("").unwrap());
    }

    #[test]
    fn set_session_then_has_session() {
        let (_t, l) = ledger();
        l.add("r1", "codex", "codex-sol", "gpt-5.6-sol", "codex-sol-1", None, "")
            .unwrap();
        assert!(!l.has_session("s9").unwrap());
        l.set_session("r1", "s9").unwrap();
        assert!(l.has_session("s9").unwrap());
        assert_eq!(l.get("s9").unwrap().record_id, "r1");
    }

    #[test]
    fn note_and_done_build_history_and_flip_status() {
        let (_t, l) = ledger();
        l.add("r1", "claude", "sonnet", "sonnet", "sonnet-1", Some("s1"), "t")
            .unwrap();
        l.note("r1", "halfway").unwrap();
        l.done("s1", "all finished").unwrap();

        let r = l.get("r1").unwrap();
        assert_eq!(r.status, STATUS_DONE);
        let events: Vec<&str> = r.history.iter().map(|h| h.event.as_str()).collect();
        assert_eq!(events, vec!["spawned", "note", "done"]);
    }

    #[test]
    fn assign_targets_the_latest_working_record_for_the_role() {
        let (_t, l) = ledger();
        l.add("old", "claude", "sonnet", "sonnet", "sonnet-1", Some("s1"), "first")
            .unwrap();
        l.done("old", "finished").unwrap();
        l.add("live", "claude", "sonnet", "sonnet", "sonnet-1", Some("s2"), "second")
            .unwrap();

        l.assign("sonnet-1", "third").unwrap();
        assert_eq!(l.get("live").unwrap().task, "third");
        // The completed record must not be rewritten.
        assert_eq!(l.get("old").unwrap().task, "first");
    }

    #[test]
    fn assign_without_a_live_worker_is_an_error() {
        let (_t, l) = ledger();
        let err = l.assign("ghost", "task").unwrap_err().to_string();
        assert!(err.contains("no working session for role 'ghost'"), "{err}");
    }

    #[test]
    fn resume_reopens_under_a_new_role_and_keeps_the_old_task_when_none_given() {
        let (_t, l) = ledger();
        l.add("r1", "claude", "opus", "opus", "opus-1", Some("s1"), "original")
            .unwrap();
        l.done("r1", "done for now").unwrap();

        l.resume("s1", "opus-7", "").unwrap();
        let r = l.get("r1").unwrap();
        assert_eq!(r.status, STATUS_WORKING);
        assert_eq!(r.role, "opus-7");
        assert_eq!(r.task, "original");
        assert_eq!(r.history.last().unwrap().text, "resumed as opus-7");

        l.resume("s1", "opus-8", "new work").unwrap();
        assert_eq!(l.get("r1").unwrap().task, "new work");
    }

    #[test]
    fn operations_on_unknown_keys_name_the_ledger_file() {
        let (_t, l) = ledger();
        for err in [
            l.note("nope", "x").unwrap_err(),
            l.done("nope", "x").unwrap_err(),
            l.set_session("nope", "s").unwrap_err(),
            l.resume("nope", "r", "").unwrap_err(),
        ] {
            let msg = err.to_string();
            assert!(msg.contains("no record matches 'nope'"), "{msg}");
            assert!(msg.contains("-Users-a-proj.json"), "{msg}");
        }
    }

    #[test]
    fn render_lists_newest_first_and_caps_history_at_three_entries() {
        let (_t, l) = ledger();
        l.add("r1", "claude", "sonnet", "sonnet", "sonnet-1", Some("s1"), "first")
            .unwrap();
        for n in 1..=4 {
            l.note("r1", &format!("note {n}")).unwrap();
        }
        l.add("r2", "codex", "codex-sol", "gpt-5.6-sol", "codex-sol-1", None, "")
            .unwrap();

        let out = l.render().unwrap();
        let r2_at = out.find("record=r2").unwrap();
        let r1_at = out.find("record=r1").unwrap();
        assert!(r2_at < r1_at, "newest record must come first:\n{out}");

        assert!(out.contains("[working] sonnet (sonnet-1)  session=s1  record=r1"));
        assert!(out.contains("session=not-yet-known"));
        assert!(out.contains("  task: (idle - awaiting assignment)"));
        // Only the last three note/done entries are shown.
        assert!(!out.contains("note 1"));
        for n in 2..=4 {
            assert!(out.contains(&format!("note {n}")), "missing note {n}:\n{out}");
        }
    }

    /// A killed pane can leave the lock dir behind; the ledger must recover
    /// rather than hang forever.
    #[test]
    fn a_stale_lock_is_broken_rather_than_deadlocking() {
        let (_t, l) = ledger();
        std::fs::create_dir_all(l.path().parent().unwrap()).unwrap();
        std::fs::create_dir(l.path().with_extension("json.lock")).unwrap();

        let started = std::time::Instant::now();
        l.add("r1", "claude", "sonnet", "sonnet", "sonnet-1", Some("s1"), "t")
            .unwrap();
        assert!(l.get("r1").is_ok());
        // ~15s of spinning before the break, then success.
        assert!(started.elapsed() < Duration::from_secs(45));
    }

    /// Records written by the bash implementation must still load.
    #[test]
    fn reads_a_ledger_written_by_the_bash_version() {
        let tmp = tempfile::tempdir().unwrap();
        let l = Ledger::for_project(tmp.path(), "/p");
        std::fs::create_dir_all(tmp.path()).unwrap();
        std::fs::write(
            l.path(),
            r#"[
              {"record_id":"3f2b","session_id":null,"agent":"codex","tier":"codex-sol",
               "model":"gpt-5.6-sol","role":"codex-sol-1","status":"working",
               "task":"(idle - awaiting assignment)",
               "history":[{"at":"2026-07-08T10:00:00Z","event":"spawned",
                           "text":"spawned idle as codex-sol-1"}],
               "created_at":"2026-07-08T10:00:00Z","updated_at":"2026-07-08T10:00:00Z"}
            ]"#,
        )
        .unwrap();

        let r = l.get("3f2b").unwrap();
        assert_eq!(r.role, "codex-sol-1");
        assert_eq!(r.session_id, None);
        // And it stays writable.
        l.note("3f2b", "still going").unwrap();
        assert_eq!(l.get("3f2b").unwrap().history.len(), 2);
    }
}
