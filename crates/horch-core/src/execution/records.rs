use std::path::Path;

use crate::execution::legacy::{HistoryEntry, Record, STATUS_DONE, STATUS_WORKING};
use crate::execution::store::ExecutionStore;
use crate::roster::Phase;
use anyhow::{bail, Result};

pub(crate) const IDLE_TASK: &str = "(idle - awaiting assignment)";

pub const ORCHESTRATING_TASK: &str = "(orchestrating)";

pub const EVENT_REPORT_FAILED: &str = "report-failed";

pub const EVENT_REPORT_QUEUED: &str = "report-queued";

#[derive(Debug, Clone)]
pub struct SessionLedger {
    store: ExecutionStore,
    project: Option<String>,
    workspace_id: Option<String>,
}

fn now() -> String {
    crate::clock::now_stamp()
}

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

impl SessionLedger {
    pub fn open_in(ctx: &crate::runtime::RuntimeContext) -> Result<Self> {
        let root = ctx.paths.project()?;
        let project = root.to_string_lossy().into_owned();
        Ok(Self {
            store: ExecutionStore::open(&ctx.paths, &root),
            project: Some(project),
            workspace_id: ctx.herdr.workspace.as_ref().map(|w| w.to_string()),
        })
    }

    pub fn with_task_ledger(mut self, db: &Path, project: &Path) -> Self {
        self.store = self.store.with_task_ledger(db, project);
        self
    }

    pub fn with_writer(
        mut self,
        writer: std::sync::Arc<crate::taskledger::store::LedgerWriter>,
    ) -> Self {
        self.store = self.store.with_writer(writer);
        self
    }

    pub fn with_workspace(mut self, workspace_id: Option<String>) -> Self {
        self.workspace_id = workspace_id;
        self
    }

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

    pub fn store(&self) -> &ExecutionStore {
        &self.store
    }

    pub fn read(&self) -> Result<Vec<Record>> {
        self.store.read()
    }

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

    pub fn supersede_orchestrators(&self, keep: impl Fn(&str) -> bool) -> Result<usize> {
        self.supersede_orchestrators_by_record(|r| r.workspace_id.as_deref().is_some_and(&keep))
    }

    pub fn supersede_orchestrators_by_record(
        &self,
        keep: impl Fn(&Record) -> bool,
    ) -> Result<usize> {
        let at = now();
        self.store.update(|records| {
            let mut n = 0;
            for r in records
                .iter_mut()
                .filter(|r| r.is_orchestrator() && r.execution_status().is_live() && !keep(r))
            {
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

    pub fn set_session(&self, key: &str, session_id: &str) -> Result<()> {
        let at = now();
        self.store.update_key(key, |r| {
            r.session_id = Some(session_id.to_string());
            r.updated_at = at.clone();
        })
    }

    pub fn set_effort(&self, key: &str, effort: Option<&str>) -> Result<()> {
        self.store
            .update_key(key, |r| r.effort = effort.map(str::to_owned))
    }

    pub fn has_session(&self, session_id: &str) -> Result<bool> {
        Ok(self
            .read()?
            .iter()
            .any(|r| r.session_id.as_deref() == Some(session_id)))
    }

    pub fn resume(&self, key: &str, role: &str, task: &str) -> Result<()> {
        self.resume_with_phase(key, role, task, None)
    }

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

    pub fn live_for_role(&self, role: &str, workspace: Option<&str>) -> Result<Option<Record>> {
        Ok(pick_live(&self.read()?, role, workspace).cloned())
    }

    pub fn assign(&self, role: &str, task: &str) -> Result<()> {
        self.assign_in(role, None, task)
    }

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

    pub fn done_recorded(&self, key: &str) -> Result<bool> {
        Ok(self
            .get(key)?
            .history
            .iter()
            .rev()
            .find(|h| matches!(h.event.as_str(), "done" | "spawned" | "resumed"))
            .is_some_and(|h| h.event == "done"))
    }

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

    pub fn get(&self, key: &str) -> Result<Record> {
        self.store.get(key)
    }

    pub fn render(&self) -> Result<String> {
        Ok(ExecutionStore::render_text(&self.read()?))
    }
}

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

    fn ledger() -> (tempfile::TempDir, SessionLedger) {
        let tmp = tempfile::tempdir().unwrap();
        let l = SessionLedger::for_project(tmp.path(), "/Users/a/proj").with_writer(
            std::sync::Arc::new(crate::taskledger::store::LedgerWriter::fleetd()),
        );
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

    #[test]
    fn slug_matches_tr_semantics() {
        assert_eq!(slug("/Users/a/proj"), "-Users-a-proj");
        assert_eq!(slug("/tmp/a.b-c_d"), "-tmp-a-b-c-d");
        assert_eq!(slug(r"C:\Users\a b\p"), "C--Users-a-b-p");
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
    fn operations_on_unknown_keys_name_the_session_records_file() {
        let (_t, l) = ledger();
        for err in [
            l.note("nope", "x").unwrap_err(),
            l.done("nope", "x").unwrap_err(),
            l.set_session("nope", "s").unwrap_err(),
            l.resume("nope", "r", "").unwrap_err(),
        ] {
            let msg = err.to_string();
            assert!(msg.contains("no record matches 'nope'"), "{msg}");
            assert!(msg.contains("ledger.sqlite3"), "{msg}");
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
        assert!(!out.contains("note 1"));
        for n in 2..=4 {
            assert!(
                out.contains(&format!("note {n}")),
                "missing note {n}:\n{out}"
            );
        }
    }

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
