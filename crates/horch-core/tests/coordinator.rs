//! The competition coordinator in process (B3): a real temp git repo, the
//! in-memory workspace, the real execution store and event log.
//!
//! `FakeWorkspace` runs no pane command, so the test plays the agents: each
//! time the coordinator sleeps between ticks, every live candidate's record
//! is moved on the way its agent would move it.

mod common;

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Utc};
use horch_core::competition::budget::UsageMeter;
use horch_core::competition::config::{self, RunFlags};
use horch_core::competition::coordinator::{
    candidate_role, candidate_task, monotonic, Coordinator, RoundOutcome, RoundSpec,
};
use horch_core::competition::judging::JobLauncher;
use horch_core::competition::observe::TelemetryUsage;
use horch_core::competition::planner::{config_id, plan_round, PlanInput, RoundPlan};
use horch_core::evaluation::scheduler::JudgeJobSpec;
use horch_core::evaluation::validator::CommandValidator;
use horch_core::execution::store::{to_execution, ExecutionStore};
use horch_core::execution::{ExecutionKind, ExecutionStatus, FailureKind, ReportTarget};
use horch_core::harness::HarnessKind;
use horch_core::ids::{ExperimentId, RoundId};
use horch_core::ids::{ModelId, TeammateName};
use horch_core::measure::event::{Actor, EventKind};
use horch_core::measure::paths::DatasetPaths;
use horch_core::measure::projection::fold;
use horch_core::measure::recorder::{JsonlRecorder, NewEvent, Recorder};
use horch_core::measure::store::StoreOptions;
use horch_core::messaging::mailbox::Mailbox;
use horch_core::roster::Roster;
use horch_core::routing::decision::RoutingMode;
use horch_core::routing::eligible::EligibilityFilter;
use horch_core::routing::policy::Policy;
use horch_core::routing::quota::{QuotaFile, QuotaView};
use horch_core::runtime::fault::Faults;
use horch_core::runtime::{MapEnv, RuntimeContext};
use horch_core::usage::Locations;
use horch_core::vcs::git::GitCli;
use horch_core::workspace::testing::FakeWorkspace;
use serde_json::json;

fn core_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn roster() -> Roster {
    let mut r = Roster::builtin().unwrap();
    r.overlay(&core_dir().join("../../teammates")).unwrap();
    r
}

fn find_git() -> Option<PathBuf> {
    let found = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|d| d.join("git"))
            .find(|p| p.is_absolute() && p.is_file())
    });
    if found.is_none() {
        assert!(
            std::env::var_os("HORCH_REQUIRE_GIT").is_none_or(|v| v != "1"),
            "HORCH_REQUIRE_GIT=1 is set but git is not on PATH"
        );
    }
    found
}

/// Never called: every candidate of these rounds fails, so no judge runs.
struct NoJudge;

impl JobLauncher for NoJudge {
    fn launch(&self, _spec: &JudgeJobSpec) -> anyhow::Result<u32> {
        panic!("no judge expected")
    }
}

struct World {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    repo: PathBuf,
    ctx: RuntimeContext,
    git: GitCli,
    base: String,
}

