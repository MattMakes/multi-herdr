//! B4 judge job, coordinator side: discovery of a job after a restart
//! (JDG-08), the single-authority judgment write (JDG-04), the terminal-set
//! rule (CMP-12) and the judge execution record (JDG-09). Dataset design
//! §4.7 and §5.1.

use std::path::Path;
use std::time::Duration;

use chrono::{DateTime, Utc};
use horch_core::evaluation::scheduler::{
    decide, discover, ExitReason, Heartbeat, JobExit, JobFacts, JobState, EXIT_FILE,
    HEARTBEAT_FILE, LOG_FILE, OUTPUT_FILE,
};

fn at(s: &str) -> DateTime<Utc> {
    horch_core::clock::parse(s).unwrap()
}

fn write(dir: &Path, name: &str, text: &str) {
    std::fs::write(dir.join(name), text).unwrap();
}

/// JDG-08: after a restart the coordinator reads the job dir. Output present
/// → parse; heartbeat fresh and pid alive → wait; otherwise lost.
#[test]
fn jdg_08_restart_discovers_job() {
    let now = at("2026-10-02T12:00:30Z");
    let stale = Duration::from_secs(30);
    let fresh_hb = |pid| Heartbeat {
        pid,
        at: "2026-10-02T12:00:25Z".into(),
    };
    let old_hb = Heartbeat {
        pid: 7,
        at: "2026-10-02T11:59:00Z".into(),
    };
    let crash = JobExit {
        reason: ExitReason::Crash,
        code: Some(3),
    };
    let rows: Vec<(&str, JobFacts, JobState)> = vec![
        ("nothing spawned", JobFacts::default(), JobState::NotStarted),
        (
            "spawned, no heartbeat yet",
            JobFacts {
                spawned_at: Some(at("2026-10-02T12:00:20Z")),
                ..JobFacts::default()
            },
            JobState::Running { pid: 0 },
        ),
        (
            "spawned long ago, never beat",
            JobFacts {
                spawned_at: Some(at("2026-10-02T11:00:00Z")),
                ..JobFacts::default()
            },
            JobState::Lost,
        ),
        (
            "fresh heartbeat, pid alive",
            JobFacts {
                heartbeat: Some(fresh_hb(42)),
                pid_alive: true,
                spawned_at: Some(at("2026-10-02T11:00:00Z")),
                ..JobFacts::default()
            },
            JobState::Running { pid: 42 },
        ),
        (
            "fresh heartbeat, pid dead",
            JobFacts {
                heartbeat: Some(fresh_hb(42)),
                pid_alive: false,
                ..JobFacts::default()
            },
            JobState::Lost,
        ),
        (
            "stale heartbeat, pid alive",
            JobFacts {
                heartbeat: Some(old_hb.clone()),
                pid_alive: true,
                ..JobFacts::default()
            },
            JobState::Lost,
        ),
        (
            "output present, job gone",
            JobFacts {
                output: true,
                heartbeat: Some(old_hb.clone()),
                ..JobFacts::default()
            },
            JobState::OutputPresent(OUTPUT_FILE.into()),
        ),
        (
            "output and exit: output wins",
            JobFacts {
                output: true,
                exit: Some(Some(crash.clone())),
                ..JobFacts::default()
            },
            JobState::OutputPresent(OUTPUT_FILE.into()),
        ),
        (
            "exit without output",
            JobFacts {
                exit: Some(Some(crash.clone())),
                heartbeat: Some(fresh_hb(42)),
                pid_alive: true,
                ..JobFacts::default()
            },
            JobState::Exited(crash.clone()),
        ),
        (
            "unreadable exit file is a crash",
            JobFacts {
                exit: Some(None),
                ..JobFacts::default()
            },
            JobState::Exited(JobExit {
                reason: ExitReason::Crash,
                code: None,
            }),
        ),
    ];
    for (name, facts, want) in rows {
        assert_eq!(decide(&facts, now, stale), want, "{name}");
    }

    // The same rules through real files.
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path();
    assert_eq!(discover(dir, now, stale), JobState::NotStarted);
    write(dir, LOG_FILE, "");
    assert_eq!(
        discover(dir, Utc::now(), stale),
        JobState::Running { pid: 0 }
    );
    let me = std::process::id();
    write(
        dir,
        HEARTBEAT_FILE,
        &serde_json::to_string(&Heartbeat {
            pid: me,
            at: horch_core::clock::stamp(Utc::now()),
        })
        .unwrap(),
    );
    assert_eq!(
        discover(dir, Utc::now(), stale),
        JobState::Running { pid: me }
    );
    // An hour later the same heartbeat is stale.
    assert_eq!(
        discover(dir, Utc::now() + chrono::Duration::hours(1), stale),
        JobState::Lost
    );
    write(dir, EXIT_FILE, r#"{"reason":"timeout","code":null}"#);
    assert_eq!(
        discover(dir, Utc::now(), stale),
        JobState::Exited(JobExit {
            reason: ExitReason::Timeout,
            code: None
        })
    );
    write(dir, OUTPUT_FILE, "{}");
    assert_eq!(
        discover(dir, Utc::now(), stale),
        JobState::OutputPresent(dir.join(OUTPUT_FILE))
    );
}

// ─── the coordinator's judging step ─────────────────────────────────────────

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;

use horch_core::competition::judging::{poll, start, JobLauncher, JudgeEnv, JudgingStatus};
use horch_core::competition::model::RoundState;
use horch_core::competition::preflight::PreflightReport;
use horch_core::evaluation::judge_input::blind_labels;
use horch_core::evaluation::judgment::JudgmentRecord;
use horch_core::evaluation::scheduler::JudgeJobSpec;
use horch_core::evaluation::validator::ValidationReport;
use horch_core::evaluation::winner::{WinnerOutcome, WinnerPolicy};
use horch_core::execution::legacy::LedgerRecordV1;
use horch_core::execution::store::{to_execution, ExecutionStore};
use horch_core::execution::{ExecutionKind, ExecutionStatus, FailureKind};
use horch_core::harness::HarnessKind;
use horch_core::ids::{ExecutionId, ExperimentId, ModelId, PaneId, RoundId, TaskId, TeammateName};
use horch_core::measure::digest::{sha256_bytes, Digest};
use horch_core::measure::event::*;
use horch_core::measure::paths::DatasetPaths;
use horch_core::measure::projection::fold;
use horch_core::measure::recorder::{JsonlRecorder, NewEvent, Recorder};
use horch_core::measure::store::StoreOptions;
use horch_core::measure::NumstatLine;
use horch_core::routing::decision::RoutingProvenance;
use horch_core::runtime::{MapEnv, RuntimeContext};
use horch_core::teacher::TeacherRef;
use horch_core::teammates::{Roster, Teammate};
use horch_core::vcs::git::{CheckoutLocation, CherryPick, GitClient, GitIdentity};
use serde_json::{json, Value};

const LABELS: [&str; 2] = ["A", "B"];

fn t0() -> DateTime<Utc> {
    at("2026-10-02T12:00:00Z")
}

fn exp_id() -> ExperimentId {
    ExperimentId::new("0199a5b0-0000-7000-8000-000000000001").unwrap()
}

fn round_id() -> RoundId {
    RoundId::new("0199a5b0-0000-7000-8000-000000000002").unwrap()
}

fn exec_id(label: &str) -> ExecutionId {
    let n = label.bytes().next().unwrap();
    ExecutionId::new(format!("0199a5b0-0000-7000-8000-0000000000{n:02x}")).unwrap()
}

fn typed<T: serde::de::DeserializeOwned>(v: Value) -> T {
    serde_json::from_value(v).unwrap()
}

fn head(label: &str) -> String {
    let c = if label == "A" { 'b' } else { 'c' };
    c.to_string().repeat(40)
}

fn report(label: &str) -> ValidationReport {
    typed(json!({
        "validation_id": "0199a5b0-0000-7000-8000-0000000000f1",
        "label": label,
        "head_sha": head(label),
        "gates": [{"name": "test", "status": {"kind": "passed"}, "duration_ms": 10,
                   "log_ref": format!("validation/{label}/test.log"),
                   "log_digest": sha256_bytes(b"log").to_string(), "log_truncated": false}],
        "mechanical_score": 1.0,
        "eligible": true,
    }))
}

fn judge_teammate() -> Teammate {
    Roster::builtin().unwrap().require("judge").unwrap().clone()
}

/// A git client that only answers `diff_patch`.
struct FakeGit;

impl GitClient for FakeGit {
    fn diff_patch(
        &self,
        _: &Path,
        _: &str,
        head: &str,
        _: usize,
    ) -> anyhow::Result<(String, bool)> {
        Ok((
            format!("diff --git a/src/lib.rs b/src/lib.rs\n+// {head}\n"),
            false,
        ))
    }
    fn toplevel(&self, _: &Path) -> anyhow::Result<PathBuf> {
        unimplemented!()
    }
    fn head(&self, _: &Path) -> anyhow::Result<String> {
        unimplemented!()
    }
    fn current_branch(&self, _: &Path) -> anyhow::Result<Option<String>> {
        unimplemented!()
    }
    fn status_porcelain(&self, _: &Path) -> anyhow::Result<String> {
        unimplemented!()
    }
    fn version(&self) -> anyhow::Result<String> {
        unimplemented!()
    }
    fn worktree_add(&self, _: &Path, _: &Path, _: &str, _: &str) -> anyhow::Result<()> {
        unimplemented!()
    }
    fn worktree_remove(&self, _: &Path, _: &Path, _: bool) -> anyhow::Result<()> {
        unimplemented!()
    }
    fn worktree_list(&self, _: &Path) -> anyhow::Result<Vec<(PathBuf, Option<String>)>> {
        unimplemented!()
    }
    fn commit_all(&self, _: &Path, _: &str, _: &GitIdentity) -> anyhow::Result<Option<String>> {
        unimplemented!()
    }
    fn rev_parse(&self, _: &Path, _: &str) -> anyhow::Result<Option<String>> {
        unimplemented!()
    }
    fn rev_list(&self, _: &Path, _: &str) -> anyhow::Result<Vec<String>> {
        unimplemented!()
    }
    fn diff_numstat(&self, _: &Path, _: &str, _: &str) -> anyhow::Result<Vec<NumstatLine>> {
        unimplemented!()
    }
    fn diff_digest(&self, _: &Path, _: &str, _: &str) -> anyhow::Result<Digest> {
        unimplemented!()
    }
    fn is_ancestor(&self, _: &Path, _: &str, _: &str) -> anyhow::Result<bool> {
        unimplemented!()
    }
    fn update_ref_cas(&self, _: &Path, _: &str, _: &str, _: &str) -> anyhow::Result<bool> {
        unimplemented!()
    }
    fn cherry_pick(&self, _: &Path, _: &str, _: &GitIdentity) -> anyhow::Result<CherryPick> {
        unimplemented!()
    }
    fn merge_ff_only(&self, _: &Path, _: &str) -> anyhow::Result<()> {
        unimplemented!()
    }
    fn branch_checkout_location(&self, _: &Path, _: &str) -> anyhow::Result<CheckoutLocation> {
        unimplemented!()
    }
}

/// What the fake job does when launched.
enum FakeJob {
    /// Writes `job.log`, a heartbeat, `output.json` and `exit.json`.
    Answer(String),
    /// Writes `job.log`, a heartbeat and `exit.json{crash}`.
    Crash,
}

/// Plays the detached job in-process: it writes the job files at once.
struct FakeLauncher {
    script: RefCell<VecDeque<FakeJob>>,
    launched: RefCell<Vec<JudgeJobSpec>>,
}

impl FakeLauncher {
    fn new(jobs: Vec<FakeJob>) -> Self {
        FakeLauncher {
            script: RefCell::new(jobs.into()),
            launched: RefCell::default(),
        }
    }
}

impl JobLauncher for FakeLauncher {
    fn launch(&self, spec: &JudgeJobSpec) -> anyhow::Result<u32> {
        self.launched.borrow_mut().push(spec.clone());
        std::fs::create_dir_all(&spec.job_dir)?;
        let dir = &spec.job_dir;
        write(dir, LOG_FILE, "");
        let hb = Heartbeat {
            pid: std::process::id(),
            at: horch_core::clock::stamp(Utc::now()),
        };
        write(dir, HEARTBEAT_FILE, &serde_json::to_string(&hb)?);
        match self.script.borrow_mut().pop_front() {
            Some(FakeJob::Answer(text)) => {
                write(dir, OUTPUT_FILE, &text);
                write(dir, EXIT_FILE, r#"{"reason":"ok","code":0}"#);
            }
            Some(FakeJob::Crash) => write(dir, EXIT_FILE, r#"{"reason":"crash","code":3}"#),
            None => {}
        }
        Ok(std::process::id())
    }
}

/// A temp world: a state root, a project, a recorder and a store. The
/// sealed (0500) bundle dirs are made writable again before removal.
struct World {
    tmp: tempfile::TempDir,
    ctx: RuntimeContext,
    paths: DatasetPaths,
    recorder: JsonlRecorder,
    store: ExecutionStore,
    judge: Teammate,
    policy: WinnerPolicy,
    tick: RefCell<i64>,
}

impl Drop for World {
    fn drop(&mut self) {
        unseal(self.tmp.path());
    }
}

fn unseal(dir: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700));
                    unseal(&p);
                }
            }
        }
    }
}

