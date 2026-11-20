//! The check of a task runs only in the sandbox profile of its worker
//! (docs/specs/fleetd/design.md, SR-4, SR-7, "Architecture and roles":
//! the acceptance commands run in the sandbox profile of the task's
//! worker, never with the rights of fleetd; preservation-04-behavior.md
//! row F10; docs/tickets/fleetd-p4/18-check-runs-in-the-worker-sandbox.md).
//!
//! Each test uses a temp ledger, a temp fleet root and temp worktrees. A
//! check runs through the launcher, as the job process runs it. No test
//! reads the load, the limits or the clock of this machine.

#![cfg(unix)]

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use horch_core::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use horch_core::fleetd::fleet_runtime::application::jobs::{
    self, outcome, CheckRunner, JobCtx, JobKey, JobRunner, Queued, SandboxProfiles,
};
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::jobs::{Kind, Outcome, State};
use horch_core::fleetd::ideas::application::needs_you::{open_items, NeedsYouList};
use horch_core::fleetd::task_execution::adapters::task_files::attempt_dir;
use horch_core::fleetd::task_execution::application::dispatch::{
    choices, CheckSandboxes, NO_CHECK_SANDBOX,
};
use horch_core::fleetd::task_execution::application::review;
use horch_core::fleetd::task_execution::domain::dispatch::{Choice, Hold, Holds};
use horch_core::fleetd::worker_profiles::adapters::check_sandbox::{
    WorkerSandboxes, NO_CHECK_SANDBOX_KIND,
};
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::ports::{
    LaunchError, LaunchSpec, Launched, ProcessHost,
};
use horch_core::harness::launch::FORBIDDEN_ENV;
use horch_core::taskledger::finish::{CodexSandbox, Tools};
use horch_core::taskledger::git::Git;
use horch_core::taskledger::model::{Actor, StepName, WorktreeMode};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::{self, NewWorker, Worker};
use horch_core::taskledger::{schema, util};
use horch_platform::NotSupported;
use rusqlite::{params, Connection, TransactionBehavior};

/// The Codex persona of each test.
const CODEX: &str = "codex-dev";
/// A persona of another CLI.
const CLAUDE: &str = "claude-dev";

struct World {
    dir: tempfile::TempDir,
    /// A folder outside each writable path of a check: not in the temp
    /// folder, not in a worktree, not in the fleet root.
    outside: tempfile::TempDir,
    conn: Connection,
    config: FleetConfig,
    fleet_root: PathBuf,
    run: Run,
}

/// A temp ledger of the current schema, a fleet root and 1 run.
fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let mut conn = Connection::open(dir.path().join("ledger.sqlite3")).unwrap();
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    let top = dir.path().canonicalize().unwrap();
    let fleet_root = top.join("fleet");
    let project = top.join("repo");
    std::fs::create_dir_all(&fleet_root).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    let run = runs::create(
        &conn,
        &project,
        &NewRun {
            slug: "r1".into(),
            ..Default::default()
        },
    )
    .unwrap();
    // FR-12: a task needs an idea in flight of the feature of its run.
    horch_core::taskledger::schema::run_idea(
        &conn,
        &run.run_id,
        &horch_core::taskledger::util::now(),
        true,
    )
    .unwrap();
    World {
        dir,
        outside,
        conn,
        config: FleetConfig::default(),
        fleet_root,
        run,
    }
}

impl World {
    /// The adapter of this world, on a host with the sandbox `gap`.
    fn sandboxes(&self, codex: CodexSandbox, gap: Option<NotSupported>) -> WorkerSandboxes {
        WorkerSandboxes {
            codex,
            fleet_root: self.fleet_root.clone(),
            gap,
            refused: Default::default(),
            fallbacks: Default::default(),
        }
    }

    /// The adapter of this world on this host.
    fn host_sandboxes(&self) -> WorkerSandboxes {
        WorkerSandboxes::new(
            self.codex(&self.dir.path().join("codex")),
            self.fleet_root.clone(),
        )
    }