fn world() -> Option<World> {
    let bin = find_git()?;
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let config = root.join("gitconfig");
    std::fs::write(&config, "").unwrap();
    let pins = [
        ("GIT_CONFIG_GLOBAL", config.to_string_lossy().into_owned()),
        ("GIT_CONFIG_NOSYSTEM", "1".into()),
        ("GIT_AUTHOR_NAME", "Horch Fixture".into()),
        ("GIT_AUTHOR_EMAIL", "fixture@horch.invalid".into()),
        ("GIT_COMMITTER_NAME", "Horch Fixture".into()),
        ("GIT_COMMITTER_EMAIL", "fixture@horch.invalid".into()),
    ];
    let mut git = GitCli::new(bin.clone());
    for (k, v) in &pins {
        git = git.with_env(k, v);
    }
    let repo = root.join("repo");
    std::fs::create_dir(&repo).unwrap();
    std::fs::write(repo.join("a.txt"), "one\n").unwrap();
    // In `repo` only: `GIT_DIR` from a hook or `git rebase -x` is removed.
    let run = |args: &[&str]| {
        let mut cmd = std::process::Command::new(&bin);
        cmd.arg("-C")
            .arg(&repo)
            .args(args)
            .current_dir(&repo)
            .envs(pins.iter().map(|(k, v)| (*k, v.as_str())));
        horch_marketplace::git::scrub_repo_env(&mut cmd);
        let out = cmd.output().unwrap();
        assert!(out.status.success(), "{out:?}");
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    };
    run(&["init", "-q", "-b", "main"]);
    run(&["add", "-A"]);
    run(&["commit", "-q", "-m", "base"]);
    let base = run(&["rev-parse", "HEAD"]);
    let env = MapEnv::new(&repo)
        .with("HORCH_STATE_DIR", &root.join("state").to_string_lossy())
        .with("HORCH_PROJECT_DIR", &repo.to_string_lossy())
        .with("HOME", &root.join("home").to_string_lossy())
        .with("TMPDIR", &root.join("tmp").to_string_lossy())
        .with_exe("/opt/horch/bin/horch");
    let ctx = RuntimeContext::from_env(&env).unwrap();
    Some(World {
        _tmp: tmp,
        root,
        repo,
        ctx,
        git,
        base,
    })
}

/// A fixed round id. The planner seeds the exploration slot with
/// `sha256(round_id)`, so a minted id (random bits) changes the plan
/// between runs.
fn fixed_round() -> RoundId {
    RoundId::new("0199a5b0-0000-7000-8000-0000000000c0").unwrap()
}

/// What [`crash_round`] leaves behind.
struct CrashRound {
    recorder: JsonlRecorder,
    store: ExecutionStore,
    experiment: ExperimentId,
    round: RoundId,
    outcome: RoundOutcome,
}

/// One 2-candidate round run in process, in which every candidate's agent
/// writes `work.txt` and crashes. `shape` may change the plan before the
/// round starts.
fn crash_round(w: &World, roster: &Roster, shape: impl FnOnce(&mut RoundPlan)) -> CrashRound {
    round_with(w, roster, shape, Agent::Crash)
}

/// What every candidate's agent does once it runs.
#[derive(Clone, Copy, PartialEq)]
enum Agent {
    /// Exits with code 2.
    Crash,
    /// Stops at Claude's trust dialog and waits.
    TrustDialog,
    /// Writes its work and waits at its prompt (herdr `idle`) without
    /// `horch done`; the round's clock moves 60 s per tick.
    Idle,
}

/// Claude's trust dialog, as a pane showed it in LA-7.
const CLAUDE_TRUST_SCREEN: &str = "Accessing workspace:\n/x/wt/A\n\nQuick safety check: Is this a project you created or one you trust? (Like your\nown code, a well-known open source project, or work from your team).\n\n> 1. Yes, I trust this folder\n  2. No, exit\n\nEnter to confirm · Esc to cancel\n";

fn round_with(
    w: &World,
    roster: &Roster,
    shape: impl FnOnce(&mut RoundPlan),
    agent: Agent,
) -> CrashRound {
    round_cfg(w, roster, shape, agent, |_| {}).0
}

