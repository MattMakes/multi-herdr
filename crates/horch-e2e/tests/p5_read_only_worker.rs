#![cfg(target_os = "macos")]

use std::io::{BufRead as _, BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Output, Stdio};

use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::task_execution::adapters::task_files::attempt_dir;
use horch_core::fleetd::worktree_custody::adapters::sqlite::{self as trees, NewWorktree};
use horch_core::harness::{CommandSpec, HarnessKind, LaunchEnv, PrepareRequest, Prepared, Session};
use horch_core::roster::Roster;
use horch_core::runtime::{MapEnv, RuntimeContext};
use horch_core::taskledger::model::{Actor, WorkerKind, WorktreeStatus};
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::NewWorker;
use horch_core::taskledger::{schema, Store};
use horch_e2e::fleetd::Fleetds;
use horch_e2e::harness::{wait_for_or, Harness};
use serde_json::Value;

const PROFILE: &str = "codex-reviewer-light";

const PROGRESS: &str = "fake codex: step 1 is done";
const DONE: &str = "fake codex: all steps are done";
const RESULT: &str = "fake codex wrote its result";

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

struct World {
    fleetds: Fleetds,
    h: Harness,
    task_id: String,
    worktree: PathBuf,
}

impl World {
    fn new(name: &str) -> World {
        let mut h = Harness::new(name).with_git();
        std::os::unix::fs::symlink(Harness::horch_bin(), h.bin.join("horch")).unwrap();
        let fleet = h.root.join("fleet");
        std::fs::create_dir_all(&fleet).unwrap();
        std::fs::create_dir_all(h.home.join(".codex")).unwrap();
        h.set(
            "HORCH_LEDGER_DB",
            h.root.join("ledger.sqlite3").to_string_lossy(),
        );
        h.set("HORCH_DATA_DIR", h.root.join("data").to_string_lossy());
        h.set("FLEET_ROOT", fleet.to_string_lossy());
        h.set("HORCH_NO_WAKE", "1");
        h.set("HORCH_NO_WATCHDOG", "1");
        h.set("HORCH_TEST_ASSERT_READ_ONLY", "1");
        let base = h.head_sha().expect("the head of the project");
        let worktree = fleet.join("work").join("t0");
        let mut add = h.git_cmd(&["worktree", "add", "-q", "-b", "fleet/r1-t0"]);
        add.arg(&worktree).current_dir(&h.project);
        let o = add.output().unwrap();
        assert!(o.status.success(), "git worktree add: {}", text(&o));
        std::fs::write(
            fleet.join("fleet.toml"),
            "pass_period_s = 1\nheartbeat_max_age_s = 300\n",
        )
        .unwrap();
        let task_id = World::record(&h, &worktree, &base);
        let fleetds = Fleetds::new(&h);
        World {
            h,
            task_id,
            worktree,
            fleetds,
        }
    }

