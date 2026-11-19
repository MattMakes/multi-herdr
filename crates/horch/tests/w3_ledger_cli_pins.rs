//! `horch ledger`: pins the output and exit code of the task-ledger
//! subcommands (status, show, run, add, plan-workers, fallback, cancel,
//! unblock, requeue, events, import, unfinished), so a refactor of the CLI
//! body keeps them. Every test runs the binary on a temp git project, with
//! the watchdog off and no herdr pane. A test fleetd serves the requests of
//! the commands.

mod support;

use std::path::PathBuf;
use std::process::Command;

use horch_core::taskledger::model::FileMode;
use horch_core::taskledger::workers::NewWorker;
use horch_core::taskledger::{tasks, Store};
use support::fleetd::{short_tempdir, TestFleetd};

const HEALTHY_MACHINE: &str = r#"{"os":"linux","arch":"x86_64","cpus":8,"load_average_1m":1.0,"mem_total_bytes":17179869184,"mem_available_bytes":8589934592,"disk_free_bytes":214748364800,"disk_total_bytes":536870912000,"gpu":"none","max_open_files":1048576,"max_processes":100000}"#;

struct Out {
    code: i32,
    out: String,
    err: String,
}

/// The fleetd drops first: fields drop in order.
struct World {
    _fleetd: Option<TestFleetd>,
    tmp: tempfile::TempDir,
}

impl World {
    /// The temp project with a test fleetd that serves its requests.
    fn new() -> World {
        let mut w = World::without_fleetd();
        w._fleetd = Some(TestFleetd::start(
            w.horch(true),
            &w.tmp.path().join("fleet"),
        ));
        w
    }

    /// The temp project, and no fleetd: no ledger exists yet.
    fn without_fleetd() -> World {
        let tmp = short_tempdir();
        let proj = tmp.path().join("proj");
        let git = |args: &[&str]| {
            let st = Command::new("git")
                .args(args)
                .current_dir(&proj)
                .env_clear()
                .env("PATH", "/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin")
                .env("HOME", tmp.path().join("home"))
                .env("GIT_AUTHOR_NAME", "n")
                .env("GIT_AUTHOR_EMAIL", "a@b")
                .env("GIT_COMMITTER_NAME", "n")
                .env("GIT_COMMITTER_EMAIL", "a@b")
                .status()
                .unwrap();
            assert!(st.success(), "git {args:?}");
        };
        std::fs::create_dir_all(&proj).unwrap();
        git(&["init", "-q", "-b", "main"]);
        git(&["commit", "-q", "--allow-empty", "-m", "init"]);
        for (n, title) in [("t1", "One"), ("t2", "Two")] {
            std::fs::write(proj.join(format!("{n}.md")), format!("# Ticket {title}\n")).unwrap();
        }
        // A fixed, healthy machine, so the health guard of `horch spawn`
        // never reads the real load of the host.
        std::fs::write(tmp.path().join("machine.json"), HEALTHY_MACHINE).unwrap();
        // The process and open-file counts have no fixture: a share of 1.0
        // holds only above the limit, which the host does not allow.
        std::fs::create_dir_all(tmp.path().join("fleet")).unwrap();
        std::fs::write(
            tmp.path().join("fleet/fleet.toml"),
            "health_max_process_share = 1.0\nhealth_max_open_file_share = 1.0\n",
        )
        .unwrap();
        World { _fleetd: None, tmp }
    }

    fn proj(&self) -> PathBuf {
        self.tmp.path().join("proj")
    }

    /// The `horch` binary with the environment of the test. `watchdog_off`
    /// sets `HORCH_NO_WATCHDOG=1`.
    fn horch(&self, watchdog_off: bool) -> Command {
        let root = self.tmp.path();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_horch"));
        cmd.current_dir(self.proj())
            .env_clear()
            .env("PATH", "/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin")
            .env("HOME", root.join("home"))
            .env("XDG_DATA_HOME", root.join("data"))
            .env("HORCH_STATE_DIR", root.join("state"))
            .env("HORCH_PROJECT_DIR", self.proj())
            .env("HORCH_MACHINE_FILE", root.join("machine.json"))
            .env("FLEET_ROOT", root.join("fleet"));
        if watchdog_off {
            cmd.env("HORCH_NO_WATCHDOG", "1");
        }
        cmd
    }