impl World {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("state");
        let project = tmp.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let ctx = RuntimeContext::from_env(
            &MapEnv::new(&project)
                .with("HORCH_STATE_DIR", state.to_str().unwrap())
                .with("HORCH_PROJECT_DIR", project.to_str().unwrap()),
        )
        .unwrap();
        let paths = DatasetPaths::new(&state, &project);
        let recorder = JsonlRecorder::open(&paths, StoreOptions::default()).unwrap();
        let store = ExecutionStore::for_project(&state, project.to_str().unwrap());
        World {
            tmp,
            ctx,
            paths,
            recorder,
            store,
            judge: judge_teammate(),
            policy: WinnerPolicy::default(),
            tick: RefCell::new(0),
        }
    }

    fn now(&self) -> DateTime<Utc> {
        let mut t = self.tick.borrow_mut();
        *t += 1;
        t0() + chrono::Duration::milliseconds(*t * 10)
    }

    fn push(&self, kind: EventKind, label: Option<&str>) {
        let key = format!("{}:{}", kind.name(), label.unwrap_or("-"));
        let experiment_level = matches!(
            kind,
            EventKind::ExperimentCreated(_) | EventKind::PreflightCompleted(_)
        );
        self.recorder
            .append(NewEvent {
                kind,
                actor: Actor::Coordinator,
                experiment_id: exp_id(),
                round_id: (!experiment_level).then(round_id),
                execution_id: label.map(exec_id),
                idempotency_key: key,
                occurred_at: self.now(),
            })
            .unwrap();
    }

    /// Events up to JUDGING_BACKGROUND; with `validate_last: false` the last
    /// candidate is frozen but not validated (state VALIDATING).
    fn round(&self, validate_last: bool) {
        self.push(
            EventKind::ExperimentCreated(ExperimentCreated {
                task_id: TaskId::new("task-1").unwrap(),
                task_digest: sha256_bytes(b"task"),
                config_digest: sha256_bytes(b"config"),
                base_sha: "a".repeat(40),
                repo_digest: sha256_bytes(b"repo"),
                environment_digest: sha256_bytes(b"env"),
                candidates: 2,
                strategy: "diverse".into(),
                budget_usd_micro: 5_000_000,
                promote_to: None,
            }),
            None,
        );
        let report: PreflightReport = typed(json!({
            "schema_version": "1.0.0",
            "checks": [{"id": "PRE-01", "status": "pass", "detail": "ok", "measured": null}],
            "safe_n": 2, "waves": 1, "projected_cost_microusd": 1,
            "machine": {"os": "macos", "arch": "aarch64", "cpus": 10,
                        "mem_total_bytes": 1u64, "mem_available_bytes": null,
                        "disk_free_bytes": 1u64, "disk_total_bytes": null,
                        "gpu": "apple_silicon", "max_open_files": 1, "max_processes": null},
            "environment_digest": sha256_bytes(b"env").to_string(),
            "passed": true,
        }));
        self.push(
            EventKind::PreflightCompleted(PreflightCompleted { report }),
            None,
        );
        self.push(
            EventKind::RoundCreated(RoundCreated {
                index: 0,
                base_sha: "a".repeat(40),
                labels: LABELS.iter().map(|l| l.to_string()).collect(),
                eligible_set: Vec::new(),
                propensities: LABELS.iter().map(|l| (l.to_string(), 0.5)).collect(),
                teacher: TeacherRef::none(),
                seed: 42,
                label_policy_version: "lp-1".into(),
            }),
            None,
        );
        for l in LABELS {
            self.push(
                EventKind::CandidatePlanned(CandidatePlanned {
                    label: l.into(),
                    teammate: TeammateName::new("sonnet").unwrap(),
                    harness: HarnessKind::Claude,
                    model: ModelId::new("sonnet").unwrap(),
                    effort: Some("high".into()),
                    slot: SlotKind::Baseline,
                    propensity: 0.5,
                    config_id: "sonnet|claude|sonnet|high".into(),
                }),
                Some(l),
            );
        }
        for l in LABELS {
            self.push(
                EventKind::WorktreeCreated(WorktreeCreated {
                    label: l.into(),
                    path: format!("/wt/{l}").into(),
                    branch: format!("mh/exp/0199a5b0/r0/{l}"),
                    base_sha: "a".repeat(40),
                }),
                Some(l),
            );
        }
        for l in LABELS {
            let routing: RoutingProvenance = typed(json!({
                "requested": "sonnet", "resolved": "sonnet", "fallback_index": null,
                "pool": "claude-max", "pool_state": "ok", "reason": null, "mode": "pinned",
            }));
            self.push(
                EventKind::CandidateSpawned(CandidateSpawned {
                    label: l.into(),
                    pane: PaneId::new(format!("p-{l}")).unwrap(),
                    routing,
                }),
                Some(l),
            );
        }
        for l in LABELS {
            self.push(
                EventKind::CandidateCompleted(CandidateCompleted {
                    label: l.into(),
                    exit_code: Some(0),
                }),
                Some(l),
            );
        }
        for (i, l) in LABELS.iter().enumerate() {
            self.push(
                EventKind::CandidateFrozen(CandidateFrozen {
                    label: l.to_string(),
                    head_sha: head(l),
                    numstat: Vec::new(),
                    diff_digest: sha256_bytes(l.as_bytes()),
                }),
                Some(l),
            );
            if validate_last || i + 1 < LABELS.len() {
                self.validate(l);
            }
        }
    }

    fn validate(&self, l: &str) {
        self.push(
            EventKind::ValidationCompleted(ValidationCompleted {
                label: l.into(),
                report: report(l),
            }),
            Some(l),
        );
    }

    fn env<'a>(
        &'a self,
        launcher: &'a dyn JobLauncher,
        clock: &'a dyn Fn() -> DateTime<Utc>,
    ) -> JudgeEnv<'a> {
        JudgeEnv {
            ctx: &self.ctx,
            recorder: &self.recorder,
            paths: &self.paths,
            git: &FakeGit,
            repo: self.tmp.path(),
            store: &self.store,
            judge: &self.judge,
            task_text: "add a greeting\n",
            policy: &self.policy,
            promotion: PromotionIntent::NotRequested,
            timeout: Duration::from_secs(60),
            stale_after: Duration::from_secs(30),
            launcher,
            clock,
        }
    }

    fn events(&self) -> Vec<horch_core::measure::event::EventEnvelope> {
        self.recorder.read_all().unwrap().events
    }

    fn kinds(&self) -> Vec<String> {
        self.events()
            .iter()
            .map(|e| e.kind.clone())
            .filter(|k| {
                k.starts_with("judge.") || k.starts_with("winner.") || k.starts_with("round.needs")
            })
            .collect()
    }

    fn judge_records(&self) -> Vec<LedgerRecordV1> {
        self.store
            .read()
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.label.as_deref().is_some_and(|l| l.starts_with("judge:")))
            .collect()
    }
}

