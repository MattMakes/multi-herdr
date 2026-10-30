//! The lifecycle rules of the persistent per-project session ledger.
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
//! The record is [`crate::execution::legacy::LedgerRecordV1`] and every read
//! and write goes through [`ExecutionStore`]. Only the lifecycle rules live
//! here.

use std::path::Path;

use crate::execution::legacy::{HistoryEntry, Record, STATUS_DONE, STATUS_WORKING};
use crate::execution::store::ExecutionStore;
use crate::roster::Phase;
use anyhow::{bail, Result};

/// Placeholder task for a worker spawned with nothing assigned yet.
const IDLE_TASK: &str = "(idle - awaiting assignment)";

/// Task an orchestrator record carries. It has no task of its own.
pub const ORCHESTRATING_TASK: &str = "(orchestrating)";

/// The history event of a `horch done` whose report did not reach the
/// orchestrator. Its text is the error; the record stays live.
pub const EVENT_REPORT_FAILED: &str = "report-failed";

/// The history event of a durable `horch done` report whose push failed.
pub const EVENT_REPORT_QUEUED: &str = "report-queued";

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

/// The fields of a fresh session record, named so adjacent strings cannot swap.
pub struct NewRecord<'a> {
    pub record_id: &'a str,
    pub agent: &'a str,
    pub tier: &'a str,
    pub model: &'a str,
    pub role: &'a str,
    pub session_id: Option<&'a str>,
    pub task: &'a str,
    pub phase: Option<Phase>,
}

impl Ledger {
    /// The ledger for the context's project, under its state root. Records
    /// inserted here default to that project and the context's workspace.
    pub fn open_in(ctx: &crate::runtime::RuntimeContext) -> Result<Self> {
        let project = ctx.paths.project()?.to_string_lossy().into_owned();
        Ok(Self::for_project(&ctx.paths.state_root, &project)
            .with_project(Some(project))
            .with_workspace(ctx.herdr.workspace.as_ref().map(|w| w.to_string())))
    }

