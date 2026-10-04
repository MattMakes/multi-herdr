//! ARC-15, ARC-16, ARC-18: spawn planning is pure, the service applies a
//! plan in a fixed order and leaves no phantom live record, and the worker
//! runs in a fixed order and records its agent's exit.

use std::cell::RefCell;
use std::path::Path;

use anyhow::{bail, Result};
use horch_core::execution::legacy::{LedgerRecordV1, KIND_ORCHESTRATOR};
use horch_core::execution::lifecycle::{run_worker, WorkerSteps};
use horch_core::execution::plan::{
    finish_plan, needs_gate, plan_launch, GateInputs, MintedIds, PlanError, PlanInputs,
};
use horch_core::execution::service::{ExecutionService, SpawnError};
use horch_core::execution::store::{from_execution, to_execution, ExecutionStore, ABANDONED_AFTER};
use horch_core::execution::{
    ExecutionKind, ExecutionPlan, ExecutionStatus, FailureKind, LaunchStage, SessionMode,
    SpawnRequest, TilingMode,
};
use horch_core::ids::{ExecutionId, RoleName, SessionId, WorkspaceId};
use horch_core::messaging::brief::Brief;
use horch_core::messaging::brief::SCHEMA;
use horch_core::messaging::mailbox::Mailbox;
use horch_core::roster::{Phase, Roster};
use horch_core::routing::decision::{GateFlags, RoutingMode};
use horch_core::routing::policy::{BalanceMode, Policy};
use horch_core::routing::quota::{QuotaFile, QuotaView};
use horch_core::runtime::{MapEnv, RuntimeContext};
use horch_core::skills::SkillCatalog;
use horch_core::workspace::client::WorkspaceClient;
use horch_core::workspace::model::{Direction, NewWorkspace, Pane};
use horch_core::workspace::testing::FakeWorkspace;

const NOW: &str = "2026-09-28T18:00:00Z";

fn core_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// The repo roster without reading `HOME` or `HORCH_TEAMMATES_DIR`.
fn roster() -> Roster {
    let mut r = Roster::builtin().unwrap();
    r.overlay(&core_dir().join("../../teammates")).unwrap();
    r
}

fn view(fixture: &str) -> QuotaView {
    let path = core_dir()
        .join("tests/fixtures/telemetry/quota")
        .join(format!("{fixture}.json"));
    let now = horch_core::clock::parse(NOW).unwrap();
    QuotaView::new(
        QuotaFile::read(&path).unwrap(),
        now,
        Policy::default(),
        true,
    )
}

fn ids() -> MintedIds {
    MintedIds {
        execution: ExecutionId::new("exec-1").unwrap(),
        session: SessionId::new("sess-1").unwrap(),
    }
}

fn req(teammate: &str, task: &str) -> SpawnRequest {
    SpawnRequest::worker(Some(teammate.parse().unwrap()), task)
}

fn resume(key: &str, task: &str) -> SpawnRequest {
    let mut r = SpawnRequest::worker(None, task);
    r.resume = Some(key.to_string());
    r
}