/// A valid answer that names bundle label `A` the winner.
fn answer() -> String {
    let scores = json!({"correctness": 8, "tests": 7, "scope": 9, "maintainability": 8, "risk": 8});
    json!({
        "schema_version": "1.0.0",
        "verdict": "winner",
        "winner": "A",
        "ranking": ["A", "B"],
        "candidates": {
            "A": {"scores": scores, "acceptable": true, "notes": "good"},
            "B": {"scores": scores, "acceptable": true, "notes": "fine"},
        },
        "confidence": 0.9,
        "rationale": "A is cleaner.",
    })
    .to_string()
}

/// The original label of bundle label `A`.
fn original_of_a() -> String {
    let originals: Vec<String> = LABELS.iter().map(|l| l.to_string()).collect();
    let map: BTreeMap<String, String> = blind_labels(&round_id(), &originals).into_iter().collect();
    map["A"].clone()
}

/// The typed kind of a record, through A6's `to_execution`.
fn typed_kind(r: &LedgerRecordV1) -> Option<ExecutionKind> {
    to_execution(r).ok().map(|e| e.kind)
}

/// JDG-04: the job only answers; the coordinator writes the judgment file
/// once (`create_immutable`, 0600) and emits every judge and winner event.
#[test]
fn jdg_04_parent_writes_judgment_atomically() {
    let w = World::new();
    w.round(true);
    let launcher = FakeLauncher::new(vec![FakeJob::Answer(answer())]);
    let clock = || w.now();
    let env = w.env(&launcher, &clock);

    start(&env, &round_id()).unwrap();
    assert_eq!(launcher.launched.borrow().len(), 1);
    let status = poll(&env, &round_id(), Utc::now()).unwrap();
    let winner = original_of_a();
    assert_eq!(
        status,
        JudgingStatus::Decided(WinnerOutcome::Winner {
            label: winner.clone()
        })
    );
    assert_eq!(
        w.kinds(),
        [
            "judge.scheduled",
            "judge.started",
            "judge.completed",
            "winner.selected"
        ]
    );
    assert!(w
        .events()
        .iter()
        .filter(|e| e.kind.starts_with("judge.") || e.kind.starts_with("winner."))
        .all(|e| e.actor == Actor::Coordinator));

    let file = w.paths.judgement(&round_id()).unwrap();
    let bytes = std::fs::read(&file).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let record: JudgmentRecord = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(record.round_id, round_id());
    assert_eq!(record.attempt, 1);
    let judge = &w.judge_records()[0];
    assert_eq!(record.execution_id.as_str(), judge.record_id);
    let projection = fold(&w.events());
    let round = &projection.rounds[&round_id()];
    assert_eq!(Some(&record.judgment_id), round.judge.judgment_id.as_ref());
    assert_eq!(round.state, RoundState::Decided);
    let selected = round.winner.as_ref().unwrap();
    assert_eq!(selected.label, winner);
    assert_eq!(selected.promotion, PromotionIntent::NotRequested);
    assert_eq!(selected.head_sha, head(&winner));
    assert!(
        projection.anomalies.is_empty(),
        "{:?}",
        projection.anomalies
    );

    // The job dir holds only what the job wrote.
    let job_dir = w.paths.job_dir(&round_id(), 1).unwrap();
    let mut names: Vec<String> = std::fs::read_dir(&job_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, [EXIT_FILE, HEARTBEAT_FILE, LOG_FILE, OUTPUT_FILE]);

    // Re-entry changes nothing: no second event, the same file.
    let before = w.events().len();
    assert_eq!(poll(&env, &round_id(), Utc::now()).unwrap(), status);
    start(&env, &round_id()).unwrap();
    assert_eq!(w.events().len(), before);
    assert_eq!(std::fs::read(&file).unwrap(), bytes);
    assert_eq!(launcher.launched.borrow().len(), 1);
}