    fn codex(&self, bin: &Path) -> CodexSandbox {
        CodexSandbox {
            bin: bin.to_path_buf(),
            state_root: self.dir.path().join("state"),
            personas: vec![CODEX.into()],
        }
    }

    /// A ready task of `persona` with no worker.
    fn ready(&mut self, title: &str, persona: &str) -> String {
        let new = NewTask::work(title, &format!("{title}.md"), persona);
        tasks::add(&self.conn, &self.run, &new, Actor::Orchestrator)
            .unwrap()
            .task_id
    }

    /// A task with the check `check`, held by attempt 1 of a worker of
    /// `persona` and `harness`. A worktree task gets a worktree folder; a
    /// shared-checkout task works in the project root.
    fn task(&mut self, check: &str, persona: &str, harness: &str, mode: WorktreeMode) -> String {
        let mut new = NewTask::work("check", "check.md", persona);
        new.land_check = Some(check.into());
        new.worktree_mode = mode;
        let task = tasks::add(&self.conn, &self.run, &new, Actor::Orchestrator).unwrap();
        let worker = NewWorker {
            run_id: self.run.run_id.clone(),
            persona: persona.into(),
            harness: Some(harness.into()),
            ..Default::default()
        };
        let worker = workers::insert(&self.conn, &worker).unwrap();
        self.conn
            .execute(
                "UPDATE workers SET task_id = ?1, attempt = 1 WHERE worker_id = ?2",
                params![task.task_id, worker],
            )
            .unwrap();
        self.conn
            .execute(
                "UPDATE tasks SET current_worker_id = ?2 WHERE task_id = ?1",
                params![task.task_id, worker],
            )
            .unwrap();
        if mode == WorktreeMode::Worktree {
            let wt = self.worktree(&task.task_id);
            std::fs::create_dir_all(&wt).unwrap();
            self.conn
                .execute(
                    "INSERT INTO worktrees (worktree_id, task_id, repo_root, path, branch, base,
                        status, origin, created_at)
                     VALUES (?1, ?1, ?2, ?3, 'fleet/t', 'main', 'open', 'native', ?4)",
                    params![
                        task.task_id,
                        self.run.project_root,
                        wt.to_string_lossy(),
                        util::now()
                    ],
                )
                .unwrap();
        }
        task.task_id
    }

    fn worktree(&self, task: &str) -> PathBuf {
        self.dir
            .path()
            .canonicalize()
            .unwrap()
            .join(format!("wt-{task}"))
    }

    /// The folder of attempt 1 of `task`, made.
    fn attempt(&self, task: &str) -> PathBuf {
        let dir = attempt_dir(&self.fleet_root, task, "A1");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Queue the `task_check` job of `task`, start its job process (`true`)
    /// through the launcher, then run the executor as that process, with
    /// the check started through `host` in the profiles `profiles`.
    fn check(
        &mut self,
        task: &str,
        profiles: &dyn SandboxProfiles,
        host: &dyn ProcessHost,
    ) -> (State, Outcome) {
        self.task_check_job(task, "tip1", profiles, host, None)
    }

    fn task_check_job(
        &mut self,
        task: &str,
        input_key: &str,
        profiles: &dyn SandboxProfiles,
        host: &dyn ProcessHost,
        sweep: Option<&Tools>,
    ) -> (State, Outcome) {
        let key = JobKey {
            kind: Kind::TaskCheck,
            subject: task,
            input_key,
        };
        let config = self.config.clone();
        let ctx = JobCtx {
            guard: None,
            config: &config,
            needs_you: &NeedsYouList,
        };
        let id = {
            let tx = self
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .unwrap();
            let q = jobs::queue(&ctx, &tx, &key, Some("/repo"), FleetSeconds(0)).unwrap();
            tx.commit().unwrap();
            match q {
                Queued::New(id) => id,
                other => panic!("expected a new run, got {other:?}"),
            }
        };
        let launcher = Launcher;
        let mut runner = JobRunner::new(&launcher);
        let spec = LaunchSpec {
            program: "true".into(),
            args: Vec::new(),
            cwd: None,
            env_rule: Default::default(),
            detach: false,
            output: None,
        };
        for job in jobs::due_to_start(&self.conn, FleetSeconds(0)).unwrap() {
            assert!(runner
                .start(&ctx, &mut self.conn, &job, &spec, FleetSeconds(0))
                .unwrap());
        }
        let me = self.row(id).pid.unwrap();
        let checks = CheckRunner {
            host,
            profiles,
            sweep,
        };
        let written = jobs::run_job(&ctx, &self.conn, id, me, &checks).unwrap();
        assert!(written.is_some(), "run_job wrote the result of job {id}");
        jobs::end_from_files(&ctx, &mut self.conn).unwrap();
        let row = self.row(id);
        (row.state, outcome(&row).unwrap())
    }

    fn row(&self, id: i64) -> JobRow {
        jobs_store::get(&self.conn, id).unwrap().unwrap()
    }
}