    fn record(h: &Harness, worktree: &Path, base: &str) -> String {
        let store = Store::open(&h.root.join("ledger.sqlite3")).unwrap();
        let conn = store.conn();
        let run = runs::create(
            conn,
            &h.project,
            &NewRun {
                slug: "r1".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let now = horch_core::clock::now_stamp();
        schema::run_idea(conn, &run.run_id, &now, true)
            .unwrap()
            .expect("the idea of r1");
        let mut new = NewTask::work("t0", "tickets/t0.md", PROFILE);
        new.worktree_mode = horch_core::taskledger::model::WorktreeMode::Shared;
        let task = tasks::add(conn, &run, &new, Actor::Orchestrator).unwrap();
        let worker = horch_core::taskledger::testing::bind_live(
            conn,
            &task.task_id,
            &NewWorker {
                run_id: run.run_id.clone(),
                kind: Some(WorkerKind::Worker),
                persona: PROFILE.into(),
                ..Default::default()
            },
            None,
        )
        .unwrap()
        .worker_id;
        let attempt = format!("{}/A1", task.task_id);
        let push = conn
            .execute("UPDATE attempts SET via = 'push' WHERE id = ?1", [&attempt])
            .unwrap();
        assert_eq!(push, 1, "the bind writes the attempt {attempt}");
        let tree = NewWorktree {
            worktree_id: "wt-t0".into(),
            task_id: Some(task.task_id.clone()),
            worker_id: Some(worker),
            repo_root: h.project.to_string_lossy().into_owned(),
            path: worktree.to_string_lossy().into_owned(),
            branch: "fleet/r1-t0".into(),
            base: base.into(),
            base_commit: Some(base.into()),
            status: WorktreeStatus::Open,
            mode: None,
        };
        let cause = Cause {
            actor: Actor::Orchestrator,
            text: "the worktree of t0",
        };
        let tx = conn.unchecked_transaction().unwrap();
        trees::insert_reserved(&tx, None, &tree, cause).unwrap();
        tx.commit().unwrap();
        task.task_id
    }

    fn fleet_root(&self) -> PathBuf {
        self.h.root.join("fleet")
    }

    fn task_dir(&self) -> PathBuf {
        attempt_dir(&self.fleet_root(), &self.task_id, "A1")
    }

    fn outbox(&self) -> PathBuf {
        self.task_dir().join("outbox")
    }

    fn ctx(&self) -> RuntimeContext {
        let env = MapEnv::new(&self.h.project)
            .with("HOME", &self.h.home.to_string_lossy())
            .with("CODEX_HOME", &self.h.home.join(".codex").to_string_lossy())
            .with("HORCH_STATE_DIR", &self.h.state.to_string_lossy());
        RuntimeContext::from_env(&env).unwrap()
    }

    fn launch(&self, sealed: bool) -> (Vec<String>, Vec<(String, String)>, Prepared) {
        let ctx = self.ctx();
        let roster = Roster::builtin().unwrap();
        let teammate = roster.require(PROFILE).unwrap().clone();
        let codex = HarnessKind::Codex.adapter();
        let prepared = codex
            .prepare(
                &ctx,
                &PrepareRequest {
                    role: "codex-reviewer-light-1",
                    exec_rules: roster.exec_rules(),
                    skills: None,
                    compact_window: None,
                    teammate: &teammate,
                    model: "gpt-5.6-terra",
                    workdir: &self.worktree,
                    shell_env: &[],
                },
            )
            .unwrap();
        let mut teammate = teammate;
        teammate.args.extend(prepared.extra_args.iter().cloned());
        let mut env = LaunchEnv::from_context(&ctx);
        if sealed {
            env.fleet_root = Some(self.fleet_root());
            env.task_dir = Some(self.task_dir());
            env.worktree = Some(self.worktree.clone());
        }
        let cmd = codex
            .command(
                &env,
                &CommandSpec {
                    teammate: &teammate,
                    session: Session::Unmanaged,
                    prompt: "review the change",
                    model_override: None,
                },
            )
            .unwrap();
        let argv = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let mut vars: Vec<(String, String)> = cmd
            .get_envs()
            .filter_map(|(k, v)| {
                Some((
                    k.to_string_lossy().into_owned(),
                    v?.to_string_lossy().into_owned(),
                ))
            })
            .collect();
        vars.extend(
            prepared
                .env
                .iter()
                .map(|(k, v)| (k.clone(), v.to_string_lossy().into_owned())),
        );
        (argv, vars, prepared)
    }

    fn wrapper(&self, scenario: &str, sealed: bool) -> Wrapper {
        let (argv, vars, prepared) = self.launch(sealed);
        let mut sh = Command::new("/bin/sh");
        self.h.seal(&mut sh);
        sh.envs(vars)
            .env("HORCH_FAKE_SCENARIO", scenario)
            .env("CODEX_BIN", self.h.bin.join("codex"))
            .current_dir(&self.worktree)
            .args([
                "-c",
                r#"read go; "$CODEX_BIN" "$@"; echo "exit $?"; read stop"#,
                "sh",
            ])
            .args(argv)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = sh.spawn().unwrap();
        let start = horch_platform::process::start_time(child.id()).expect("the wrapper start");
        let store = Store::open(&self.h.root.join("ledger.sqlite3")).unwrap();
        let attempt = format!("{}/A1", self.task_id);
        let live = store
            .conn()
            .execute(
                "UPDATE attempts SET pid = ?1, pid_start = ?2, state = 'live' WHERE id = ?3",
                rusqlite::params![
                    i64::from(child.id()),
                    i64::try_from(start).unwrap(),
                    attempt
                ],
            )
            .unwrap();
        assert_eq!(live, 1, "the attempt {attempt}");
        drop(store);
        let lines = BufReader::new(child.stdout.take().unwrap());
        Wrapper {
            child,
            lines,
            _prepared: prepared,
        }
    }

    fn report_call(&self) -> Value {
        horch_e2e::read_log(&self.h.log)
            .into_iter()
            .rev()
            .find(|c| c["fake"] == "codex" && c.get("rules").is_some())
            .unwrap_or_else(|| panic!("no report call: {:?}", self.h.calls_of("codex")))
    }

    fn start_fleetd(&mut self) {
        self.fleetds.serve(&self.h, "fleetd");
    }

    fn wait_for_progress(&self, text: &str) {
        wait_for_or(
            "the progress event of the worker",
            || {
                self.events()
                    .iter()
                    .any(|(kind, t)| kind == "progress" && t.contains(text))
                    .then_some(())
            },
            || {
                format!(
                    "events: {:?}\nfleetd: {}",
                    self.events(),
                    self.fleetds.output("fleetd", "stderr")
                )
            },
        );
    }

    fn events(&self) -> Vec<(String, String)> {
        let store = Store::open_read_only(&self.h.root.join("ledger.sqlite3")).unwrap();
        let mut stmt = store
            .conn()
            .prepare(
                "SELECT kind, coalesce(text, '') FROM events WHERE task_id = ?1 ORDER BY rowid",
            )
            .unwrap();
        stmt.query_map([&self.task_id], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }
}

struct Wrapper {
    child: Child,
    lines: BufReader<ChildStdout>,
    _prepared: Prepared,
}

impl Wrapper {
    fn run_agent(&mut self) -> i32 {
        writeln!(self.child.stdin.as_mut().unwrap(), "go").unwrap();
        let mut line = String::new();
        self.lines.read_line(&mut line).unwrap();
        line.trim()
            .strip_prefix("exit ")
            .and_then(|c| c.parse().ok())
            .unwrap_or_else(|| panic!("no exit line from the wrapper: {line:?}"))
    }

    fn end(mut self) {
        let _ = writeln!(self.child.stdin.as_mut().unwrap(), "stop");
        let _ = self.child.wait();
    }
}

impl Drop for Wrapper {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn assert_sandboxed_ok(call: &Value, key: &str) {
    let out = &call[key];
    assert_eq!(
        out["sandboxed"], true,
        "{key} did not run in the sandbox: {out}"
    );
    assert_eq!(out["exit"], 0, "{key}: {out}");
    let stderr = out["stderr"].as_str().unwrap_or_default();
    assert!(!stderr.contains("readonly database"), "{key}: {stderr}");
}

#[test]
fn worker_with_a_read_only_ledger_reports_progress_and_done_with_no_error() {
    let mut w = World::new("p5-ro-worker");
    let ledger = w.h.root.join("ledger.sqlite3");
    let mut wrapper = w.wrapper("report_progress,report_done", true);
    w.start_fleetd();
    assert_eq!(wrapper.run_agent(), 0);
    let call = w.report_call();
    let writable: Vec<&str> = call["report_progress"]["writable"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(
        !writable.iter().any(|root| ledger.starts_with(root)),
        "the ledger {} is writable for the worker: {writable:?}",
        ledger.display()
    );
    assert_sandboxed_ok(&call, "report_progress");
    assert_sandboxed_ok(&call, "report_done");
    assert!(w.outbox().join("progress.jsonl").is_file());
    let result: Value =
        serde_json::from_str(&std::fs::read_to_string(w.outbox().join("result.json")).unwrap())
            .unwrap();
    assert_eq!(result["status"], "completed", "{result}");
    assert_eq!(result["summary"], DONE, "{result}");
    w.wait_for_progress(PROGRESS);
    wrapper.end();
}

#[test]
fn fm17_codex_worker_writes_its_result_in_the_task_folder() {
    let mut w = World::new("p5-fm17-result");
    let mut wrapper = w.wrapper("report_result", true);
    w.start_fleetd();
    assert_eq!(wrapper.run_agent(), 0);
    let call = w.report_call();
    assert_sandboxed_ok(&call, "report_result");
    let writable = call["report_result"]["writable"].to_string();
    assert!(
        writable.contains(&*w.task_dir().to_string_lossy()),
        "the task folder is not a writable root: {writable}"
    );
    let result: Value =
        serde_json::from_str(&std::fs::read_to_string(w.outbox().join("result.json")).unwrap())
            .unwrap();
    assert_eq!(result["task"], w.task_id.as_str(), "{result}");
    assert_eq!(result["status"], "completed", "{result}");
    assert_eq!(result["summary"], RESULT, "{result}");
    wrapper.end();

    let mut w = World::new("p5-fm17-nowrite");
    let mut wrapper = w.wrapper("report_result", false);
    w.start_fleetd();
    assert_eq!(wrapper.run_agent(), 0);
    let call = w.report_call();
    assert_eq!(call["report_result"]["sandboxed"], true, "{call}");
    assert_ne!(call["report_result"]["exit"], 0, "{call}");
    assert!(!w.outbox().join("result.json").exists());
    wrapper.end();
}

#[test]
fn fm17_codex_worker_needs_no_ledger_allow_rule() {
    let mut w = World::new("p5-fm17-rules");
    let mut wrapper = w.wrapper("report_progress,report_result", true);
    w.start_fleetd();
    assert_eq!(wrapper.run_agent(), 0);
    let call = w.report_call();
    let rules = call["rules"].as_array().unwrap();
    assert!(!rules.is_empty(), "the launch has no execpolicy rules");
    let commands: [&[&str]; 3] = [
        &["horch", "ledger", "progress", PROGRESS],
        &["horch", "ledger", "done", DONE],
        &[
            "horch",
            "result",
            "--status",
            "completed",
            "--summary",
            RESULT,
        ],
    ];
    for rule in rules.iter().filter(|r| r["decision"] == "allow") {
        let pattern: Vec<&str> = rule["pattern"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        for command in commands {
            assert!(
                !command.starts_with(&pattern),
                "the allow rule {rule} lets {command:?} leave the sandbox"
            );
        }
    }
    assert_sandboxed_ok(&call, "report_progress");
    assert_sandboxed_ok(&call, "report_result");
    w.wait_for_progress(PROGRESS);
    wrapper.end();
}