/// JDG-04 re-entry: a coordinator that stopped after `judge.completed` and
/// before the judgment file writes the same file on resume, once.
#[test]
fn jdg_04_resume_after_completed_writes_once() {
    let w = World::new();
    w.round(true);
    let launcher = FakeLauncher::new(vec![FakeJob::Answer(answer())]);
    let clock = || w.now();
    let env = w.env(&launcher, &clock);
    start(&env, &round_id()).unwrap();
    poll(&env, &round_id(), Utc::now()).unwrap();
    let file = w.paths.judgement(&round_id()).unwrap();
    let bytes = std::fs::read(&file).unwrap();

    // A second world replays the log up to judge.completed, with the same
    // store and job dir: the projection has a judgment id but no winner.
    let w2 = World::new();
    for e in w.events().iter().filter(|e| e.kind != "winner.selected") {
        w2.recorder
            .append(NewEvent {
                kind: e.event().unwrap(),
                actor: e.actor,
                experiment_id: e.experiment_id.clone(),
                round_id: e.round_id.clone(),
                execution_id: e.execution_id.clone(),
                idempotency_key: e.idempotency_key.clone(),
                occurred_at: horch_core::clock::parse(&e.occurred_at).unwrap(),
            })
            .unwrap();
    }
    for r in w.store.read().unwrap() {
        w2.store.insert(r).unwrap();
    }
    let job_src = w.paths.job_dir(&round_id(), 1).unwrap();
    let job_dst = w2.paths.job_dir(&round_id(), 1).unwrap();
    std::fs::create_dir_all(&job_dst).unwrap();
    for name in [LOG_FILE, HEARTBEAT_FILE, OUTPUT_FILE, EXIT_FILE] {
        std::fs::copy(job_src.join(name), job_dst.join(name)).unwrap();
    }
    let launcher2 = FakeLauncher::new(Vec::new());
    let clock2 = || w2.now();
    let env2 = w2.env(&launcher2, &clock2);
    let status = poll(&env2, &round_id(), Utc::now()).unwrap();
    assert!(matches!(
        status,
        JudgingStatus::Decided(WinnerOutcome::Winner { .. })
    ));
    assert_eq!(
        std::fs::read(w2.paths.judgement(&round_id()).unwrap()).unwrap(),
        bytes
    );
    assert!(launcher2.launched.borrow().is_empty());
    assert_eq!(
        w2.kinds()
            .iter()
            .filter(|k| *k == "judge.completed")
            .count(),
        1
    );
}