/// A process host that records each spec, then starts it through the
/// launcher of fleetd.
#[derive(Default)]
struct Recording {
    specs: RefCell<Vec<LaunchSpec>>,
}

impl ProcessHost for Recording {
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, LaunchError> {
        self.specs.borrow_mut().push(spec.clone());
        Launcher.launch(spec)
    }
}

/// Why this host has no platform sandbox, or `None` when it has one.
fn host_gap() -> Option<NotSupported> {
    horch_platform::sandbox::host_problem(None)
}

/// The outcome of a check that did not run: no verdict, and the SR-4
/// cause.
fn assert_not_run(state: State, o: &Outcome) {
    assert_eq!(state, State::Failed, "{}", o.text);
    assert!(!o.verdict, "{}", o.text);
    assert!(o.text.starts_with("SR-4: "), "{}", o.text);
}

#[test]
fn check_runs_in_the_worker_sandbox_not_with_fleetd_rights() {
    let mut w = world();
    let escaped = w.outside.path().join("escaped");
    // The test process, with the rights of fleetd, can write there.
    std::fs::write(&escaped, "probe").unwrap();
    std::fs::remove_file(&escaped).unwrap();

    let check = format!("touch '{}'", escaped.display());
    let task = w.task(&check, CLAUDE, "claude", WorktreeMode::Worktree);
    let sandboxes = w.host_sandboxes();
    let (state, o) = w.check(&task, &sandboxes, &Launcher);
    match host_gap() {
        None => {
            assert_eq!(state, State::Failed, "{}", o.text);
            assert!(o.verdict, "the check ran and failed: {}", o.text);
        }
        Some(_) => assert_not_run(state, &o),
    }
    assert!(!escaped.exists(), "the check wrote outside its paths");
}

#[test]
fn codex_worker_check_keeps_codex_sandbox() {
    use std::os::unix::fs::PermissionsExt;
    let mut w = world();
    // A stand-in for `codex`: it records its args and runs nothing.
    let args = w.dir.path().join("codex-args");
    let bin = w.dir.path().join("codex");
    let script = format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n", args.display());
    std::fs::write(&bin, script).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    let sandboxes = w.sandboxes(w.codex(&bin), None);

    let task = w.task("touch ran-outside", CODEX, "codex", WorktreeMode::Worktree);
    let (state, o) = w.check(&task, &sandboxes, &Launcher);
    assert_eq!(state, State::Passed, "{}", o.text);
    let wt = w.worktree(&task);
    let seen = std::fs::read_to_string(&args).expect("the check ran through codex");
    let seen: Vec<&str> = seen.lines().collect();
    assert_eq!(seen[0], "sandbox");
    let dir = seen
        .iter()
        .position(|a| *a == "-C")
        .expect("a project root");
    assert_eq!(seen[dir + 1], wt.to_string_lossy());
    assert!(
        seen.iter().any(|a| a.contains("fleet_write")),
        "the fleet_write profile: {seen:?}"
    );
    assert_eq!(seen[seen.len() - 2..], ["-c", "touch ran-outside"]);
    assert!(
        !wt.join("ran-outside").exists(),
        "no check ran outside codex"
    );
}

