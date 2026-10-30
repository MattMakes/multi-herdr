//! `ExecutionService::spawn` pins: the brief stage failure, the role-taken
//! refusal and each arm of the record upsert (fresh push, resumed update,
//! duplicate record, vanished record).

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use horch_core::execution::legacy::LedgerRecordV1;
use horch_core::execution::plan::{
    needs_gate, plan_launch, GateInputs, MintedIds, PlanError, PlanInputs,
};
use horch_core::execution::service::{ExecutionService, SpawnError};
use horch_core::execution::store::ExecutionStore;
use horch_core::execution::{
    ExecutionPlan, ExecutionStatus, LaunchStage, SpawnRequest, TilingMode,
};
use horch_core::ids::{ExecutionId, SessionId};
use horch_core::messaging::mailbox::Mailbox;
use horch_core::roster::{Phase, Roster};
use horch_core::routing::policy::{BalanceMode, Policy};
use horch_core::routing::quota::{QuotaFile, QuotaView};
use horch_core::runtime::{MapEnv, RuntimeContext};
use horch_core::skills::{harness_support, SkillCatalog, SkillResolution};
use horch_core::workspace::client::WorkspaceClient;
use horch_core::workspace::testing::FakeWorkspace;

const NOW: &str = "2026-09-28T18:00:00Z";

struct World {
    _tmp: tempfile::TempDir,
    ctx: RuntimeContext,
    store: ExecutionStore,
    mailbox: Mailbox,
    fake: FakeWorkspace,
}

/// A temp state root, project and mailbox, and a fake workspace `w1` whose
/// root pane is `p1`.
fn world() -> World {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let env = MapEnv::new(&project)
        .with(
            "HORCH_STATE_DIR",
            &tmp.path().join("state").to_string_lossy(),
        )
        .with("HORCH_PROJECT_DIR", &project.to_string_lossy())
        .with("TMPDIR", &tmp.path().join("tmp").to_string_lossy())
        .with_exe("/opt/horch/bin/horch");
    let ctx = RuntimeContext::from_env(&env).unwrap();
    let store = ExecutionStore::open(&ctx.paths, &project);
    let mailbox = Mailbox::in_context(&ctx, "w1");
    let fake = FakeWorkspace::new();
    fake.workspace_create("w1", None, false).unwrap();
    World {
        _tmp: tmp,
        ctx,
        store,
        mailbox,
        fake,
    }
}