/// CMP-12: the judge starts only on a terminal, fully validated set.
#[test]
fn cmp_12_judge_waits_for_terminal_set() {
    let w = World::new();
    w.round(false);
    let launcher = FakeLauncher::new(vec![FakeJob::Answer(answer())]);
    let clock = || w.now();
    let env = w.env(&launcher, &clock);

    let state = fold(&w.events()).rounds[&round_id()].state;
    assert_eq!(state, RoundState::Validating);
    let err = start(&env, &round_id()).unwrap_err().to_string();
    assert!(
        err.contains("waits until every candidate is validated"),
        "{err}"
    );
    assert!(poll(&env, &round_id(), Utc::now()).is_err());
    assert!(w.kinds().is_empty(), "{:?}", w.kinds());
    assert!(launcher.launched.borrow().is_empty());
    assert!(w.judge_records().is_empty());
    assert!(!w.paths.jobs_root().join(round_id().as_str()).exists());

    w.validate(LABELS[LABELS.len() - 1]);
    start(&env, &round_id()).unwrap();
    assert_eq!(w.kinds(), ["judge.scheduled"]);
    assert_eq!(launcher.launched.borrow().len(), 1);
}

/// JDG-09: each attempt is a `kind: judge` execution in the normal store,
/// Starting once launched, then Done or Failed to match the outcome.
#[test]
fn jdg_09_judge_execution_in_ledger() {
    let w = World::new();
    w.round(true);
    let launcher = FakeLauncher::new(vec![FakeJob::Crash, FakeJob::Answer(answer())]);
    let clock = || w.now();
    let env = w.env(&launcher, &clock);

    start(&env, &round_id()).unwrap();
    let records = w.judge_records();
    assert_eq!(records.len(), 1);
    let r = &records[0];
    assert_eq!(
        typed_kind(r),
        Some(ExecutionKind::Judge {
            round: round_id(),
            attempt: 1
        })
    );
    assert_eq!(r.label.as_deref(), Some("judge:1"));
    assert_eq!(r.round_id.as_deref(), Some(round_id().as_str()));
    assert_eq!(r.experiment_id, None);
    assert_eq!(
        (r.tier.as_str(), r.agent.as_str(), r.model.as_str()),
        ("judge", "claude", "opus")
    );
    assert_eq!(r.effort.as_deref(), Some("high"));
    assert_eq!(r.pane_id, None);
    assert_eq!(r.execution_status(), ExecutionStatus::Starting);
    let session = r.session_id.clone().unwrap();
    assert_eq!(launcher.launched.borrow()[0].session.as_str(), session);
    assert_eq!(
        r.workdir.as_deref(),
        Some(launcher.launched.borrow()[0].input_dir.to_str().unwrap())
    );

    // Attempt 1 crashed: recorded as data, retried as attempt 2.
    assert_eq!(
        poll(&env, &round_id(), Utc::now()).unwrap(),
        JudgingStatus::Waiting
    );
    // Attempt 2 answers.
    let status = poll(&env, &round_id(), Utc::now()).unwrap();
    assert!(matches!(
        status,
        JudgingStatus::Decided(WinnerOutcome::Winner { .. })
    ));
    let records = w.judge_records();
    assert_eq!(records.len(), 2);
    assert_eq!(
        records[0].execution_status(),
        ExecutionStatus::Failed {
            failure: FailureKind::AgentExited { code: Some(3) }
        }
    );
    assert_eq!(records[1].execution_status(), ExecutionStatus::Done);
    assert_eq!(
        typed_kind(&records[1]),
        Some(ExecutionKind::Judge {
            round: round_id(),
            attempt: 2
        })
    );
    assert_ne!(records[0].session_id, records[1].session_id);
    // Every judge event names its attempt's execution.
    for e in w.events().iter().filter(|e| e.kind.starts_with("judge.")) {
        let attempt = e.payload["attempt"].as_u64().unwrap() as usize;
        assert_eq!(
            e.execution_id.as_ref().unwrap().as_str(),
            records[attempt - 1].record_id
        );
    }
    let failed: Vec<Value> = w
        .events()
        .iter()
        .filter(|e| e.kind == "judge.failed")
        .map(|e| e.payload.clone())
        .collect();
    assert_eq!(
        failed,
        [json!({"attempt": 1, "cause": {"kind": "crashed", "code": 3}})]
    );
}