#[test]
fn claude_worker_check_runs_in_the_platform_sandbox() {
    let mut w = world();
    let sandboxes = w.host_sandboxes();

    // A worktree task: the check writes its worktree and its attempt
    // folder.
    let task = w.task("true", CLAUDE, "claude", WorktreeMode::Worktree);
    let attempt = w.attempt(&task);
    let check = format!(
        "touch made-here && touch '{}'",
        attempt.join("made").display()
    );
    w.conn
        .execute(
            "UPDATE tasks SET land_check = ?2 WHERE task_id = ?1",
            params![task, check],
        )
        .unwrap();
    let host = Recording::default();
    let (state, o) = w.check(&task, &sandboxes, &host);
    if let Some(gap) = host_gap() {
        assert_not_run(state, &o);
        assert!(host.specs.borrow().is_empty(), "no check started: {gap}");
        return;
    }
    assert_eq!(state, State::Passed, "{}", o.text);
    assert!(w.worktree(&task).join("made-here").exists());
    assert!(attempt.join("made").exists());
    let specs = host.specs.borrow();
    assert_eq!(specs.len(), 1, "1 check started through the host");
    let sandbox_host = if cfg!(target_os = "macos") {
        "sandbox-exec"
    } else {
        "bwrap"
    };
    assert!(
        specs[0].program.ends_with(sandbox_host),
        "the check starts in {sandbox_host}: {:?}",
        specs[0].program
    );
    drop(specs);

    // A shared-checkout task: the project root is its writable path.
    let root = PathBuf::from(&w.run.project_root);
    let task = w.task("touch made-in-root", CLAUDE, "claude", WorktreeMode::Shared);
    let (state, o) = w.check(&task, &sandboxes, &Launcher);
    assert_eq!(state, State::Passed, "{}", o.text);
    assert!(root.join("made-in-root").exists());
}

#[test]
fn not_supported_persona_is_not_dispatched_and_opens_1_item_per_persona() {
    let mut w = world();
    let gap = NotSupported::new("sandbox", "test-os").because("no sandbox host");
    let sandboxes = w.sandboxes(w.codex(Path::new("/no/codex")), Some(gap));
    assert!(sandboxes.supported(CODEX), "codex keeps its own sandbox");
    assert!(!sandboxes.supported(CLAUDE));
    assert!(w.host_sandboxes().supported(CLAUDE) == host_gap().is_none());

    let a = w.ready("a", CLAUDE);
    let b = w.ready("b", CLAUDE);
    let c = w.ready("c", CODEX);
    let chosen = choices(&w.conn, &Holds::default(), &sandboxes, None).unwrap();
    let mut held = Vec::new();
    for choice in chosen {
        match choice {
            Choice::Held { task_id, hold } => held.push((task_id, hold)),
            Choice::Bind { task_id, persona } => {
                assert_eq!((task_id.as_str(), persona.as_str()), (c.as_str(), CODEX));
            }
        }
    }
    let mut ids: Vec<&str> = held.iter().map(|(t, _)| t.as_str()).collect();
    ids.sort();
    let mut want = vec![a.as_str(), b.as_str()];
    want.sort();
    assert_eq!(ids, want, "no task of {CLAUDE} is dispatched");
    for (_, hold) in &held {
        assert_eq!(
            hold,
            &Hold::Persona {
                persona: CLAUDE.into(),
                cause: NO_CHECK_SANDBOX.into(),
            }
        );
    }

    // 2 passes: still 1 item for the persona, not 1 for each task.
    for _ in 0..2 {
        let tx = w
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        sandboxes.open_items(&tx, &NeedsYouList, &held).unwrap();
        tx.commit().unwrap();
    }
    let items: Vec<_> = open_items(&w.conn)
        .unwrap()
        .into_iter()
        .filter(|i| i.kind == NO_CHECK_SANDBOX_KIND)
        .collect();
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0].key, format!("no_check_sandbox:{CLAUDE}"));
    assert!(items[0].text.contains(CLAUDE), "{}", items[0].text);
    assert!(
        items[0].text.contains("no sandbox host"),
        "{}",
        items[0].text
    );
}

