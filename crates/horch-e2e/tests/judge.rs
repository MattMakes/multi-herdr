//! JDG-01: the judge is headless only, so `horch spawn judge` is refused
//! before anything touches herdr or the ledger.

use horch_e2e::harness::{repo_root, Harness};

#[test]
fn jdg_01_spawn_judge_refused() {
    let mut h = Harness::new("jdg01");
    h.set("HORCH_WORKSPACE_ID", "w1");
    let out = h.run(&["spawn", "judge", "x", "--from-pane", "w1:p1", "--no-tile"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "spawn judge succeeded\n{stderr}");
    assert!(stderr.contains("HEADLESS_ONLY"), "{stderr}");
    assert!(stderr.contains("'judge' is headless only"), "{stderr}");
    assert!(h.calls_of("herdr").is_empty(), "{:?}", h.calls());
    assert!(h.calls_of("claude").is_empty(), "{:?}", h.calls());
    assert!(h.state_files().is_empty(), "{:?}", h.state_files());
}

// ─── B4 judge job (U31): the real judge-job binary and fake-claude ─────────
//
// The coordinator side runs in-process (`competition::judging::start` and
// `poll`) against a seeded event log. Its launcher runs the real
// `multi-herdr-dataset judge-job` in the sealed harness environment and
// waits for it, so each `poll` sees a finished job. The full round through
// `run` and `resume` comes with the B3 coordinator.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use horch_core::competition::judging::{poll, start, JobLauncher, JudgeEnv, JudgingStatus};
use horch_core::competition::preflight::PreflightReport;
use horch_core::evaluation::scheduler::{job_args, JudgeJobSpec, EXIT_FILE, LOG_FILE};
use horch_core::evaluation::validator::ValidationReport;
use horch_core::evaluation::winner::{WinnerOutcome, WinnerPolicy};
use horch_core::execution::store::{to_execution, ExecutionStore};
use horch_core::execution::{ExecutionKind, SessionState};
use horch_core::harness::HarnessKind;
use horch_core::ids::{ExecutionId, ExperimentId, ModelId, PaneId, RoundId, TaskId, TeammateName};
use horch_core::measure::digest::{sha256_bytes, Digest};
use horch_core::measure::event::*;
use horch_core::measure::paths::DatasetPaths;
use horch_core::measure::recorder::{JsonlRecorder, NewEvent, Recorder};
use horch_core::measure::store::StoreOptions;
use horch_core::measure::NumstatLine;
use horch_core::roster::{Roster, Teammate};
use horch_core::routing::decision::RoutingProvenance;
use horch_core::runtime::{MapEnv, RuntimeContext};
use horch_core::teacher::TeacherRef;
use horch_core::vcs::git::{CheckoutLocation, CherryPick, GitClient, GitIdentity};
use horch_e2e::bin_dir;
use serde_json::{json, Value};

const LABELS: [&str; 2] = ["A", "B"];

fn dataset_bin() -> PathBuf {
    let path = bin_dir().join(format!(
        "multi-herdr-dataset{}",
        std::env::consts::EXE_SUFFIX
    ));
    assert!(
        path.is_file(),
        "{} is not built; run `cargo build --workspace --bins` first",
        path.display()
    );
    path
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

fn head(label: &str) -> String {
    let c = if label == "A" { 'b' } else { 'c' };
    c.to_string().repeat(40)
}

/// Only `diff_patch` is used: the bundle's patches.
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
            format!("diff --git a/src/lib.rs b/src/lib.rs\n+// change {head}\n"),
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
    fn branch_checkout_location(&self, _: &Path, _: &str) -> anyhow::Result<CheckoutLocation> {
        unimplemented!()
    }
}

/// Runs the real `judge-job` in the sealed environment, as `schedule` would
/// (job dir, `job.log`, the same argv), and waits for it. With `run: false`
/// it only records the spec.
struct SealedJob<'h> {
    h: &'h Harness,
    run: bool,
    specs: RefCell<Vec<JudgeJobSpec>>,
}

impl SealedJob<'_> {
    fn exec(&self, spec: &JudgeJobSpec) -> std::process::ExitStatus {
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(spec.job_dir.join(LOG_FILE))
            .unwrap();
        let mut cmd = Command::new(dataset_bin());
        cmd.args(job_args(spec));
        self.h.seal(&mut cmd);
        cmd.stdout(log.try_clone().unwrap()).stderr(log);
        cmd.status().expect("running judge-job")
    }
}