/// JDG-08: two failed attempts leave the round in NEEDS_INTERVENTION, with
/// no judgment and no third attempt.
#[test]
fn jdg_08_second_failure_needs_intervention() {
    let w = World::new();
    w.round(true);
    let launcher = FakeLauncher::new(vec![
        FakeJob::Crash,
        FakeJob::Answer("```json\n{}\n```".into()),
    ]);
    let clock = || w.now();
    let env = w.env(&launcher, &clock);
    start(&env, &round_id()).unwrap();
    assert_eq!(
        poll(&env, &round_id(), Utc::now()).unwrap(),
        JudgingStatus::Waiting
    );
    assert_eq!(
        poll(&env, &round_id(), Utc::now()).unwrap(),
        JudgingStatus::NeedsIntervention
    );
    assert_eq!(
        poll(&env, &round_id(), Utc::now()).unwrap(),
        JudgingStatus::NeedsIntervention
    );
    assert_eq!(launcher.launched.borrow().len(), 2);
    let round = &fold(&w.events()).rounds[&round_id()];
    assert_eq!(round.state, RoundState::NeedsIntervention);
    assert!(round.judge.judgment_id.is_none());
    assert!(!w.paths.judgement(&round_id()).unwrap().exists());
    let last = w
        .events()
        .into_iter()
        .rfind(|e| e.kind == "judge.failed")
        .unwrap();
    assert_eq!(last.payload["cause"]["kind"], "malformed");
}