#[test]
fn check_job_has_no_forbidden_env() {
    let mut w = world();
    let sandboxes = w.host_sandboxes();
    let task = w.task("env > env.txt", CLAUDE, "claude", WorktreeMode::Worktree);
    let host = Recording::default();
    let (state, o) = w.check(&task, &sandboxes, &host);
    if host_gap().is_some() {
        assert_not_run(state, &o);
        return;
    }
    assert_eq!(state, State::Passed, "{}", o.text);
    let specs = host.specs.borrow();
    assert_eq!(specs.len(), 1, "the check starts through the launcher host");
    for key in FORBIDDEN_ENV {
        assert!(
            !specs[0].env_rule.vars.iter().any(|(k, _)| k == key),
            "the check spec sets {key}"
        );
    }
    let seen = std::fs::read_to_string(w.worktree(&task).join("env.txt")).unwrap();
    assert!(!seen.is_empty(), "the check wrote its environment");
    for key in FORBIDDEN_ENV {
        assert!(
            !seen.lines().any(|l| l.starts_with(&format!("{key}="))),
            "{key} reached the check"
        );
    }
}

fn horch_lines(text: &str) -> Vec<&str> {
    text.lines().filter(|l| l.starts_with("horch:")).collect()
}

#[test]
fn failed_check_keeps_the_escaped_tail_of_its_output_and_names_the_sandbox_rules() {
    let mut w = world();
    let sandboxes = w.host_sandboxes();
    let check = "printf 'first-%s\\n' line; head -c 20000 /dev/zero | tr '\\0' a; \
                 printf '\\nhorch: forg%s\\033[31m\\n' ed >&2; exit 3";
    let task = w.task(check, CLAUDE, "claude", WorktreeMode::Worktree);
    let (state, o) = w.check(&task, &sandboxes, &Launcher);
    if host_gap().is_some() {
        assert_not_run(state, &o);
        return;
    }
    assert_eq!(state, State::Failed, "{}", o.text);
    assert!(o.verdict, "{}", o.text);
    let text = &o.text;
    assert!(text.contains("exit Some(3)"), "{text}");
    assert!(
        text.contains("forged"),
        "the stderr of the check is kept: {text}"
    );
    assert!(
        !text.contains("first-line"),
        "only the tail of the output is kept: {text}"
    );
    assert!(
        text.len() < 10 * 1024,
        "the output is bounded: {} bytes",
        text.len()
    );
    assert!(
        !text.contains('\u{1b}'),
        "a control character is escaped: {text:?}"
    );
    assert!(text.contains(r"\u{1b}"), "{text}");
    let horch = horch_lines(text);
    assert!(
        horch.iter().all(|l| !l.contains("forged")),
        "a line of the check reads as a horch line: {horch:?}"
    );
    assert!(
        horch.iter().any(|l| l.starts_with("horch: SR-3")),
        "{horch:?}"
    );
    assert!(
        horch.iter().any(|l| l.starts_with("horch: SR-4")),
        "{horch:?}"
    );
}