impl JobLauncher for SealedJob<'_> {
    fn launch(&self, spec: &JudgeJobSpec) -> anyhow::Result<u32> {
        self.specs.borrow_mut().push(spec.clone());
        std::fs::create_dir_all(&spec.job_dir)?;
        std::fs::File::create(spec.job_dir.join(LOG_FILE))?;
        if self.run {
            self.exec(spec);
        }
        Ok(0)
    }
}

/// A sealed harness with a dataset round in JUDGING_BACKGROUND.
struct JudgeWorld {
    h: Harness,
    ctx: RuntimeContext,
    paths: DatasetPaths,
    recorder: JsonlRecorder,
    store: ExecutionStore,
    judge: Teammate,
    policy: WinnerPolicy,
    tick: RefCell<i64>,
}

impl Drop for JudgeWorld {
    fn drop(&mut self) {
        unseal(&self.h.state);
    }
}

/// Make the sealed (0500) bundle dirs writable again so the harness can
/// remove its temp dir.
fn unseal(dir: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() && !p.is_symlink() {
                    let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700));
                    unseal(&p);
                }
            }
        }
    }
}

impl JudgeWorld {
    /// `attempts`: what fake-claude does on each judge call.
    fn new(name: &str, attempts: &[&str]) -> Self {
        let h = Harness::new(name);
        std::fs::write(
            format!("{}.judge.json", h.log.display()),
            json!({ "attempts": attempts }).to_string(),
        )
        .unwrap();
        let ctx = RuntimeContext::from_env(
            &MapEnv::new(&h.project)
                .with_exe(dataset_bin())
                .with("HOME", h.home.to_str().unwrap())
                .with("PATH", h.bin.to_str().unwrap())
                .with("HORCH_STATE_DIR", h.state.to_str().unwrap())
                .with("HORCH_PROJECT_DIR", h.project.to_str().unwrap())
                .with("HORCH_CLAUDE_BIN", h.bin.join("claude").to_str().unwrap()),
        )
        .unwrap();
        let paths = DatasetPaths::new(&h.state, &h.project);
        let recorder = JsonlRecorder::open(&paths, StoreOptions::default()).unwrap();
        let store = ExecutionStore::for_project(&h.state, h.project.to_str().unwrap());
        let judge = Roster::load_layered(None, Some(&repo_root().join("teammates")), None)
            .unwrap()
            .require("judge")
            .unwrap()
            .clone();
        let w = JudgeWorld {
            h,
            ctx,
            paths,
            recorder,
            store,
            judge,
            policy: WinnerPolicy::default(),
            tick: RefCell::new(0),
        };
        w.seed();
        w
    }

