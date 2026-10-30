//! `horch ledger`: pins the output and exit code of the task-ledger
//! subcommands (status, show, run, add, plan-workers, fallback, cancel,
//! unblock, requeue, events, claim, worktree, steps, finish, done, import,
//! unfinished), so a refactor of the CLI body keeps them. Every test runs the
//! binary on a temp git project, with the watchdog off and no herdr pane.

use std::path::{Path, PathBuf};
use std::process::Command;

struct Out {
    code: i32,
    out: String,
    err: String,
}

struct World {
    tmp: tempfile::TempDir,
}

impl World {
    fn new() -> World {
        let tmp = tempfile::tempdir().unwrap();
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
        World { tmp }
    }

    fn proj(&self) -> PathBuf {
        self.tmp.path().join("proj")
    }

    fn run_as(&self, worker: Option<&str>, args: &[&str]) -> Out {
        self.run_env(worker, true, args)
    }

    /// `watchdog_off` sets `HORCH_NO_WATCHDOG=1`.
    fn run_env(&self, worker: Option<&str>, watchdog_off: bool, args: &[&str]) -> Out {
        let root = self.tmp.path();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_horch"));
        cmd.args(args)
            .current_dir(self.proj())
            .env_clear()
            .env("PATH", "/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin")
            .env("HOME", root.join("home"))
            .env("XDG_DATA_HOME", root.join("data"))
            .env("HORCH_STATE_DIR", root.join("state"))
            .env("HORCH_PROJECT_DIR", self.proj());
        if watchdog_off {
            cmd.env("HORCH_NO_WATCHDOG", "1");
        }
        if let Some(w) = worker {
            cmd.env("HORCH_WORKER_ID", w);
        }
        let o = cmd.output().unwrap();
        Out {
            code: o.status.code().unwrap(),
            out: String::from_utf8(o.stdout).unwrap(),
            err: String::from_utf8(o.stderr).unwrap(),
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

fn path_free(p: &Path) -> String {
    p.file_name().unwrap().to_string_lossy().into_owned()
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
    assert_eq!(w.ok(&["ledger", "status"]), "No runs.\n");
    assert_eq!(
        json(&w.ok(&["ledger", "status", "--json"])),
        serde_json::json!({ "runs": [] })
    );
    assert_eq!(w.ok(&["ledger", "run", "list"]), "No runs.\n");
    assert_eq!(w.ok(&["ledger", "run", "list", "--json"]), "[]\n");
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
    assert!(
        unblock
            .err
            .starts_with("horch ledger: the orchestrator was not woken: "),
        "{}",
        unblock.err
    );
    assert_eq!(
        w.ok(&["ledger", "cancel", "T-2", "nope"]),
        "T-2 cancelled.\n"
    );
    let requeue = w.run(&["ledger", "requeue", "T-2", "again"]);
    assert_eq!(
        (requeue.code, requeue.out.as_str()),
        (0, "T-2 is queued again.\n")
    );
    let refused = w.run(&["ledger", "requeue", "T-1", "again"]);
    assert_eq!(refused.code, 1);
    assert_eq!(
        refused.err,
        "horch: T-1 is queued; only a held or cancelled task is requeued\n"
    );
    assert_eq!(
        w.ok(&["ledger", "show", "T-2"])
            .lines()
            .take(3)
            .collect::<Vec<_>>(),
        [
            "T-2 queued: Ticket Two",
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
    let events = untimed(&w.ok(&["ledger", "events"]));
    assert_eq!(
        events[0],
        "orchestrator run_created   Run r1 created. Run branch: fleet/r1."
    );
    assert_eq!(
        events[1],
        "orchestrator added         T-1 queued for dev: Ticket One."
    );
    assert_eq!(
        events[3],
        "orchestrator fallback      T-1 moves to fallback dev2: because"
    );
    assert_eq!(
        events[4],
        "orchestrator note          T-2 no longer waits for T-1."
    );
    assert_eq!(events.len(), 9, "{events:?}");
    let close = w.run(&["ledger", "run", "close"]);
    assert_eq!(close.code, 1);
    assert!(
        close
            .err
            .starts_with("horch: run r1 still has open work: T-1 (queued), T-2 (queued)."),
        "{}",
        close.err
    );
    assert_eq!(
        w.ok(&["ledger", "run", "close", "--abandon", "gone"]),
        "T-1 cancelled.\nT-2 cancelled.\nRun r1 closed as abandoned.\n"
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

#[test]
fn a_worker_claims_makes_a_worktree_and_finishes() {
    let w = World::new();
    w.ok(&["ledger", "run", "new", "r1"]);
    w.ok(&["ledger", "add", "--file", "t1.md", "--primary", "dev"]);
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
    let as_worker = |args: &[&str]| w.run_as(Some(&id), args);
    let none = w.run(&["ledger", "claim"]);
    assert_eq!(none.code, 1);
    assert!(
        none.err.starts_with("horch: HORCH_WORKER_ID is not set"),
        "{}",
        none.err
    );
    let claim = as_worker(&["ledger", "claim"]);
    assert_eq!(claim.code, 0, "{}", claim.err);
    let lines: Vec<&str> = claim.out.lines().collect();
    assert_eq!(lines[0], "Claimed T-1 (attempt 1): Ticket One");
    assert!(
        lines[1].starts_with("Ticket: ")
            && lines[1].ends_with("t1.md. Read it in full and follow it exactly.")
    );
    assert_eq!(
        lines[2],
        "Make your worktree first: horch ledger worktree new ticket-one"
    );
    assert_eq!(
        lines[3],
        "Record milestones with: horch ledger progress \"<text>\""
    );
    assert_eq!(
        lines[4],
        "When the work is complete, run: horch ledger finish"
    );
    assert_eq!(as_worker(&["ledger", "steps"]).out, "");
    assert_eq!(as_worker(&["ledger", "progress", "half way"]).code, 0);
    let early = as_worker(&["ledger", "finish", "--json"]);
    assert_eq!(early.code, 1);
    assert_eq!(
        early.err,
        "horch: T-1 has no worktree. Make it with horch ledger worktree new <name>, work and commit there, then run finish\n"
    );
    let wt = as_worker(&["ledger", "worktree", "new", "feat"]);
    let wt_lines: Vec<&str> = wt.out.lines().collect();
    assert_eq!(
        path_free(Path::new(wt_lines[0].strip_prefix("path: ").unwrap())),
        "T1-feat"
    );
    assert_eq!(wt_lines[1], "branch: fleet/r1-T1-feat");
    assert_eq!(wt_lines[2], "base: fleet/r1");
    assert!(wt_lines[3].ends_with(". When the work is complete, run: horch ledger finish"));
    let scratch = as_worker(&["ledger", "worktree", "scratch", "scr"]);
    assert!(scratch.out.contains("branch: fleet-scratch-r1-scr\nScratch worktree: drop it when it is no longer needed; it does not block completion.\n"), "{}", scratch.out);
    let dropped = as_worker(&["ledger", "worktree", "drop", "scr", "--reason", "r"]);
    assert!(
        dropped.out.starts_with("abandoned: ") && dropped.out.ends_with("/scr (r)\n"),
        "{}",
        dropped.out
    );

    let report = json(&as_worker(&["ledger", "finish", "--json"]).out);
    assert_eq!(report["task"], "T-1");
    let steps: Vec<(&str, &str)> = report["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["name"].as_str().unwrap(), s["status"].as_str().unwrap()))
        .collect();
    assert_eq!(
        steps,
        [
            ("check", "not_applicable"),
            ("code-sweep", "not_applicable"),
            ("complexity-sweep", "not_applicable"),
            ("recheck", "not_applicable"),
            ("land", "passed"),
            ("pr", "not_applicable"),
            ("cleanup", "passed"),
        ]
    );
    let table = as_worker(&["ledger", "steps"]).out;
    let rows: Vec<Vec<&str>> = table
        .lines()
        .map(|l| l.split_whitespace().collect())
        .collect();
    assert_eq!(rows.len(), 7);
    assert_eq!(&rows[0][..4], ["1", "check", "not_applicable", "not"]);
    assert_eq!(&rows[4][..3], ["5", "land", "passed"]);
    assert_eq!(rows[6][..3], ["7", "cleanup", "passed"]);
    let by_task = w.ok(&["ledger", "steps", "T-1", "--json"]);
    assert_eq!(json(&by_task).as_array().unwrap().len(), 7);
    let done = as_worker(&["ledger", "done", "all", "good"]);
    assert_eq!(done.code, 0, "{}", done.err);
    assert_eq!(done.out, "T-1 done.\n");
    let shown = w.ok(&["ledger", "show", "dev1"]);
    assert!(shown.starts_with("dev1 done ("), "{shown}");
    assert!(shown.contains("persona dev.\n"), "{shown}");
    let worker_json = json(&w.ok(&["ledger", "show", "dev1", "--json"]));
    assert_eq!(worker_json["worker"]["role"], "dev1");
    assert!(worker_json["stops"].is_array());
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
    assert_eq!(dismiss.err, "horch: no finding 5: Query returned no rows\n");
    w.ok(&["ledger", "run", "new", "r1"]);
    w.ok(&["ledger", "add", "--file", "t1.md", "--primary", "dev"]);
    let waive = w.run(&["ledger", "waive", "T-1", "F1", "reason"]);
    assert_eq!(waive.code, 1);
    assert_eq!(waive.err, "horch: T-1 has no code-sweep round\n");
    let stop = w.run(&["ledger", "stop", "nobody", "why"]);
    assert_eq!(stop.code, 1);
    assert!(stop.err.starts_with("horch: "), "{}", stop.err);
    let pid = w.run(&["ledger", "worker", "pid", "nobody", "--agent", "1"]);
    assert_eq!(pid.code, 0, "{}", pid.err);
}

/// A world with run `r1` (checks every 10 s) and 1 claimed task.
fn world_with_a_claimed_task() -> (World, String) {
    let w = World::new();
    w.ok(&["ledger", "run", "new", "r1", "--check-every", "10"]);
    w.ok(&["ledger", "add", "--file", "t1.md", "--primary", "dev"]);
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
    assert_eq!(w.run_as(Some(&id), &["ledger", "claim"]).code, 0);
    (w, id)
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
    // Forced: the worker has no agent process, so it is dead and reaped.
    let forced = w.run(&["ledger", "watch", "--once", "--force"]);
    assert_eq!(forced.code, 0, "{}", forced.err);
    assert_eq!(
        forced.out,
        "dev1 T-1: dead (round 0): the agent process is gone\n"
    );
    let status = w.ok(&["ledger", "show", "T-1"]);
    assert!(status.starts_with("T-1 queued: Ticket One\n"), "{status}");
    assert!(status.contains("Attempt 1: dev1"), "{status}");
}

#[test]
fn watch_once_json_lists_each_checked_worker() {
    let (w, id) = world_with_a_claimed_task();
    let forced = w.run(&["ledger", "watch", "--once", "--force", "--json"]);
    assert_eq!(forced.code, 0, "{}", forced.err);
    let checked = json(&forced.out);
    let list = checked.as_array().unwrap();
    assert_eq!(list.len(), 1, "{checked}");
    assert_eq!(list[0]["worker_id"], id.as_str());
    assert_eq!(list[0]["role"], "dev1");
    assert_eq!(list[0]["task"], "T-1");
    assert_eq!(list[0]["verdict"], "dead");
    assert_eq!(list[0]["note"], "the agent process is gone");
}

#[test]
fn ensure_starts_a_quiet_watchdog_once() {
    let (w, _) = world_with_a_claimed_task();
    // The first command that runs with the watchdog on starts one.
    let started = w.run_env(None, false, &["ledger", "status"]);
    assert_eq!(started.code, 1, "{}", started.err);
    assert!(
        started.err.starts_with(
            "horch: warning: the watchdog of run r1 was not running; started it (PID "
        ),
        "{}",
        started.err
    );
    // A second command finds a live watchdog and starts none.
    let again = w.run_env(None, false, &["ledger", "status"]);
    assert!(!again.err.contains("started it"), "{}", again.err);
    // Closing the run ends the watchdog loop at its next tick.
    w.ok(&["ledger", "run", "close", "--abandon", "test over"]);
}

#[test]
fn ensure_starts_nothing_with_the_watchdog_off_or_without_a_ledger() {
    let (w, _) = world_with_a_claimed_task();
    let off = w.run_env(None, true, &["ledger", "status"]);
    assert!(!off.err.contains("watchdog"), "{}", off.err);
    let bare = World::new();
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
    let outside = w.run(&["spawn", "sonnet", "do a thing"]);
    assert!(
        outside.err.starts_with("horch spawn: NOTE: this spawn has task text, so it runs outside task-ledger run r1. Give --run <slug> to join a run.\n"),
        "{}",
        outside.err
    );
    let missing = w.run(&["spawn", "sonnet", "--run", "nosuch"]);
    assert_eq!(missing.code, 1);
    assert!(
        missing.err.starts_with("horch: no run 'nosuch' in "),
        "{}",
        missing.err
    );
    w.ok(&["ledger", "run", "close"]);
    let closed = w.run(&["spawn", "sonnet", "--run", "r1"]);
    assert_eq!(closed.code, 1);
    assert_eq!(
        closed.err,
        "horch: run r1 is done; a closed run takes no new worker\n"
    );
}