/// [`round_with`] with `tune` applied to the config, which also returns
/// every workspace call.
fn round_cfg(
    w: &World,
    roster: &Roster,
    shape: impl FnOnce(&mut RoundPlan),
    agent: Agent,
    tune: impl FnOnce(&mut config::DatasetConfig),
) -> (CrashRound, Vec<horch_core::workspace::testing::FakeCall>) {
    let paths = DatasetPaths::new(&w.ctx.paths.state_root, &w.repo);
    let recorder = JsonlRecorder::open(&paths, StoreOptions::default()).unwrap();
    let store = ExecutionStore::open(&w.ctx.paths, &w.repo);
    let fake = FakeWorkspace::new();

    let mut config = config::load(
        &w.repo,
        &RunFlags {
            task: "add a greeting".into(),
            budget_usd: Some("100".into()),
            ..RunFlags::default()
        },
    )
    .unwrap();
    config.candidates = 2;
    config.worktree_root = Some(w.root.join("worktrees"));
    tune(&mut config);
    let now = horch_core::clock::parse("2026-10-02T12:00:00Z").unwrap();
    let experiment = ExperimentId::mint(now);
    let round = fixed_round();
    let view = QuotaView::new(
        QuotaFile::read(&core_dir().join("tests/fixtures/telemetry/quota/all-ok.json")).unwrap(),
        now,
        Policy::default(),
        true,
    );
    let mut plan = plan_round(&PlanInput {
        round_id: &round,
        index: 0,
        base_sha: &w.base,
        n: 2,
        baseline: None,
        roster,
        view: &view,
        filter: &EligibilityFilter::default(),
        config: &config,
    });
    assert_eq!(plan.candidates.len(), 2);
    shape(&mut plan);

    // The experiment as `run` leaves it after preflight.
    let digest = || json!(format!("sha256:{}", "0".repeat(64)));
    for (kind, payload) in [
        (
            "experiment.created",
            json!({"task_id": "task-1", "task_digest": digest(), "config_digest": digest(),
                   "base_sha": w.base, "repo_digest": digest(), "environment_digest": digest(),
                   "candidates": 2, "strategy": "diverse", "budget_usd_micro": 100_000_000,
                   "promote_to": null}),
        ),
        (
            "preflight.completed",
            json!({"report": {"schema_version": "1.0.0", "checks": [], "safe_n": 2, "waves": 1,
                   "projected_cost_microusd": 0,
                   "machine": {"os": "macos", "arch": "aarch64", "cpus": 10,
                               "mem_total_bytes": null, "mem_available_bytes": null,
                               "disk_free_bytes": null, "disk_total_bytes": null,
                               "gpu": "apple_silicon", "max_open_files": null,
                               "max_processes": null},
                   "environment_digest": digest(), "passed": true}}),
        ),
    ] {
        recorder
            .append(NewEvent {
                kind: EventKind::from_parts(kind, &payload).unwrap(),
                actor: Actor::Coordinator,
                experiment_id: experiment.clone(),
                round_id: None,
                execution_id: None,
                idempotency_key: format!("{kind}:{experiment}"),
                occurred_at: now,
            })
            .unwrap();
    }

    // The agents: every live candidate writes a file and its agent crashes.
    let ticks = Cell::new(0u32);
    let offset = Cell::new(chrono::Duration::zero());
    let sleep = |_: Duration| {
        ticks.set(ticks.get() + 1);
        assert!(ticks.get() < 50, "the round never ended");
        if agent == Agent::Idle {
            offset.set(offset.get() + chrono::Duration::seconds(60));
        }
        for r in store.read().unwrap() {
            if r.execution_status() == ExecutionStatus::Starting {
                if let Some(dir) = &r.workdir {
                    std::fs::write(Path::new(dir).join("work.txt"), "partial\n").unwrap();
                }
                store
                    .set_state(&r.record_id, ExecutionStatus::Running)
                    .unwrap();
                match agent {
                    Agent::Crash => store.record_exit(&r.record_id, Some(2)).unwrap(),
                    Agent::TrustDialog => {
                        fake.set_screen(r.pane_id.as_deref().unwrap(), CLAUDE_TRUST_SCREEN)
                    }
                    Agent::Idle => fake.set_agent_states(
                        r.pane_id.as_deref().unwrap(),
                        &[(Some("claude"), Some("idle"))],
                    ),
                }
            }
        }
    };
    let validator = CommandValidator::new(
        Vec::new(),
        paths.root().join("validation"),
        Default::default(),
    );
    let usage = TelemetryUsage {
        locations: Locations::under_home(&w.root.join("home"), &w.ctx.inherited),
    };
    let meter = UsageMeter::default();
    let clock = monotonic(|| Utc::now() + offset.get());
    let disk = || None;
    let faults = Faults::default();
    let coordinator = Coordinator {
        ctx: &w.ctx,
        recorder: &recorder,
        paths: &paths,
        git: &w.git,
        workspace: &fake,
        store: &store,
        roster,
        validator: &validator,
        usage: &usage,
        meter: &meter,
        disk_free: &disk,
        clock: &clock as &dyn Fn() -> DateTime<Utc>,
        sleep: &sleep,
        faults: &faults,
        launcher: &NoJudge,
    };
    let spec = RoundSpec {
        experiment: experiment.clone(),
        round: round.clone(),
        index: 0,
        config: config.clone(),
        task: "add a greeting".into(),
        repo: w.repo.clone(),
        worktree_root: w.root.join("worktrees"),
        safe_n: 2,
        tick: Duration::from_millis(1),
        watch_command: "watch".into(),
    };
    let outcome = coordinator.start(&spec, &plan).unwrap();
    let calls = fake.calls();
    (
        CrashRound {
            recorder,
            store,
            experiment,
            round,
            outcome,
        },
        calls,
    )
}