#[test]
fn check_that_writes_outside_its_paths_names_sr4_and_the_refusal() {
    let mut w = world();
    let escaped = w.outside.path().join("escaped");
    let check = format!("echo outside > '{}'", escaped.display());
    let task = w.task(&check, CLAUDE, "claude", WorktreeMode::Worktree);
    let sandboxes = w.host_sandboxes();
    let (state, o) = w.check(&task, &sandboxes, &Launcher);
    if host_gap().is_some() {
        assert_not_run(state, &o);
        return;
    }
    assert_eq!(state, State::Failed, "{}", o.text);
    assert!(
        o.text.contains("Operation not permitted") || o.text.contains("Read-only file system"),
        "the refusal of the sandbox is in the verdict: {}",
        o.text
    );
    assert!(
        horch_lines(&o.text)
            .iter()
            .any(|l| l.starts_with("horch: SR-4")),
        "{}",
        o.text
    );
}

#[test]
fn passed_check_names_no_sandbox_rule() {
    let mut w = world();
    let sandboxes = w.host_sandboxes();
    let task = w.task("echo fine", CLAUDE, "claude", WorktreeMode::Worktree);
    let (state, o) = w.check(&task, &sandboxes, &Launcher);
    if host_gap().is_some() {
        assert_not_run(state, &o);
        return;
    }
    assert_eq!(state, State::Passed, "{}", o.text);
    assert!(horch_lines(&o.text).is_empty(), "{}", o.text);
}

struct NoProfiles;

impl SandboxProfiles for NoProfiles {
    fn check_launch(&self, _: &Worker, _: &Path, _: &str) -> anyhow::Result<Option<LaunchSpec>> {
        Ok(None)
    }
}

fn git(dir: &Path, args: &[&str]) {
    let o = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("LC_ALL", "C")
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {}: {}",
        args.join(" "),
        String::from_utf8_lossy(&o.stderr)
    );
}

impl World {
    fn sweep_tools(&self) -> Tools {
        Tools {
            git: Git::new("git"),
            gh: "gh".into(),
            node: "node".into(),
            complexity_cli: None,
            code_sweep_dir: None,
            evidence_root: self.dir.path().join("evidence"),
            shell: "/bin/sh".into(),
            reviewers_down: Vec::new(),
            codex: CodexSandbox::default(),
            jobs: None,
        }
    }

    fn running_gate(&mut self) -> i64 {
        let config = self.config.clone();
        let ctx = JobCtx {
            guard: None,
            config: &config,
            needs_you: &NeedsYouList,
        };
        let key = JobKey {
            kind: Kind::FastGate,
            subject: "feature-1",
            input_key: "gate1",
        };
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let q = jobs::queue(&ctx, &tx, &key, Some("/repo"), FleetSeconds(0)).unwrap();
        tx.commit().unwrap();
        let Queued::New(id) = q else {
            panic!("expected a new run, got {q:?}");
        };
        let launcher = Launcher;
        let mut runner = JobRunner::new(&launcher);
        let spec = LaunchSpec {
            program: "true".into(),
            args: Vec::new(),
            cwd: None,
            env_rule: Default::default(),
            detach: false,
            output: None,
        };
        for job in jobs::due_to_start(&self.conn, FleetSeconds(0)).unwrap() {
            assert!(runner
                .start(&ctx, &mut self.conn, &job, &spec, FleetSeconds(0))
                .unwrap());
        }
        assert_eq!(self.row(id).state, State::Running);
        id
    }

    fn end_from_files(&mut self) {
        let config = self.config.clone();
        let ctx = JobCtx {
            guard: None,
            config: &config,
            needs_you: &NeedsYouList,
        };
        jobs::end_from_files(&ctx, &mut self.conn).unwrap();
    }
}

fn complexity_key() -> String {
    review::step_input_key(
        "tip1",
        StepName::ComplexitySweep,
        review::COMPLEXITY_COMMAND,
    )
}