/// JDG-09 for `schedule`: it starts the dataset binary's `judge-job` with the
/// spec's argv, detached in a new session, stdin null, stdout and stderr in
/// `job.log`, the state and project dirs set, and no `ANTHROPIC_API_KEY`.
#[cfg(unix)]
#[test]
fn jdg_09_schedule_detaches_and_strips_env() {
    use horch_core::evaluation::scheduler::{schedule, DATASET_BIN};
    use horch_core::ids::SessionId;

    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let out = tmp.path().join("out");
    let script = bin.join(DATASET_BIN);
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\n\
             printf '%s\\n' \"$@\" > {out}.args\n\
             echo \"key=${{ANTHROPIC_API_KEY+set}} state=$HORCH_STATE_DIR project=$HORCH_PROJECT_DIR\" > {out}.env\n\
             echo $$ > {out}.pid\n\
             ps -o pgid= -p $$ > {out}.pgid 2>/dev/null\n\
             read line && echo stdin-not-null > {out}.stdin\n\
             echo to-stdout\n\
             echo to-stderr >&2\n\
             touch {out}.done\n",
            out = out.display()
        ),
    )
    .unwrap();
    horch_core::runtime::process::make_executable(&script).unwrap();
    let state = tmp.path().join("state");
    let project = tmp.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let ctx = RuntimeContext::from_env(
        &MapEnv::new(&project)
            .with_exe(&script)
            .with("HORCH_STATE_DIR", state.to_str().unwrap())
            .with("HORCH_PROJECT_DIR", project.to_str().unwrap()),
    )
    .unwrap();
    let job_dir = DatasetPaths::new(&state, &project)
        .job_dir(&round_id(), 1)
        .unwrap();
    let spec = JudgeJobSpec {
        round: round_id(),
        attempt: 1,
        input_dir: tmp.path().join("bundle"),
        job_dir: job_dir.clone(),
        session: SessionId::new("0199a5b0-0000-7000-8000-0000000000aa").unwrap(),
        model: "opus".into(),
        effort: "high".into(),
        timeout: Duration::from_secs(90),
    };
    let pid = schedule(&ctx, &spec).unwrap();
    assert!(pid > 0);
    let done = PathBuf::from(format!("{}.done", out.display()));
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !done.exists() {
        assert!(std::time::Instant::now() < deadline, "the job never ran");
        std::thread::sleep(Duration::from_millis(20));
    }
    let read =
        |ext: &str| std::fs::read_to_string(format!("{}.{ext}", out.display())).unwrap_or_default();
    let args: Vec<String> = read("args").lines().map(str::to_owned).collect();
    assert_eq!(args, horch_core::evaluation::scheduler::job_args(&spec));
    assert_eq!(
        read("env").trim(),
        format!(
            "key= state={} project={}",
            state.display(),
            project.display()
        )
    );
    assert_eq!(read("stdin"), "", "stdin is not null");
    // A new session: the job leads its own process group.
    let pgid = read("pgid");
    if !pgid.trim().is_empty() {
        assert_eq!(pgid.trim(), read("pid").trim());
        assert_eq!(read("pid").trim(), pid.to_string());
    }
    let log = std::fs::read_to_string(job_dir.join(LOG_FILE)).unwrap();
    assert!(
        log.contains("to-stdout") && log.contains("to-stderr"),
        "{log}"
    );
}