    /// The next tick: 2026-10-02T12:00:00Z plus 10 ms per event, so
    /// `occurred_at` is monotonic.
    fn next_ms(&self) -> i64 {
        let mut t = self.tick.borrow_mut();
        *t += 1;
        1_790_942_400_000 + *t * 10
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
                occurred_at: horch_core::clock::from_epoch_ms(self.next_ms()).unwrap(),
            })
            .unwrap();
    }

    fn seed(&self) {
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
        let report = serde_json::from_value::<PreflightReport>(json!({
            "schema_version": "1.0.0",
            "checks": [{"id": "PRE-01", "status": "pass", "detail": "ok", "measured": null}],
            "safe_n": 2, "waves": 1, "projected_cost_microusd": 1,
            "machine": {"os": "macos", "arch": "aarch64", "cpus": 10,
                        "mem_total_bytes": 1u64, "mem_available_bytes": null,
                        "disk_free_bytes": 1u64, "disk_total_bytes": null,
                        "gpu": "apple_silicon", "max_open_files": 1, "max_processes": null},
            "environment_digest": sha256_bytes(b"env").to_string(),
            "passed": true,
        }))
        .unwrap();
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
            let routing = serde_json::from_value::<RoutingProvenance>(json!({
                "requested": "sonnet", "resolved": "sonnet", "fallback_index": null,
                "pool": "claude-max", "pool_state": "ok", "reason": null, "mode": "pinned",
            }))
            .unwrap();
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
        for l in LABELS {
            self.push(
                EventKind::CandidateFrozen(CandidateFrozen {
                    label: l.into(),
                    head_sha: head(l),
                    numstat: Vec::new(),
                    diff_digest: sha256_bytes(l.as_bytes()),
                }),
                Some(l),
            );
            let report = serde_json::from_value::<ValidationReport>(json!({
                "validation_id": "0199a5b0-0000-7000-8000-0000000000f1",
                "label": l,
                "head_sha": head(l),
                "gates": [{"name": "test", "status": {"kind": "passed"}, "duration_ms": 10,
                           "log_ref": format!("validation/{l}/test.log"),
                           "log_digest": sha256_bytes(b"log").to_string(),
                           "log_truncated": false}],
                "mechanical_score": 1.0,
                "eligible": true,
            }))
            .unwrap();
            self.push(
                EventKind::ValidationCompleted(ValidationCompleted {
                    label: l.into(),
                    report,
                }),
                Some(l),
            );
        }
    }

    fn launcher(&self, run: bool) -> SealedJob<'_> {
        SealedJob {
            h: &self.h,
            run,
            specs: RefCell::default(),
        }
    }

    fn events(&self) -> Vec<EventEnvelope> {
        self.recorder.read_all().unwrap().events
    }

    fn payloads(&self, kind: &str) -> Vec<Value> {
        self.events()
            .into_iter()
            .filter(|e| e.kind == kind)
            .map(|e| e.payload)
            .collect()
    }

    /// The judge calls of fake-claude, in call order.
    fn judge_calls(&self) -> Vec<Value> {
        self.h
            .calls_of("claude")
            .into_iter()
            .filter(|c| c["mode"] == "judge")
            .collect()
    }

    fn judge_sessions(&self) -> Vec<String> {
        self.store
            .read()
            .unwrap()
            .iter()
            .filter_map(|r| to_execution(r).ok())
            .filter(|e| matches!(e.kind, ExecutionKind::Judge { .. }))
            .filter_map(|e| match e.session {
                SessionState::Known(s) => Some(s.to_string()),
                _ => None,
            })
            .collect()
    }
}

fn value_after(argv: &Value, flag: &str) -> Option<String> {
    let argv: Vec<&str> = argv.as_array()?.iter().filter_map(Value::as_str).collect();
    argv.windows(2)
        .find(|w| w[0] == flag)
        .map(|w| w[1].to_string())
}

/// `let $env = JudgeEnv { .. }` over world `$w`, with a monotonic clock
/// that continues the seeded events' ticks.
macro_rules! judge_env {
    ($env:ident = $w:expr, $launcher:expr, $timeout:expr) => {
        let clock = || horch_core::clock::from_epoch_ms($w.next_ms()).unwrap();
        let $env = JudgeEnv {
            ctx: &$w.ctx,
            recorder: &$w.recorder,
            paths: &$w.paths,
            git: &FakeGit,
            repo: &$w.h.project,
            store: &$w.store,
            judge: &$w.judge,
            task_text: "add a greeting\n",
            policy: &$w.policy,
            promotion: PromotionIntent::NotRequested,
            timeout: $timeout,
            stale_after: Duration::from_secs(30),
            launcher: $launcher,
            clock: &clock,
        };
    };
}

/// Path → sha256 of the bytes, for every file under `dir`.
fn snapshot(dir: &Path) -> BTreeMap<PathBuf, String> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if let Ok(bytes) = std::fs::read(&p) {
                out.insert(p, sha256_bytes(&bytes).to_string());
            }
        }
    }
    out
}

