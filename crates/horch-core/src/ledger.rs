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
//!
//! Since A6 this is a facade: the record is
//! [`crate::execution::legacy::LedgerRecordV1`] and every read and write goes
//! through [`ExecutionStore`]. Only the lifecycle rules live here.

use std::path::Path;

use crate::execution::store::ExecutionStore;
use crate::teammates::Phase;
use anyhow::{bail, Result};

pub use crate::execution::legacy::{HistoryEntry, Record, KIND_ORCHESTRATOR, KIND_WORKER};
pub use crate::execution::store::slug;

/// A worker session's lifecycle state.
pub const STATUS_WORKING: &str = "working";
pub const STATUS_DONE: &str = "done";

/// Placeholder task for a worker spawned with nothing assigned yet.
const IDLE_TASK: &str = "(idle - awaiting assignment)";

/// Task an orchestrator record carries. It has no task of its own.
pub const ORCHESTRATING_TASK: &str = "(orchestrating)";

/// A project's ledger file.
#[derive(Debug, Clone)]
pub struct Ledger {
    store: ExecutionStore,
    /// What [`Ledger::insert`] fills into a record that leaves them empty.
    project: Option<String>,
    workspace_id: Option<String>,
}

/// UTC timestamp in the format the bash implementation wrote
/// (`date -u +%Y-%m-%dT%H:%M:%SZ`). Lexicographic order equals chronological
/// order, which is what the sorting relies on.
fn now() -> String {
    crate::clock::now_stamp()
}

/// Root directory for ledger files: `$HORCH_STATE_DIR`, else
/// `${XDG_STATE_HOME:-$HOME/.local/state}/horch`. Reads the process
/// environment; a command uses `RuntimeContext::paths` instead.
pub fn state_root() -> std::path::PathBuf {
    let env = crate::runtime::ProcessEnv;
    crate::runtime::paths::state_root(&env, &crate::runtime::paths::home_dir(&env))
}

/// The project dir a ledger belongs to: `$HORCH_PROJECT_DIR`, else the cwd.
/// Reads the process environment; a command uses `RuntimeContext::paths`.
pub fn project_dir() -> Result<std::path::PathBuf> {
    crate::runtime::paths::project_dir(&crate::runtime::ProcessEnv)
}

impl Ledger {
    /// The ledger for the ambient project, honouring `HORCH_STATE_DIR` and
    /// `HORCH_PROJECT_DIR`. Reads the process environment; a command uses
    /// [`Ledger::open_in`].
    pub fn open() -> Result<Self> {
        let project = project_dir()?;
        let workspace =
            crate::runtime::EnvSource::var(&crate::runtime::ProcessEnv, "HORCH_WORKSPACE_ID")
                .filter(|s| !s.is_empty());
        Ok(Self::for_project(state_root(), &project.to_string_lossy())
            .with_project(Some(project.to_string_lossy().into_owned()))
            .with_workspace(workspace))
    }

    /// The ledger for the context's project, under its state root. Records
    /// inserted here default to that project and the context's workspace.
    pub fn open_in(ctx: &crate::runtime::RuntimeContext) -> Result<Self> {
        let project = ctx.paths.project()?.to_string_lossy().into_owned();
        Ok(Self::for_project(&ctx.paths.state_root, &project)
            .with_project(Some(project))
            .with_workspace(ctx.herdr.workspace.as_ref().map(|w| w.to_string())))
    }

    /// The project [`Ledger::insert`] fills into a record without one.
    pub fn with_project(mut self, project: Option<String>) -> Self {
        self.project = project;
        self
    }

    /// The workspace [`Ledger::insert`] fills into a record without one.
    pub fn with_workspace(mut self, workspace_id: Option<String>) -> Self {
        self.workspace_id = workspace_id;
        self
    }

    /// The ledger for an explicit state root and project path.
    pub fn for_project(state_root: impl AsRef<Path>, project: &str) -> Self {
        Self {
            store: ExecutionStore::for_project(state_root, project),
            project: None,
            workspace_id: None,
        }
    }