#[test]
fn arc_24_candidates_are_ordinary_executions() {
    let Some(w) = world() else { return };
    let roster = roster();
    let CrashRound {
        recorder,
        store,
        experiment,
        round,
        outcome,
    } = crash_round(&w, &roster, |_| {});
    // Every agent crashed: no eligible candidate, the round is rejected.
    assert_eq!(outcome, RoundOutcome::Rejected { budget: false });

    // The candidates live in the project's one execution store, as
    // ordinary executions of kind candidate.
    let records = store.read().unwrap();
    assert_eq!(records.len(), 2, "{records:?}");
    let events = recorder.read_all().unwrap().events;
    let projection = fold(&events);
    let r = &projection.rounds[&round];
    for record in &records {
        let e = to_execution(record).unwrap();
        let ExecutionKind::Candidate {
            experiment: ex,
            round: ro,
            label,
        } = &e.kind
        else {
            panic!("not a candidate: {:?}", e.kind)
        };
        assert_eq!(ex, &experiment);
        assert_eq!(ro, &round);
        assert_eq!(record.kind, "worker", "old readers see a worker");
        assert_eq!(e.idempotency_key(), Some(format!("spawn:{round}:{label}")));
        // Created by ExecutionService: role allocated, pane split from the
        // dataset workspace's root pane, history `spawned`, pinned routing.
        assert_eq!(e.role.as_str(), candidate_role(label));
        assert!(e.pane.is_some());
        assert_eq!(e.history[0].event, "spawned");
        assert_eq!(e.routing.as_ref().unwrap().mode, RoutingMode::Pinned);
        assert_eq!(
            e.status,
            ExecutionStatus::Failed {
                failure: FailureKind::AgentExited { code: Some(2) }
            }
        );
        let c = &r.candidates[label];
        assert_eq!(c.execution_id.as_ref(), Some(&e.id));
        let wt = c.worktree.as_ref().unwrap();
        assert_eq!(e.workdir.as_deref(), Some(wt.path.as_path()));
        assert_eq!(wt.base_sha, w.base);
        // The partial work is frozen into the round, not eligible.
        assert!(c
            .frozen
            .as_ref()
            .unwrap()
            .numstat
            .iter()
            .any(|n| n.path == "work.txt"));
        assert!(!c.is_eligible());
        // The brief: the rules in the task text, and no report target.
        let mailbox = Mailbox::in_context(&w.ctx, e.workspace.as_ref().unwrap().as_str());
        let brief = mailbox.read_brief(e.role.as_str()).unwrap();
        assert_eq!(brief.report_to, ReportTarget::None);
        assert_eq!(
            brief.task,
            candidate_task(&roster, "add a greeting", &wt.path).unwrap()
        );
        assert_eq!(brief.workdir.as_deref(), Some(&*wt.path.to_string_lossy()));
    }
    assert!(
        projection.anomalies.is_empty(),
        "{:?}",
        projection.anomalies
    );

    // No second registry: the state root holds the ledger, its lock and the
    // dataset dir, nothing else.
    let mut entries: Vec<String> = std::fs::read_dir(&w.ctx.paths.state_root)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !n.ends_with(".lock"))
        .collect();
    entries.sort();
    let ledger = store
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let mut expected = vec![ledger, "multi-herdr".to_string()];
    expected.sort();
    assert_eq!(entries, expected);
}