    fn run_as(&self, worker: Option<&str>, args: &[&str]) -> Out {
        self.run_env(worker, true, args)
    }

    /// `watchdog_off` sets `HORCH_NO_WATCHDOG=1`.
    fn run_env(&self, worker: Option<&str>, watchdog_off: bool, args: &[&str]) -> Out {
        let mut cmd = self.horch(watchdog_off);
        cmd.args(args);
        if let Some(w) = worker {
            cmd.env("HORCH_WORKER_ID", w);
        }
        let o = cmd.output().unwrap();
        Out {
            code: o.status.code().unwrap(),
            out: String::from_utf8(o.stdout).unwrap(),
            // A host that cannot read a count (a sandbox) gets a guard note.
            err: String::from_utf8(o.stderr)
                .unwrap()
                .lines()
                .filter(|l| !l.starts_with("horch: NOTE: the health guard "))
                .map(|l| format!("{l}\n"))
                .collect(),
        }
    }

    fn run(&self, args: &[&str]) -> Out {
        self.run_as(None, args)
    }

    /// Run `args`, require exit 0, return stdout.
    fn ok(&self, args: &[&str]) -> String {
        let o = self.run(args);
        assert_eq!(o.code, 0, "{args:?}: {}", o.err);
        o.out
    }
}

/// An event line without its leading timestamp.
fn untimed(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| l.trim_start().split_once(' ').unwrap().1.to_string())
        .collect()
}

fn json(text: &str) -> serde_json::Value {
    serde_json::from_str(text).unwrap_or_else(|e| panic!("{e}: {text}"))
}

#[test]
fn run_new_creates_the_branch_and_reports_it() {
    let w = World::new();
    let o = w.run(&["ledger", "run", "new", "r1", "--steps", "check,land"]);
    assert_eq!(o.code, 0, "{}", o.err);
    let first = o.out.lines().next().unwrap();
    assert!(
        first.starts_with("Run r1 created (")
            && first.ends_with("). Run branch: fleet/r1. Finish steps: check, land."),
        "{}",
        o.out
    );
    assert_eq!(o.out.lines().count(), 1);
    assert!(
        o.err
            .starts_with("horch ledger: created run branch fleet/r1 from main.\n"),
        "{}",
        o.err
    );
    assert!(
        o.err.contains("horch ledger: warning: run r1 has no workspace, because this command ran outside a herdr pane."),
        "{}",
        o.err
    );
    let branches = Command::new("git")
        .args(["branch", "--list", "fleet/r1"])
        .current_dir(w.proj())
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&branches.stdout).contains("fleet/r1"));
    let bad = w.run(&["ledger", "run", "new", "r2", "--steps", "bogus"]);
    assert_eq!(bad.code, 1);
    assert!(bad.err.starts_with("horch: "), "{}", bad.err);
}