    pub fn path(&self) -> &Path {
        self.store.path()
    }

    /// The store this ledger reads and writes through.
    pub fn store(&self) -> &ExecutionStore {
        &self.store
    }

    /// All records, oldest write order preserved. An absent or empty file reads
    /// as an empty ledger, matching the bash `[]` default.
    pub fn read(&self) -> Result<Vec<Record>> {
        self.store.read()
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
        self.add_with_phase(record_id, agent, tier, model, role, session_id, task, None)
    }

    /// Record a fresh session with its resolved work phase.
    #[allow(clippy::too_many_arguments)]
    pub fn add_with_phase(
        &self,
        record_id: &str,
        agent: &str,
        tier: &str,
        model: &str,
        role: &str,
        session_id: Option<&str>,
        task: &str,
        phase: Option<Phase>,
    ) -> Result<()> {
        self.insert(Record {
            record_id: record_id.to_string(),
            session_id: session_id.map(str::to_owned),
            agent: agent.to_string(),
            tier: tier.to_string(),
            model: model.to_string(),
            phase,
            role: role.to_string(),
            task: task.to_string(),
            ..Record::default()
        })
    }

    /// Record a fresh session from a filled-in [`Record`]. The lifecycle
    /// fields are set here: status `working`, the idle placeholder for an
    /// empty task, the `spawned` history entry, both timestamps, and the plan
    /// slug, project and workspace when the caller left them empty.
    pub fn insert(&self, mut record: Record) -> Result<()> {
        let at = now();
        record.session_id = record.session_id.filter(|s| !s.is_empty());
        record.set_legacy_status(STATUS_WORKING);
        let spawned = if record.task.is_empty() {
            format!("spawned idle as {}", record.role)
        } else {
            record.task.clone()
        };
        if record.task.is_empty() {
            record.task = IDLE_TASK.to_string();
        }
        if record.plan.is_none() {
            record.plan = crate::telemetry::plan_slug(&record.task);
        }
        if record.project.is_none() {
            record.project = self.project.clone();
        }
        if record.workspace_id.is_none() {
            record.workspace_id = self.workspace_id.clone();
        }
        record.history = vec![HistoryEntry {
            at: at.clone(),
            event: "spawned".to_string(),
            text: spawned,
        }];
        record.created_at = at.clone();
        record.updated_at = at;
        self.store.insert(record)
    }

    /// Mark live (starting or running) orchestrator records done, as
    /// `superseded`, unless `keep` says their workspace is still open. An
    /// orchestrator never runs `horch done`; the next `horch fleet` retires
    /// the ones whose workspace is gone instead. Returns how many records it
    /// closed. A planned record never launched, so it is left alone.
    pub fn supersede_orchestrators(&self, keep: impl Fn(&str) -> bool) -> Result<usize> {
        let at = now();
        self.store.update(|records| {
            let mut n = 0;
            for r in records.iter_mut().filter(|r| {
                r.is_orchestrator()
                    && r.execution_status().is_live()
                    && !r.workspace_id.as_deref().is_some_and(&keep)
            }) {
                r.set_legacy_status(STATUS_DONE);
                r.updated_at = at.clone();
                r.history.push(HistoryEntry {
                    at: at.clone(),
                    event: "done".to_string(),
                    text: "superseded".to_string(),
                });
                n += 1;
            }
            Ok(n)
        })
    }

    /// Attach a session id discovered after launch (the codex case).
    pub fn set_session(&self, key: &str, session_id: &str) -> Result<()> {
        let at = now();
        self.store.update_key(key, |r| {
            r.session_id = Some(session_id.to_string());
            r.updated_at = at.clone();
        })
    }

    /// Record the effort level a session was launched with. `None` clears it:
    /// the agent then ran at its own default (or the operator's config).
    pub fn set_effort(&self, key: &str, effort: Option<&str>) -> Result<()> {
        self.store
            .update_key(key, |r| r.effort = effort.map(str::to_owned))
    }