/// A candidate pane stopped at a trust dialog ends at the next look, with
/// the reason, and not at the deadline (1 hour here; the test allows 50
/// ticks of 1 ms).
#[test]
fn pre_14_trust_dialog_pane_ends_the_candidate_at_once() {
    let Some(w) = world() else { return };
    let roster = roster();
    let CrashRound {
        recorder,
        store,
        round,
        outcome,
        ..
    } = round_with(&w, &roster, |_| {}, Agent::TrustDialog);
    assert_eq!(outcome, RoundOutcome::Rejected { budget: false });
    let records = store.read().unwrap();
    assert_eq!(records.len(), 2, "{records:?}");
    let blocked = FailureKind::Cancelled {
        reason: "trust_dialog".into(),
    };
    for record in &records {
        let e = to_execution(record).unwrap();
        assert_eq!(
            e.status,
            ExecutionStatus::Failed {
                failure: blocked.clone()
            }
        );
    }
    let projection = fold(&recorder.read_all().unwrap().events);
    for c in projection.rounds[&round].candidates.values() {
        assert!(!c.is_eligible());
    }
}

/// A candidate that waits at its prompt without `horch done` gets exactly
/// one nudge after `idle_nudge_after_s`, then ends as
/// `Cancelled{idle_without_done}` after `idle_end_after_s`, long before the
/// deadline; its work stays frozen in the round (F5, dataset design 4.11.5).
#[test]
fn idle_candidate_is_nudged_once_then_ends() {
    let Some(w) = world() else { return };
    let roster = roster();
    let (
        CrashRound {
            recorder,
            store,
            round,
            outcome,
            ..
        },
        calls,
    ) = round_cfg(
        &w,
        &roster,
        |_| {},
        Agent::Idle,
        |c| {
            c.caps.idle_nudge_after_s = 120;
            c.caps.idle_end_after_s = 180;
            c.caps.candidate_deadline_s = 3600;
        },
    );
    // Neither candidate ran `horch done`: nothing is eligible.
    assert_eq!(outcome, RoundOutcome::Rejected { budget: false });
    let records = store.read().unwrap();
    assert_eq!(records.len(), 2);
    let idle = FailureKind::Cancelled {
        reason: "idle_without_done".into(),
    };
    for record in &records {
        let e = to_execution(record).unwrap();
        assert_eq!(
            e.status,
            ExecutionStatus::Failed {
                failure: idle.clone()
            }
        );
        let pane = e.pane.as_ref().unwrap().as_str().to_string();
        let nudges: Vec<_> = calls
            .iter()
            .filter(|c| c.method == "agent_prompt" && c.args[0] == pane)
            .collect();
        assert_eq!(nudges.len(), 1, "one nudge per candidate: {calls:?}");
        assert!(nudges[0].args[1].contains("horch done"));
        assert!(nudges[0].args[1].contains("Do not commit"));
    }
    let projection = fold(&recorder.read_all().unwrap().events);
    assert!(
        projection.anomalies.is_empty(),
        "{:?}",
        projection.anomalies
    );
    for c in projection.rounds[&round].candidates.values() {
        assert!(!c.is_eligible());
        // The work is frozen all the same.
        let frozen = c.frozen.as_ref().unwrap();
        assert!(frozen.numstat.iter().any(|n| n.path == "work.txt"));
    }
}

/// With `idle_nudge_after_s: 0` an idle candidate is never nudged and waits
/// for the deadline, as before F5.
#[test]
fn idle_rule_off_waits_for_the_deadline() {
    let Some(w) = world() else { return };
    let roster = roster();
    let (CrashRound { store, .. }, calls) = round_cfg(
        &w,
        &roster,
        |_| {},
        Agent::Idle,
        |c| {
            c.caps.idle_nudge_after_s = 0;
            c.caps.candidate_deadline_s = 600;
        },
    );
    assert!(
        !calls.iter().any(|c| c.method == "agent_prompt"),
        "{calls:?}"
    );
    for record in store.read().unwrap() {
        let e = to_execution(&record).unwrap();
        assert_eq!(
            e.status,
            ExecutionStatus::Failed {
                failure: FailureKind::TimedOut
            }
        );
    }
}