/// JDG-04: the job writes only `jobs/<round>/judge-<n>/`. Nothing else in
/// the dataset dir or the project changes while it runs.
#[test]
fn jdg_04_job_writes_only_job_dir() {
    let w = JudgeWorld::new("jdg04job", &["valid"]);
    let launcher = w.launcher(false);
    judge_env!(env = w, &launcher, Duration::from_secs(60));
    start(&env, &round_id()).unwrap();
    let spec = launcher.specs.borrow()[0].clone();

    let state_before = snapshot(&w.h.state);
    let project_before = snapshot(&w.h.project);
    let status = launcher.exec(&spec);
    assert!(
        status.success(),
        "{}",
        std::fs::read_to_string(spec.job_dir.join(LOG_FILE)).unwrap()
    );
    let state_after = snapshot(&w.h.state);
    assert_eq!(snapshot(&w.h.project), project_before);

    let changed: Vec<&PathBuf> = state_after
        .iter()
        .filter(|(p, d)| state_before.get(*p) != Some(d))
        .map(|(p, _)| p)
        .chain(
            state_before
                .keys()
                .filter(|p| !state_after.contains_key(*p)),
        )
        .collect();
    assert!(!changed.is_empty());
    for p in &changed {
        assert!(
            p.starts_with(&spec.job_dir),
            "the job wrote {}",
            p.display()
        );
    }
    let mut names: Vec<String> = std::fs::read_dir(&spec.job_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["exit.json", "heartbeat", "job.log", "output.json"]);
    let exit: Value =
        serde_json::from_str(&std::fs::read_to_string(spec.job_dir.join(EXIT_FILE)).unwrap())
            .unwrap();
    assert_eq!(exit, json!({"reason": "ok", "code": 0}));
    // The job emitted no event: the coordinator is the single authority.
    let kinds: Vec<String> = w.events().iter().map(|e| e.kind.clone()).collect();
    assert_eq!(kinds.iter().filter(|k| k.starts_with("judge.")).count(), 1);

    // The coordinator then decides from the job's output.
    let status = poll(&env, &round_id(), horch_core::clock::now()).unwrap();
    assert!(
        matches!(status, JudgingStatus::Decided(WinnerOutcome::Winner { .. })),
        "{status:?}"
    );
    assert!(w.paths.judgement(&round_id()).unwrap().is_file());
    assert!(w.h.violations().is_empty(), "{:?}", w.h.violations());
}

/// JDG-08: a crashed attempt is recorded as data and retried once; the
/// retry's answer decides the round.
#[test]
fn jdg_08_crash_records_and_retries() {
    let w = JudgeWorld::new("jdg08crash", &["crash", "valid"]);
    let launcher = w.launcher(true);
    judge_env!(env = w, &launcher, Duration::from_secs(60));
    start(&env, &round_id()).unwrap();
    let job1 = w.paths.job_dir(&round_id(), 1).unwrap();
    let exit: Value =
        serde_json::from_str(&std::fs::read_to_string(job1.join(EXIT_FILE)).unwrap()).unwrap();
    assert_eq!(exit, json!({"reason": "crash", "code": 3}));

    assert_eq!(
        poll(&env, &round_id(), horch_core::clock::now()).unwrap(),
        JudgingStatus::Waiting
    );
    let status = poll(&env, &round_id(), horch_core::clock::now()).unwrap();
    assert!(
        matches!(status, JudgingStatus::Decided(WinnerOutcome::Winner { .. })),
        "{status:?}"
    );

    assert_eq!(
        w.payloads("judge.failed"),
        [json!({"attempt": 1, "cause": {"kind": "crashed", "code": 3}})]
    );
    let attempts: Vec<u64> = w
        .payloads("judge.scheduled")
        .iter()
        .map(|p| p["attempt"].as_u64().unwrap())
        .collect();
    assert_eq!(attempts, [1, 2]);
    assert_eq!(w.payloads("judge.completed")[0]["attempt"], 2);
    assert_eq!(w.payloads("winner.selected").len(), 1);
    let calls = w.judge_calls();
    assert_eq!(calls.len(), 2);
    assert_ne!(
        value_after(&calls[0]["argv"], "--session-id"),
        value_after(&calls[1]["argv"], "--session-id")
    );
    assert!(w.h.violations().is_empty(), "{:?}", w.h.violations());
}

/// JDG-08: a judge that hangs is killed at the timeout, recorded as
/// `timed_out`, and retried.
#[test]
fn jdg_08_timeout() {
    let w = JudgeWorld::new("jdg08timeout", &["hang", "valid"]);
    let launcher = w.launcher(true);
    judge_env!(env = w, &launcher, Duration::from_secs(1));
    let started = std::time::Instant::now();
    start(&env, &round_id()).unwrap();
    assert!(started.elapsed() < Duration::from_secs(30));
    let job1 = w.paths.job_dir(&round_id(), 1).unwrap();
    let exit: Value =
        serde_json::from_str(&std::fs::read_to_string(job1.join(EXIT_FILE)).unwrap()).unwrap();
    assert_eq!(exit["reason"], "timeout");
    assert!(!job1.join("output.json").exists());
    // The hung judge is gone.
    let hung = &w.judge_calls()[0];
    let pid = hung["pid"].as_u64().unwrap() as u32;
    assert!(
        !horch_core::fsx::pid_alive(pid),
        "the hung judge {pid} still runs"
    );

    assert_eq!(
        poll(&env, &round_id(), horch_core::clock::now()).unwrap(),
        JudgingStatus::Waiting
    );
    assert_eq!(
        w.payloads("judge.failed"),
        [json!({"attempt": 1, "cause": {"kind": "timed_out"}})]
    );
    let status = poll(&env, &round_id(), horch_core::clock::now()).unwrap();
    assert!(
        matches!(status, JudgingStatus::Decided(WinnerOutcome::Winner { .. })),
        "{status:?}"
    );
}