    /// Record what routing decided for a session. `None` clears it.
    pub fn set_routing(
        &self,
        key: &str,
        routing: Option<&crate::routing::decision::RoutingProvenance>,
    ) -> Result<()> {
        self.store.update_key(key, |r| r.routing = routing.cloned())
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
        self.resume_with_phase(key, role, task, None)
    }

    /// Resume, preserving the phase unless an override is supplied.
    pub fn resume_with_phase(
        &self,
        key: &str,
        role: &str,
        task: &str,
        phase: Option<Phase>,
    ) -> Result<()> {
        let at = now();
        self.store.update_key(key, |r| {
            r.phase = phase.or(r.phase);
            r.set_legacy_status(STATUS_WORKING);
            r.role = role.to_string();
            r.updated_at = at.clone();
            if !task.is_empty() {
                r.task = task.to_string();
                r.plan = crate::telemetry::plan_slug(task);
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
        })
    }

    /// Point a live worker at a new task.
    ///
    /// Targets the most recently updated live (starting or running) record
    /// for the role, so assigning updates what that worker is doing without
    /// minting a new session. A planned record has no agent yet.
    pub fn assign(&self, role: &str, task: &str) -> Result<()> {
        let at = now();
        self.store.update(|records| {
            let key = records
                .iter()
                .filter(|r| r.role == role && r.execution_status().is_live())
                .max_by(|a, b| a.updated_at.cmp(&b.updated_at))
                .map(|r| r.record_id.clone());
            let Some(key) = key else {
                bail!("no working session for role '{role}'")
            };
            for r in records.iter_mut().filter(|r| r.record_id == key) {
                r.task = task.to_string();
                r.plan = crate::telemetry::plan_slug(task);
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
        self.store.update_key(key, |r| {
            r.set_legacy_status(STATUS_DONE);
            r.updated_at = at.clone();
            r.history.push(HistoryEntry {
                at: at.clone(),
                event: "done".to_string(),
                text: summary.to_string(),
            });
        })
    }

    fn append_event(&self, key: &str, event: &str, text: &str) -> Result<()> {
        let at = now();
        self.store.update_key(key, |r| {
            r.updated_at = at.clone();
            r.history.push(HistoryEntry {
                at: at.clone(),
                event: event.to_string(),
                text: text.to_string(),
            });
        })
    }

    /// The newest record addressed by `key`.
    pub fn get(&self, key: &str) -> Result<Record> {
        self.store.get(key)
    }

    /// Human-readable ledger, newest first.
    ///
    /// Written for the orchestrator LLM to read when deciding "fresh session or
    /// resume": every line it needs to weigh relevance and pick a resumable
    /// session id.
    pub fn render(&self) -> Result<String> {
        Ok(ExecutionStore::render_text(&self.read()?))
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

    #[test]
    fn phase_persists_and_resume_can_override_it() {
        let (_t, l) = ledger();
        l.add_with_phase(
            "r1",
            "codex",
            "codex-sol",
            "sol",
            "worker",
            Some("s1"),
            "task",
            Some(Phase::Research),
        )
        .unwrap();
        l.resume("r1", "worker-2", "").unwrap();
        assert_eq!(l.get("r1").unwrap().phase, Some(Phase::Research));
        l.resume_with_phase("r1", "worker-3", "implement", Some(Phase::Implementation))
            .unwrap();
        assert_eq!(l.get("s1").unwrap().phase, Some(Phase::Implementation));
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
        assert_eq!(
            l.render().unwrap(),
            "no sessions recorded for this project yet\n"
        );
    }

    #[test]
    fn add_then_get_round_trips() {
        let (_t, l) = ledger();
        l.add(
            "r1",
            "claude",
            "sonnet",
            "sonnet",
            "sonnet-1",
            Some("s1"),
            "fix auth",
        )
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
        l.add("r1", "claude", "opus", "opus", "opus-1", None, "")
            .unwrap();
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
        l.add(
            "r1",
            "codex",
            "codex-sol",
            "gpt-5.6-sol",
            "codex-sol-1",
            Some(""),
            "",
        )
        .unwrap();
        assert_eq!(l.get("r1").unwrap().session_id, None);
        assert!(!l.has_session("").unwrap());
    }

    #[test]
    fn set_session_then_has_session() {
        let (_t, l) = ledger();
        l.add(
            "r1",
            "codex",
            "codex-sol",
            "gpt-5.6-sol",
            "codex-sol-1",
            None,
            "",
        )
        .unwrap();
        assert!(!l.has_session("s9").unwrap());
        l.set_session("r1", "s9").unwrap();
        assert!(l.has_session("s9").unwrap());
        assert_eq!(l.get("s9").unwrap().record_id, "r1");
    }

    #[test]
    fn note_and_done_build_history_and_flip_status() {
        let (_t, l) = ledger();
        l.add(
            "r1",
            "claude",
            "sonnet",
            "sonnet",
            "sonnet-1",
            Some("s1"),
            "t",
        )
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
        l.add(
            "old",
            "claude",
            "sonnet",
            "sonnet",
            "sonnet-1",
            Some("s1"),
            "first",
        )
        .unwrap();
        l.done("old", "finished").unwrap();
        l.add(
            "live",
            "claude",
            "sonnet",
            "sonnet",
            "sonnet-1",
            Some("s2"),
            "second",
        )
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
        l.add(
            "r1",
            "claude",
            "opus",
            "opus",
            "opus-1",
            Some("s1"),
            "original",
        )
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
        l.add(
            "r1",
            "claude",
            "sonnet",
            "sonnet",
            "sonnet-1",
            Some("s1"),
            "first",
        )
        .unwrap();
        for n in 1..=4 {
            l.note("r1", &format!("note {n}")).unwrap();
        }
        l.add(
            "r2",
            "codex",
            "codex-sol",
            "gpt-5.6-sol",
            "codex-sol-1",
            None,
            "",
        )
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
            assert!(
                out.contains(&format!("note {n}")),
                "missing note {n}:\n{out}"
            );
        }
    }

    /// A fleet's orchestrator is a record too; it is rendered apart from the
    /// workers and retired by the next fleet in its workspace.
    #[test]
    fn tel_02_orchestrator_records_render_apart_and_are_superseded() {
        let (_t, l) = ledger();
        l.add(
            "w1",
            "claude",
            "sonnet",
            "sonnet",
            "sonnet-1",
            Some("s1"),
            "t",
        )
        .unwrap();
        l.insert(Record {
            record_id: "o1".into(),
            session_id: Some("s-orch".into()),
            agent: "claude".into(),
            tier: "orchestrator".into(),
            model: "opus".into(),
            role: "orchestrator".into(),
            kind: KIND_ORCHESTRATOR.into(),
            task: ORCHESTRATING_TASK.into(),
            workspace_id: Some("w9".into()),
            ..Record::default()
        })
        .unwrap();
        let out = l.render().unwrap();
        let orch = out.find("record=o1").unwrap();
        let workers = out.find("== workers ==").unwrap();
        assert!(
            orch < workers && workers < out.find("record=w1").unwrap(),
            "{out}"
        );
        assert!(l.get("o1").unwrap().is_orchestrator());

        assert_eq!(
            l.supersede_orchestrators(|ws| ws == "w9").unwrap(),
            0,
            "w9 is open"
        );
        assert_eq!(l.supersede_orchestrators(|ws| ws == "w8").unwrap(), 1);
        let o = l.get("o1").unwrap();
        assert_eq!(o.status, STATUS_DONE);
        assert_eq!(o.history.last().unwrap().text, "superseded");
        assert_eq!(
            l.get("w1").unwrap().status,
            STATUS_WORKING,
            "workers untouched"
        );
    }
}