/// A worktree that cannot be made (here: B's path is taken) stops the round
/// for the operator, with the reason, before any candidate starts; the
/// operator's `cleanup --force` then removes the worktree that was made
/// (LA-11 found the round stuck in PROVISIONING).
#[test]
fn provisioning_failure_needs_the_operator_and_cleans_up() {
    use horch_core::competition::cleanup::{CleanupOptions, CleanupOutcome, RoundCleanup};
    use horch_core::competition::model::RoundState;

    let Some(w) = world() else { return };
    let roster = roster();
    let taken = w.root.join("worktrees").join("B");
    std::fs::create_dir_all(&taken).unwrap();
    std::fs::write(taken.join("other.txt"), "an earlier round\n").unwrap();
    let CrashRound {
        recorder,
        store,
        round,
        outcome,
        ..
    } = crash_round(&w, &roster, |_| {});
    assert_eq!(outcome, RoundOutcome::NeedsIntervention);
    assert!(store.read().unwrap().is_empty(), "no candidate started");

    let events = recorder.read_all().unwrap().events;
    let projection = fold(&events);
    assert!(
        projection.anomalies.is_empty(),
        "{:?}",
        projection.anomalies
    );
    let view = &projection.rounds[&round];
    assert_eq!(view.state, RoundState::NeedsIntervention);
    let reason = events
        .iter()
        .find(|e| e.kind == "round.needs_intervention")
        .and_then(|e| e.payload["reason"].as_str().map(str::to_string))
        .unwrap();
    assert!(reason.contains("creating the worktree of B"), "{reason}");
    assert!(reason.contains("--force"), "{reason}");
    let made = view.candidates["A"].worktree.as_ref().unwrap().path.clone();
    assert!(made.is_dir());
    assert!(view.candidates["B"].worktree.is_none());

    let cleaned = RoundCleanup {
        git: &w.git,
        recorder: &recorder,
        paths: &DatasetPaths::new(&w.ctx.paths.state_root, &w.repo),
        faults: &Faults::default(),
        repo: w.repo.clone(),
    }
    .run(
        &round,
        view,
        CleanupOptions {
            prune_branches: false,
            force: true,
        },
    )
    .unwrap();
    assert_eq!(
        cleaned,
        CleanupOutcome::Done {
            removed: vec!["A".to_string()],
            failed: Vec::new(),
        }
    );
    assert!(!made.exists());
    assert!(taken.join("other.txt").is_file(), "B's path is not ours");
    let projection = fold(&recorder.read_all().unwrap().events);
    assert_eq!(projection.rounds[&round].state, RoundState::Complete);
    assert!(projection.anomalies.is_empty());
}

/// A candidate teammate with `operator_skills` gets them on its ledger
/// record, as a `horch spawn` worker does: the coordinator reads the
/// operator dir before it plans the launch.
#[test]
fn arc_24_candidate_record_lists_operator_skills() {
    let Some(w) = world() else { return };
    let skill = w.root.join("home/.agents/skills/test-modernizer");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        "---\nname: test-modernizer\ndescription: Operator-local test modernizer.\n---\nbody\n",
    )
    .unwrap();
    let overlay = w.root.join("teammates");
    std::fs::create_dir_all(&overlay).unwrap();
    std::fs::write(
        overlay.join("op-probe.md"),
        "---\nname: op-probe\nbrief_description: Operator skills probe.\nbase: fleet-worker\n\
         agent: claude\nmodel: sonnet\nskills: [tdd]\ndisallowed_tools: [Agent]\n\
         operator_skills:\n  dir: ~/.agents/skills\n  names: [test-modernizer]\n---\nProbe.\n",
    )
    .unwrap();
    let mut roster = roster();
    roster.overlay(&overlay).unwrap();

    let probe: TeammateName = "op-probe".parse().unwrap();
    let model = ModelId::new("sonnet").unwrap();
    let run = crash_round(&w, &roster, |plan| {
        for slot in &mut plan.candidates {
            slot.teammate = probe.clone();
            slot.harness = HarnessKind::Claude;
            slot.model = model.clone();
            slot.effort = None;
            slot.config_id = config_id(&probe, HarnessKind::Claude, &model, None);
        }
    });
    assert_eq!(run.outcome, RoundOutcome::Rejected { budget: false });

    let records = run.store.read().unwrap();
    assert_eq!(records.len(), 2, "{records:?}");
    for record in &records {
        assert_eq!(record.tier, "op-probe", "{}", record.record_id);
        let ids: Vec<&str> = record.skills.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["tdd", "test-modernizer"], "{}", record.record_id);
        let op = &record.skills[1];
        assert!(op.version.0.starts_with("operator+"), "{:?}", op.version);
    }
}