#[test]
fn the_board_commands_print_their_pinned_text() {
    let w = World::new();
    assert_no_runs(&w);
    w.ok(&["ledger", "run", "new", "r1", "--steps", "check,land"]);
    assert_eq!(
        w.ok(&["ledger", "status"]),
        "Run r1 (active). Branch: fleet/r1 -> main. PR: none.\nNo tasks.\n0 tasks: 0 done, 0 running, 0 landing, 0 queued, 0 cancelled. Complete.\n"
    );
    assert_eq!(
        w.ok(&["ledger", "plan-workers"]),
        "No task can start now. Spawn no worker.\n"
    );
    assert_eq!(
        w.ok(&[
            "ledger",
            "add",
            "--file",
            "t1.md",
            "--primary",
            "dev",
            "--fallback",
            "dev2"
        ]),
        "T-1 queued for dev: Ticket One.\n"
    );
    assert_eq!(
        w.ok(&[
            "ledger",
            "add",
            "--file",
            "t2.md",
            "--primary",
            "dev",
            "--blocked-by",
            "T-1",
            "--exclusive"
        ]),
        "T-2 queued for dev: Ticket Two.\n"
    );
    let status = w.run(&["ledger", "status"]);
    assert_eq!(status.code, 1);
    assert!(
        status.out.ends_with(
            "2 tasks: 0 done, 0 running, 0 landing, 2 queued, 0 cancelled. Not complete.\n"
        ),
        "{}",
        status.out
    );
    assert_eq!(
        w.ok(&["ledger", "plan-workers"]),
        "dev: spawn 1 (1 can start, 0 on the way).\n"
    );
    let plan = json(&w.ok(&["ledger", "plan-workers", "--json"]));
    assert_eq!(
        plan,
        serde_json::json!([{
            "persona": "dev", "startable": 1, "on_the_way": 0, "spawn": 1, "replaces": []
        }])
    );
    assert_eq!(
        w.ok(&["ledger", "fallback", "T-1", "because"]),
        "T-1 can now be claimed by dev2.\n"
    );
    let unblock = w.run(&["ledger", "unblock", "T-2", "--from", "T-1"]);
    assert_eq!(
        (unblock.code, unblock.out.as_str()),
        (0, "T-2 no longer waits for T-1.\n")
    );
    // The command writes no wake (ticket 22 of fleetd-p5, T-471), so it
    // prints no wake note.
    assert_eq!(unblock.err, "");
    assert_eq!(
        w.ok(&["ledger", "cancel", "T-2", "nope"]),
        "T-2 cancelled.\n"
    );
    // `cancelled` is terminal (fleetd-p2 MAP.md, Q22): the door refuses
    // the requeue. fleetd refuses the request (ticket 22 of fleetd-p5,
    // T-471).
    let requeue = w.run(&["ledger", "requeue", "T-2", "again"]);
    assert_eq!((requeue.code, requeue.out.as_str()), (1, ""));
    assert!(
        requeue
            .err
            .starts_with("refused: illegal transition of task ")
            && requeue.err.ends_with(": cancelled -> ready\n"),
        "{}",
        requeue.err
    );
    let refused = w.run(&["ledger", "requeue", "T-1", "again"]);
    assert_eq!(refused.code, 1);
    assert_eq!(
        refused.err,
        "refused: T-1 is queued; only a held task is requeued\n"
    );
    assert_show_of_t2(&w);
    assert_board_events(&untimed(&w.ok(&["ledger", "events"])));
    assert_refused_requeue(&w);
    let close = w.run(&["ledger", "run", "close"]);
    assert_eq!(close.code, 1);
    assert!(
        close
            .err
            .starts_with("refused: run r1 still has open work: T-1 (queued)."),
        "{}",
        close.err
    );
    assert_eq!(
        w.ok(&["ledger", "run", "close", "--abandon", "gone"]),
        "T-1 cancelled.\nRun r1 closed as abandoned.\n"
    );
    let line = w.ok(&["ledger", "run", "list"]);
    assert!(
        line.starts_with("r1 abandoned fleet/r1 -> main ("),
        "{line}"
    );
    assert!(w
        .ok(&["ledger", "status", "--run", "r1"])
        .ends_with("2 tasks: 0 done, 0 running, 0 landing, 0 queued, 2 cancelled. Complete.\n"));
}

/// The board of a ledger with no run.
fn assert_no_runs(w: &World) {
    assert_eq!(w.ok(&["ledger", "status"]), "No runs.\n");
    assert_eq!(
        json(&w.ok(&["ledger", "status", "--json"])),
        serde_json::json!({ "runs": [] })
    );
    assert_eq!(w.ok(&["ledger", "run", "list"]), "No runs.\n");
    assert_eq!(w.ok(&["ledger", "run", "list", "--json"]), "[]\n");
}

/// `horch ledger show` of T-2, as text and as JSON, and of an unknown task.
fn assert_show_of_t2(w: &World) {
    assert_eq!(
        w.ok(&["ledger", "show", "T-2"])
            .lines()
            .take(3)
            .collect::<Vec<_>>(),
        [
            "T-2 cancelled: Ticket Two",
            "Ticket: t2.md",
            "Persona: dev (primary dev, fallback -)."
        ]
    );
    let shown = json(&w.ok(&["ledger", "show", "T-2", "--json"]));
    let mut keys: Vec<&str> = shown
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort();
    assert_eq!(keys, ["attempts", "blocked_by", "events", "own", "task"]);
    assert_eq!(shown["task"]["title"], "Ticket Two");
    let unknown = w.run(&["ledger", "show", "T-9"]);
    assert_eq!(unknown.code, 1);
    assert_eq!(
        unknown.err,
        "horch: 'T-9' is neither a task nor a worker of run r1: no worker 'T-9' in this run\n"
    );
}