/// JDG-08: a job that still beats but runs far past its timeout is killed
/// by the coordinator and recorded as `timed_out`.
#[cfg(unix)]
#[test]
fn jdg_08_overdue_job_times_out() {
    let w = World::new();
    w.round(true);
    // The fake job writes only job.log and a heartbeat; a `sleep` child
    // stands in for the stuck job process.
    let launcher = FakeLauncher::new(Vec::new());
    let clock = || w.now();
    let env = w.env(&launcher, &clock);
    start(&env, &round_id()).unwrap();
    let mut stuck = std::process::Command::new("sleep")
        .arg("60")
        .spawn()
        .unwrap();
    let job_dir = w.paths.job_dir(&round_id(), 1).unwrap();
    let beat = |at: DateTime<Utc>| {
        let hb = Heartbeat {
            pid: stuck.id(),
            at: horch_core::clock::stamp(at),
        };
        write(
            &job_dir,
            HEARTBEAT_FILE,
            &serde_json::to_string(&hb).unwrap(),
        );
    };
    beat(Utc::now());
    assert_eq!(
        poll(&env, &round_id(), Utc::now()).unwrap(),
        JudgingStatus::Waiting
    );
    assert!(w.kinds().iter().all(|k| k != "judge.failed"));

    // 10 minutes on, the heartbeat is fresh but the job is overdue
    // (timeout 60 s + 2 × 30 s): killed, failed, retried.
    let later = Utc::now() + chrono::Duration::minutes(10);
    beat(later);
    assert_eq!(
        poll(&env, &round_id(), later).unwrap(),
        JudgingStatus::Waiting
    );
    let failed: Vec<Value> = w
        .events()
        .iter()
        .filter(|e| e.kind == "judge.failed")
        .map(|e| e.payload.clone())
        .collect();
    assert_eq!(
        failed,
        [json!({"attempt": 1, "cause": {"kind": "timed_out"}})]
    );
    assert_eq!(launcher.launched.borrow().len(), 2);
    let status = stuck.wait().unwrap();
    assert!(!status.success(), "the stuck job was not killed");
}