fn plan(req: &SpawnRequest, exec: &str, existing: Option<&LedgerRecordV1>) -> ExecutionPlan {
    let mut roster = Roster::builtin().unwrap();
    roster
        .overlay(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../teammates"))
        .unwrap();
    let skills = SkillResolution::read(
        SkillCatalog::bundled().unwrap(),
        None,
        None,
        harness_support,
    )
    .unwrap();
    let ids = MintedIds {
        execution: ExecutionId::new(exec).unwrap(),
        session: SessionId::new("sess-1").unwrap(),
    };
    let view = QuotaView::new(
        QuotaFile::read(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/telemetry/quota/all-ok.json"),
        )
        .unwrap(),
        horch_core::clock::parse(NOW).unwrap(),
        Policy::default(),
        true,
    );
    let inputs = PlanInputs {
        roster: &roster,
        skills: &skills,
        gate: needs_gate(req, &roster).then_some(GateInputs {
            view: &view,
            balance: BalanceMode::Auto,
        }),
        existing,
        now: horch_core::clock::parse(NOW).unwrap(),
        ids: &ids,
        project: Path::new("/work/alpha"),
    };
    plan_launch(req, &inputs).unwrap()
}

fn fresh(exec: &str) -> ExecutionPlan {
    let mut req = SpawnRequest::worker(Some("sonnet".parse().unwrap()), "build");
    req.from_pane = Some("p1".into());
    plan(&req, exec, None)
}

/// A finished record a resume can reopen.
fn finished() -> LedgerRecordV1 {
    LedgerRecordV1 {
        record_id: "rec-1".into(),
        session_id: Some("s-old".into()),
        agent: "claude".into(),
        tier: "sonnet".into(),
        model: "sonnet".into(),
        effort: Some("high".into()),
        phase: Some(Phase::Research),
        role: "old-1".into(),
        status: "done".into(),
        task: "earlier work".into(),
        created_at: "2026-09-28T17:00:00Z".into(),
        updated_at: "2026-09-28T17:30:00Z".into(),
        ..LedgerRecordV1::default()
    }
}

fn resume_plan(record: &LedgerRecordV1) -> ExecutionPlan {
    let mut req = SpawnRequest::worker(None, "more");
    req.resume = Some(record.record_id.clone());
    req.from_pane = Some("p1".into());
    plan(&req, "unused", Some(record))
}

impl World {
    fn spawn(&self, plan: ExecutionPlan, role: Option<&str>) -> Result<String, SpawnError> {
        let tile = |_: &str, _: TilingMode| {};
        let service = ExecutionService {
            ctx: &self.ctx,
            store: &self.store,
            workspace: &self.fake,
            mailbox: &self.mailbox,
            tile: &tile,
            ledger_worker: None,
        };
        service.spawn(plan, role).map(|o| o.pane)
    }

    fn pane_calls(&self) -> Vec<String> {
        self.fake
            .calls()
            .into_iter()
            .map(|c| c.method.to_string())
            .filter(|m| m != "workspace_create")
            .collect()
    }
}

#[test]
fn brief_failure_records_launch_failed_before_any_pane() {
    let w = world();
    // Unwritable mailbox: the role is explicit, so only the brief write fails.
    std::fs::create_dir_all(w.mailbox.dir()).unwrap();
    std::fs::set_permissions(w.mailbox.dir(), std::fs::Permissions::from_mode(0o500)).unwrap();
    let out = w.spawn(fresh("exec-1"), Some("sonnet-9"));
    std::fs::set_permissions(w.mailbox.dir(), std::fs::Permissions::from_mode(0o700)).unwrap();
    match out.unwrap_err() {
        SpawnError::LaunchFailed { stage, id, .. } => {
            assert_eq!(stage, LaunchStage::Brief);
            assert_eq!(id.as_str(), "exec-1");
        }
        other => panic!("{other}"),
    }
    assert!(w.pane_calls().is_empty(), "no pane before the brief");
    let records = w.store.read().unwrap();
    assert_eq!(records.len(), 1);
    assert!(matches!(
        records[0].execution_status(),
        ExecutionStatus::LaunchFailed {
            stage: LaunchStage::Brief,
            ..
        }
    ));
    assert!(w.store.live().unwrap().is_empty());
}

#[test]
fn a_role_held_by_a_live_worker_is_refused() {
    let w = world();
    w.spawn(fresh("exec-1"), Some("dup-1")).unwrap();
    match w.spawn(fresh("exec-2"), Some("dup-1")).unwrap_err() {
        SpawnError::Plan(PlanError::RoleTaken(role)) => assert_eq!(role, "dup-1"),
        other => panic!("{other}"),
    }
    let records = w.store.read().unwrap();
    assert_eq!(records.len(), 1, "the refused spawn wrote no record");
    assert_eq!(records[0].record_id, "exec-1");
}

#[test]
fn a_fresh_spawn_pushes_its_record_after_the_existing_ones() {
    let w = world();
    w.spawn(fresh("exec-1"), None).unwrap();
    w.spawn(fresh("exec-2"), None).unwrap();
    let records = w.store.read().unwrap();
    let ids: Vec<_> = records.iter().map(|r| r.record_id.as_str()).collect();
    assert_eq!(ids, ["exec-1", "exec-2"]);
    let roles: Vec<_> = records.iter().map(|r| r.role.as_str()).collect();
    assert_eq!(roles, ["sonnet-1", "sonnet-2"]);
}

#[test]
fn a_fresh_plan_for_an_existing_record_id_is_refused() {
    let w = world();
    w.spawn(fresh("exec-1"), Some("one-1")).unwrap();
    let err = w.spawn(fresh("exec-1"), Some("two-1")).unwrap_err();
    assert!(
        matches!(&err, SpawnError::Store(e) if format!("{e:#}").contains("record exec-1 exists already")),
        "{err}"
    );
    assert_eq!(w.store.read().unwrap().len(), 1);
}

#[test]
fn a_resume_updates_its_record_in_place_and_keeps_its_history() {
    let w = world();
    let mut old = finished();
    old.history = vec![horch_core::execution::legacy::HistoryEntry {
        at: "2026-09-28T17:10:00Z".into(),
        event: "note".into(),
        text: "written since the plan read it".into(),
    }];
    let planned = resume_plan(&old);
    // History the worker wrote after the plan read the record.
    let mut stored = old.clone();
    stored
        .history
        .push(horch_core::execution::legacy::HistoryEntry {
            at: "2026-09-28T17:20:00Z".into(),
            event: "note".into(),
            text: "late".into(),
        });
    w.store
        .update(|rs| {
            rs.push(stored);
            Ok(())
        })
        .unwrap();
    w.spawn(planned, Some("again-1")).unwrap();
    let records = w.store.read().unwrap();
    assert_eq!(records.len(), 1, "updated in place, not pushed");
    let r = &records[0];
    assert_eq!(r.record_id, "rec-1");
    assert_eq!(r.role, "again-1");
    assert_eq!(r.execution_status(), ExecutionStatus::Starting);
    let events: Vec<_> = r.history.iter().map(|h| h.text.as_str()).collect();
    assert_eq!(events, ["written since the plan read it", "late", "more"]);
}

#[test]
fn a_resume_whose_record_vanished_is_refused() {
    let w = world();
    let planned = resume_plan(&finished());
    let err = w.spawn(planned, Some("again-1")).unwrap_err();
    assert!(
        matches!(&err, SpawnError::Store(e) if format!("{e:#}").contains("record rec-1 vanished before the resume")),
        "{err}"
    );
    assert!(w.store.read().unwrap().is_empty());
    assert!(w.pane_calls().is_empty());
}