/// The events of the board test, in order: `horch ledger events` without
/// its times.
/// fleetd records the refused requeue of the cancelled T-2 in its request
/// row (ticket 22 of fleetd-p5, T-471).
fn assert_refused_requeue(w: &World) {
    let conn = rusqlite::Connection::open(w.tmp.path().join("home/.horch/ledger.sqlite3")).unwrap();
    let refused: Vec<(String, String)> = conn
        .prepare("SELECT kind, reason FROM requests WHERE state = 'refused' ORDER BY rowid")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(
        refused.iter().any(|(kind, reason)| kind == "task_requeue"
            && reason.starts_with("illegal transition of task ")
            && reason.ends_with(": cancelled -> ready")),
        "{refused:?}"
    );
}

fn assert_board_events(events: &[String]) {
    assert_eq!(
        events[0],
        "orchestrator run_created   Run r1 created. Run branch: fleet/r1."
    );
    // OR-2: the orchestrator row of the new run moves to working, with its
    // event and its cause.
    assert!(
        events[1].starts_with(
            "orchestrator transition    agent_started: the orchestrator of run r1 runs in its pane; worker "
        ) && events[1].ends_with(": spawning -> working"),
        "{events:?}"
    );
    assert_eq!(
        events[2],
        "orchestrator added         T-1 queued for dev: Ticket One."
    );
    assert_eq!(
        events[4],
        "orchestrator fallback      T-1 moves to fallback dev2: because"
    );
    assert_eq!(
        events[5],
        "orchestrator note          T-2 no longer waits for T-1."
    );
    // T-2 waits for nothing now: it moves from pending to ready, both
    // shown as queued.
    assert!(
        events[6].starts_with(
            "horch        transition    dependencies_met: each task that T-2 waits for is done; task "
        ) && events[6].ends_with(": queued -> queued"),
        "{events:?}"
    );
    // The commands write no wake (ticket 22 of fleetd-p5, T-471), so no
    // wake event follows.
    assert_eq!(events[7], "orchestrator cancelled     T-2 cancelled: nope");
    // `cancelled` is terminal: fleetd refuses the requeue and records it in
    // its request row (see `assert_refused_requeue`), not as an event.
    assert_eq!(events.len(), 8, "{events:?}");
}

#[test]
fn the_unfinished_import_and_refusal_arms_print_their_pinned_text() {
    let w = World::new();
    assert!(w
        .ok(&["ledger", "path"])
        .ends_with("/home/.horch/ledger.sqlite3\n"));
    assert_eq!(w.ok(&["ledger", "unfinished"]), "No unfinished work.\n");
    assert_eq!(w.ok(&["ledger", "unfinished", "--json"]), "[]\n");
    assert_eq!(
        w.ok(&["ledger", "import", "--dry-run"]),
        "Dry run, nothing kept. 0 files: 0 records copied (0 already there); 0 workers, 0 tasks, 0 worktrees mapped; 0 could not be mapped.\n"
    );
    let dismiss = w.run(&["ledger", "dismiss", "5", "x"]);
    assert_eq!(dismiss.code, 1);
    assert_eq!(
        dismiss.err,
        "refused: no finding 5: Query returned no rows\n"
    );
    w.ok(&["ledger", "run", "new", "r1"]);
    w.ok(&["ledger", "add", "--file", "t1.md", "--primary", "dev"]);
    let waive = w.run(&["ledger", "waive", "T-1", "F1", "reason"]);
    assert_eq!(waive.code, 1);
    assert_eq!(waive.err, "refused: T-1 has no code-sweep round\n");
    let stop = w.run(&["ledger", "stop", "nobody", "why"]);
    assert_eq!(stop.code, 1);
    assert!(stop.err.starts_with("refused: "), "{}", stop.err);
    let pid = w.run(&["ledger", "worker", "pid", "nobody", "--agent", "1"]);
    assert_eq!(pid.code, 1, "{}", pid.err);
    assert_eq!(pid.err, "refused: no worker nobody in the task ledger\n");
}

/// A world with run `r1` (checks every 10 s) and 1 task bound to the
/// worker `dev1` (FR-5).
fn world_with_a_claimed_task() -> (World, String) {
    let w = World::new();
    w.ok(&["ledger", "run", "new", "r1", "--check-every", "10"]);
    w.ok(&["ledger", "add", "--file", "t1.md", "--primary", "dev"]);
    let id = bind_dev1(&w, None);
    (w, id)
}