/// A finished worker record, as a ledger holds it.
fn record(tier: &str, agent: &str, model: &str) -> LedgerRecordV1 {
    LedgerRecordV1 {
        record_id: "rec-1".into(),
        session_id: Some("s-old".into()),
        agent: agent.into(),
        tier: tier.into(),
        model: model.into(),
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

/// Plan `req` against a quota fixture, reading the gate inputs only when the
/// shell would.
fn plan(
    req: &SpawnRequest,
    fixture: &str,
    existing: Option<&LedgerRecordV1>,
) -> Result<ExecutionPlan, PlanError> {
    let roster = roster();
    let catalog = SkillCatalog::bundled().unwrap();
    let view = view(fixture);
    let ids = ids();
    let inputs = PlanInputs {
        roster: &roster,
        catalog: &catalog,
        gate: needs_gate(req, &roster).then_some(GateInputs {
            view: &view,
            balance: BalanceMode::Auto,
        }),
        existing,
        now: horch_core::clock::parse(NOW).unwrap(),
        ids: &ids,
        project: Path::new("/work/alpha"),
    };
    plan_launch(req, &inputs)
}

// ─── ARC-15 ─────────────────────────────────────────────────────────────────

#[test]
fn arc_15_plan_deterministic() {
    let cases = [
        (req("sonnet", "build ai_docs/plans/p.md"), "all-ok", None),
        (req("researcher", "look"), "claude-exhausted-codex-ok", None),
        (req("codex-sol", ""), "claude-tight-codex-ok", None),
        (
            resume("rec-1", "more"),
            "all-exhausted",
            Some(record("sonnet", "claude", "sonnet")),
        ),
    ];
    for (req, fixture, existing) in &cases {
        let a = plan(req, fixture, existing.as_ref()).unwrap();
        let b = plan(req, fixture, existing.as_ref()).unwrap();
        assert_eq!(a, b, "{fixture}");
        let ws = WorkspaceId::new("w1").unwrap();
        let role = RoleName::new("r-1").unwrap();
        assert_eq!(
            finish_plan(a, role.clone(), &ws),
            finish_plan(b, role, &ws),
            "{fixture}"
        );
    }
}

/// One row: what a request plans to, in short.
#[derive(Debug, PartialEq)]
struct Row {
    decision: String,
    teammate: String,
    harness: String,
    effort: Option<String>,
    session: String,
    mode: Option<RoutingMode>,
    skills: Vec<String>,
}

fn row(result: Result<ExecutionPlan, PlanError>) -> Row {
    let p = match result {
        Ok(p) => p,
        Err(e) => {
            let kind = match &e {
                PlanError::Refused { .. } => "refused",
                PlanError::NotResumable { .. } => "not-resumable",
                PlanError::Unspawnable(_) => "unspawnable",
                PlanError::BadEffort(_) => "bad-effort",
                PlanError::SkillUnsupported(_) => "skill-unsupported",
                PlanError::UnknownTeammate(_) => "unknown-teammate",
                PlanError::NothingToSpawn => "nothing",
                other => panic!("unexpected {other}"),
            };
            return Row {
                decision: kind.into(),
                teammate: String::new(),
                harness: String::new(),
                effort: None,
                session: String::new(),
                mode: None,
                skills: Vec::new(),
            };
        }
    };
    let routing = p.execution.routing.as_ref().unwrap();
    let decision = match (p.resumed, &p.execution.via) {
        (true, _) => "resume",
        (false, Some(_)) => "substitute",
        (false, None) => "spawn",
    };
    Row {
        decision: decision.into(),
        teammate: format!("{}/{}", p.execution.teammate, routing.resolved),
        harness: p.launch.teammate.agent.as_str().into(),
        effort: p.launch.teammate.effort.clone(),
        session: match &p.launch.session {
            SessionMode::Fresh(Some(id)) => format!("fresh:{id}"),
            SessionMode::Fresh(None) => "fresh".into(),
            SessionMode::Resume(id) => format!("resume:{id}"),
        },
        mode: Some(routing.mode),
        skills: p
            .skills
            .activated_ids()
            .into_iter()
            .map(str::to_owned)
            .collect(),
    }
}

#[allow(clippy::too_many_arguments)]
fn ok(
    decision: &str,
    teammate: &str,
    harness: &str,
    effort: Option<&str>,
    session: &str,
    mode: RoutingMode,
    skills: &[&str],
) -> Row {
    Row {
        decision: decision.into(),
        teammate: teammate.into(),
        harness: harness.into(),
        effort: effort.map(str::to_owned),
        session: session.into(),
        mode: Some(mode),
        skills: skills.iter().map(|s| (*s).to_owned()).collect(),
    }
}

fn err(kind: &str) -> Row {
    Row {
        decision: kind.into(),
        teammate: String::new(),
        harness: String::new(),
        effort: None,
        session: String::new(),
        mode: None,
        skills: Vec::new(),
    }
}

#[test]
fn arc_15_plan_table() {
    let research = ["brainstorm", "handoff", "research-codebase", "trace"];
    let implementation = ["check", "debug", "execute", "handoff", "tdd"];
    let sonnet = || record("sonnet", "claude", "sonnet");
    let mut working = sonnet();
    working.status = "working".into();
    let mut orchestrator = sonnet();
    orchestrator.kind = KIND_ORCHESTRATOR.into();
    let mut no_session = sonnet();
    no_session.session_id = None;
    let mut reserved = sonnet();
    reserved.model = "claude-fable-5".into();
    let mut substituted = record("researcher", "codex", "gpt-5.6-sol");
    substituted.via = Some("codex-sol".into());
    substituted.effort = None;
    let effort = |mut r: SpawnRequest, e: &str| {
        r.effort = Some(e.into());
        r
    };
    let phase = |mut r: SpawnRequest, p: Phase| {
        r.phase = Some(p);
        r
    };
    let flags = |mut r: SpawnRequest, exact: bool, force: bool| {
        r.flags = GateFlags { exact, force };
        r
    };
    let pinned = |mut r: SpawnRequest| {
        r.pinned = true;
        r
    };

    let table: Vec<(&str, SpawnRequest, &str, Option<LedgerRecordV1>, Row)> = vec![
        (
            "fresh claude, caller-minted session",
            req("sonnet", "x"),
            "all-ok",
            None,
            ok(
                "spawn",
                "sonnet/sonnet",
                "claude",
                Some("medium"),
                "fresh:sess-1",
                RoutingMode::Auto,
                &implementation,
            ),
        ),
        (
            "fresh codex, discovered session",
            req("codex-sol", "x"),
            "all-ok",
            None,
            ok(
                "spawn",
                "codex-sol/codex-sol",
                "codex",
                Some("medium"),
                "fresh",
                RoutingMode::Auto,
                &implementation,
            ),
        ),
        (
            "substitution onto the fallback",
            req("researcher", "x"),
            "claude-exhausted-codex-ok",
            None,
            ok(
                "substitute",
                "researcher/codex-sol",
                "codex",
                Some("medium"),
                "fresh",
                RoutingMode::Auto,
                &research,
            ),
        ),
        (
            "--exact never substitutes, so it refuses",
            flags(req("researcher", "x"), true, false),
            "claude-exhausted-codex-ok",
            None,
            err("refused"),
        ),
        (
            "refusal",
            req("opus", "x"),
            "all-exhausted",
            None,
            err("refused"),
        ),
        (
            "--force spawns anyway",
            flags(req("opus", "x"), false, true),
            "all-exhausted",
            None,
            ok(
                "spawn",
                "opus/opus",
                "claude",
                Some("medium"),
                "fresh:sess-1",
                RoutingMode::Force,
                &implementation,
            ),
        ),
        (
            "a pinned candidate skips the gate",
            pinned(req("opus", "x")),
            "all-exhausted",
            None,
            ok(
                "spawn",
                "opus/opus",
                "claude",
                Some("medium"),
                "fresh:sess-1",
                RoutingMode::Pinned,
                &implementation,
            ),
        ),
        (
            "--phase selects the skills",
            phase(req("sonnet", "x"), Phase::Research),
            "all-ok",
            None,
            ok(
                "spawn",
                "sonnet/sonnet",
                "claude",
                Some("medium"),
                "fresh:sess-1",
                RoutingMode::Auto,
                &research,
            ),
        ),
        (
            "--effort overrides",
            effort(req("sonnet", "x"), "max"),
            "all-ok",
            None,
            ok(
                "spawn",
                "sonnet/sonnet",
                "claude",
                Some("max"),
                "fresh:sess-1",
                RoutingMode::Auto,
                &implementation,
            ),
        ),
        (
            "a bad effort",
            effort(req("codex-sol", "x"), "minimal"),
            "all-ok",
            None,
            err("bad-effort"),
        ),
        (
            "an unknown teammate",
            req("haiku", "x"),
            "all-ok",
            None,
            err("unknown-teammate"),
        ),
        (
            "a headless-only teammate",
            req("judge", "x"),
            "all-ok",
            None,
            err("unspawnable"),
        ),
        (
            "resume keeps the record's effort and phase, ungated",
            resume("rec-1", "more"),
            "all-exhausted",
            Some(sonnet()),
            ok(
                "resume",
                "sonnet/sonnet",
                "claude",
                Some("high"),
                "resume:s-old",
                RoutingMode::Resume,
                &research,
            ),
        ),
        (
            "a substituted session resumes on its fallback",
            resume("rec-1", ""),
            "all-ok",
            Some(substituted),
            ok(
                "resume",
                "researcher/codex-sol",
                "codex",
                Some("medium"),
                "resume:s-old",
                RoutingMode::Resume,
                &research,
            ),
        ),
        (
            "resume of a working record",
            resume("rec-1", ""),
            "all-ok",
            Some(working),
            err("not-resumable"),
        ),
        (
            "resume of an orchestrator",
            resume("rec-1", ""),
            "all-ok",
            Some(orchestrator),
            err("not-resumable"),
        ),
        (
            "resume without a session id",
            resume("rec-1", ""),
            "all-ok",
            Some(no_session),
            err("not-resumable"),
        ),
        (
            "resume of a missing record",
            resume("rec-9", ""),
            "all-ok",
            None,
            err("not-resumable"),
        ),
        (
            "resume onto a reserved tier",
            resume("rec-1", ""),
            "all-ok",
            Some(reserved),
            err("unspawnable"),
        ),
        (
            "nothing to spawn",
            SpawnRequest::worker(None, "x"),
            "all-ok",
            None,
            err("nothing"),
        ),
    ];
    let mut failures = Vec::new();
    for (name, req, fixture, existing, want) in table {
        let got = row(plan(&req, fixture, existing.as_ref()));
        if got != want {
            failures.push(format!("{name}:\n  got  {got:?}\n  want {want:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The plan's record: Planned, idle placeholder, the spawn history and the
/// gate's line; `finish_plan` names the role.
#[test]
fn arc_15_plan_record_and_finish() {
    let p = plan(&req("researcher", ""), "claude-exhausted-codex-ok", None).unwrap();
    assert_eq!(
        p.gate_line.as_deref(),
        Some("SUBSTITUTED: researcher runs on codex-sol. Reason: claude 7d 100%, resets 2026-10-02T14:00Z.")
    );
    let e = &p.execution;
    assert_eq!(e.status, ExecutionStatus::Planned);
    assert_eq!(e.task.text, horch_core::execution::plan::IDLE_TASK);
    assert_eq!(e.via.as_deref(), Some("codex-sol"));
    assert_eq!(e.created_at, NOW);
    let done = finish_plan(
        p,
        RoleName::new("researcher-1").unwrap(),
        &WorkspaceId::new("w1").unwrap(),
    );
    assert_eq!(
        done.execution.history[0].text,
        "spawned idle as researcher-1"
    );
    assert_eq!(done.worker.unwrap().as_str(), "w1:researcher-1");
    let r = from_execution(&done.execution);
    assert_eq!(r.status, "working");
    assert_eq!(r.workspace_id.as_deref(), Some("w1"));
    assert_eq!(r.kind, "worker");
}

/// A skill catalog the teammate cannot load fails before any side effect.
#[test]
fn invalid_selection_is_refused_before_spawn_side_effects() {
    let mut roster = roster();
    let mut t = roster.require("opus").unwrap().clone();
    t.skills = vec!["missing-bundle".into()];
    roster.insert_for_test(t);
    let catalog = SkillCatalog::bundled().unwrap();
    let view = view("all-ok");
    let ids = ids();
    let inputs = PlanInputs {
        roster: &roster,
        catalog: &catalog,
        gate: Some(GateInputs {
            view: &view,
            balance: BalanceMode::Auto,
        }),
        existing: None,
        now: horch_core::clock::parse(NOW).unwrap(),
        ids: &ids,
        project: Path::new("/p"),
    };
    let e = plan_launch(&req("opus", "x"), &inputs).unwrap_err();
    assert!(matches!(e, PlanError::SkillUnsupported(_)), "{e}");
    assert!(e.to_string().contains("missing-bundle"), "{e}");
}

// ─── ARC-17 (conversion) ────────────────────────────────────────────────────

/// Every record of every legacy ledger oracle converts to an `Execution` and
/// back to the same bytes.
#[test]
fn arc_17_execution_conversion_lossless() {
    let dir = core_dir().join("tests/oracles/ledgers");
    for name in ["bash-era", "orchestrator", "pr14-substituted", "pre-effort"] {
        let raw = std::fs::read_to_string(dir.join(format!("{name}.saved.json"))).unwrap();
        let records: Vec<LedgerRecordV1> = serde_json::from_str(&raw).unwrap();
        let back: Vec<LedgerRecordV1> = records
            .iter()
            .map(|r| from_execution(&to_execution(r).unwrap()))
            .collect();
        assert_eq!(
            ExecutionStore::render_json(&back).unwrap(),
            raw,
            "{name} round-trips"
        );
    }
    // The new kinds and states round-trip through their legacy keys.
    let mut e = to_execution(&record("sonnet", "claude", "sonnet")).unwrap();
    for kind in [
        ExecutionKind::Candidate {
            experiment: "e1".parse().unwrap(),
            round: "r1".parse().unwrap(),
            label: "A".into(),
        },
        ExecutionKind::Judge {
            round: "r1".parse().unwrap(),
            attempt: 2,
        },
    ] {
        e.kind = kind;
        e.status = ExecutionStatus::Failed {
            failure: FailureKind::AgentExited { code: Some(3) },
        };
        e.typed_status = true;
        e.exit_code = Some(3);
        let r = from_execution(&e);
        assert_eq!(r.kind, "worker");
        assert_eq!(r.status, "done");
        assert_eq!(to_execution(&r).unwrap(), e);
    }
    assert_eq!(e.idempotency_key(), None);
}

// ─── ARC-16 ─────────────────────────────────────────────────────────────────

struct World {
    _tmp: tempfile::TempDir,
    ctx: RuntimeContext,
    store: ExecutionStore,
    mailbox: Mailbox,
    fake: FakeWorkspace,
}

/// A temp state root, project and mailbox, and a fake workspace `w1` whose
/// root pane is `p1`.
fn world(faults: Option<&str>) -> World {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let mut env = MapEnv::new(&project)
        .with(
            "HORCH_STATE_DIR",
            &tmp.path().join("state").to_string_lossy(),
        )
        .with("HORCH_PROJECT_DIR", &project.to_string_lossy())
        .with("TMPDIR", &tmp.path().join("tmp").to_string_lossy())
        .with_exe("/opt/horch/bin/horch");
    if let Some(f) = faults {
        env = env.with("HORCH_FAULT", f);
    }
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

impl World {
    fn spawn(&self, mut req: SpawnRequest) -> (Result<String, SpawnError>, Vec<String>) {
        req.from_pane = Some("p1".into());
        let p = plan(&req, "all-ok", None).unwrap();
        let tiled = RefCell::new(Vec::new());
        let tile = |pane: &str, _: TilingMode| tiled.borrow_mut().push(pane.to_string());
        let service = ExecutionService {
            ctx: &self.ctx,
            store: &self.store,
            workspace: &self.fake,
            mailbox: &self.mailbox,
            tile: &tile,
        };
        let out = service.spawn(p, None).map(|o| o.pane);
        (out, tiled.into_inner())
    }

    fn calls(&self) -> Vec<String> {
        self.fake
            .calls()
            .into_iter()
            .map(|c| c.method.to_string())
            .filter(|m| m != "workspace_create")
            .collect()
    }

    fn only_record(&self) -> LedgerRecordV1 {
        let records = self.store.read().unwrap();
        assert_eq!(records.len(), 1, "{records:?}");
        records.into_iter().next().unwrap()
    }
}

#[test]
fn arc_16_service_apply_order() {
    let w = world(None);
    let (out, tiled) = w.spawn(req("sonnet", "build"));
    let pane = out.unwrap();
    assert_eq!(pane, "p2");
    assert_eq!(w.calls(), ["pane_split", "pane_run"]);
    assert_eq!(tiled, ["p2"]);
    let r = w.only_record();
    assert_eq!(r.execution_status(), ExecutionStatus::Starting);
    assert_eq!(r.status, "working");
    assert_eq!(r.pane_id.as_deref(), Some("p2"));
    assert_eq!(r.role, "sonnet-1");
    assert!(!r.skills.is_empty(), "SKL-04: the skill refs are recorded");
    let brief = w.mailbox.read_brief("sonnet-1").unwrap();
    assert_eq!(brief.record_id, r.record_id);
    assert_eq!(brief.task, "build");
    let run = &w.fake.calls()[2];
    assert!(
        run.args[1].contains("worker") && run.args[1].contains("sonnet-1"),
        "{run:?}"
    );
}

#[test]
fn arc_16_split_failure_launch_failed() {
    for (faults, inject) in [(None, true), (Some("fail-pane-split"), false)] {
        let w = world(faults);
        if inject {
            w.fake.fail_next("pane_split", "no such pane");
        }
        let (out, tiled) = w.spawn(req("sonnet", "build"));
        match out.unwrap_err() {
            SpawnError::LaunchFailed { stage, id, .. } => {
                assert_eq!(stage, LaunchStage::Split);
                assert_eq!(id.as_str(), "exec-1");
            }
            other => panic!("{other}"),
        }
        assert!(tiled.is_empty());
        let r = w.only_record();
        assert!(
            matches!(
                r.execution_status(),
                ExecutionStatus::LaunchFailed {
                    stage: LaunchStage::Split,
                    ..
                }
            ),
            "{:?}",
            r.state
        );
        assert_eq!(r.status, "done", "an old reader sees it finished");
        assert!(r.finished_at.is_some());
        assert!(w.store.live().unwrap().is_empty(), "no phantom live record");
        assert!(!w.mailbox.role_taken("sonnet-1"), "the role is free again");
        assert!(!w.calls().contains(&"pane_run".to_string()));
    }
}

#[test]
fn arc_16_run_failure_closes_pane() {
    for (faults, inject) in [(None, true), (Some("fail-pane-run"), false)] {
        let w = world(faults);
        if inject {
            w.fake.fail_next("pane_run", "pane closed");
        }
        let (out, tiled) = w.spawn(req("sonnet", ""));
        assert!(
            matches!(
                out.unwrap_err(),
                SpawnError::LaunchFailed {
                    stage: LaunchStage::Run,
                    ..
                }
            ),
            "{faults:?}"
        );
        assert!(tiled.is_empty());
        let calls = w.calls();
        assert_eq!(calls.last().map(String::as_str), Some("pane_close"));
        assert_eq!(w.fake.pane_ids(), ["p1"], "the new pane is closed");
        let r = w.only_record();
        assert!(matches!(
            r.execution_status(),
            ExecutionStatus::LaunchFailed {
                stage: LaunchStage::Run,
                ..
            }
        ));
        assert!(w.store.live().unwrap().is_empty());
        assert!(!w.mailbox.role_taken("sonnet-1"));
    }
}

/// A workspace whose `pane_run` also acts out a fast worker: before the
/// spawner gets the call's answer, the worker sets its record to `worker`.
/// A loaded machine can schedule the spawner that late (cmp_05, D18).
struct FastWorker<'a> {
    inner: &'a FakeWorkspace,
    store: &'a ExecutionStore,
    worker: ExecutionStatus,
}

impl WorkspaceClient for FastWorker<'_> {
    fn pane_get(&self, pane: &str) -> Result<Pane> {
        self.inner.pane_get(pane)
    }
    fn pane_list(&self, workspace: &str) -> Result<Vec<Pane>> {
        self.inner.pane_list(workspace)
    }
    fn pane_split(&self, from: &str, direction: Direction) -> Result<String> {
        self.inner.pane_split(from, direction)
    }
    fn pane_run(&self, pane: &str, command: &str) -> Result<()> {
        self.inner.pane_run(pane, command)?;
        let id = self.store.read()?[0].record_id.clone();
        self.store.set_state(&id, ExecutionStatus::Running)?;
        self.store.set_state(&id, self.worker.clone())
    }
    fn pane_close(&self, pane: &str) -> Result<()> {
        self.inner.pane_close(pane)
    }
    fn agent_prompt(&self, pane: &str, text: &str) -> Result<()> {
        self.inner.agent_prompt(pane, text)
    }
    fn pane_send_text(&self, pane: &str, text: &str) -> Result<()> {
        self.inner.pane_send_text(pane, text)
    }
    fn pane_send_keys(&self, pane: &str, keys: &str) -> Result<()> {
        self.inner.pane_send_keys(pane, keys)
    }
    fn pane_read(&self, pane: &str, source: &str) -> Result<String> {
        self.inner.pane_read(pane, source)
    }
    fn workspace_create(
        &self,
        label: &str,
        cwd: Option<&str>,
        focus: bool,
    ) -> Result<NewWorkspace> {
        self.inner.workspace_create(label, cwd, focus)
    }
    fn workspace_close(&self, workspace: &str) -> Result<()> {
        self.inner.workspace_close(workspace)
    }
    fn server_reachable(&self) -> bool {
        self.inner.server_reachable()
    }
}

/// The worker runs once `pane_run` answers, and the spawner records the
/// pane after that. What the worker recorded in between stays: `Starting`
/// never takes a record back from `Running`, `Done` or `Failed`. Before
/// D18 a `Done` became `Starting` again, and the dataset coordinator
/// recorded the finished candidate as `pane_vanished`.
#[test]
fn arc_16_mark_starting_keeps_what_the_worker_recorded() {
    for worker in [
        ExecutionStatus::Running,
        ExecutionStatus::Done,
        ExecutionStatus::Failed {
            failure: FailureKind::AgentExited { code: Some(3) },
        },
    ] {
        let w = world(None);
        let fast = FastWorker {
            inner: &w.fake,
            store: &w.store,
            worker: worker.clone(),
        };
        let mut req = req("sonnet", "build");
        req.from_pane = Some("p1".into());
        let service = ExecutionService {
            ctx: &w.ctx,
            store: &w.store,
            workspace: &fast,
            mailbox: &w.mailbox,
            tile: &|_, _| {},
        };
        let pane = service
            .spawn(plan(&req, "all-ok", None).unwrap(), None)
            .unwrap()
            .pane;
        let r = w.only_record();
        assert_eq!(r.execution_status(), worker);
        assert_eq!(r.pane_id.as_deref(), Some(pane.as_str()), "{worker:?}");
        assert_eq!(r.finished_at.is_some(), worker.is_terminal(), "{worker:?}");
    }
}

/// The other writers that could take a record back (D18):
/// `mark_running` (the worker) keeps an end the coordinator wrote while the
/// worker started, and `end_live` (a timeout, cancel or launch failure)
/// keeps an end the worker wrote after the caller's look.
#[test]
fn arc_16_guarded_writers_keep_an_end() {
    let failed = ExecutionStatus::Failed {
        failure: FailureKind::TimedOut,
    };
    let launch_failed = ExecutionStatus::LaunchFailed {
        stage: LaunchStage::Split,
        reason: "abandoned".into(),
    };
    // (before, after mark_running)
    for (before, after) in [
        (ExecutionStatus::Planned, ExecutionStatus::Running),
        (ExecutionStatus::Starting, ExecutionStatus::Running),
        (ExecutionStatus::Running, ExecutionStatus::Running),
        (launch_failed.clone(), ExecutionStatus::Running),
        (ExecutionStatus::Done, ExecutionStatus::Done),
        (failed.clone(), failed.clone()),
    ] {
        let mut r = record("sonnet", "claude", "sonnet");
        r.set_state(before.clone());
        let (_tmp, store) = store_with(r);
        store.mark_running("rec-1").unwrap();
        let r = store.get("rec-1").unwrap();
        assert_eq!(r.execution_status(), after, "mark_running from {before:?}");
        if !after.is_terminal() {
            assert!(r.finished_at.is_none(), "{before:?}");
        }
    }
    // (before, state end_live returns and leaves)
    for (before, after) in [
        (ExecutionStatus::Planned, failed.clone()),
        (ExecutionStatus::Starting, failed.clone()),
        (ExecutionStatus::Running, failed.clone()),
        (ExecutionStatus::Done, ExecutionStatus::Done),
        (
            ExecutionStatus::Failed {
                failure: FailureKind::AgentExited { code: Some(3) },
            },
            ExecutionStatus::Failed {
                failure: FailureKind::AgentExited { code: Some(3) },
            },
        ),
        (launch_failed.clone(), launch_failed.clone()),
    ] {
        let mut r = record("sonnet", "claude", "sonnet");
        r.set_state(before.clone());
        let (_tmp, store) = store_with(r);
        let end = store.end_live("rec-1", failed.clone()).unwrap();
        assert_eq!(end, after, "end_live from {before:?}");
        let r = store.get("rec-1").unwrap();
        assert_eq!(r.execution_status(), after);
        if !before.is_terminal() {
            assert!(r.finished_at.is_some(), "{before:?}");
        }
    }
    let (_tmp, store) = store_with(record("sonnet", "claude", "sonnet"));
    assert!(store.end_live("nope", failed).is_err());
}

/// The recovery rule: a `Planned` record with no pane is never live, and
/// once [`ABANDONED_AFTER`] has passed, the next spawn closes it as
/// `LaunchFailed` and frees its role. A younger one is left alone: its
/// spawner may still be on its way to a pane.
#[test]
fn arc_16_planned_record_recovery() {
    let w = world(None);
    let p = plan(&req("sonnet", "x"), "all-ok", None).unwrap();
    let p = finish_plan(
        p,
        RoleName::new("sonnet-7").unwrap(),
        &WorkspaceId::new("w1").unwrap(),
    );
    w.store.insert_execution(&p.execution).unwrap();
    std::fs::create_dir_all(w.mailbox.dir()).unwrap();
    std::fs::write(w.mailbox.dir().join("sonnet-7.brief.json"), "{}").unwrap();
    assert!(w.store.live().unwrap().is_empty(), "Planned is not live");
    assert!(w.mailbox.role_taken("sonnet-7"));

    let created = horch_core::clock::parse(NOW).unwrap();
    let early = created + ABANDONED_AFTER - chrono::Duration::seconds(1);
    assert!(w.store.recover_abandoned(early).unwrap().is_empty());
    assert_eq!(w.only_record().state, Some(ExecutionStatus::Planned));

    let late = created + ABANDONED_AFTER;
    let closed = w.store.recover_abandoned(late).unwrap();
    assert_eq!(closed.len(), 1);
    let r = w.only_record();
    assert!(matches!(
        r.execution_status(),
        ExecutionStatus::LaunchFailed { .. }
    ));
    assert_eq!(r.status, "done");
}

// ─── ARC-18 ─────────────────────────────────────────────────────────────────

fn brief(record_id: &str) -> Brief {
    Brief {
        schema: SCHEMA,
        role: "sonnet-1".into(),
        teammate: "sonnet".into(),
        agent: "claude".into(),
        model: "sonnet".into(),
        record_id: record_id.into(),
        session: SessionMode::Fresh(Some("s1".parse().unwrap())),
        task: "t".into(),
        project_dir: "/p".into(),
        state_dir: None,
        claude_bin: None,
        codex_bin: None,
        resolved: None,
        teammates_dir: None,
        data_root: None,
        workdir: None,
        bin_overrides: Default::default(),
        report_to: horch_core::execution::ReportTarget::Orchestrator,
    }
}

/// Records each step; the agent exits with `exit`, or fails to start.
struct Steps<'a> {
    calls: Vec<String>,
    exit: Result<Option<i32>, &'static str>,
    store: Option<&'a ExecutionStore>,
}

impl WorkerSteps for Steps<'_> {
    fn load_brief(&mut self) -> Result<Brief> {
        self.calls.push("load_brief".into());
        Ok(brief("rec-1"))
    }
    fn enter_context(&mut self, _: &Brief) -> Result<()> {
        self.calls.push("enter_context".into());
        Ok(())
    }
    fn register(&mut self, _: &Brief) -> Result<()> {
        self.calls.push("register".into());
        Ok(())
    }
    fn set_running(&mut self, b: &Brief) -> Result<()> {
        self.calls.push("set_running".into());
        match self.store {
            Some(s) => s.set_state(&b.record_id, ExecutionStatus::Running),
            None => Ok(()),
        }
    }
    fn launch(&mut self, _: &Brief) -> Result<Option<i32>> {
        self.calls.push("launch".into());
        match self.exit {
            Ok(code) => Ok(code),
            Err(why) => bail!(why),
        }
    }
    fn agent_exited(&mut self, b: &Brief, code: Option<i32>) -> Result<()> {
        self.calls.push(format!("agent_exited({code:?})"));
        match self.store {
            Some(s) => s.record_exit(&b.record_id, code),
            None => Ok(()),
        }
    }
}

#[test]
fn arc_18_worker_startup_order() {
    let mut steps = Steps {
        calls: Vec::new(),
        exit: Ok(Some(0)),
        store: None,
    };
    assert_eq!(run_worker(&mut steps).unwrap(), 0);
    assert_eq!(
        steps.calls,
        [
            "load_brief",
            "enter_context",
            "register",
            "set_running",
            "launch",
            "agent_exited(Some(0))"
        ]
    );

    // An agent that cannot start is still recorded, then the error returns.
    let mut steps = Steps {
        calls: Vec::new(),
        exit: Err("agent CLI 'claude' not found on PATH"),
        store: None,
    };
    let e = run_worker(&mut steps).unwrap_err();
    assert!(e.to_string().contains("not found"), "{e}");
    assert_eq!(steps.calls.last().unwrap(), "agent_exited(None)");
}

fn store_with(record: LedgerRecordV1) -> (tempfile::TempDir, ExecutionStore) {
    let tmp = tempfile::tempdir().unwrap();
    let store = ExecutionStore::for_project(tmp.path(), "/p");
    store.insert(record).unwrap();
    (tmp, store)
}

/// Fails at `fail_at` (`enter_context` or `register`); records each step.
struct FailingSteps<'a> {
    calls: Vec<String>,
    fail_at: &'static str,
    store: &'a ExecutionStore,
}

impl FailingSteps<'_> {
    fn step(&mut self, name: &'static str) -> Result<()> {
        self.calls.push(name.into());
        if self.fail_at == name {
            bail!("{name} failed");
        }
        Ok(())
    }
}

impl WorkerSteps for FailingSteps<'_> {
    fn load_brief(&mut self) -> Result<Brief> {
        self.step("load_brief")?;
        Ok(brief("rec-1"))
    }
    fn enter_context(&mut self, _: &Brief) -> Result<()> {
        self.step("enter_context")
    }
    fn register(&mut self, _: &Brief) -> Result<()> {
        self.step("register")
    }
    fn set_running(&mut self, _: &Brief) -> Result<()> {
        self.step("set_running")
    }
    fn launch(&mut self, _: &Brief) -> Result<Option<i32>> {
        self.calls.push("launch".into());
        Ok(Some(0))
    }
    fn agent_exited(&mut self, b: &Brief, code: Option<i32>) -> Result<()> {
        self.calls.push(format!("agent_exited({code:?})"));
        self.store.record_exit(&b.record_id, code)
    }
}

/// A worker that fails at `fail_at` ends its `Starting` record at once and
/// never launches the agent; the original error returns.
fn startup_failure_records_failed(fail_at: &'static str) {
    // As a spawn leaves it: inserted `Planned`, then given its pane.
    let mut live = record("sonnet", "claude", "sonnet");
    live.set_state(ExecutionStatus::Planned);
    let (_tmp, store) = store_with(live);
    store.mark_starting("rec-1", "p1").unwrap();
    assert_eq!(
        store.get("rec-1").unwrap().execution_status(),
        ExecutionStatus::Starting
    );
    let mut steps = FailingSteps {
        calls: Vec::new(),
        fail_at,
        store: &store,
    };
    let e = run_worker(&mut steps).unwrap_err();
    assert_eq!(e.to_string(), format!("{fail_at} failed"));
    assert_eq!(steps.calls.last().unwrap(), "agent_exited(None)");
    assert!(!steps.calls.iter().any(|c| c == "launch"));
    let r = store.get("rec-1").unwrap();
    assert_eq!(
        r.execution_status(),
        ExecutionStatus::Failed {
            failure: FailureKind::AgentExited { code: None }
        }
    );
    assert!(r.finished_at.is_some());
}

#[test]
fn arc_18_register_failure_records_failed() {
    startup_failure_records_failed("register");
}

#[test]
fn arc_18_enter_context_failure_records_failed() {
    startup_failure_records_failed("enter_context");
}

#[test]
fn arc_18_agent_exit_recorded() {
    let mut live = record("sonnet", "claude", "sonnet");
    live.status = "working".into();

    // A worker whose agent exits 3: Failed(AgentExited{3}), exit code 3.
    let (_tmp, store) = store_with(live.clone());
    let mut steps = Steps {
        calls: Vec::new(),
        exit: Ok(Some(3)),
        store: Some(&store),
    };
    assert_eq!(run_worker(&mut steps).unwrap(), 3);
    let r = store.get("rec-1").unwrap();
    assert_eq!(
        r.execution_status(),
        ExecutionStatus::Failed {
            failure: FailureKind::AgentExited { code: Some(3) }
        }
    );
    assert_eq!(r.exit_code, Some(3));
    assert_eq!(r.status, "done");
    assert!(r.finished_at.is_some());

    // Exit 0 ends the execution as Done.
    let (_tmp, store) = store_with(live.clone());
    let mut steps = Steps {
        calls: Vec::new(),
        exit: Ok(Some(0)),
        store: Some(&store),
    };
    assert_eq!(run_worker(&mut steps).unwrap(), 0);
    assert_eq!(
        store.get("rec-1").unwrap().execution_status(),
        ExecutionStatus::Done
    );

    // A signal: no code, exit 1.
    let (_tmp, store) = store_with(live.clone());
    let mut steps = Steps {
        calls: Vec::new(),
        exit: Ok(None),
        store: Some(&store),
    };
    assert_eq!(run_worker(&mut steps).unwrap(), 1);
    assert_eq!(
        store.get("rec-1").unwrap().execution_status(),
        ExecutionStatus::Failed {
            failure: FailureKind::AgentExited { code: None }
        }
    );

    // `horch done` ran first: the state stays Done, the code is recorded.
    let mut finished = live.clone();
    finished.status = "done".into();
    let (_tmp, store) = store_with(finished);
    store.record_exit("rec-1", Some(3)).unwrap();
    let r = store.get("rec-1").unwrap();
    assert_eq!(r.execution_status(), ExecutionStatus::Done);
    assert_eq!(r.exit_code, Some(3));

    // An orchestrator keeps its state.
    let mut orch = live;
    orch.kind = KIND_ORCHESTRATOR.into();
    let (_tmp, store) = store_with(orch);
    store.record_exit("rec-1", Some(3)).unwrap();
    assert!(store.get("rec-1").unwrap().execution_status().is_live());
}