/// SEC-06 in the job: an answer over 1 MiB is never written; the attempt is
/// `over_cap`.
#[test]
fn jdg_08_oversize_answer_is_over_cap() {
    let w = JudgeWorld::new("jdg08oversize", &["oversize", "invalid"]);
    let launcher = w.launcher(true);
    judge_env!(env = w, &launcher, Duration::from_secs(60));
    start(&env, &round_id()).unwrap();
    let job1 = w.paths.job_dir(&round_id(), 1).unwrap();
    assert!(!job1.join("output.json").exists());
    assert_eq!(
        poll(&env, &round_id(), horch_core::clock::now()).unwrap(),
        JudgingStatus::Waiting
    );
    assert_eq!(
        poll(&env, &round_id(), horch_core::clock::now()).unwrap(),
        JudgingStatus::NeedsIntervention
    );
    let causes: Vec<Value> = w
        .payloads("judge.failed")
        .iter()
        .map(|p| p["cause"]["kind"].clone())
        .collect();
    assert_eq!(causes, [json!("over_cap"), json!("malformed")]);
    assert!(!w.paths.judgement(&round_id()).unwrap().exists());
}

/// JDG-09: headless `claude -p`, read-only tools, the minted session id,
/// and no `ANTHROPIC_API_KEY` although the parent has one.
#[test]
fn jdg_09_argv_and_env() {
    let mut w = JudgeWorld::new("jdg09", &["valid"]);
    w.h.set("ANTHROPIC_API_KEY", "SENTINEL");
    let launcher = w.launcher(true);
    judge_env!(env = w, &launcher, Duration::from_secs(60));
    start(&env, &round_id()).unwrap();
    poll(&env, &round_id(), horch_core::clock::now()).unwrap();

    let calls = w.judge_calls();
    assert_eq!(calls.len(), 1);
    let call = &calls[0];
    let argv = &call["argv"];
    assert_eq!(argv[0], "-p");
    assert_eq!(
        value_after(argv, "--output-format").as_deref(),
        Some("json")
    );
    assert_eq!(
        value_after(argv, "--tools").as_deref(),
        Some("Read,Grep,Glob")
    );
    assert_eq!(
        value_after(argv, "--allowedTools").as_deref(),
        Some("Read,Grep,Glob")
    );
    assert_eq!(
        value_after(argv, "--disallowedTools").as_deref(),
        Some("Agent,Edit,Write,NotebookEdit,Bash")
    );
    assert_eq!(value_after(argv, "--model").as_deref(), Some("opus"));
    assert_eq!(value_after(argv, "--effort").as_deref(), Some("high"));
    assert_eq!(
        value_after(argv, "--session-id"),
        w.judge_sessions().first().cloned()
    );
    let env_keys: Vec<&str> = call["env_keys"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(!env_keys.contains(&"ANTHROPIC_API_KEY"), "{env_keys:?}");
    assert_eq!(call["prompt_has_rubric"], true);
    assert!(w.h.violations().is_empty(), "{:?}", w.h.violations());
}

/// SEC-04: the judge runs in the bundle dir, with read-only tools, and the
/// bundle files are 0400 in 0500 dirs.
#[test]
fn sec_04_judge_cwd_bundle_tools_readonly() {
    let w = JudgeWorld::new("sec04", &["valid"]);
    let launcher = w.launcher(true);
    judge_env!(env = w, &launcher, Duration::from_secs(60));
    start(&env, &round_id()).unwrap();

    let bundle = w.paths.judge_input_dir(&exp_id(), &round_id()).unwrap();
    assert_eq!(launcher.specs.borrow()[0].input_dir, bundle);
    let call = &w.judge_calls()[0];
    let cwd = PathBuf::from(call["cwd"].as_str().unwrap());
    assert_eq!(
        std::fs::canonicalize(&cwd).unwrap(),
        std::fs::canonicalize(&bundle).unwrap()
    );
    let denied = value_after(&call["argv"], "--disallowedTools").unwrap();
    for tool in ["Edit", "Write", "NotebookEdit", "Bash", "Agent"] {
        assert!(denied.split(',').any(|t| t == tool), "{denied}");
    }
    assert!(call["argv"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a == "--strict-mcp-config"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let files = snapshot(&bundle);
        assert!(files.len() >= 5, "{files:?}");
        for f in files.keys() {
            assert_eq!(mode(f), 0o400, "{}", f.display());
        }
        assert_eq!(mode(&bundle), 0o500);
        assert_eq!(mode(&bundle.join("candidates")), 0o500);
    }
}

/// SEC-08 (judge part): `ANTHROPIC_API_KEY=SENTINEL` in the parent reaches
/// no child, and no file under the state dir holds it.
#[test]
fn sec_08_e2e_no_api_key_in_judge_job() {
    let mut w = JudgeWorld::new("sec08judge", &["crash", "valid"]);
    w.h.set("ANTHROPIC_API_KEY", "SENTINEL");
    let launcher = w.launcher(true);
    judge_env!(env = w, &launcher, Duration::from_secs(60));
    start(&env, &round_id()).unwrap();
    while poll(&env, &round_id(), horch_core::clock::now()).unwrap() == JudgingStatus::Waiting {}

    for call in w.h.calls() {
        let keys = call["env_keys"].as_array().cloned().unwrap_or_default();
        assert!(
            !keys.iter().any(|k| k == "ANTHROPIC_API_KEY"),
            "{} got the key",
            call["fake"]
        );
    }
    assert_eq!(w.judge_calls().len(), 2);
    for (path, _) in snapshot(&w.h.state) {
        let bytes = std::fs::read(&path).unwrap();
        assert!(
            !bytes.windows(8).any(|win| win == b"SENTINEL"),
            "{} holds the key",
            path.display()
        );
    }
    assert!(w.h.violations().is_empty(), "{:?}", w.h.violations());
}

/// CMP-16: the judge job is the only detached process. The source holds a
/// detached spawn (`setsid`, `process_group`, `spawn_detached`) in exactly
/// these files, each for a stated reason:
/// - `runtime/process.rs` defines `spawn_detached`;
/// - `workspace/arrange.rs` detaches the grid tidy a closing pane leaves;
/// - `evaluation/validator.rs` puts each gate in its own process group to
///   kill it at its timeout, and waits for it (not detached);
/// - `horch-e2e/src/bin/fake-herdr.rs` is a test fake;
/// - `evaluation/scheduler.rs` starts the judge job.
///
/// No competition, measure or dataset command code detaches anything.
#[test]
fn cmp_16_only_judge_detached() {
    let root = repo_root();
    let allowed = [
        "crates/horch-core/src/runtime/process.rs",
        "crates/horch-core/src/workspace/arrange.rs",
        "crates/horch-core/src/evaluation/validator.rs",
        "crates/horch-e2e/src/bin/fake-herdr.rs",
        "crates/horch-core/src/evaluation/scheduler.rs",
    ];
    let mut found = Vec::new();
    let mut stack = vec![root.join("crates")];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                if p.file_name().is_some_and(|n| n != "target" && n != "tests") {
                    stack.push(p);
                }
            } else if p.extension().is_some_and(|x| x == "rs") {
                let text = std::fs::read_to_string(&p).unwrap();
                if ["setsid", "process_group", "spawn_detached"]
                    .iter()
                    .any(|t| text.contains(t))
                {
                    found.push(
                        p.strip_prefix(&root)
                            .unwrap()
                            .to_string_lossy()
                            .replace('\\', "/"),
                    );
                }
            }
        }
    }
    found.sort();
    let mut want: Vec<String> = allowed.iter().map(|s| s.to_string()).collect();
    want.sort();
    assert_eq!(found, want);
    let scheduler =
        std::fs::read_to_string(root.join("crates/horch-core/src/evaluation/scheduler.rs"))
            .unwrap();
    assert!(scheduler.contains("spawn_detached(&mut cmd)"));
    for dir in [
        "crates/horch-core/src/competition",
        "crates/horch-core/src/measure",
        "crates/horch/src/dataset",
    ] {
        assert!(!found.iter().any(|f| f.starts_with(dir)), "{dir}");
    }
}