/// Bind the next task of `dev` to a new worker with the role `dev1` and the
/// agent `agent` (FR-5). Gives the worker id.
fn bind_dev1(w: &World, agent: Option<u32>) -> String {
    let db = w.tmp.path().join("home/.horch/ledger.sqlite3");
    let worker = NewWorker {
        persona: "dev".into(),
        role: Some("dev1".into()),
        ..Default::default()
    };
    support::bind::bind(&db, worker, agent).worker_id
}

#[test]
fn watch_once_reports_in_text_and_json() {
    let (w, _) = world_with_a_claimed_task();
    // Not due yet: nothing is checked, in either form.
    assert_eq!(
        w.ok(&["ledger", "watch", "--once"]),
        "No worker was due for a check.\n"
    );
    assert_eq!(w.ok(&["ledger", "watch", "--once", "--json"]), "[]\n");
    // Forced: the worker has no agent process. The fleetd pass ends its
    // attempt before the duty check (I4), so no worker is left to check,
    // and the task is queued again.
    let forced = w.run(&["ledger", "watch", "--once", "--force"]);
    assert_eq!(forced.code, 0, "{}", forced.err);
    assert_eq!(forced.out, "No worker was due for a check.\n");
    let status = w.ok(&["ledger", "show", "T-1"]);
    assert!(status.starts_with("T-1 queued: Ticket One\n"), "{status}");
    assert!(status.contains("Attempt 1: dev1"), "{status}");
}

#[test]
fn watch_once_json_lists_each_checked_worker() {
    let w = World::new();
    // Many stall rounds: the forced check does not reap the worker, whose
    // agent is this test.
    w.ok(&[
        "ledger",
        "run",
        "new",
        "r1",
        "--check-every",
        "10",
        "--stall-rounds",
        "1000",
    ]);
    w.ok(&["ledger", "add", "--file", "t1.md", "--primary", "dev"]);
    let id = bind_dev1(&w, Some(std::process::id()));
    let forced = w.run(&["ledger", "watch", "--once", "--force", "--json"]);
    assert_eq!(forced.code, 0, "{}", forced.err);
    let checked = json(&forced.out);
    let list = checked.as_array().unwrap();
    assert_eq!(list.len(), 1, "{checked}");
    assert_eq!(list[0]["worker_id"], id.as_str());
    assert_eq!(list[0]["role"], "dev1");
    assert_eq!(list[0]["task"], "T-1");
    assert_eq!(list[0]["verdict"], "no_progress", "{checked}");
    assert_eq!(list[0]["round"], 1, "{checked}");
    // The note gives the seconds since the claim, which the test cannot fix.
    let note = list[0]["note"].as_str().unwrap();
    assert!(
        note.starts_with("no ledger call, no transcript growth, no state change"),
        "{note}"
    );
}

#[test]
fn a_command_with_no_fleetd_row_starts_no_watchdog() {
    let (w, _) = world_with_a_claimed_task();
    // The check before the command is on. No fleetd ran on this ledger, so
    // it starts nothing and says nothing.
    let status = w.run_env(None, false, &["ledger", "status"]);
    assert_eq!(status.code, 1, "{}", status.err);
    assert_eq!(status.err, "", "{}", status.err);
    let locks = w.tmp.path().join("home/.horch/locks");
    let held: Vec<_> = std::fs::read_dir(&locks)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .collect();
    assert!(held.is_empty(), "{held:?}");
}

#[test]
fn ensure_starts_nothing_with_the_watchdog_off_or_without_a_ledger() {
    let (w, _) = world_with_a_claimed_task();
    let off = w.run_env(None, true, &["ledger", "status"]);
    assert!(!off.err.contains("watchdog"), "{}", off.err);
    let bare = World::without_fleetd();
    let none = bare.run_env(None, false, &["ledger", "path"]);
    assert_eq!(none.code, 0);
    assert!(!none.err.contains("watchdog"), "{}", none.err);
    assert!(!bare.tmp.path().join("home/.horch/ledger.sqlite3").exists());
}