#[test]
fn cmp_10_committed_spend_counts_running_candidates() {
    // 3 candidates, 2 at once, no measured spend at all. The hard limit
    // minus the judge reserve is the projected cost of the first 2, so
    // once they run the third never starts, and the 2 running are not
    // cancelled.
    let Some(w) = world() else { return };
    let roster = roster();
    let paths = DatasetPaths::new(&w.ctx.paths.state_root, &w.repo);
    let recorder = JsonlRecorder::open(&paths, StoreOptions::default()).unwrap();
    let store = ExecutionStore::open(&w.ctx.paths, &w.repo);
    let fake = FakeWorkspace::new();
    let meter = UsageMeter::default();

    let mut config = config::load(
        &w.repo,
        &RunFlags {
            task: "add a greeting".into(),
            budget_usd: Some("100".into()),
            ..RunFlags::default()
        },
    )
    .unwrap();
    config.candidates = 3;
    config.worktree_root = Some(w.root.join("worktrees"));
    let now = horch_core::clock::parse("2026-10-02T12:00:00Z").unwrap();
    let experiment = ExperimentId::mint(now);
    let round = fixed_round();
    let view = QuotaView::new(
        QuotaFile::read(&core_dir().join("tests/fixtures/telemetry/quota/all-ok.json")).unwrap(),
        now,
        Policy::default(),
        true,
    );
    let plan = plan_round(&PlanInput {
        round_id: &round,
        index: 0,
        base_sha: &w.base,
        n: 3,
        baseline: None,
        roster: &roster,
        view: &view,
        filter: &EligibilityFilter::default(),
        config: &config,
    });
    assert_eq!(plan.candidates.len(), 3);
    // A and B start first (label order). Together they commit exactly the
    // limit; C's model does not matter.
    let first_two: i64 = plan
        .candidates
        .iter()
        .filter(|c| ["A", "B"].contains(&c.label.as_str()))
        .map(|c| meter.projected(c.model.as_str()).0)
        .sum();
    assert!(first_two > 0, "A or B has a priced model: {plan:?}");
    config.budget.judge_reserve_usd_micro = 1_000_000;
    config.budget.hard_usd_micro = 1_000_000 + first_two;
    config.budget.soft_usd_micro = config.budget.hard_usd_micro;

    // The experiment as `run` leaves it after preflight.
    let digest = || json!(format!("sha256:{}", "0".repeat(64)));
    for (kind, payload) in [
        (
            "experiment.created",
            json!({"task_id": "task-1", "task_digest": digest(), "config_digest": digest(),
                   "base_sha": w.base, "repo_digest": digest(), "environment_digest": digest(),
                   "candidates": 3, "strategy": "diverse", "budget_usd_micro": 100_000_000,
                   "promote_to": null}),
        ),
        (
            "preflight.completed",
            json!({"report": {"schema_version": "1.0.0", "checks": [], "safe_n": 2, "waves": 1,
                   "projected_cost_microusd": 0,
                   "machine": {"os": "macos", "arch": "aarch64", "cpus": 10,
                               "mem_total_bytes": null, "mem_available_bytes": null,
                               "disk_free_bytes": null, "disk_total_bytes": null,
                               "gpu": "apple_silicon", "max_open_files": null,
                               "max_processes": null},
                   "environment_digest": digest(), "passed": true}}),
        ),
    ] {
        recorder
            .append(NewEvent {
                kind: EventKind::from_parts(kind, &payload).unwrap(),
                actor: Actor::Coordinator,
                experiment_id: experiment.clone(),
                round_id: None,
                execution_id: None,
                idempotency_key: format!("{kind}:{experiment}"),
                occurred_at: now,
            })
            .unwrap();
    }

    // The agents: a started candidate runs for 3 ticks, then crashes.
    let ticks = Cell::new(0u32);
    let sleep = |_: Duration| {
        ticks.set(ticks.get() + 1);
        assert!(ticks.get() < 50, "the round never ended");
        for r in store.read().unwrap() {
            match r.execution_status() {
                ExecutionStatus::Starting => {
                    store
                        .set_state(&r.record_id, ExecutionStatus::Running)
                        .unwrap();
                }
                ExecutionStatus::Running if ticks.get() >= 3 => {
                    store.record_exit(&r.record_id, Some(2)).unwrap();
                }
                _ => {}
            }
        }
    };
    let validator = CommandValidator::new(
        Vec::new(),
        paths.root().join("validation"),
        Default::default(),
    );
    // No transcripts: the measured spend stays 0.
    let usage = TelemetryUsage {
        locations: Locations::under_home(&w.root.join("home"), &w.ctx.inherited),
    };
    let clock = monotonic(Utc::now);
    let disk = || None;
    let faults = Faults::default();
    let coordinator = Coordinator {
        ctx: &w.ctx,
        recorder: &recorder,
        paths: &paths,
        git: &w.git,
        workspace: &fake,
        store: &store,
        roster: &roster,
        validator: &validator,
        usage: &usage,
        meter: &meter,
        disk_free: &disk,
        clock: &clock as &dyn Fn() -> DateTime<Utc>,
        sleep: &sleep,
        faults: &faults,
        launcher: &NoJudge,
    };
    let spec = RoundSpec {
        experiment: experiment.clone(),
        round: round.clone(),
        index: 0,
        config: config.clone(),
        task: "add a greeting".into(),
        repo: w.repo.clone(),
        worktree_root: w.root.join("worktrees"),
        safe_n: 2,
        tick: Duration::from_millis(1),
        watch_command: "watch".into(),
    };
    let outcome = coordinator.start(&spec, &plan).unwrap();
    assert_eq!(outcome, RoundOutcome::Rejected { budget: true });

    // 2 candidates started and ran to their own end; the third was
    // cancelled for the budget before it started.
    let records = store.read().unwrap();
    assert_eq!(records.len(), 2, "{records:?}");
    for record in &records {
        assert_eq!(
            to_execution(record).unwrap().status,
            ExecutionStatus::Failed {
                failure: FailureKind::AgentExited { code: Some(2) }
            }
        );
    }
    let events = recorder.read_all().unwrap().events;
    let r = &fold(&events).rounds[&round];
    let unstarted: Vec<_> = r
        .candidates
        .values()
        .filter(|c| c.spawned.is_none())
        .collect();
    assert_eq!(unstarted.len(), 1);
    assert_eq!(
        unstarted[0].failed.as_ref().unwrap().failure,
        FailureKind::Cancelled {
            reason: "budget".into()
        }
    );
}

/// D20 item 5: `world` with `GIT_DIR` and `GIT_WORK_TREE` aimed at a decoy
/// repository leaves the decoy unchanged.
#[test]
fn git_env_cannot_reach_another_repository() {
    let Some(bin) = find_git() else { return };
    common::assert_decoy_untouched(&bin, "git_env_decoy_child");
}

#[test]
#[ignore = "run by git_env_cannot_reach_another_repository, with GIT_DIR set"]
fn git_env_decoy_child() {
    assert!(world().is_some());
}