    /// The project [`Ledger::insert`] fills into a record without one.
    pub(crate) fn with_project(mut self, project: Option<String>) -> Self {
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

    /// Record a freshly spawned session with its resolved work phase.
    pub fn add(&self, new: NewRecord<'_>) -> Result<()> {
        self.insert(Record {
            record_id: new.record_id.to_string(),
            session_id: new.session_id.map(str::to_owned),
            agent: new.agent.to_string(),
            tier: new.tier.to_string(),
            model: new.model.to_string(),
            phase: new.phase,
            role: new.role.to_string(),
            task: new.task.to_string(),
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
    pub(crate) fn resume_with_phase(
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

    /// The live (starting or running) record of `role` with the newest
    /// `updated_at`. With `Some(ws)`, only records whose `workspace_id` is
    /// `ws` or absent count (review finding 13): 2 fleets of 1 project can
    /// both have a `sonnet-1`. The 1 selection rule of [`Ledger::assign_in`],
    /// `horch compact` and the note check.
    pub fn live_for_role(&self, role: &str, workspace: Option<&str>) -> Result<Option<Record>> {
        Ok(pick_live(&self.read()?, role, workspace).cloned())
    }

    /// Point a live worker at a new task.
    ///
    /// Targets the most recently updated live (starting or running) record
    /// for the role, so assigning updates what that worker is doing without
    /// minting a new session. A planned record has no agent yet.
    pub fn assign(&self, role: &str, task: &str) -> Result<()> {
        self.assign_in(role, None, task)
    }

    /// [`Ledger::assign`] to the record [`Ledger::live_for_role`] selects in
    /// `workspace`.
    pub fn assign_in(&self, role: &str, workspace: Option<&str>, task: &str) -> Result<()> {
        let at = now();
        self.store.update(|records| {
            let Some(key) = pick_live(records, role, workspace).map(|r| r.record_id.clone()) else {
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

    /// Mark a session finished and record its handoff summary. The done
    /// time is the record's `finished_at` (FDS-23).
    pub fn done(&self, key: &str, summary: &str) -> Result<()> {
        let at = now();
        self.store.update_key(key, |r| {
            r.set_legacy_status(STATUS_DONE);
            r.finished_at = Some(at.clone());
            r.updated_at = at.clone();
            r.history.push(HistoryEntry {
                at: at.clone(),
                event: "done".to_string(),
                text: summary.to_string(),
            });
        })
    }

    /// Whether the record `key` holds a `done` summary from its current run:
    /// a `done` history entry newer than its last `spawned` or `resumed`. A
    /// retried `horch done` asks, so it does not report or record twice.
    pub fn done_recorded(&self, key: &str) -> Result<bool> {
        Ok(self
            .get(key)?
            .history
            .iter()
            .rev()
            .find(|h| matches!(h.event.as_str(), "done" | "spawned" | "resumed"))
            .is_some_and(|h| h.event == "done"))
    }

    /// Append 1 history entry `event` with `text` to the record `key`: the
    /// context-policy events (`compact-requested`, `context-warned`,
    /// `compacted`, `compact-failed`).
    pub fn record_event(&self, key: &str, event: &str, text: &str) -> Result<()> {
        self.append_event(key, event, text)
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

/// The selection of [`Ledger::live_for_role`].
fn pick_live<'a>(records: &'a [Record], role: &str, workspace: Option<&str>) -> Option<&'a Record> {
    records
        .iter()
        .filter(|r| r.role == role && r.execution_status().is_live())
        .filter(|r| match (workspace, r.workspace_id.as_deref()) {
            (Some(ws), Some(own)) => own == ws,
            _ => true,
        })
        .max_by(|a, b| a.updated_at.cmp(&b.updated_at))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::legacy::KIND_ORCHESTRATOR;
    use crate::execution::store::slug;

    fn ledger() -> (tempfile::TempDir, Ledger) {
        let tmp = tempfile::tempdir().unwrap();
        let l = Ledger::for_project(tmp.path(), "/Users/a/proj");
        (tmp, l)
    }

    #[test]
    fn phase_persists_and_resume_can_override_it() {
        let (_t, l) = ledger();
        l.add(NewRecord {
            record_id: "r1",
            agent: "codex",
            tier: "codex-sol",
            model: "sol",
            role: "worker",
            session_id: Some("s1"),
            task: "task",
            phase: Some(Phase::Research),
        })
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
        l.add(NewRecord {
            record_id: "r1",
            agent: "claude",
            tier: "sonnet",
            model: "sonnet",
            role: "sonnet-1",
            session_id: Some("s1"),
            task: "fix auth",
            phase: None,
        })
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
        l.add(NewRecord {
            record_id: "r1",
            agent: "claude",
            tier: "opus",
            model: "opus",
            role: "opus-1",
            session_id: None,
            task: "",
            phase: None,
        })
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
        l.add(NewRecord {
            record_id: "r1",
            agent: "codex",
            tier: "codex-sol",
            model: "gpt-5.6-sol",
            role: "codex-sol-1",
            session_id: Some(""),
            task: "",
            phase: None,
        })
        .unwrap();
        assert_eq!(l.get("r1").unwrap().session_id, None);
        assert!(!l.has_session("").unwrap());
    }

    #[test]
    fn set_session_then_has_session() {
        let (_t, l) = ledger();
        l.add(NewRecord {
            record_id: "r1",
            agent: "codex",
            tier: "codex-sol",
            model: "gpt-5.6-sol",
            role: "codex-sol-1",
            session_id: None,
            task: "",
            phase: None,
        })
        .unwrap();
        assert!(!l.has_session("s9").unwrap());
        l.set_session("r1", "s9").unwrap();
        assert!(l.has_session("s9").unwrap());
        assert_eq!(l.get("s9").unwrap().record_id, "r1");
    }

    #[test]
    fn note_and_done_build_history_and_flip_status() {
        let (_t, l) = ledger();
        l.add(NewRecord {
            record_id: "r1",
            agent: "claude",
            tier: "sonnet",
            model: "sonnet",
            role: "sonnet-1",
            session_id: Some("s1"),
            task: "t",
            phase: None,
        })
        .unwrap();
        l.note("r1", "halfway").unwrap();
        l.done("s1", "all finished").unwrap();

        let r = l.get("r1").unwrap();
        assert_eq!(r.status, STATUS_DONE);
        let events: Vec<&str> = r.history.iter().map(|h| h.event.as_str()).collect();
        assert_eq!(events, vec!["spawned", "note", "done"]);
    }

    /// W2: a retried `horch done` asks whether its summary is recorded. Only
    /// a `done` after the newest spawn or resume counts; a failed report
    /// and a resumed record do not.
    #[test]
    fn w2_done_recorded_only_after_the_newest_start() {
        let (_t, l) = ledger();
        l.add(NewRecord {
            record_id: "r1",
            agent: "claude",
            tier: "sonnet",
            model: "sonnet",
            role: "sonnet-1",
            session_id: Some("s1"),
            task: "t",
            phase: None,
        })
        .unwrap();
        assert!(!l.done_recorded("r1").unwrap());
        l.record_event("r1", EVENT_REPORT_FAILED, "relay refused")
            .unwrap();
        assert!(!l.done_recorded("r1").unwrap());
        l.done("r1", "all finished").unwrap();
        assert!(l.done_recorded("r1").unwrap());
        l.resume("r1", "sonnet-1", "more").unwrap();
        assert!(!l.done_recorded("r1").unwrap());
    }

    #[test]
    fn assign_targets_the_latest_working_record_for_the_role() {
        let (_t, l) = ledger();
        l.add(NewRecord {
            record_id: "old",
            agent: "claude",
            tier: "sonnet",
            model: "sonnet",
            role: "sonnet-1",
            session_id: Some("s1"),
            task: "first",
            phase: None,
        })
        .unwrap();
        l.done("old", "finished").unwrap();
        l.add(NewRecord {
            record_id: "live",
            agent: "claude",
            tier: "sonnet",
            model: "sonnet",
            role: "sonnet-1",
            session_id: Some("s2"),
            task: "second",
            phase: None,
        })
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
        l.add(NewRecord {
            record_id: "r1",
            agent: "claude",
            tier: "opus",
            model: "opus",
            role: "opus-1",
            session_id: Some("s1"),
            task: "original",
            phase: None,
        })
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
        l.add(NewRecord {
            record_id: "r1",
            agent: "claude",
            tier: "sonnet",
            model: "sonnet",
            role: "sonnet-1",
            session_id: Some("s1"),
            task: "first",
            phase: None,
        })
        .unwrap();
        for n in 1..=4 {
            l.note("r1", &format!("note {n}")).unwrap();
        }
        l.add(NewRecord {
            record_id: "r2",
            agent: "codex",
            tier: "codex-sol",
            model: "gpt-5.6-sol",
            role: "codex-sol-1",
            session_id: None,
            task: "",
            phase: None,
        })
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
        l.add(NewRecord {
            record_id: "w1",
            agent: "claude",
            tier: "sonnet",
            model: "sonnet",
            role: "sonnet-1",
            session_id: Some("s1"),
            task: "t",
            phase: None,
        })
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

    /// CTX-11 (review finding 13): the live record of a role, in 1 workspace
    /// when the caller names it. A record with no workspace matches any.
    #[test]
    fn ctx_11_live_for_role_filters_by_workspace() {
        let (_t, l) = ledger();
        let rec = |id: &str, ws: Option<&str>| Record {
            record_id: id.into(),
            agent: "claude".into(),
            role: "sonnet-1".into(),
            task: "t".into(),
            workspace_id: ws.map(Into::into),
            ..Record::default()
        };
        l.insert(rec("in-w1", Some("w1"))).unwrap();
        l.insert(rec("in-w2", Some("w2"))).unwrap();
        let stamp = |id: &str, at: &str| {
            l.store()
                .update_key(id, |r| r.updated_at = at.to_string())
                .unwrap()
        };
        stamp("in-w1", "2026-10-06T10:00:00Z");
        stamp("in-w2", "2026-10-06T11:00:00Z");

        let id = |ws| {
            l.live_for_role("sonnet-1", ws)
                .unwrap()
                .map(|r| r.record_id)
        };
        assert_eq!(id(Some("w1")).as_deref(), Some("in-w1"), "w2 is newer");
        assert_eq!(id(None).as_deref(), Some("in-w2"), "no filter: newest");
        assert_eq!(id(Some("w3")), None);
        assert_eq!(
            l.live_for_role("ghost", None).unwrap().map(|r| r.record_id),
            None
        );

        l.insert(rec("anywhere", None)).unwrap();
        stamp("anywhere", "2026-10-06T12:00:00Z");
        assert_eq!(id(Some("w1")).as_deref(), Some("anywhere"));
        assert_eq!(id(Some("w3")).as_deref(), Some("anywhere"));

        l.done("anywhere", "finished").unwrap();
        l.assign_in("sonnet-1", Some("w1"), "next").unwrap();
        assert_eq!(l.get("in-w1").unwrap().task, "next");
        assert_eq!(l.get("in-w2").unwrap().task, "t");

        l.record_event("in-w2", "compact-requested", "tokens 1 threshold 2")
            .unwrap();
        let last = l.get("in-w2").unwrap().history.pop().unwrap();
        assert_eq!(
            (last.event.as_str(), last.text.as_str()),
            ("compact-requested", "tokens 1 threshold 2")
        );
    }
}