#[test]
fn spawn_checks_the_run_the_persona_and_the_replaced_worker() {
    let w = World::new();
    let no_run = w.run(&["spawn", "sonnet", "--replaces", "nope"]);
    assert_eq!(no_run.code, 1);
    assert!(
        no_run
            .err
            .starts_with("horch: --replaces needs an active task-ledger run in ")
            && no_run.err.ends_with("/proj\n"),
        "{}",
        no_run.err
    );
    w.ok(&["ledger", "run", "new", "r1"]);
    let id = w
        .ok(&[
            "ledger",
            "worker",
            "new",
            "--persona",
            "dev",
            "--role",
            "dev1",
        ])
        .trim()
        .to_string();
    let live = w.run(&["spawn", "sonnet", "--replaces", &id]);
    assert_eq!(live.code, 1);
    assert_eq!(
        live.err,
        format!("horch: worker dev1 is still spawning. Run horch ledger stop {id} \"<reason>\" --run r1, then spawn again\n")
    );
    let text = w.run(&["spawn", "sonnet", "do a thing"]);
    assert_eq!(text.code, 1);
    assert!(
        text.err
            .contains("horch ledger task 'do a thing' --persona sonnet"),
        "{}",
        text.err
    );
    let missing = w.run(&["spawn", "sonnet", "--run", "nosuch"]);
    assert_eq!(missing.code, 1);
    assert!(
        missing.err.starts_with("horch: no run 'nosuch' in "),
        "{}",
        missing.err
    );
    w.ok(&["ledger", "run", "gate", "--check", "true"]);
    w.ok(&["ledger", "run", "close"]);
    let closed = w.run(&["spawn", "sonnet", "--run", "r1"]);
    assert_eq!(closed.code, 1);
    assert_eq!(
        closed.err,
        "horch: run r1 is done; a closed run takes no new worker\n"
    );
}

const TICKET_WITH_FACTS: &str =
    "# 03: From the ticket\n**Check:** `true`\n**Own:** `src/a`, `src/b`\n**Do not touch:** `src/c`\n";

/// `ledger add`: what a ticket says about itself fills in what no flag says,
/// and a flag beats the ticket.
#[test]
fn add_takes_the_title_check_and_files_from_the_ticket_unless_a_flag_names_them() {
    let w = World::new();
    w.ok(&["ledger", "run", "new", "r1"]);
    std::fs::write(w.proj().join("facts.md"), TICKET_WITH_FACTS).unwrap();
    // The next task of `dev` is bound to a new worker (FR-5): its facts.
    let db = w.tmp.path().join("home/.horch/ledger.sqlite3");
    let claim = || {
        let worker = NewWorker {
            persona: "dev".into(),
            ..Default::default()
        };
        let task = support::bind::bind(&db, worker, None).task_id;
        let store = Store::open(&db).unwrap();
        let c = store.conn();
        serde_json::json!({
            "task": tasks::get(c, &task).unwrap(),
            "own": tasks::files(c, &task, FileMode::Own).unwrap(),
            "do_not_touch": tasks::files(c, &task, FileMode::DoNotTouch).unwrap(),
        })
    };

    assert_eq!(
        w.ok(&["ledger", "add", "--file", "facts.md", "--primary", "dev"]),
        "T-1 queued for dev: From the ticket.\n"
    );
    let first = claim();
    let task = &first["task"];
    assert_eq!(task["title"], "From the ticket");
    assert_eq!(task["land_check"], "true");
    assert_eq!(first["own"], serde_json::json!(["src/a", "src/b"]));
    assert_eq!(first["do_not_touch"], serde_json::json!(["src/c"]));
    assert_eq!(task["fallback_persona"], serde_json::Value::Null);
    assert_eq!(task["priority"], 100);
    assert_eq!(task["parallel"], true);
    assert_eq!(task["worktree_mode"], "worktree");

    let flags = [
        "ledger",
        "add",
        "--file",
        "facts.md",
        "--primary",
        "dev",
        "--fallback",
        "dev2",
        "--title",
        "Named",
        "--phase",
        "p1",
        "--issue",
        "http://i/1",
        "--spec",
        "http://s/1",
        "--priority",
        "7",
        "--check",
        "echo hi",
        "--own",
        "x/1,x/2",
        "--no-touch",
        "y/1",
        "--check-timeout",
        "33",
    ];
    assert_eq!(w.ok(&flags), "T-2 queued for dev: Named.\n");
    let second = claim();
    let task = &second["task"];
    assert_eq!(task["title"], "Named");
    assert_eq!(task["land_check"], "echo hi");
    assert_eq!(second["own"], serde_json::json!(["x/1", "x/2"]));
    assert_eq!(second["do_not_touch"], serde_json::json!(["y/1"]));
    assert_eq!(task["fallback_persona"], "dev2");
    assert_eq!(task["phase"], "p1");
    assert_eq!(task["issue_url"], "http://i/1");
    assert_eq!(task["spec_ref"], "http://s/1");
    assert_eq!(task["priority"], 7);
    // The timeout sits in the evidence folder beside the ledger file.
    let task_id = task["task_id"].as_str().unwrap();
    let timeout = w
        .tmp
        .path()
        .join("home/.horch/evidence")
        .join(task_id)
        .join("check-timeout-s");
    assert_eq!(std::fs::read_to_string(timeout).unwrap(), "33\n");

    let flags = ["ledger", "add", "--file", "facts.md", "--primary", "dev"];
    let both = [flags.as_slice(), &["--shared", "--exclusive"]].concat();
    assert_eq!(w.ok(&both), "T-3 queued for dev: From the ticket.\n");
    let shown = json(&w.ok(&["ledger", "show", "T-3", "--json"]));
    assert_eq!(shown["task"]["worktree_mode"], "shared");
    assert_eq!(shown["task"]["parallel"], false);
    assert_eq!(shown["own"], serde_json::json!(["src/a", "src/b"]));
}