#[test]
fn complexity_sweep_build_script_cannot_forge_another_running_job_result() {
    let mut w = world();
    w.conn
        .execute(
            "UPDATE runs SET base_branch = 'main' WHERE run_id = ?1",
            [&w.run.run_id],
        )
        .unwrap();
    let gate = w.running_gate();
    let jobs_dir = w.fleet_root.join("run").join("jobs");
    std::fs::create_dir_all(&jobs_dir).unwrap();
    let forged = jobs_dir.join(format!("{gate}-1.json"));
    let payload =
        format!(r#"{{"job_id":{gate},"run":1,"passed":true,"verdict":true,"text":"forged"}}"#);

    let task = w.task("true", CLAUDE, "claude", WorktreeMode::Worktree);
    let wt = w.worktree(&task);
    git(&wt, &["init", "--quiet", "-b", "main"]);
    for (k, v) in [
        ("user.name", "t"),
        ("user.email", "t@example.com"),
        ("commit.gpgsign", "false"),
    ] {
        git(&wt, &["config", k, v]);
    }
    std::fs::create_dir_all(wt.join("src")).unwrap();
    std::fs::write(
        wt.join("Cargo.toml"),
        "[package]\nname = \"fx\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
    )
    .unwrap();
    std::fs::write(wt.join(".gitignore"), "target/\nbuild-ran\n").unwrap();
    std::fs::write(wt.join("src/lib.rs"), "pub fn a() -> u32 {\n    1\n}\n").unwrap();
    git(&wt, &["add", "."]);
    git(&wt, &["commit", "--quiet", "-m", "base"]);
    git(&wt, &["checkout", "--quiet", "-b", "fleet/t"]);
    std::fs::write(wt.join("src/lib.rs"), "pub fn a() -> u32 {\n    2\n}\n").unwrap();
    let build = format!(
        "fn main() {{\n    let dir = std::env::var(\"CARGO_MANIFEST_DIR\").unwrap();\n    \
         std::fs::write(format!(\"{{dir}}/build-ran\"), \"1\").unwrap();\n    \
         use std::io::Write;\n    use std::os::unix::fs::OpenOptionsExt;\n    \
         let f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open({forged:?});\n    \
         if let Ok(mut f) = f {{\n        let _ = f.write_all({payload:?}.as_bytes());\n    }}\n}}\n"
    );
    std::fs::write(wt.join("build.rs"), build).unwrap();
    git(&wt, &["add", "."]);
    git(&wt, &["commit", "--quiet", "-m", "work"]);

    let tools = w.sweep_tools();
    let sandboxes = w.host_sandboxes();
    let key = complexity_key();
    let (state, o) = w.task_check_job(&task, &key, &sandboxes, &Launcher, Some(&tools));
    match host_gap() {
        None => {
            assert_eq!(state, State::Passed, "{}", o.text);
            assert!(o.verdict, "{}", o.text);
            assert!(
                wt.join("build-ran").exists(),
                "the build script ran: {}",
                o.text
            );
            let evidence = tools.evidence_root.join(&task);
            let clippy: Vec<String> = std::fs::read_dir(&evidence)
                .unwrap()
                .flat_map(|d| std::fs::read_dir(d.unwrap().path()).unwrap())
                .map(|e| e.unwrap().path())
                .filter(|p| p.to_string_lossy().ends_with("-clippy.json"))
                .map(|p| std::fs::read_to_string(p).unwrap())
                .collect();
            assert_eq!(clippy.len(), 1, "1 clippy report");
            assert!(
                clippy[0].contains("\"reason\":\"build-finished\""),
                "the report holds the stdout of clippy: {}",
                clippy[0]
            );
        }
        Some(_) => assert_not_run(state, &o),
    }
    let archived = jobs_dir.join("archive").join(format!("{gate}-1.json"));
    assert!(
        !forged.exists() && !archived.exists(),
        "the build script wrote a result file for job {gate}"
    );
    w.end_from_files();
    assert_eq!(
        w.row(gate).state,
        State::Running,
        "the other running gate job is still running"
    );

    let task = w.task("true", CLAUDE, "claude", WorktreeMode::Worktree);
    let (state, o) = w.task_check_job(&task, &key, &NoProfiles, &Launcher, Some(&tools));
    assert_not_run(state, &o);
}
