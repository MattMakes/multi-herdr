mod support;

use std::path::PathBuf;
use std::process::Command;

use horch_core::taskledger::map::ticket_facts;
use horch_core::taskledger::model::{Actor, StepName};
use horch_core::taskledger::runs::{self, NewRun, Thresholds};
use horch_core::taskledger::tasks::{self, NewTask, Task};
use horch_core::taskledger::workers::NewWorker;
use horch_core::taskledger::{schema, util, Store};
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

    fn ledger_db(&self) -> PathBuf {
        self.tmp.path().join("home/.horch/ledger.sqlite3")
    }

    fn seed_run(&self, thresholds: Thresholds, finish_steps: Vec<StepName>) {
        let git = Command::new("git")
            .args(["branch", "fleet/r1", "main"])
            .current_dir(self.proj())
            .output()
            .unwrap();
        assert!(git.status.success(), "git branch fleet/r1");
        let root = self.proj().canonicalize().unwrap();
        let new = NewRun {
            slug: "r1".into(),
            base_branch: Some("main".into()),
            run_branch: Some("fleet/r1".into()),
            finish_steps,
            thresholds,
            ..Default::default()
        };
        let mut store = Store::open(&self.ledger_db()).unwrap();
        store
            .immediate(|tx| {
                let run = runs::create(tx, &root, &new)?;
                schema::run_idea(tx, &run.run_id, &util::now(), true)?;
                Ok(())
            })
            .unwrap();
    }

    fn seed_task(&self, ticket: &str, new: impl FnOnce(&mut NewTask)) -> Task {
        let text = std::fs::read_to_string(self.proj().join(ticket)).unwrap();
        let facts = ticket_facts(&text);
        let title = facts.title.clone().unwrap();
        let mut task = NewTask::work(&title, ticket, "dev");
        task.ticket_snapshot = Some(text);
        new(&mut task);
        let mut store = Store::open(&self.ledger_db()).unwrap();
        store
            .immediate(|tx| {
                let run_id: String =
                    tx.query_row("SELECT run_id FROM runs WHERE slug = 'r1'", [], |r| {
                        r.get(0)
                    })?;
                let run = runs::get(tx, &run_id)?;
                tasks::add(tx, &run, &task, Actor::Orchestrator)
            })
            .unwrap()
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
fn the_board_commands_print_their_pinned_text() {
    let w = World::new();
    assert_no_runs(&w);
    w.seed_run(Thresholds::default(), vec![StepName::Check, StepName::Land]);
    assert_eq!(
        w.ok(&["ledger", "status"]),
        "Run r1 (active). Branch: fleet/r1 -> main. PR: none.\nNo tasks.\nFeature F-0001 (r1) open: not pushed.\n0 tasks: 0 done, 0 running, 0 landing, 0 queued, 0 cancelled. Complete.\n"
    );
    assert_eq!(
        w.seed_task("t1.md", |t| t.fallback = Some("dev2".into()))
            .short(),
        "T-1"
    );
    assert_eq!(
        w.seed_task("t2.md", |t| {
            t.blocked_by = vec!["T-1".into()];
            t.parallel = false;
        })
        .short(),
        "T-2"
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
        w.ok(&["ledger", "fallback", "T-1", "because"]),
        "T-1 can now be claimed by dev2.\n"
    );
    // Ticket 48 of fleetd-p6 retires unblock.
    let unblock = w.run(&["ledger", "unblock", "T-2", "--from", "T-1"]);
    assert_eq!((unblock.code, unblock.out.as_str()), (1, ""));
    assert_eq!(
        unblock.err,
        "horch ledger unblock is retired. Queue the task again: horch task requeue <task>\n"
    );
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
    let line = w.ok(&["ledger", "run", "list"]);
    assert!(line.starts_with("r1 active fleet/r1 -> main ("), "{line}");
    let status = w.run(&["ledger", "status", "--run", "r1"]);
    assert_eq!(status.code, 1);
    assert!(
        status.out.ends_with(
            "2 tasks: 0 done, 0 running, 0 landing, 1 queued, 1 cancelled. Not complete.\n"
        ),
        "{}",
        status.out
    );
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
        events,
        [
            "orchestrator run_created   Run r1 created. Run branch: fleet/r1.",
            "orchestrator added         T-1 queued for dev: Ticket One.",
            "orchestrator added         T-2 queued for dev: Ticket Two.",
            "orchestrator fallback      T-1 moves to fallback dev2: because",
            "orchestrator cancelled     T-2 cancelled: nope",
        ]
    );
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
    w.seed_run(Thresholds::default(), Vec::new());
    w.seed_task("t1.md", |_| {});
    let waive = w.run(&["ledger", "waive", "T-1", "F1", "reason"]);
    assert_eq!(waive.code, 1);
    assert_eq!(waive.err, "refused: T-1 has no code-sweep round\n");
    let stop = w.run(&["ledger", "stop", "nobody", "why"]);
    assert_eq!(stop.code, 1);
    assert!(stop.err.starts_with("refused: "), "{}", stop.err);
    // Ticket 48 of fleetd-p6 retires worker pid.
    let pid = w.run(&["ledger", "worker", "pid", "nobody", "--agent", "1"]);
    assert_eq!(pid.code, 1, "{}", pid.err);
    assert_eq!(
        pid.err,
        "horch ledger worker pid is retired. fleetd records each attempt at its launch\n"
    );
}

/// A world with run `r1` (checks every 3600 s, so no check is due within
/// a test on a loaded machine) and 1 task bound to the worker `dev1`
/// (FR-5).
fn world_with_a_claimed_task() -> (World, String) {
    let w = World::without_fleetd();
    w.seed_run(
        Thresholds {
            check_every_s: Some(3600),
            ..Default::default()
        },
        Vec::new(),
    );
    w.seed_task("t1.md", |_| {});
    let id = bind_dev1(&w, None);
    (w, id)
}

/// Bind the next task of `dev` to a new worker with the role `dev1` and the
/// agent `agent` (FR-5). Gives the worker id.
fn bind_dev1(w: &World, agent: Option<u32>) -> String {
    let db = w.ledger_db();
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
    let forced = w.run(&["ledger", "watch", "--once", "--force"]);
    assert_eq!(forced.code, 0, "{}", forced.err);
    assert_eq!(
        forced.out,
        "dev1 T-1: dead (round 0): the agent process is gone\n"
    );
    let status = w.ok(&["ledger", "show", "T-1"]);
    assert!(status.starts_with("T-1 running: Ticket One\n"), "{status}");
    assert!(status.contains("Attempt 1: dev1 working"), "{status}");
}

#[test]
fn watch_once_json_lists_each_checked_worker() {
    let w = World::without_fleetd();
    // Many stall rounds: the forced check does not reap the worker, whose
    // agent is this test.
    w.seed_run(
        Thresholds {
            check_every_s: Some(10),
            stall_rounds: Some(1000),
            ..Default::default()
        },
        Vec::new(),
    );
    w.seed_task("t1.md", |_| {});
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