/// A check timeout is set after the task is queued: a refused one leaves the
/// task queued.
#[test]
fn add_refuses_a_check_timeout_of_zero_after_it_queued_the_task() {
    let w = World::new();
    w.ok(&["ledger", "run", "new", "r1"]);
    let o = w.run(&[
        "ledger",
        "add",
        "--file",
        "t1.md",
        "--primary",
        "dev",
        "--check-timeout",
        "0",
    ]);
    assert_eq!(o.code, 1);
    assert_eq!(o.out, "");
    assert_eq!(o.err, "horch: a check timeout is 1 to 86400 s, not 0 s\n");
    assert!(w
        .ok(&["ledger", "show", "T-1"])
        .starts_with("T-1 queued: Ticket One\n"));
}

/// `ledger add-map`: each queued task, then the map path it read, relative to
/// the current directory or as given.
#[test]
fn add_map_prints_each_task_and_the_path_of_the_map_it_read() {
    let map = "# Map: x\nSpec: [#28](https://example/28)\n\n| # | Issue | Title | Blocked by | Wave |\n|---|---|---|---|---|\n\
               | [01](m1.md) | [#29](https://example/29) | Store | - | 1 |\n\
               | [02](m2.md) | [#30](https://example/30) | Claim | #29 | 2 |\n";
    let queue = |as_given: &dyn Fn(&World) -> PathBuf| {
        let w = World::new();
        w.ok(&["ledger", "run", "new", "r1"]);
        let tickets = [
            (
                "m1.md",
                "# 01: Store\n\n**Persona:** primary `dev`, fallback `dev2` · **Parallel:** yes\n",
            ),
            (
                "m2.md",
                "# 02: Claim\n\n**Persona:** primary `dev` · **Parallel:** no\n",
            ),
        ];
        for (name, body) in tickets {
            std::fs::write(w.proj().join(name), body).unwrap();
        }
        std::fs::write(w.proj().join("MAP.md"), map).unwrap();
        let given = as_given(&w);
        let out = w.ok(&["ledger", "add-map", &given.to_string_lossy()]);
        // `status` exits 1 while work remains.
        let status = json(&w.run(&["ledger", "status", "--json"]).out);
        assert_eq!(status["tasks"].as_array().unwrap().len(), 2, "{status}");
        (out, std::fs::canonicalize(w.proj()).unwrap())
    };

    let (out, cwd) = queue(&|_| PathBuf::from("MAP.md"));
    assert_eq!(
        out,
        format!(
            "T-1 queued for dev: Store.\nT-2 queued for dev: Claim.\n2 tasks queued from {}.\n",
            cwd.join("MAP.md").display()
        )
    );
    let (out, _) = queue(&|w| w.proj().join("MAP.md"));
    let given = out.lines().last().unwrap().to_string();
    assert!(
        given.starts_with("2 tasks queued from ") && given.ends_with("/proj/MAP.md."),
        "{out}"
    );
}
