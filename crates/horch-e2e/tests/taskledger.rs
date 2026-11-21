//! The task ledger at the CLI seam (#28): the real `horch` binary, the fake
//! herdr and fake agents, a temporary ledger file per test.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::{Duration, Instant};

use horch_core::fleetd::fleet_runtime::adapters::{clock_store, config_file, jobs_store};
use horch_core::fleetd::fleet_runtime::application::jobs::{self, JobCtx, JobKey, Queued};
use horch_core::fleetd::fleet_runtime::domain::jobs::{Kind as JobKind, State as JobState};
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::taskledger::model::{StepName, WorktreeMode};
use horch_core::taskledger::runs::{self, Run};
use horch_core::taskledger::tasks::{self, Task};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::Store;
use horch_e2e::bind::{bind, try_bind};
use horch_e2e::fleetd::Fleetds;
use horch_e2e::harness::{copy_tree, fixtures, repo_root, Harness};
use horch_e2e::ledger_seed::{seed_run, seed_task, seed_worker};
use serde_json::Value;

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// A harness with its own ledger file and a git repository.
fn world(name: &str) -> Harness {
    let h = world_without_fleetd(name);
    h.serve_fleetd();
    h
}

/// [`world`] with no test fleetd: for a test of a command that reads the
/// ledger only, such as `horch ledger status`.
fn world_without_fleetd(name: &str) -> Harness {
    let mut h = Harness::new(name).with_git();
    let db = h.root.join("ledger.sqlite3");
    h.set("HORCH_LEDGER_DB", db.to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
    h
}

/// Start the test fleetd again, so that it has the environment that the
/// caller set after [`world`].
fn restart_fleetd(h: &Harness) {
    h.stop_fleetd();
    h.serve_fleetd();
}

/// A harness in workspace `w1` (root pane `w1:p1`) whose panes run their
/// commands, like `horch fleet` leaves it.
fn fleet(name: &str, scenario: &str) -> Harness {
    let mut h = world(name);
    let quota = fixtures().join("quota/all-ok.json");
    h.set("HORCH_QUOTA_FILE", quota.to_string_lossy());
    h.set("HORCH_FAKE_SCENARIO", scenario);
    let mut create = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut create);
    let o = create
        .args(["workspace", "create", "--label", name])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", text(&o));
    restart_fleetd(&h);
    h
}

/// A harness in workspace `w1` (root pane `w1:p1`) with the run `slug`
/// (heartbeat every 1 s) and 1 queued task of persona `codex-terra` for
/// each title of `tasks`. fleetd runs a pass each second, so it binds and
/// launches each task by push (Phase 4) and ingests the wrapper files
/// (Phase 5). Gives the harness when fleetd serves.
fn push_fleet(name: &str, scenario: &str, slug: &str, tasks: &[&str]) -> Harness {
    let mut h = world_without_fleetd(name);
    push_env(&mut h, scenario);
    let run = seed_run(&h, slug, Some(1), "w1:p1");
    for (n, title) in tasks.iter().enumerate() {
        let p = ticket(&h, &format!("t{n}"), &format!("# {title}\n"));
        seed_task(&h, &run, &p, "codex-terra");
    }
    h.serve_fleetd();
    h
}

/// The environment of [`push_fleet`] in `h`, whose fleetd is down: the
/// herdr workspace of the fleet ([`Harness::fleet_workspace`]), where fleetd
/// opens the pane of each attempt, codex-terra interactive, and a pass of
/// fleetd each second.
fn push_env(h: &mut Harness, scenario: &str) {
    let quota = fixtures().join("quota/all-ok.json");
    h.set("HORCH_QUOTA_FILE", quota.to_string_lossy());
    h.set("HORCH_FAKE_SCENARIO", scenario);
    h.set("HORCH_FAULT", "delivery-probe-ms:100");
    h.set("HORCH_HOSTNAME", "test-host");
    let ws = h.fleet_workspace();
    h.set("HORCH_WORKSPACE_ID", ws);
    // codex-terra runs interactive, as before exec became the default mode
    // (fleetd-p7 ticket 19): the fake agent stays in its pane.
    let teammates = h.root.join("teammates");
    copy_tree(&repo_root().join("teammates"), &teammates);
    let terra = teammates.join("codex-terra.md");
    let md = std::fs::read_to_string(&terra).unwrap();
    let interactive = md.replacen("\nagent: codex\n", "\nagent: codex\nmode: interactive\n", 1);
    assert_ne!(interactive, md, "codex-terra.md names its agent");
    std::fs::write(&terra, interactive).unwrap();
    h.set("HORCH_TEAMMATES_DIR", teammates.to_string_lossy());
    let root = h.fleet_root();
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("fleet.toml"), "pass_period_s = 1\n").unwrap();
    // The gates of the project: fleetd holds each dispatch of a project
    // with no fast gate. The run branch gets them from main.
    project_file(
        h,
        "fleet.toml",
        "[gates]\nfast = \"true\"\nfull = \"true\"\n",
    );
    git_commit_all(h, "the gates of the project");
}

fn seed_run_with(h: &Harness, slug: &str, steps: &[StepName], check_every_s: Option<i64>) -> Run {
    let run = seed_run(h, slug, None, "w1:p1");
    let words: Vec<String> = steps.iter().map(|s| s.as_str().to_string()).collect();
    let mut store = Store::open(&h.ledger_db()).unwrap();
    store
        .immediate(|tx| {
            tx.execute(
                "UPDATE runs SET finish_steps = ?2, check_every_s = COALESCE(?3, check_every_s)
                 WHERE run_id = ?1",
                rusqlite::params![
                    run.run_id,
                    horch_core::taskledger::util::json_list(&words),
                    check_every_s
                ],
            )?;
            runs::get(tx, &run.run_id)
        })
        .unwrap()
}

fn seed_task_with(
    h: &Harness,
    run: &Run,
    ticket: &Path,
    primary: &str,
    edit: impl FnOnce(&mut tasks::NewTask),
) -> Task {
    let text = std::fs::read_to_string(ticket).unwrap();
    let facts = horch_core::taskledger::map::ticket_facts(&text);
    let mut new = tasks::NewTask {
        land_check: facts.check.clone(),
        check_none: facts.check_none,
        ticket_snapshot: Some(text.clone()),
        ..tasks::NewTask::work(
            facts.title.as_deref().unwrap(),
            &ticket.to_string_lossy(),
            primary,
        )
    };
    edit(&mut new);
    let mut store = Store::open(&h.ledger_db()).unwrap();
    store
        .immediate(|tx| {
            tasks::add(
                tx,
                run,
                &new,
                horch_core::taskledger::model::Actor::Orchestrator,
            )
        })
        .unwrap()
}

fn seed_blocked_task(
    h: &Harness,
    run: &Run,
    ticket: &Path,
    primary: &str,
    blocked_by: Option<&str>,
) -> Task {
    seed_task_with(h, run, ticket, primary, |new| {
        new.blocked_by = blocked_by.into_iter().map(String::from).collect();
    })
}

fn seed_limits(h: &Harness, run: &Run, max_attempts: Option<i64>, heartbeat_stale_s: Option<i64>) {
    let mut store = Store::open(&h.ledger_db()).unwrap();
    store
        .immediate(|tx| {
            tx.execute(
                "UPDATE runs SET max_attempts = COALESCE(?2, max_attempts),
                    heartbeat_stale_s = COALESCE(?3, heartbeat_stale_s)
                 WHERE run_id = ?1",
                rusqlite::params![run.run_id, max_attempts, heartbeat_stale_s],
            )?;
            Ok(())
        })
        .unwrap();
}

/// The JSON output of the fake herdr for `args`.
fn herdr_json(h: &Harness, args: &[&str]) -> Value {
    let mut cmd = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut cmd);
    let o = cmd.args(args).output().unwrap();
    assert!(o.status.success(), "{}", text(&o));
    serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&o)))
}

/// The worker rows of kind `worker` in `horch ledger status`.
fn worker_rows(h: &Harness) -> Vec<Value> {
    let st = json_of(h, &["ledger", "status", "--json"]);
    st["workers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["worker"].clone())
        .filter(|w| w["kind"] == "worker")
        .collect()
}

/// Wait until fleetd has launched `n` workers and ingested the agent PID
/// of each. Gives the worker rows.
fn launched(h: &Harness, n: usize) -> Vec<Value> {
    wait_push(&format!("{n} agent PIDs"), || {
        let ws = worker_rows(h);
        (ws.len() == n && ws.iter().all(|w| w["agent_started"].is_i64())).then_some(ws)
    })
}

fn ok(h: &Harness, args: &[&str]) -> String {
    let o = h.run(args);
    assert!(o.status.success(), "horch {}: {}", args.join(" "), text(&o));
    out(&o)
}

fn ok_as(h: &Harness, worker: &str, args: &[&str]) -> String {
    let mut cmd = h.horch(args);
    cmd.env("HORCH_WORKER_ID", worker);
    let o = cmd.output().unwrap();
    assert!(o.status.success(), "horch {}: {}", args.join(" "), text(&o));
    out(&o)
}

fn json_of(h: &Harness, args: &[&str]) -> Value {
    let o = h.run(args);
    serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&o)))
}

fn ticket(h: &Harness, name: &str, body: &str) -> PathBuf {
    let dir = h.project.join("tickets");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(format!("{name}.md"));
    std::fs::write(&p, body).unwrap();
    p
}

fn wait_for<T>(what: &str, f: impl FnMut() -> Option<T>) -> T {
    wait_within(Duration::from_secs(30), what, f)
}

/// [`wait_for`] for an event of the fleetd push flow: a launch, an ingest
/// or a stop. Each one waits for a pass of fleetd, and parallel tests with
/// their own fleetd and panes slow each pass, so the limit is longer.
fn wait_push<T>(what: &str, f: impl FnMut() -> Option<T>) -> T {
    wait_within(Duration::from_secs(120), what, f)
}

fn wait_pipeline<T>(what: &str, f: impl FnMut() -> Option<T>) -> T {
    wait_within(Duration::from_secs(480), what, f)
}

fn wait_within<T>(limit: Duration, what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let until = Instant::now() + limit;
    while Instant::now() < until {
        if let Some(v) = f() {
            return v;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("timed out waiting for {what}");
}

#[test]
fn status_on_a_fresh_ledger_creates_it_and_says_no_runs() {
    let h = world_without_fleetd("tl-fresh");
    let db = h.root.join("ledger.sqlite3");
    assert!(!db.exists());
    let o = h.run(&["ledger", "status"]);
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert!(
        text(&o).contains("fleetd creates it at its start"),
        "{}",
        text(&o)
    );
    assert!(!db.exists(), "status creates no ledger file");
    h.serve_fleetd();
    assert!(db.exists(), "fleetd creates the ledger file");
    let o = h.run(&["ledger", "status"]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(out(&o).trim(), "No runs.");
    let o = h.run(&["ledger", "status", "--json"]);
    assert!(o.status.success());
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["runs"], Value::Array(vec![]));
}

#[test]
fn the_ledger_path_is_in_horch_home_by_default() {
    let mut h = Harness::new("tl-path");
    h.unset("HORCH_LEDGER_DB");
    let p = ok(&h, &["ledger", "path"]);
    assert_eq!(
        Path::new(p.trim()),
        h.home.join(".horch").join("ledger.sqlite3")
    );
}

#[test]
fn legacy_plumbing_lives_under_ledger_legacy() {
    let h = world("tl-legacy");
    let p = ok(&h, &["ledger", "legacy", "path"]);
    assert_eq!(p.lines().count(), 1, "{p}");
    assert_eq!(Path::new(p.trim()), h.ledger_db(), "{p}");
}

#[test]
fn queue_claim_and_status_through_the_cli() {
    let h = world("tl-queue");
    let run = seed_run(&h, "alpha", None, "w1:p1");
    // The run branch exists, from the checked-out branch.
    let mut git = h.git_cmd(&["branch", "--list", "fleet/alpha"]);
    git.current_dir(&h.project);
    let branches = git.output().unwrap();
    assert!(
        out(&branches).contains("fleet/alpha"),
        "{}",
        text(&branches)
    );

    let a = ticket(&h, "01-a", "# 01: Parser\n**Check:** `true`\n");
    let b = ticket(&h, "02-b", "# 02: Printer\n");
    let added = seed_task_with(&h, &run, &a, "opus", |new| {
        new.own = vec!["src/parse".into()];
    });
    assert_eq!(
        (added.short().as_str(), added.title.as_str()),
        ("T-1", "Parser")
    );
    seed_task_with(&h, &run, &b, "opus", |new| {
        new.blocked_by = vec!["T-1".into()];
    });

    let o = h.run(&["ledger", "plan-workers", "--json"]);
    assert!(!o.status.success(), "{}", text(&o));
    assert!(
        text(&o).contains("horch ledger plan-workers is retired"),
        "{}",
        text(&o)
    );

    let o = h.run(&["ledger", "status"]);
    assert_eq!(o.status.code(), Some(1), "work remains: {}", text(&o));
    assert!(out(&o).contains("blocked by T-1 (queued)"), "{}", out(&o));

    let bound = bind(&h, "opus", None);
    let w1 = bound.worker_id.clone();
    // T-2 waits for T-1: no bind takes it.
    let none = try_bind(&h, "opus", None);
    assert!(none.is_err(), "{none:?}");

    ok(&h, &["ledger", "cancel", "T-2", "not needed"]);
    let st = json_of(&h, &["ledger", "status", "--json"]);
    assert_eq!(st["counts"]["cancelled"], 1);
    assert_eq!(st["complete"], false);

    let me = std::process::id();
    let start = horch_core::procid::start_time(me).unwrap();
    Store::open(&h.ledger_db())
        .unwrap()
        .conn()
        .execute(
            "UPDATE attempts SET pid = ?2, pid_start = ?3 WHERE id = ?1",
            rusqlite::params![bound.attempt_id, me, start as i64],
        )
        .unwrap();
    let attempt = h
        .fleet_root()
        .join("tasks")
        .join(&bound.task_id)
        .join("attempts")
        .join("A1");
    std::fs::create_dir_all(&attempt).unwrap();
    ok_as(&h, &w1, &["ledger", "progress", "parser compiles"]);
    let lines = std::fs::read_to_string(attempt.join("outbox/progress.jsonl")).unwrap();
    assert!(lines.contains("parser compiles"), "{lines}");
    let shown = json_of(&h, &["ledger", "show", "T-1", "--json"]);
    let kinds: Vec<&str> = shown["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds.first(), Some(&"added"), "{kinds:?}");
}

/// Write `rel` under the project, with `body`.
fn project_file(h: &Harness, rel: &str, body: &str) {
    let p = h.project.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

fn git_commit_all(h: &Harness, message: &str) {
    for args in [&["add", "-A"][..], &["commit", "-q", "-m", message][..]] {
        let o = h.git_cmd(args).current_dir(&h.project).output().unwrap();
        assert!(o.status.success(), "git {}: {}", args.join(" "), text(&o));
    }
}

fn worker_json(h: &Harness, id: &str) -> Value {
    json_of(h, &["ledger", "show", id, "--json"])["worker"].clone()
}

#[test]
fn a_spawned_worker_records_its_pids_session_and_checkins() {
    let h = push_fleet("tl-spawn", "exec,stay", "gamma", &["01: A"]);
    // fleetd binds T-1 and launches its attempt in a new pane of w1.
    let w = launched(&h, 1).remove(0);
    let id = w["worker_id"].as_str().unwrap().to_string();
    assert_eq!(w["persona"], "codex-terra");
    assert_eq!(w["host"], "test-host");
    let pane = w["pane_id"].as_str().expect("the worker has its pane");
    assert_ne!(pane, "w1:p1", "{w}");
    let panes = herdr_json(&h, &["pane", "list", "--workspace", "w1"]);
    assert!(
        panes["result"]["panes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["pane_id"] == pane),
        "{panes}"
    );

    // The supervisor and the agent record their PIDs; the start times match.
    let w = wait_push("both PIDs", || {
        let w = worker_json(&h, &id);
        (w["agent_pid"].is_i64() && w["supervisor_pid"].is_i64()).then_some(w)
    });
    for (pid, started) in [
        ("supervisor_pid", "supervisor_started"),
        ("agent_pid", "agent_started"),
    ] {
        let pid = w[pid].as_i64().unwrap() as u32;
        let want = horch_core::procid::start_time(pid).expect("the process runs");
        assert_eq!(w[started].as_i64().unwrap() as u64, want, "{pid}");
    }
    // The agent was told to read START.md of its attempt (push). The
    // fake writes its call record after the PIDs are in, so wait for it.
    wait_push("the agent's task names inbox/START.md", || {
        h.calls_of("codex")
            .into_iter()
            .rev()
            .find(|c| c["argv"].to_string().contains("inbox/START.md"))
    });

    // Checkins advance while the agent runs.
    let first = w["last_checkin_at"].as_str().unwrap().to_string();
    wait_push("2 more checkins", || {
        let a = worker_json(&h, &id)["last_checkin_at"]
            .as_str()?
            .to_string();
        (a > first).then_some(())?;
        std::thread::sleep(Duration::from_millis(1100));
        let b = worker_json(&h, &id)["last_checkin_at"]
            .as_str()?
            .to_string();
        (b > a).then_some(())
    });
    // The discovered session and its transcript reach the row.
    let w = wait_push("the session", || {
        let w = worker_json(&h, &id);
        w["session_id"].is_string().then_some(w)
    });
    let w = if w["transcript_path"].is_string() {
        w
    } else {
        wait_push("the transcript", || {
            let w = worker_json(&h, &id);
            w["transcript_path"].is_string().then_some(w)
        })
    };
    assert!(
        Path::new(w["transcript_path"].as_str().unwrap()).is_file(),
        "{w}"
    );

    // The spawn bound the task to the attempt (FR-5). The agent is
    // killed: the task is queued again.
    let agent = w["agent_pid"].as_i64().unwrap() as u32;
    let started = w["agent_started"].as_i64().unwrap() as u64;
    assert!(horch_core::procid::signal_same(
        agent,
        started,
        libc_sigterm()
    ));
    let w = wait_push("the exit", || {
        let w = worker_json(&h, &id);
        w["ended_at"].is_string().then_some(w)
    });
    assert_eq!(w["status"], "incomplete", "{w}");
    // fleetd sees the process of the attempt gone, before a result.
    let task_id = w["task_id"].as_str().unwrap();
    assert_eq!(
        w["end_reason"],
        format!("dead: the process of attempt {task_id}/A1 is gone").as_str(),
        "{w}"
    );
    let checkin = w["last_checkin_at"].as_str().unwrap().to_string();
    std::thread::sleep(Duration::from_millis(2500));
    assert_eq!(
        worker_json(&h, &id)["last_checkin_at"].as_str().unwrap(),
        checkin,
        "checkins stop with the agent"
    );
    let t = json_of(&h, &["ledger", "show", "T-1", "--json"]);
    assert_eq!(t["task"]["status"], "queued");
    assert!(
        t["task"]["priority"].as_i64().unwrap() < 0,
        "first in line: {t}"
    );
}

fn libc_sigterm() -> i32 {
    15
}

/// A fake `gh`: `pr create` prints a URL and keeps the body, `pr view`
/// prints the body and the remote run branch's head, `pr edit` replaces
/// the body. Calls go to `<dir>/calls`. `warm` does nothing.
const FAKE_GH: &str = r#"#!/bin/sh
if [ "$1" = warm ]; then exit 0; fi
st="$FAKE_GH_DIR"
cmd="$1 $2"
shift 2
echo "$cmd $*" >> "$st/calls"
case "$cmd" in
"pr create")
  while [ $# -gt 0 ]; do
    case "$1" in
      --body) printf '%s' "$2" > "$st/body"; shift 2;;
      *) shift;;
    esac
  done
  echo "https://github.com/o/r/pull/7";;
"pr view")
  head=$("$HORCH_GIT_BIN" rev-parse "refs/remotes/origin/$FAKE_GH_BRANCH")
  b=$(/usr/bin/sed -e 's/\\/\\\\/g' -e 's/"/\\"/g' "$st/body" | /usr/bin/awk 'BEGIN{ORS="\\n"}{print}')
  printf '{"body":"%s","headRefOid":"%s"}\n' "$b" "$head";;
"pr edit")
  shift
  if [ "$1" = "--body-file" ]; then /bin/cat "$2" > "$st/body"; fi;;
esac
"#;

fn git_in(h: &Harness, dir: &Path, args: &[&str]) -> String {
    let mut c = h.git_cmd(args);
    c.current_dir(dir);
    let o = c.output().unwrap();
    assert!(o.status.success(), "git {}: {}", args.join(" "), text(&o));
    out(&o).trim().to_string()
}

/// A world with a bare remote, a fake gh, a fake analyser and the code-sweep
/// reviewer prompts.
fn finish_world(name: &str) -> Harness {
    let mut h = world(name);
    let remote = h.root.join("remote.git");
    let root = h.root.clone();
    git_in(
        &h,
        &root,
        &["init", "--quiet", "--bare", &remote.to_string_lossy()],
    );
    git_in(
        &h,
        &h.project.clone(),
        &["remote", "add", "origin", &remote.to_string_lossy()],
    );
    git_in(
        &h,
        &h.project.clone(),
        &["push", "--quiet", "origin", "main"],
    );
    let gh_dir = h.root.join("gh");
    std::fs::create_dir_all(&gh_dir).unwrap();
    h.write_warmed("gh", FAKE_GH.as_bytes());
    h.set("HORCH_GH_BIN", h.bin.join("gh").to_string_lossy());
    h.set("FAKE_GH_DIR", gh_dir.to_string_lossy());
    h.set("FAKE_GH_BRANCH", "fleet/alpha");
    let cli = h.write_warmed(
        "fake-complexity",
        b"#!/bin/sh\necho '{\"files\":[],\"violations\":[]}'\n",
    );
    h.set("HORCH_COMPLEXITY_CLI", cli.to_string_lossy());
    let sweep = h.root.join("code-sweep");
    std::fs::create_dir_all(sweep.join("reviewers")).unwrap();
    for r in [
        "bug-hunter",
        "clarity-checker",
        "coverage-analyst",
        "vulnerability-scanner",
        "structure-inspector",
        "efficiency-auditor",
        "contract-guardian",
        "waste-detector",
        "reliability-auditor",
        "dependency-examiner",
        "power-of-ten-reviewer",
        "interface-auditor",
    ] {
        std::fs::write(
            sweep.join("reviewers").join(format!("{r}.md")),
            format!("You are {r}.\n"),
        )
        .unwrap();
    }
    h.set("HORCH_CODE_SWEEP_DIR", sweep.to_string_lossy());
    restart_fleetd(&h);
    h
}

fn push_finish_world(name: &str, scenario: &str) -> Harness {
    let mut h = finish_world(name);
    h.stop_fleetd();
    push_env(&mut h, scenario);
    h
}

#[test]
fn a_task_finishes_through_review_land_pr_and_cleanup() {
    let h = push_finish_world("tl-finish", "exec,stay,commit,result");
    let run = seed_run(&h, "alpha", None, "w1:p1");
    let p = ticket(&h, "01-a", "# 01: Feature\n");
    seed_task_with(&h, &run, &p, "opus", |new| {
        new.land_check = Some("test -f fake-claude.txt".into());
        new.issue_url = Some("https://github.com/o/r/issues/29".into());
    });
    h.serve_fleetd();
    let tree = wait_push("the worktree of T-1", || task_worktree(&h, "T-1"));
    let reviews = wait_done_playing_reviewers(&h, "T-1");
    assert!(reviews >= 2, "bugs and clarity at least: {reviews}");
    wait_pipeline("the branch of T-1 landed", || task_branch_landed(&h, 1));
    let project = h.project.clone();
    let log = git_in(&h, &project, &["log", "--oneline", "fleet/alpha"]);
    assert!(log.contains("fake claude: commit"), "{log}");
    wait_push("the worktree of T-1 is gone and cleaned", || {
        (!tree.exists() && tree_status(&h, "T-1").as_deref() == Some("cleaned")).then_some(())
    });
}

fn open_reviews(h: &Harness) -> Vec<(String, PathBuf)> {
    let store = Store::open(&h.ledger_db()).unwrap();
    let mut stmt = store
        .conn()
        .prepare(
            "SELECT claimable_by, spec_ref FROM tasks
             WHERE kind = 'review' AND spec_ref IS NOT NULL ORDER BY rowid",
        )
        .unwrap();
    let rows: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    rows.into_iter()
        .map(|(persona, file)| (persona, PathBuf::from(file)))
        .filter(|(_, file)| !file.exists())
        .collect()
}

fn review_round_with(h: &Harness, critical: &str, important: &str) -> Vec<(String, PathBuf)> {
    let round = wait_pipeline("the reviews of a round", || {
        Some(open_reviews(h)).filter(|r| !r.is_empty())
    });
    for (i, (_, file)) in round.iter().enumerate() {
        let (crit, imp) = if i == 0 {
            (critical, important)
        } else {
            ("- none", "- none")
        };
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(
            file,
            format!("## Critical\n{crit}\n## Important\n{imp}\n## Minor\n- none\n"),
        )
        .unwrap();
    }
    round
}

fn fix_rounds_and_attempts(h: &Harness, seq: i64) -> (i64, i64) {
    let store = Store::open(&h.ledger_db()).unwrap();
    store
        .conn()
        .query_row(
            "SELECT t.fix_rounds, (SELECT count(*) FROM attempts a WHERE a.task_id = t.task_id)
             FROM tasks t WHERE t.seq = ?1",
            [seq],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
}

#[test]
fn a_critical_finding_fails_until_a_fix_is_reviewed_again() {
    let h = push_finish_world("tl-critical", "exec,stay,commit,result");
    let run = seed_run_with(&h, "alpha", &[StepName::CodeSweep], None);
    let p = ticket_of(&h, "a", "Feature", "opus");
    seed_task_with(&h, &run, &p, "opus", |_| {});
    h.serve_fleetd();

    let round = review_round_with(&h, "- a.txt:1 data loss. Fix: x", "- none");
    assert!(
        round.iter().all(|(p, _)| p == "codex-reviewer"),
        "{round:?}"
    );
    wait_pipeline("the fix attempt", || {
        (fix_rounds_and_attempts(&h, 1) == (1, 2)).then_some(())
    });

    let round = review_round_with(&h, "- none", "- a.txt:1 slow. Fix: cache");
    assert!(
        round.iter().all(|(p, _)| p == "codex-reviewer-light"),
        "{round:?}"
    );
    wait_pipeline("the second fix attempt", || {
        (fix_rounds_and_attempts(&h, 1) == (2, 3)).then_some(())
    });

    let round = wait_pipeline("the reviews of round 3", || {
        Some(open_reviews(&h)).filter(|r| !r.is_empty())
    });
    let answers = round[0].1.parent().unwrap().join("answers.md");
    let mut text = std::fs::read_to_string(&answers).unwrap_or_default();
    text.push_str("F2: the input is 3 items at most\n");
    std::fs::write(&answers, text).unwrap();
    for (i, (_, file)) in round.iter().enumerate() {
        let imp = if i == 0 {
            "- a.txt:1 slow. Fix: cache"
        } else {
            "- none"
        };
        std::fs::write(
            file,
            format!("## Critical\n- none\n## Important\n{imp}\n## Minor\n- none\n"),
        )
        .unwrap();
    }
    wait_pipeline("T-1 done", || {
        let t = json_of(&h, &["ledger", "show", "T-1", "--json"]);
        (t["task"]["status"] == "done").then_some(())
    });
    assert_eq!(fix_rounds_and_attempts(&h, 1), (2, 3));
}

#[test]
fn the_reviews_being_in_wakes_nobody_because_fleetd_carries_on() {
    let h = push_finish_world("tl-reviews-in", "exec,stay,commit,result");
    let run = seed_run_with(&h, "alpha", &[StepName::CodeSweep], None);
    let p = ticket_of(&h, "a", "Feature", "opus");
    seed_task_with(&h, &run, &p, "opus", |_| {});
    h.serve_fleetd();
    let reviews = wait_done_playing_reviewers(&h, "T-1");
    assert!(reviews > 0, "the code-sweep queued its reviews");
    let events = json_of(&h, &["ledger", "events", "--json"]);
    let woke: Vec<&Value> = events
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| {
            e["kind"] == "woke"
                && e["text"]
                    .as_str()
                    .is_some_and(|t| t.contains("The reviews of T-1 are in"))
        })
        .collect();
    assert!(woke.is_empty(), "{woke:?}");
    let sent = outbox(&h);
    assert!(
        !sent.iter().any(|w| w.contains("The reviews of T-1 are in")),
        "{sent:?}"
    );
}

#[test]
fn stopping_one_of_three_workers_leaves_the_others_running() {
    let mut h = push_fleet("tl-stop", "exec,stay", "delta", &["T0", "T1", "T2"]);
    h.set("HORCH_STOP_GRACE_MS", "2000");
    let ws = launched(&h, 3);
    let ids: Vec<String> = ws
        .iter()
        .map(|w| w["worker_id"].as_str().unwrap().to_string())
        .collect();
    let pids: Vec<(u32, u64)> = ws
        .iter()
        .map(|w| {
            (
                w["agent_pid"].as_i64().unwrap() as u32,
                w["agent_started"].as_i64().unwrap() as u64,
            )
        })
        .collect();
    let o = ok(&h, &["ledger", "stop", &ids[0], "the operator dropped it"]);
    assert!(o.contains("Stopped"), "{o}");
    wait_push("the stopped agent is gone", || {
        (!horch_core::procid::alive(pids[0].0, Some(pids[0].1))).then_some(())
    });
    let shown = wait_push("the stop of the pane", || {
        let shown = json_of(&h, &["ledger", "show", &ids[0], "--json"]);
        shown["stops"]
            .as_array()?
            .iter()
            .any(|s| s["target"] == "pane")
            .then_some(shown)
    });
    assert!(horch_core::procid::alive(pids[1].0, Some(pids[1].1)));
    assert!(horch_core::procid::alive(pids[2].0, Some(pids[2].1)));
    assert_eq!(shown["worker"]["status"], "stopped");
    let stops = shown["stops"].as_array().unwrap();
    let agent = stops.iter().find(|s| s["target"] == "agent").unwrap();
    assert_eq!(agent["pid"].as_i64().unwrap() as u32, pids[0].0);
    assert_eq!(agent["pid_started"].as_i64().unwrap() as u64, pids[0].1);
    assert_eq!(agent["signal"], "TERM");
    assert_eq!(agent["result"], "sent");
    assert!(
        stops
            .iter()
            .any(|s| s["target"] == "pane" && s["result"] == "pane_closed"),
        "{shown}"
    );
    let st = json_of(&h, &["ledger", "status", "--json"]);
    assert_eq!(
        st["counts"]["queued"], 1,
        "the task of the stopped worker is queued again"
    );
    assert_eq!(st["counts"]["running"], 2);
    let o = json_of(&h, &["ledger", "stop", &ids[0], "again", "--json"]);
    assert!(
        o["steps"]
            .as_array()
            .unwrap()
            .iter()
            .all(|s| s["result"] != "sent"),
        "{o}"
    );
    for id in &ids[1..] {
        ok(&h, &["ledger", "stop", id, "test teardown"]);
    }
}

#[test]
fn a_worker_with_no_progress_is_stalled_by_the_ladder_and_stopped_by_its_pid() {
    let mut h = push_fleet("tl-watch", "exec,stay", "eps", &["T0"]);
    h.set("HORCH_STOP_GRACE_MS", "2000");
    let w = launched(&h, 1).remove(0);
    let id = w["worker_id"].as_str().unwrap().to_string();
    let (pid, started) = (
        w["agent_pid"].as_i64().unwrap() as u32,
        w["agent_started"].as_i64().unwrap() as u64,
    );
    stall_by_the_ladder(&h, &id, pid, started);
    let shown = json_of(&h, &["ledger", "show", &id, "--json"]);
    assert_eq!(shown["worker"]["status"], "incomplete");
    assert_eq!(shown["worker"]["agent_pid"].as_i64(), Some(pid as i64));
    assert_ladder_texts(&shown["events"], "T-1");
    let db = horch_core::taskledger::Store::open(&h.ledger_db()).unwrap();
    let task_id = t_id(&db, &id);
    let stuck: Vec<String> = {
        let mut stmt = db
            .conn()
            .prepare("SELECT text FROM needs_you WHERE kind = 'stuck' AND state = 'open'")
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    assert_eq!(stuck.len(), 1, "{stuck:?}");
    assert!(stuck[0].contains("T-1: retry after 30 s"), "{}", stuck[0]);
    let t = json_of(&h, &["ledger", "show", "T-1", "--json"]);
    assert_eq!(t["task"]["status"], "queued");
    let tree = worktrees_of(&db, &task_id);
    assert_eq!(tree.len(), 1, "{tree:?}");
    h.advance_fleet_clock(30);
    let attempts: Vec<String> = wait_push("the launch of attempt 2 of T-1", || {
        let ids = attempt_ids(&db, &task_id);
        (ids.len() >= 2).then_some(ids)
    });
    assert!(!horch_core::procid::alive(pid, Some(started)));
    assert_eq!(attempts.len(), 2, "exactly 1 attempt 2: {attempts:?}");
    assert!(attempts[0].ends_with("/A1"), "{attempts:?}");
    assert!(attempts[1].ends_with("/A2"), "{attempts:?}");
    assert_eq!(worktrees_of(&db, &task_id), tree, "the same worktree");
}

fn t_id(db: &Store, worker: &str) -> String {
    db.conn()
        .query_row(
            "SELECT task_id FROM attempts WHERE worker_id = ?1",
            [worker],
            |r| r.get(0),
        )
        .unwrap()
}

fn attempt_ids(db: &Store, task_id: &str) -> Vec<String> {
    let mut stmt = db
        .conn()
        .prepare("SELECT id FROM attempts WHERE task_id = ?1 ORDER BY rowid")
        .unwrap();
    stmt.query_map([task_id], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn worktrees_of(db: &Store, task_id: &str) -> Vec<String> {
    let mut stmt = db
        .conn()
        .prepare("SELECT path FROM worktrees WHERE task_id = ?1 ORDER BY path")
        .unwrap();
    stmt.query_map([task_id], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// The checks of today see `worker` with no progress, but no stall round
/// reaps (docs/tickets/fleetd-p2/MAP.md, Q14). With no activity past
/// stall_after_s (300 s) of the claim, the stall ladder ends the attempt
/// `stalled` and stops the agent by its PID.
fn stall_by_the_ladder(h: &Harness, worker: &str, pid: u32, started: u64) {
    let mut verdicts = Vec::new();
    for _ in 0..3 {
        std::thread::sleep(Duration::from_millis(1100));
        let v =
            h.without_fleetd(|| json_of(h, &["ledger", "watch", "--once", "--force", "--json"]));
        let mine = v
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["worker_id"] == worker);
        verdicts.extend(mine.map(|c| c["verdict"].as_str().unwrap().to_string()));
    }
    assert!(!verdicts.iter().any(|v| v == "dead"), "{verdicts:?}");
    assert!(
        horch_core::procid::alive(pid, Some(started)),
        "no check stops the agent"
    );
    let db = horch_core::taskledger::Store::open(&h.ledger_db()).unwrap();
    let task_status = || -> String {
        db.conn()
            .query_row(
                "SELECT t.status FROM tasks t JOIN attempts a ON a.task_id = t.task_id
                 WHERE a.worker_id = ?1",
                [worker],
                |r| r.get(0),
            )
            .unwrap()
    };
    let status = h.without_fleetd(|| {
        assert!(
            horch_core::procid::alive(pid, Some(started)),
            "the agent lives before the stall pass"
        );
        h.advance_fleet_clock(400);
        json_of(h, &["ledger", "watch", "--once", "--force", "--json"]);
        task_status()
    });
    // The stall pass saw the old process alive: the task stays stalled, so
    // no claim starts a new attempt beside it (FM-14).
    assert_eq!(status, "stalled", "its old process lived in the pass");
    wait_for("the stop of the agent", || {
        (!horch_core::procid::alive(pid, Some(started))).then_some(())
    });
    // The next pass sees the old process gone: rung 6.
    h.without_fleetd(|| json_of(h, &["ledger", "watch", "--once", "--force", "--json"]));
    assert_eq!(task_status(), "ready", "rung 6 after the process is gone");
    let outcome: String = db
        .conn()
        .query_row(
            "SELECT outcome FROM attempts WHERE worker_id = ?1",
            [worker],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(outcome, "stalled");
}

/// Assert that the events have each event of horch for the stall ladder
/// end of `task`.
fn assert_ladder_texts(events: &Value, task: &str) {
    let by_horch: Vec<&str> = events
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["actor"] == "horch")
        .filter_map(|e| e["text"].as_str())
        .collect();
    let queued = format!("{task} is queued again");
    for text in [
        "the stall ladder ended it: stalled",
        "working -> incomplete",
        queued.as_str(),
    ] {
        assert!(
            by_horch.iter().any(|t| t.contains(text)),
            "{text}: {events}"
        );
    }
}

/// In `h`, run `zeta` with 1 running task of a worker whose agent is a
/// live child of the test, and the fleetd check before each command on.
/// The test runs within stall_after_s (300 s) of the claim on the fleet
/// clock, so the stall ladder does not stop the worker. The task is bound
/// to the worker (FR-5). Gives the fleetds of the test and the agent.
fn claimed_run(mut h: Harness) -> (Harness, Fleetds, Stand) {
    h.unset("HORCH_NO_WATCHDOG");
    let run = seed_run_with(&h, "zeta", StepName::ALL, Some(1));
    let p = ticket(&h, "t0", "# T0\n");
    seed_task(&h, &run, &p, "opus");
    // The test runs its own fleetd after the routed setup.
    h.stop_fleetd();
    let fleetds = Fleetds::new(&h);
    let agent = Stand::new();
    bind(&h, "opus", Some(agent.0.id()));
    (h, fleetds, agent)
}

/// A dead fleetd is started again by the next horch command, in stand-in
/// mode on this temporary ledger.
#[test]
fn a_dead_fleetd_is_restarted_by_the_next_command() {
    let (h, mut fleetds, _agent) = claimed_run(world("tl-fleetd-dead"));
    fleetds.start(&h, "fleetd-1");
    let (pid, _) = fleetds.kill_current();
    // `status` exits 1 while the run is not complete.
    let o = h.run(&["ledger", "status"]);
    assert!(
        text(&o).contains(&format!("fleetd PID {pid} is gone.")),
        "{}",
        text(&o)
    );
    let (new, started) = wait_for("a new fleetd in the row", || {
        fleetds.row().filter(|&(p, _)| p != pid)
    });
    assert!(
        horch_core::procid::alive(new, Some(started)),
        "the new fleetd runs"
    );
}

/// `HORCH_NO_WATCHDOG=1` turns off the fleetd check before each command
/// (MAP.md, Q17): a command does not start a dead fleetd again.
#[test]
fn the_no_watchdog_setting_turns_off_the_fleetd_check() {
    let h = world("tl-fleetd-off");
    project_file(
        &h,
        "fleet.toml",
        "[gates]\nfast = \"true\"\nfull = \"true\"\n",
    );
    git_commit_all(&h, "the gates of the project");
    seed_run(&h, "zeta", None, "w1:p1");
    // The test runs its own fleetd after the routed setup.
    h.stop_fleetd();
    let mut fleetds = Fleetds::new(&h);
    fleetds.start(&h, "fleetd-1");
    let (pid, started) = fleetds.kill_current();
    // `status` exits 1 while the run is not complete.
    let o = h.run(&["ledger", "status"]);
    assert!(!text(&o).contains("fleetd"), "{}", text(&o));
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(fleetds.row(), Some((pid, started)), "no fleetd was started");
}

/// 2 fleetds that start at once leave 1: the fleetd lock refuses the
/// second, which exits 0 and says so, and the row keeps the first.
#[test]
fn two_fleetds_started_at_once_leave_1() {
    let (h, mut fleetds, _agent) = claimed_run(world("tl-fleetd-twice"));
    let pids = [fleetds.spawn(&h, "fleetd-a"), fleetds.spawn(&h, "fleetd-b")];
    let (first, started) = wait_for("the fleetd row", || {
        fleetds.row().filter(|(p, _)| pids.contains(p))
    });
    let (second, name) = if first == pids[0] {
        (pids[1], "fleetd-b")
    } else {
        (pids[0], "fleetd-a")
    };
    let status = fleetds.wait_exit(second);
    let said = fleetds.output(name, "stdout");
    assert!(status.success(), "{status}: {said}");
    assert!(said.contains("Another fleetd holds"), "{said}");
    assert_eq!(
        fleetds.row(),
        Some((first, started)),
        "the row keeps the first"
    );
    assert!(
        horch_core::procid::alive(first, Some(started)),
        "the first runs"
    );
}

#[test]
fn the_import_finds_work_that_never_landed() {
    let h = world("tl-import");
    let proj = h.project.clone();
    let wtdir = proj.join(".worktrees");
    std::fs::create_dir_all(&wtdir).unwrap();
    let mk = |name: &str| {
        let p = wtdir.join(name);
        git_in(
            &h,
            &proj,
            &[
                "worktree",
                "add",
                "--quiet",
                "-b",
                name,
                &p.to_string_lossy(),
                "main",
            ],
        );
        p
    };
    // Unlanded: 2 commits ahead of main.
    let un = mk("w-unlanded");
    for n in 0..2 {
        std::fs::write(un.join(format!("f{n}.txt")), "x\n").unwrap();
        git_in(&h, &un, &["add", "."]);
        git_in(
            &h,
            &un,
            &["commit", "--quiet", "--no-gpg-sign", "-m", &format!("c{n}")],
        );
    }
    // Dirty: an uncommitted file, nothing ahead.
    let dirty = mk("w-dirty");
    std::fs::write(dirty.join("wip.txt"), "wip\n").unwrap();
    // Orphan: git knows it, no claim names it.
    mk("w-orphan");
    let landed = wtdir.join("w-landed"); // gone from disk
                                         // The old records spell each path through a symlink, as macOS spells `/tmp`
                                         // for `/private/tmp`; git lists the real paths, and the import must still
                                         // match them.
    let link = h.root.join("proj-link");
    std::os::unix::fs::symlink(&proj, &link).unwrap();
    let p = link.to_string_lossy();
    let ledger = format!(
        r#"[
 {{"record_id":"r1","session_id":null,"agent":"claude","tier":"opus","model":"opus","role":"opus-1","status":"done","task":"Old done work","history":[{{"at":"2026-10-01T00:00:00Z","event":"done","text":"Did it"}}],"created_at":"2026-10-01T00:00:00Z","updated_at":"2026-10-01T00:00:00Z","project":"{p}"}},
 {{"record_id":"r2","agent":"codex","tier":"codex-terra","model":"gpt","role":"codex-terra-1","status":"working","task":"Crashed work","history":[],"created_at":"2026-10-02T00:00:00Z","updated_at":"2026-10-02T00:00:00Z","project":"{p}","workspace_id":"w9"}},
 {{"record_id":"r3","agent":"claude","tier":"opus","model":"opus","role":"opus-2","status":"done","task":"No project field","history":[],"created_at":"x","updated_at":"x"}}
]"#
    );
    std::fs::write(h.state.join("old-project.json"), ledger).unwrap();
    std::fs::create_dir_all(h.state.join("worktrees")).unwrap();
    let claim = |id: &str, path: &Path, branch: &str, state: &str, record: &str| {
        let path = link.join(path.strip_prefix(&proj).unwrap());
        serde_json::json!({
            "id": id, "name": id, "role": "x", "record_id": record,
            "path": path.display().to_string(), "branch": branch, "base": "main",
            "base_commit": "0", "created_at": "2026-10-01T00:00:00Z", "state": state,
            "landed_commit": null, "reason": null,
        })
    };
    std::fs::write(
        h.state.join("worktrees/repo.json"),
        serde_json::json!([
            claim("c1", &landed, "w-landed", "landed", "r1"),
            claim("c2", &un, "w-unlanded", "open", "r1"),
            claim("c3", &dirty, "w-dirty", "open", "r2"),
        ])
        .to_string(),
    )
    .unwrap();

    let dry = json_of(&h, &["ledger", "import", "--dry-run", "--json"]);
    assert_eq!(dry["dry_run"], true);
    assert_eq!(
        json_of(&h, &["ledger", "unfinished", "--json"])
            .as_array()
            .unwrap()
            .len(),
        0,
        "a dry run keeps nothing"
    );

    let r = json_of(&h, &["ledger", "import", "--json"]);
    assert_eq!(r["raw_added"], 6, "{r}");
    assert_eq!(
        r["map_errors"], 1,
        "the record with no project keeps its raw row: {r}"
    );
    let f = &r["findings"];
    assert_eq!(f["worktree_unlanded"], 1, "{r}");
    assert_eq!(
        f["done_not_landed"], 1,
        "r1 is done but its claim c2 did not land: {r}"
    );
    assert_eq!(f["dirty_worktree"], 1, "{r}");
    assert_eq!(f["worktree_orphan"], 1, "{r}");
    assert_eq!(f["ended_without_done"], 1, "{r}");

    let again = json_of(&h, &["ledger", "import", "--json"]);
    assert_eq!(again["raw_added"], 0);
    assert_eq!(again["raw_skipped"], 6);
    assert!(again["findings"].as_object().unwrap().is_empty(), "{again}");

    let list = json_of(&h, &["ledger", "unfinished", "--json"]);
    let list = list.as_array().unwrap();
    assert_eq!(list.len(), 5);
    let orphan = list
        .iter()
        .find(|f| f["kind"] == "worktree_orphan")
        .unwrap();
    let o = h.run(&[
        "ledger",
        "dismiss",
        &orphan["finding_id"].to_string(),
        "x",
        "--cleanup",
    ]);
    assert!(
        !o.status.success() && text(&o).contains("no recorded worktree"),
        "{}",
        text(&o)
    );
    assert!(
        wtdir.join("w-orphan").exists(),
        "an orphan is never removed"
    );
    let unl = list
        .iter()
        .find(|f| f["kind"] == "worktree_unlanded")
        .unwrap();
    let o = h.run(&[
        "ledger",
        "dismiss",
        &unl["finding_id"].to_string(),
        "x",
        "--cleanup",
    ]);
    assert!(
        !o.status.success()
            && text(&o).contains("is orphaned: only a landed or rescued tree is removed"),
        "{}",
        text(&o)
    );
    assert!(un.exists());

    // Reopen it: the new task gets the old worktree.
    seed_run(&h, "again", None, "w1:p1");
    let t = ticket(&h, "finish-it", "# Finish the unlanded work\n");
    ok(
        &h,
        &[
            "ledger",
            "reopen",
            &unl["finding_id"].to_string(),
            "--file",
            &t.to_string_lossy(),
            "--primary",
            "opus",
        ],
    );
    // The import adopts the tree as `orphaned`: the bind does not take it
    // before its rescue (ticket 11 of fleetd-p3).
    let refused = try_bind(&h, "opus", None);
    assert!(
        matches!(&refused, Err(e) if e.contains("is orphaned: its rescue comes first")),
        "{refused:?}"
    );
}

#[test]
fn trf_43_assign_is_gone() {
    let mut h = fleet("tl-assign", "default");
    h.set("HORCH_NO_WATCHDOG", "1");
    seed_run(&h, "eta", None, "w1:p1");
    let o = h.run(&["assign", "opus-1", "do it"]);
    assert!(!o.status.success());
    assert!(
        text(&o).contains("unrecognized subcommand 'assign'"),
        "{}",
        text(&o)
    );
}

#[test]
fn the_whole_flow_on_the_fake_herdr() {
    horch_e2e::expected::check(file!(), "the_whole_flow_on_the_fake_herdr", || {
        let mut h = finish_world("tl-e2e");
        h.stop_fleetd();
        h.set("HORCH_STOP_GRACE_MS", "2000");
        push_env(&mut h, "exec,stay");

        // 1. The orchestrator queues B first: fleetd launches its agent, the
        // fake codex that stays and never ends with a result.
        let run = seed_run(&h, "alpha", Some(1), "w1:p1");
        let b = ticket_of(&h, "b", "B", "codex-terra");
        seed_blocked_task(&h, &run, &b, "codex-terra", None);
        h.serve_fleetd();
        let b = wait_push("B's agent PID", || {
            worker_rows(&h)
                .into_iter()
                .find(|w| w["persona"] == "codex-terra" && w["agent_started"].is_i64())
        });
        let wb = b["worker_id"].as_str().unwrap().to_string();
        let (bpid, bstart) = (
            b["agent_pid"].as_i64().unwrap() as u32,
            b["agent_started"].as_i64().unwrap() as u64,
        );
        // fleetd bound T-1 to B (FR-5).
        let t1 = json_of(&h, &["ledger", "show", "T-1", "--json"]);
        assert_eq!(t1["task"]["current_worker_id"], wb.as_str(), "{t1}");

        // 2. Then A and C, C blocked by A. Their agents commit their work and
        // end with horch result. fleetd launches A beside B; C waits.
        h.stop_fleetd();
        h.set("HORCH_FAKE_SCENARIO", "exec,stay,commit,result");
        for (n, t, blocked_by) in [("a", "A", None), ("c", "C", Some("T-2"))] {
            let p = ticket_of(&h, n, t, "opus");
            seed_blocked_task(&h, &run, &p, "opus", blocked_by);
        }
        h.serve_fleetd();
        let a_tree = wait_push("A's worktree", || task_worktree(&h, "T-2"));
        let workers: Vec<String> = worker_rows(&h)
            .iter()
            .map(|w| w["persona"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(workers.len(), 2, "1 worker for A and 1 for B: {workers:?}");
        assert!(workers.iter().any(|p| p == "opus"), "{workers:?}");
        let t3 = json_of(&h, &["ledger", "show", "T-3", "--json"]);
        assert_eq!(t3["task"]["status"], "queued", "{t3}");

        // 3. B stops making progress: its fake agent runs, but shows no timer
        // and writes nothing.
        // 4. A lands: its result passes the verifier and its reviews, its
        // folder is gone, and C can start.
        let reviews = land_by_push(&h, "T-2", &a_tree);
        assert!(reviews > 0, "the code-sweep of A queued its reviews");
        // FM-16: no orchestrator is woken for C. Under fleetd the wake
        // `Tasks can start` is retired (wakes.rs, `startable_wake`): the
        // dispatcher launches C with no command, in step 5.
        let events = json_of(&h, &["ledger", "events", "--json"]);
        assert!(
            !events
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["kind"] == "woke"
                    && e["text"].as_str().unwrap().contains("Tasks can start")),
            "no wake for C: {events}"
        );
        // 5. B's checks and the stall ladder; A's worker is untouched.
        stall_by_the_ladder(&h, &wb, bpid, bstart);
        assert_ladder_texts(&json_of(&h, &["ledger", "events", "--json"]), "T-1");
        // FM-16: no orchestrator is woken for B. With no live
        // sub-orchestrator of the feature, a stall writes no wake; the stall
        // ladder of fleetd acts on the attempt (wakes.rs).
        let wakes = outbox(&h);
        assert!(
            !wakes.iter().any(|w| w.contains("shows no progress")),
            "{wakes:?}"
        );
        // B waits for its retry backoff of 30 s on the fleet clock (rung 6).
        // Then fleetd launches C, and B again as attempt 2 of T-1, which
        // replaces the stalled worker.
        h.advance_fleet_clock(30);
        let c_tree = wait_push("C's worktree", || task_worktree(&h, "T-3"));
        let t1 = json_of(&h, &["ledger", "show", "T-1", "--json"]);
        let rb = wait_push("the replacement of B", || {
            worker_rows(&h)
                .into_iter()
                .find(|w| w["task_id"] == t1["task"]["task_id"] && w["attempt"] == 2)
        });
        // FM-16: fleetd launches attempt 2 of T-1 on a new worker; no
        // `horch spawn --replaces` names the stalled one (wakes.rs,
        // `spawn_advice`).
        assert_ne!(rb["worker_id"], wb.as_str(), "{rb}");

        // 6. No task is left for a spare worker of opus.
        let none = try_bind(&h, "opus", None);
        assert!(none.is_err(), "{none:?}");

        // 7. B (replaced) and C land; everything is done and cleaned.
        let b_tree = wait_push("B's worktree", || task_worktree(&h, "T-1"));
        land_by_push(&h, "T-1", &b_tree);
        land_by_push(&h, "T-3", &c_tree);
        let o = h.run(&["ledger", "status"]);
        assert_eq!(o.status.code(), Some(0), "{}", text(&o));
        let st = json_of(&h, &["ledger", "status", "--json"]);
        assert!(
            st["worktrees"]
                .as_array()
                .unwrap()
                .iter()
                .all(|w| w["status"] == "cleaned"),
            "{}\n{st}",
            not_cleaned(&st)
        );
        let body = std::fs::read_to_string(h.root.join("gh/body")).unwrap();
        for t in ["T-1", "T-2", "T-3"] {
            assert!(
                body.contains(&format!("<!-- horch:task {t} -->")),
                "{t} in the PR: {body}"
            );
        }
        // Every recorded stop names an exact PID or pane.
        let shown = json_of(&h, &["ledger", "show", &wb, "--json"]);
        for s in shown["stops"].as_array().unwrap() {
            assert!(s["pid"].is_i64() || s["pane_id"].is_string(), "{s}");
        }
        assert!(events.as_array().is_some());
    });
}

#[test]
fn a_push_review_task_ends_done_by_its_findings_file_and_its_task_lands() {
    let mut h = finish_world("tl-review");
    h.stop_fleetd();
    push_env(&mut h, "exec,stay,commit,result");
    let run = seed_run(&h, "alpha", Some(1), "w1:p1");
    let a = ticket_of(&h, "a", "A", "opus");
    seed_blocked_task(&h, &run, &a, "opus", None);
    h.serve_fleetd();
    let mut written = 0;
    wait_pipeline("T-1 done", || {
        written += write_findings(&h);
        let t = json_of(&h, &["ledger", "show", "T-1", "--json"]);
        (t["task"]["status"] == "done").then_some(())
    });
    assert!(written > 0, "the code-sweep of A queued its reviews");
    let store = Store::open(&h.ledger_db()).unwrap();
    let conn = store.conn();
    let rows = |sql: &str| -> Vec<String> {
        let mut stmt = conn.prepare(sql).unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    let reviews = rows("SELECT status FROM tasks WHERE kind = 'review'");
    assert!(!reviews.is_empty());
    assert!(reviews.iter().all(|s| s == "done"), "{reviews:?}");
    let outcomes = rows(
        "SELECT COALESCE(a.outcome, '') FROM tasks t JOIN attempts a ON a.task_id = t.task_id
         WHERE t.kind = 'review'
           AND a.rowid = (SELECT max(rowid) FROM attempts WHERE task_id = t.task_id)",
    );
    assert_eq!(outcomes.len(), reviews.len(), "{outcomes:?}");
    assert!(outcomes.iter().all(|o| o == "verified"), "{outcomes:?}");
    let trees = rows(
        "SELECT w.worktree_id FROM worktrees w JOIN tasks t ON t.task_id = w.task_id
         WHERE t.kind = 'review'",
    );
    assert!(trees.is_empty(), "{trees:?}");
    let checks = rows(
        "SELECT j.subject FROM jobs j JOIN tasks t ON t.task_id = j.subject
         WHERE j.kind = 'task_check' AND t.kind = 'review'",
    );
    assert!(checks.is_empty(), "{checks:?}");
    drop(store);
    wait_pipeline("the branch of T-1 landed", || task_branch_landed(&h, 1));
}

fn task_branch_landed(h: &Harness, seq: i64) -> Option<()> {
    let store = Store::open(&h.ledger_db()).unwrap();
    let landed: bool = store
        .conn()
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM branches b JOIN tasks t ON t.task_id = b.task_id
                 WHERE t.seq = ?1 AND b.landing_state = 'landed')",
            [seq],
            |r| r.get(0),
        )
        .unwrap();
    landed.then_some(())
}

/// The ticket `n` of the task `t` for `persona`, with no check.
fn ticket_of(h: &Harness, n: &str, t: &str, persona: &str) -> PathBuf {
    let body = format!("# {t}\n\n**Persona:** primary `{persona}`\n**Check:** none\n");
    ticket(h, n, &body)
}

/// The worktree path of the task `short` (`T-<n>`) in `horch ledger
/// status`, when fleetd has made it. Any status: an exec agent can end
/// before a poll sees its worktree open.
fn task_worktree(h: &Harness, short: &str) -> Option<PathBuf> {
    let st = json_of(h, &["ledger", "status", "--json"]);
    st["worktrees"]
        .as_array()?
        .iter()
        .find(|w| w["task"] == short)
        .map(|w| PathBuf::from(w["path"].as_str().unwrap()))
}

fn land_by_push(h: &Harness, short: &str, tree: &Path) -> usize {
    let written = wait_done_playing_reviewers(h, short);
    wait_push(
        &format!("the worktree of {short} is gone and cleaned"),
        || (!tree.exists() && tree_status(h, short).as_deref() == Some("cleaned")).then_some(()),
    );
    written
}

fn not_cleaned(st: &serde_json::Value) -> String {
    let trees = st["worktrees"].as_array().cloned().unwrap_or_default();
    trees
        .iter()
        .filter(|w| w["status"] != "cleaned")
        .map(|w| {
            let path = w["path"].as_str().unwrap_or_default();
            let name = Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let kind = if name.starts_with("gate-") {
                "gate tree"
            } else {
                "tree"
            };
            format!(
                "the {kind} {name} stays {}",
                w["status"].as_str().unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn tree_status(h: &Harness, short: &str) -> Option<String> {
    let st = json_of(h, &["ledger", "status", "--json"]);
    st["worktrees"]
        .as_array()?
        .iter()
        .find(|w| w["task"] == short)
        .and_then(|w| w["status"].as_str().map(str::to_string))
}

fn wait_done_playing_reviewers(h: &Harness, short: &str) -> usize {
    let mut written = 0;
    wait_pipeline(&format!("{short} done"), || {
        written += write_findings(h);
        let t = json_of(h, &["ledger", "show", short, "--json"]);
        (t["task"]["status"] == "done").then_some(())
    });
    written
}

/// Write the findings file of each review task that has none: 0 findings
/// of each level. Gives the count of files written.
fn write_findings(h: &Harness) -> usize {
    let store = Store::open(&h.ledger_db()).unwrap();
    let mut stmt = store
        .conn()
        .prepare(
            "SELECT spec_ref FROM tasks
             WHERE kind = 'review' AND spec_ref IS NOT NULL AND status != 'cancelled'",
        )
        .unwrap();
    let files: Vec<String> = stmt
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let mut written = 0;
    for f in files.iter().map(Path::new).filter(|f| !f.exists()) {
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, NO_FINDINGS).unwrap();
        written += 1;
    }
    written
}

/// The findings of a reviewer that finds nothing.
const NO_FINDINGS: &str = "## Critical\n- none\n## Important\n- none\n## Minor\n- none\n";

/// The output of `horch ledger <args>` as `worker`, success or not.
fn as_worker(h: &Harness, worker: &str, args: &[&str]) -> Output {
    let mut cmd = h.horch(args);
    cmd.env("HORCH_WORKER_ID", worker);
    cmd.output().unwrap()
}

struct AgentShell {
    child: std::process::Child,
    dir: PathBuf,
    horch: PathBuf,
}

impl AgentShell {
    fn start(h: &Harness) -> AgentShell {
        let dir = h.root.join("agent-shell");
        std::fs::create_dir_all(&dir).unwrap();
        use std::os::unix::process::CommandExt;
        let mut cmd = std::process::Command::new("/bin/sh");
        h.seal(&mut cmd);
        let child = cmd
            .arg("-c")
            .arg("while [ ! -f \"$0/go.sh\" ]; do /bin/sleep 0.05; done; . \"$0/go.sh\"")
            .arg(&dir)
            .process_group(0)
            .spawn()
            .expect("starting the agent shell");
        AgentShell {
            child,
            dir,
            horch: Harness::horch_bin(),
        }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn run(mut self, args: &[&str]) -> (i32, String) {
        let quote = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
        let words: Vec<String> = args.iter().map(|a| quote(a)).collect();
        let script = format!(
            "{} {} > {dir}/out 2>&1\necho $? > {dir}/code\n",
            quote(&self.horch.to_string_lossy()),
            words.join(" "),
            dir = quote(&self.dir.to_string_lossy()),
        );
        std::fs::write(self.dir.join("go.tmp"), script).unwrap();
        std::fs::rename(self.dir.join("go.tmp"), self.dir.join("go.sh")).unwrap();
        self.child.wait().unwrap();
        let code = std::fs::read_to_string(self.dir.join("code")).unwrap();
        let said = std::fs::read_to_string(self.dir.join("out")).unwrap();
        (code.trim().parse().unwrap(), said)
    }
}

impl Drop for AgentShell {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn a_shared_task_claims_on_the_run_branch_and_lands_only_new_clean_commits() {
    let h = push_finish_world("tl-shared", "exec,stay");
    let p = ticket(&h, "01-a", "# 01: Doc\n");
    git_commit_all(&h, "ticket");
    let run = seed_run_with(&h, "alpha", &[StepName::Check, StepName::Land], None);
    seed_task_with(&h, &run, &p, "opus", |new| {
        new.worktree_mode = WorktreeMode::Shared;
        new.land_check = Some("true".into());
    });
    let project = h.project.clone();
    git_in(&h, &project, &["switch", "--quiet", "fleet/alpha"]);
    // A shell of the test is the agent: its process is the agent of the live
    // attempt, so the `horch result` that it runs finds the attempt. The
    // test itself is no fleet process, so it can start fleetd (T-171).
    let agent = AgentShell::start(&h);
    let bound = bind(&h, "opus", Some(agent.pid()));
    assert!(
        bound.worktree.is_none(),
        "a shared task has no worktree: {bound:?}"
    );
    h.serve_fleetd();

    std::fs::write(&p, "# 01: Doc, edited\n").unwrap();
    std::fs::write(project.join("draft.md"), "draft\n").unwrap();
    commit_file(&h, &project, "doc.md");
    let head = git_in(&h, &project, &["rev-parse", "HEAD"]);
    let (code, said) = agent.run(&[
        "result",
        "--status",
        "completed",
        "--summary",
        "doc",
        "--commit",
        &head,
        "--file",
        "doc.md",
    ]);
    assert_eq!(code, 0, "{said}");
    wait_push("T-1 done", || {
        let t = json_of(&h, &["ledger", "show", "T-1", "--json"]);
        (t["task"]["status"] == "done").then_some(())
    });
    let log = git_in(&h, &project, &["log", "--oneline", "fleet/alpha"]);
    assert!(log.contains("doc.md"), "{log}");
    let store = Store::open(&h.ledger_db()).unwrap();
    let count = |sql: &str| -> i64 { store.conn().query_row(sql, [], |r| r.get(0)).unwrap() };
    assert_eq!(count("SELECT COUNT(*) FROM worktrees"), 0);
    assert_eq!(
        count("SELECT COUNT(*) FROM branches WHERE task_id IS NOT NULL"),
        0
    );
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "# 01: Doc, edited\n");
    assert_eq!(
        std::fs::read_to_string(project.join("draft.md")).unwrap(),
        "draft\n"
    );
}

const PUSH_SCENARIO: &str = "exec,stay,commit,result";

struct Claimed {
    task_id: String,
    worker_id: String,
    path: PathBuf,
    file: String,
    add_job: i64,
    agent: AgentShell,
}

impl Claimed {
    fn ready(&self, h: &Harness) {
        wait_push("the worktree_add job of the task", || {
            let store = Store::open_read_only(&h.ledger_db()).unwrap();
            let state: String = store
                .conn()
                .query_row(
                    "SELECT state FROM jobs WHERE id = ?1",
                    [self.add_job],
                    |r| r.get(0),
                )
                .unwrap();
            (state == "passed").then_some(())
        });
        commit_file(h, &self.path, &self.file);
    }

    fn finish(self) {
        let (code, said) = self.agent.run(&["ledger", "finish"]);
        assert_eq!(code, 0, "{said}");
    }
}

fn bind_claimed(h: &Harness, persona: &str, file: &str) -> Claimed {
    let agent = AgentShell::start(h);
    let bound = bind(h, persona, Some(agent.pid()));
    let tree = bound.worktree.expect("a worktree task has its tree");
    Claimed {
        task_id: bound.task_id,
        worker_id: bound.worker_id,
        path: PathBuf::from(tree.path),
        file: file.to_string(),
        add_job: tree.add_job.expect("the bind queues the worktree_add job"),
        agent,
    }
}

fn push_claimed(
    h: &Harness,
    persona: &str,
    steps: &str,
    check: Option<&str>,
    file: &str,
) -> Claimed {
    let steps: Vec<StepName> = steps
        .split(',')
        .map(|s| StepName::parse(s).unwrap())
        .collect();
    let run = seed_run_with(h, "alpha", &steps, None);
    // No check: the ticket says so, or the task does not land (TRF-17).
    let body = match check {
        Some(_) => "# 01: Feature\n",
        None => "# 01: Feature\n\n**Check:** none\n",
    };
    let p = ticket(h, "01-a", body);
    seed_task_with(h, &run, &p, persona, |new| {
        if let Some(c) = check {
            new.land_check = Some(c.to_string());
        }
    });
    bind_claimed(h, persona, file)
}

fn task_steps(h: &Harness, seq: i64) -> Vec<(String, String, String, String)> {
    let store = Store::open_read_only(&h.ledger_db()).unwrap();
    let mut stmt = store
        .conn()
        .prepare(
            "SELECT s.name, s.status, COALESCE(s.verdict, ''), COALESCE(s.note, '')
             FROM task_steps s JOIN tasks t ON t.task_id = s.task_id
             WHERE t.seq = ?1 ORDER BY s.rowid",
        )
        .unwrap();
    let rows = stmt
        .query_map([seq], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    rows
}

fn wait_step(h: &Harness, name: &str) -> (String, String, String) {
    wait_pipeline(&format!("the step {name} of T-1 ends"), || {
        task_steps(h, 1)
            .into_iter()
            .find(|s| s.0 == name && s.1 != "pending" && s.1 != "running")
            .map(|(_, status, verdict, note)| (status, verdict, note))
    })
}

/// Commit `file` in `dir`. fleetd reads the status of a worktree it owns, and
/// that read can hold the index lock for a moment: a git command waits for it.
fn commit_file(h: &Harness, dir: &Path, file: &str) {
    std::fs::write(dir.join(file), format!("{file}\n")).unwrap();
    git_locked(h, dir, &["add", file]);
    git_locked(h, dir, &["commit", "--quiet", "--no-gpg-sign", "-m", file]);
}

fn git_locked(h: &Harness, dir: &Path, args: &[&str]) {
    for _ in 0..100 {
        let mut c = h.git_cmd(args);
        c.current_dir(dir);
        let o = c.output().unwrap();
        if o.status.success() {
            return;
        }
        if !String::from_utf8_lossy(&o.stderr).contains("index.lock") {
            panic!("git {}: {}", args.join(" "), text(&o));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("git {}: the index lock stayed for 10 s", args.join(" "));
}

fn queue_check(h: &Harness, task: &str, limit_s: u64) -> i64 {
    let fleet_root = h.fleet_root();
    let config = config_file::load(&fleet_root).unwrap();
    let ctx = JobCtx {
        guard: None,
        config: &config,
        needs_you: &NeedsYouList,
    };
    let key = JobKey {
        kind: JobKind::TaskCheck,
        subject: task,
        input_key: "tip1",
    };
    let mut store = Store::open(&h.ledger_db()).unwrap();
    store
        .immediate(|tx| {
            let clock = clock_store::read(tx)?;
            let Queued::New(id) = jobs::queue(&ctx, tx, &key, None, clock)? else {
                anyhow::bail!("the task_check job of {task} was not queued");
            };
            tx.execute(
                "UPDATE jobs SET time_limit_s = ?1 WHERE id = ?2",
                [i64::try_from(limit_s)?, id],
            )?;
            Ok(id)
        })
        .unwrap()
}

fn job_row(h: &Harness, id: i64) -> jobs_store::JobRow {
    let store = Store::open_read_only(&h.ledger_db()).unwrap();
    jobs_store::get(store.conn(), id).unwrap().unwrap()
}

/// Wait until the process `pid` is gone. A killed process is reaped by
/// init, not by fleetd.
fn wait_gone(what: &str, pid: &str) {
    wait_for(&format!("the process in {what} to be gone"), || {
        let alive = std::process::Command::new("kill")
            .args(["-0", pid])
            .status()
            .unwrap()
            .success();
        (!alive).then_some(())
    });
}

fn sandbox_calls(h: &Harness) -> Vec<Value> {
    h.calls_of("codex")
        .into_iter()
        .filter(|c| c["argv"][0] == "sandbox")
        .collect()
}

/// The processes with `marker` in their command line.
fn marked(marker: &str) -> Vec<String> {
    horch_e2e::process::try_processes()
        .expect("listing the processes")
        .into_iter()
        .filter(|p| p.command.contains(marker))
        .map(|p| format!("{} {}", p.pid, p.command))
        .collect()
}

fn assert_check_killed_at_its_limit(name: &str, persona: &str) -> (Claimed, Harness) {
    let h = push_finish_world(name, PUSH_SCENARIO);
    // The marker is in the command line of each process of the check job
    // but `sleep`: on Linux the PIDs in the files are of the PID namespace
    // of the sandbox, not of the host.
    let marker = format!("tl-check-marker-{}-{name}", std::process::id());
    let check =
        format!("echo $PPID > host.pid; /bin/sleep 3600 & echo $! > sleep.pid; wait; : {marker}");
    let c = push_claimed(&h, persona, "check", Some(&check), "a.txt");
    h.serve_fleetd();
    c.ready(&h);
    if persona.starts_with("codex") {
        // macOS scans the fake codex at its first exec, which can take
        // longer than the limit of the check: start it once first. `debug
        // models` records no call.
        let warmed = std::process::Command::new(h.bin.join("codex"))
            .args(["debug", "models"])
            .output()
            .expect("warming the fake codex");
        assert!(warmed.status.success(), "warming the fake codex failed");
    }
    let job = queue_check(&h, &c.task_id, 2);
    let mut seen = false;
    let row = wait_push("the job to end at its time limit", || {
        let row = job_row(&h, job);
        seen = seen || !marked(&marker).is_empty();
        (row.state != JobState::Queued && row.state != JobState::Running).then_some(row)
    });
    assert_eq!(row.state, JobState::Abandoned, "{row:?}");
    assert!(seen, "the search finds the processes of the running check");
    for file in ["sleep.pid", "host.pid"] {
        let pid = std::fs::read_to_string(c.path.join(file))
            .unwrap_or_else(|e| panic!("the check wrote no {file}: {e}"))
            .trim()
            .to_string();
        if cfg!(not(target_os = "linux")) {
            wait_gone(file, &pid);
        }
    }
    // On Linux the sandbox ends with its namespace init, and the kernel
    // then ends `sleep` too.
    wait_for("each process of the check to be gone", || {
        marked(&marker).is_empty().then_some(())
    });
    (c, h)
}

#[test]
fn a_check_past_its_timeout_is_killed_with_its_children() {
    assert_check_killed_at_its_limit("tl-check-timeout", "opus");
}

#[test]
fn a_codex_workers_sandboxed_check_past_its_timeout_leaves_no_process() {
    let (c, h) = assert_check_killed_at_its_limit("tl-codex-check-timeout", "codex-sol");
    drop(c);
    assert_eq!(sandbox_calls(&h).len(), 1);
}

#[test]
fn a_codex_workers_check_cannot_write_outside_its_worktree() {
    for persona in ["codex-sol", "opus"] {
        let h = push_finish_world(&format!("tl-codex-check-{persona}"), PUSH_SCENARIO);
        let outside = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
        let escaped = outside.path().join("outside.txt");
        let check = format!("echo in > inside.txt && echo out > {}", escaped.display());
        let c = push_claimed(&h, persona, "check", Some(&check), "a.txt");
        h.serve_fleetd();
        c.ready(&h);
        let job = queue_check(&h, &c.task_id, 60);
        let row = wait_push("the check job to end", || {
            let row = job_row(&h, job);
            (row.state != JobState::Queued && row.state != JobState::Running).then_some(row)
        });
        let result = row.result_json.clone().unwrap_or_default();
        assert_eq!(row.state, JobState::Failed, "{persona}: {row:?}");
        assert!(result.contains("SR-4"), "{persona}: {result}");
        assert!(!escaped.exists(), "the check wrote outside its worktree");
        assert!(
            c.path.join("inside.txt").is_file(),
            "the worktree is writable"
        );
        let codex = sandbox_calls(&h);
        if persona == "codex-sol" {
            assert_eq!(codex.len(), 1, "{codex:?}");
            let writable: Vec<PathBuf> =
                serde_json::from_value(codex[0]["sandbox_writable"].clone()).unwrap();
            assert!(writable.contains(&c.path), "{codex:?}");
            assert!(
                writable.iter().all(|w| !escaped.starts_with(w)),
                "{codex:?}"
            );
        } else {
            assert!(codex.is_empty(), "{codex:?}");
        }
    }
}

#[test]
fn a_land_checks_outside_the_lock_and_lands_after_a_faster_task() {
    let h = push_finish_world("tl-land-race", PUSH_SCENARIO);
    let run = seed_run_with(&h, "alpha", &[StepName::Check, StepName::Land], None);
    let started = h.tmp.join("a-started");
    let release = h.tmp.join("a-release");
    let slow = format!(
        "/usr/bin/touch {}; i=0; while [ $i -lt 6000 ]; do [ -e {} ] && exit 0; /bin/sleep 0.1; i=$((i + 1)); done; exit 1",
        started.display(),
        release.display()
    );
    let mut claimed = Vec::new();
    for (name, check, file) in [("01-a", slow.as_str(), "a.txt"), ("02-b", "true", "b.txt")] {
        let p = ticket(&h, name, &format!("# {name}\n"));
        seed_task_with(&h, &run, &p, "opus", |new| {
            new.land_check = Some(check.to_string());
        });
        claimed.push(bind_claimed(&h, "opus", file));
    }
    h.serve_fleetd();
    for c in &claimed {
        c.ready(&h);
    }
    let b = claimed.pop().unwrap();
    let a = claimed.pop().unwrap();
    a.finish();
    wait_pipeline("A's check", || started.exists().then_some(()));
    b.finish();
    wait_pipeline("B lands while A checks", || task_branch_landed(&h, 2));
    assert!(task_branch_landed(&h, 1).is_none(), "A still checks");
    std::fs::write(&release, "").unwrap();
    wait_pipeline("A lands", || task_branch_landed(&h, 1));
    let project = h.project.clone();
    let log = git_in(&h, &project, &["log", "--format=%s", "fleet/alpha"]);
    let order: Vec<&str> = log
        .lines()
        .filter(|l| *l == "a.txt" || *l == "b.txt")
        .collect();
    assert_eq!(order, vec!["a.txt", "b.txt"], "A lands after B: {log}");
}

/// A run with only the code-sweep step and 1 claimed task with 1 commit.
fn sweep_pushed(h: &Harness) -> Claimed {
    push_claimed(h, "opus", "code-sweep", None, "a.txt")
}

fn sweep_started(name: &str, set: impl FnOnce(&mut Harness)) -> Harness {
    let mut h = push_finish_world(name, PUSH_SCENARIO);
    set(&mut h);
    let c = sweep_pushed(&h);
    h.serve_fleetd();
    c.ready(&h);
    c.finish();
    h
}

fn wait_reviews(h: &Harness) -> Vec<(String, String, String)> {
    wait_pipeline("the reviews of a round", || {
        Some(reviews(h)).filter(|r| !r.is_empty())
    })
}

/// (task short, status, claimable_by) of every review task, in seq order.
fn reviews(h: &Harness) -> Vec<(String, String, String)> {
    let st = json_of(h, &["ledger", "status", "--json"]);
    st["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| &t["task"])
        .filter(|t| t["kind"] == "review")
        .map(|t| {
            (
                format!("T-{}", t["seq"]),
                t["status"].as_str().unwrap().to_string(),
                t["claimable_by"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect()
}

#[test]
fn a_cancelled_review_is_queued_again_and_the_sweep_then_passes() {
    let h = sweep_started("tl-sweep-cancel", |_| {});
    let all = wait_reviews(&h);
    let n = all.len();
    ok(
        &h,
        &["ledger", "cancel", &all[0].0, "the reviewer cannot start"],
    );
    let all = wait_pipeline("the cancelled review is queued again", || {
        write_findings(&h);
        Some(reviews(&h)).filter(|r| r.len() == n + 1)
    });
    assert_eq!(
        all.iter().filter(|r| r.1 == "cancelled").count(),
        1,
        "{all:?}"
    );
    wait_done_playing_reviewers(&h, "T-1");
    let (status, verdict, _) = wait_step(&h, "code-sweep");
    assert_eq!((status.as_str(), verdict.as_str()), ("passed", "PASS"));
}

#[test]
fn only_the_orchestrator_waives_a_critical_finding() {
    let h = push_finish_world("tl-sweep-waive", PUSH_SCENARIO);
    let run = seed_run_with(&h, "alpha", &[StepName::CodeSweep], None);
    let p = ticket_of(&h, "a", "Feature", "opus");
    seed_task_with(&h, &run, &p, "opus", |_| {});
    h.serve_fleetd();
    let critical = "- a.txt:1 SQL injection. Fix: bind it";
    review_round_with(&h, critical, "- none");
    wait_pipeline("the fix attempt", || {
        (fix_rounds_and_attempts(&h, 1) == (1, 2)).then_some(())
    });
    let t = json_of(&h, &["ledger", "show", "T-1", "--json"]);
    let author = t["task"]["current_worker_id"].as_str().unwrap().to_string();

    let o = as_worker(&h, &author, &["ledger", "waive", "T-1", "F1", "it is mine"]);
    assert!(
        !o.status.success(),
        "the author may not waive: {}",
        text(&o)
    );
    assert!(text(&o).contains("only the orchestrator"), "{}", text(&o));
    ok(
        &h,
        &[
            "ledger",
            "waive",
            "T-1",
            "F1",
            "the query binds its parameters",
        ],
    );

    review_round_with(&h, critical, "- none");
    wait_done_playing_reviewers(&h, "T-1");
    let report = wait_pipeline("the evidence of the code-sweep", || {
        let store = Store::open_read_only(&h.ledger_db()).unwrap();
        let ev: Option<String> = store
            .conn()
            .query_row(
                "SELECT s.evidence_path FROM task_steps s JOIN tasks t ON t.task_id = s.task_id
                 WHERE t.seq = 1 AND s.name = 'code-sweep' AND s.status = 'passed'",
                [],
                |r| r.get(0),
            )
            .ok()
            .flatten();
        ev.and_then(|p| std::fs::read_to_string(p).ok())
    });
    assert!(
        report.contains("F1 [")
            && report.contains("waived by the orchestrator: the query binds its parameters"),
        "{report}"
    );
}

#[test]
fn a_missing_reviewer_prompt_queues_no_review() {
    let h = sweep_started("tl-sweep-noprompt", |h| {
        std::fs::remove_file(h.root.join("code-sweep/reviewers/clarity-checker.md")).unwrap();
    });
    let (status, _, note) = wait_step(&h, "code-sweep");
    assert_eq!(status, "failed", "{note}");
    assert!(note.contains("clarity-checker"), "{note}");
    assert!(reviews(&h).is_empty(), "{:?}", reviews(&h));
}

fn link_system_tools(h: &Harness) {
    for tool in ["mktemp", "cat", "rm"] {
        let real = ["/usr/bin", "/bin"]
            .iter()
            .map(|dir| Path::new(dir).join(tool))
            .find(|p| p.is_file())
            .unwrap_or_else(|| panic!("no {tool} on this host"));
        std::os::unix::fs::symlink(real, h.bin.join(tool)).unwrap();
    }
}

fn complexity_run(h: &Harness, files: &[&str]) {
    link_system_tools(h);
    let c = push_claimed(h, "opus", "complexity-sweep", None, files[0]);
    h.serve_fleetd();
    c.ready(h);
    for f in &files[1..] {
        commit_file(h, &c.path, f);
    }
    c.finish();
}

#[test]
fn a_rust_only_change_is_not_sent_to_the_complexity_analyser() {
    let mut h = push_finish_world("tl-complexity-rs", PUSH_SCENARIO);
    let called = h.tmp.join("analyser-called");
    let cli = h.write_warmed(
        "marking-complexity",
        format!(
            "#!/bin/sh\nif [ \"$1\" = warm ]; then exit 0; fi\n/usr/bin/touch {}\necho '{{\"files\":[],\"violations\":[]}}'\n",
            called.display()
        )
        .as_bytes(),
    );
    h.set("HORCH_COMPLEXITY_CLI", cli.to_string_lossy());
    complexity_run(&h, &["lib.rs", "node.gd"]);
    let (status, _, note) = wait_step(&h, "complexity-sweep");
    assert_eq!(status, "not_applicable", "{note}");
    assert!(!called.exists(), "the analyser parses TypeScript only");
}

#[test]
fn finish_finds_the_complexity_analyser_in_a_local_marketplace_of_the_store() {
    let mut h = push_finish_world("tl-complexity-store", PUSH_SCENARIO);
    // The store installed code-sweep from a local marketplace checkout. The
    // analyser sits in the same checkout, outside the store.
    let market = h.root.join("skill-marketplace/plugins");
    let dist = market.join("code/skills/complexity-sweep/code/cli/dist");
    std::fs::create_dir_all(&dist).unwrap();
    let called = h.tmp.join("analyser-called");
    std::fs::write(
        dist.join("index.js"),
        format!(
            "/usr/bin/touch {}\necho '{{\"files\":[],\"violations\":[]}}'\n",
            called.display()
        ),
    )
    .unwrap();
    let data = h.root.join("data");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(
        data.join("marketplace.lock"),
        serde_json::to_string(&serde_json::json!({
            "version": 1,
            "skills": [{
                "id": "code-sweep",
                "source": market.join("dev/skills/code-sweep").to_string_lossy(),
                "requested_revision": null,
                "resolved_commit": null,
                "version": "local+b9691d7496c9",
                "digest": "sha256:b9691d7496c98131e15b6c8c8ac80e3f97871d43fcda10d994f59915b2a831cc",
                "installed_at": "2026-10-28T09:45:37Z"
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    h.set("HORCH_DATA_DIR", data.to_string_lossy());
    h.set("HORCH_COMPLEXITY_CLI", "");
    // index.js is a shell script here: run it with sh as the node binary.
    h.set("HORCH_NODE_BIN", "/bin/sh");
    complexity_run(&h, &["a.ts"]);
    let (status, _, note) = wait_step(&h, "complexity-sweep");
    assert_eq!(status, "passed", "{note}");
    assert!(called.exists(), "the marketplace analyser ran");
}

#[test]
fn a_complexity_report_past_64_kib_is_read_whole() {
    let mut h = push_finish_world("tl-complexity-big", PUSH_SCENARIO);
    // Node exits before a piped stdout drains: a pipe gets the first 64 KiB.
    // The violation at the end of the report is past that.
    let mut files: Vec<String> = (0..2000)
        .map(|i| format!("{{\"filePath\":\"pad/file-{i:04}.ts\",\"functions\":[]}}"))
        .collect();
    files.push("{\"filePath\":\"a.ts\",\"functions\":[{\"line\":1}]}".to_string());
    let report = format!(
        "{{\"files\":[{}],\"violations\":[{{\"functionName\":\"big\",\"filePath\":\"a.ts\",\"line\":1,\"metric\":\"cyclomatic\",\"value\":30,\"threshold\":10}}]}}\n",
        files.join(",")
    );
    assert!(report.len() > 65_536);
    let report_path = h.root.join("report.json");
    std::fs::write(&report_path, &report).unwrap();
    let cli = h.write_warmed(
        "node-like-complexity",
        format!(
            "#!/bin/sh\nif [ -p /dev/stdout ]; then /usr/bin/head -c 65536 {r}; else /bin/cat {r}; fi\nexit 0\n",
            r = report_path.display()
        )
        .as_bytes(),
    );
    h.set("HORCH_COMPLEXITY_CLI", cli.to_string_lossy());
    complexity_run(&h, &["a.ts"]);
    let (status, verdict, note) = wait_step(&h, "complexity-sweep");
    assert_eq!(
        (status.as_str(), verdict.as_str()),
        ("failed", "1 violations"),
        "{note}"
    );
}

#[test]
fn an_outer_git_dir_reaches_no_child_of_finish() {
    let mut h = push_finish_world("tl-scrub", PUSH_SCENARIO);
    // Another repository, as a hook or `git rebase -x` would name it.
    let other = h.root.join("other");
    std::fs::create_dir_all(&other).unwrap();
    git_in(&h, &other, &["init", "--quiet", "-b", "main"]);
    std::fs::write(other.join("o.txt"), "o\n").unwrap();
    git_in(&h, &other, &["add", "o.txt"]);
    git_in(
        &h,
        &other,
        &["commit", "--quiet", "--no-gpg-sign", "-m", "other"],
    );
    let dumps = h.tmp.join("env");
    std::fs::create_dir_all(&dumps).unwrap();
    let cli = h.write_warmed(
        "complexity-env",
        format!(
            "#!/bin/sh\nif [ \"$1\" = warm ]; then exit 0; fi\n/usr/bin/env > {}/complexity.env\necho '{{\"files\":[],\"violations\":[]}}'\n",
            dumps.display()
        )
        .as_bytes(),
    );
    h.set("HORCH_COMPLEXITY_CLI", cli.to_string_lossy());
    let check = format!("/usr/bin/env > {}/check.env", dumps.display());
    link_system_tools(&h);
    let c = push_claimed(&h, "opus", "check,complexity-sweep", Some(&check), "a.ts");
    h.set("GIT_DIR", other.join(".git").to_string_lossy());
    h.set("GIT_WORK_TREE", other.to_string_lossy());
    h.serve_fleetd();
    h.unset("GIT_DIR");
    h.unset("GIT_WORK_TREE");
    c.ready(&h);
    c.finish();
    wait_pipeline("T-1 landed", || task_branch_landed(&h, 1));
    let project = h.project.clone();
    assert_eq!(git_in(&h, &project, &["show", "fleet/alpha:a.ts"]), "a.ts");
    let branches = git_in(&h, &other, &["branch", "--list"]);
    assert_eq!(
        branches.trim(),
        "* main",
        "the other repository is untouched"
    );
    for name in ["check", "complexity"] {
        let env = std::fs::read_to_string(dumps.join(format!("{name}.env")))
            .unwrap_or_else(|e| panic!("{name} ran: {e}"));
        for var in ["GIT_DIR=", "GIT_WORK_TREE="] {
            assert!(
                !env.lines().any(|l| l.starts_with(var)),
                "{name} got {var}: {env}"
            );
        }
    }
}

fn first_review_prompt(h: &Harness) -> String {
    wait_reviews(h);
    let st = json_of(h, &["ledger", "status", "--json"]);
    let file = st["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["task"]["kind"] == "review")
        .unwrap()["task"]["task_file"]
        .as_str()
        .unwrap()
        .to_string();
    std::fs::read_to_string(file).unwrap()
}

#[test]
fn finish_finds_the_code_sweep_skill_in_the_skill_store() {
    let h = sweep_started("tl-sweep-store", |h| {
        let data = h.root.join("data");
        let skill = data.join("skills/code-sweep/local+b9691d7496c9");
        std::fs::create_dir_all(skill.join("reviewers")).unwrap();
        std::fs::write(skill.join("SKILL.md"), "# code-sweep\n").unwrap();
        for entry in std::fs::read_dir(h.root.join("code-sweep/reviewers")).unwrap() {
            let p = entry.unwrap().path();
            std::fs::copy(&p, skill.join("reviewers").join(p.file_name().unwrap())).unwrap();
        }
        h.set("HORCH_DATA_DIR", data.to_string_lossy());
        h.set("HORCH_CODE_SWEEP_DIR", "");
    });
    let ticket = first_review_prompt(&h);
    assert!(
        ticket.contains("You are bug-hunter."),
        "the store prompt: {ticket}"
    );
}

/// The status of the finish step `complexity-sweep` of a task that commits
/// `a.ts`, and whether the analyser in `layout` (a folder of `h.root`) ran.
/// A folder named like the skill that holds no analyser sits first in the walk
/// and must be skipped.
fn complexity_step_with_the_analyser_in(name: &str, layout: &str) -> (String, bool) {
    let mut h = push_finish_world(name, PUSH_SCENARIO);
    let called = h.tmp.join("analyser-called");
    let skill = h.root.join(layout);
    let analyser = skill.join("code/cli/dist/index.js");
    std::fs::create_dir_all(analyser.parent().unwrap()).unwrap();
    std::fs::write(
        &analyser,
        format!(
            "/usr/bin/touch {}\necho '{{\"files\":[],\"violations\":[]}}'\n",
            called.display()
        ),
    )
    .unwrap();
    let decoy = skill.parent().unwrap().parent().unwrap();
    std::fs::create_dir_all(decoy.join("aa-decoy/complexity-sweep")).unwrap();
    h.set("HORCH_DATA_DIR", h.root.join("data").to_string_lossy());
    h.set("HORCH_COMPLEXITY_CLI", "");
    // index.js is a shell script here: run it with sh as the node binary.
    h.set("HORCH_NODE_BIN", "/bin/sh");
    complexity_run(&h, &["a.ts"]);
    let (status, _, _) = wait_step(&h, "complexity-sweep");
    (status, called.exists())
}

#[test]
fn finish_finds_the_complexity_analyser_in_the_plugins_of_the_home() {
    // <home>/.claude/plugins/<plugin>/<skill>/
    let (status, ran) = complexity_step_with_the_analyser_in(
        "tl-cx-plugins",
        "home/.claude/plugins/zz/complexity-sweep",
    );
    assert_eq!(status, "passed");
    assert!(ran, "the plugin analyser ran");
}

#[test]
fn finish_finds_the_complexity_analyser_in_the_skill_store() {
    // <data>/skills/<skill>/<source>+<hash>/
    let (status, ran) = complexity_step_with_the_analyser_in(
        "tl-cx-store",
        "data/skills/complexity-sweep/local+b9691d7496c9",
    );
    assert_eq!(status, "passed");
    assert!(ran, "the store analyser ran");
}

#[test]
fn finish_finds_the_code_sweep_skill_in_the_plugins_of_the_home() {
    let h = sweep_started("tl-sweep-plugins", |h| {
        let plugins = h.home.join(".claude/plugins");
        std::fs::create_dir_all(plugins.join("aa-decoy/code-sweep")).unwrap();
        let skill = plugins.join("zz/code-sweep");
        std::fs::create_dir_all(skill.join("reviewers")).unwrap();
        for entry in std::fs::read_dir(h.root.join("code-sweep/reviewers")).unwrap() {
            let p = entry.unwrap().path();
            std::fs::copy(&p, skill.join("reviewers").join(p.file_name().unwrap())).unwrap();
        }
        h.set("HORCH_DATA_DIR", h.root.join("data").to_string_lossy());
        h.set("HORCH_CODE_SWEEP_DIR", "");
    });
    let ticket = first_review_prompt(&h);
    assert!(
        ticket.contains("You are bug-hunter."),
        "the plugin prompt: {ticket}"
    );
}

// ─── #89: closing a run, and which run a command means ──────────────────────

/// `horch <args>` with extra environment variables.
fn run_env(h: &Harness, env: &[(&str, &str)], args: &[&str]) -> Output {
    let mut cmd = h.horch(args);
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.output().unwrap()
}

fn the_run(h: &Harness) -> Run {
    let root = h.project.canonicalize().unwrap();
    let store = Store::open(&h.ledger_db()).unwrap();
    runs::resolve(store.conn(), &root, None, None).unwrap().0
}

fn queue(h: &Harness, name: &str, persona: &str, extra: &[&str]) {
    let p = ticket(h, name, &format!("# {name}\n"));
    seed_task_with(h, &the_run(h), &p, persona, |new| {
        for pair in extra.chunks(2) {
            match pair {
                ["--blocked-by", task] => new.blocked_by.push(task.to_string()),
                ["--check", command] => new.land_check = Some(command.to_string()),
                _ => panic!("queue takes no {pair:?}"),
            }
        }
    });
}

/// Whether `ledger status` printed the board of run `slug`.
fn shows_run(o: &Output, slug: &str) -> bool {
    out(o).starts_with(&format!("Run {slug} (active)."))
}

/// The run a command means (`resolve_run`): `--run`, else `HORCH_RUN_ID`,
/// else the run of the worker in `HORCH_WORKER_ID`, else the 1 active run.
#[test]
fn a_command_means_the_run_it_names_before_the_run_of_its_worker() {
    let h = world("tl-which-run");
    seed_run(&h, "a", None, "w1:p1");
    let run_b = seed_run(&h, "b", None, "w1:p2");
    let in_b = seed_worker(&h, &run_b, "opus");

    // Nothing names a run, and 2 are active: the command refuses.
    let o = h.run(&["ledger", "status"]);
    assert!(!o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("2 active runs"), "{}", text(&o));

    // `--run` names it. The environment names it too, and `--run` wins.
    let o = h.run(&["ledger", "status", "--run", "a"]);
    assert!(shows_run(&o, "a"), "{}", text(&o));
    let o = run_env(&h, &[("HORCH_RUN_ID", "b")], &["ledger", "status"]);
    assert!(shows_run(&o, "b"), "{}", text(&o));
    let o = run_env(
        &h,
        &[("HORCH_RUN_ID", "b")],
        &["ledger", "status", "--run", "a"],
    );
    assert!(shows_run(&o, "a"), "{}", text(&o));

    // Without either, a worker means its own run.
    let as_worker = |extra: &[(&str, &str)], args: &[&str]| {
        let mut env = vec![("HORCH_WORKER_ID", in_b.as_str())];
        env.extend_from_slice(extra);
        run_env(&h, &env, args)
    };
    let o = as_worker(&[], &["ledger", "status"]);
    assert!(shows_run(&o, "b"), "{}", text(&o));
    // An empty `HORCH_RUN_ID` names nothing, so the worker's run still counts.
    let o = as_worker(&[("HORCH_RUN_ID", "")], &["ledger", "status"]);
    assert!(shows_run(&o, "b"), "{}", text(&o));
    // A name beats the worker, from the flag and from the environment.
    let o = as_worker(&[], &["ledger", "status", "--run", "a"]);
    assert!(shows_run(&o, "a"), "{}", text(&o));
    let o = as_worker(&[("HORCH_RUN_ID", "a")], &["ledger", "status"]);
    assert!(shows_run(&o, "a"), "{}", text(&o));

    // A worker the ledger does not know means no run.
    let o = run_env(
        &h,
        &[("HORCH_WORKER_ID", "no-such-worker")],
        &["ledger", "status"],
    );
    assert!(!o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("2 active runs"), "{}", text(&o));
}

#[test]
fn a_command_from_another_workspace_says_whose_run_it_uses() {
    let mut h = fleet("tl-which-ws", "default");
    h.set("HORCH_WORKSPACE_ID", "w1");
    seed_run(&h, "a", None, "w1:p1");
    let note = "horch: NOTE: this command uses run a of workspace w1, not of this workspace w2. \
                Name a run with --run <slug>.";
    let o = run_env(&h, &[("HORCH_WORKSPACE_ID", "w2")], &["ledger", "status"]);
    assert!(shows_run(&o, "a"), "{}", text(&o));
    let stderr = String::from_utf8_lossy(&o.stderr).into_owned();
    assert!(stderr.lines().any(|l| l == note), "{}", text(&o));

    // The run's own workspace and a command that names the run get no note.
    for (env, args) in [
        (&[][..], &["ledger", "status"][..]),
        (
            &[("HORCH_WORKSPACE_ID", "w2")][..],
            &["ledger", "status", "--run", "a"][..],
        ),
    ] {
        let o = run_env(&h, env, args);
        assert!(shows_run(&o, "a"), "{}", text(&o));
        assert!(
            !String::from_utf8_lossy(&o.stderr).contains("this command uses run"),
            "{}",
            text(&o)
        );
    }
}

fn close_run(h: &Harness, run: &Run, abandon: Option<&str>) -> anyhow::Result<runs::Closed> {
    let git = horch_core::taskledger::git::Git::new("git");
    let mut store = Store::open(&h.ledger_db()).unwrap();
    store.immediate(|tx| runs::close_run(tx, &git, run, abandon))
}

#[test]
fn a_closed_run_frees_the_project_for_the_next_run() {
    let h = world("tl-close");
    let run_a = seed_run(&h, "a", None, "w1:p1");
    queue(&h, "01-a", "opus", &[]);

    // A queued task holds the run open, and the refusal names it.
    let err = close_run(&h, &run_a, None).unwrap_err();
    assert!(format!("{err:#}").contains("T-1 (queued)"), "{err:#}");

    ok(&h, &["ledger", "cancel", "T-1", "not needed"]);
    let closed = close_run(&h, &run_a, None).unwrap();
    assert_eq!(closed.status, "done", "{closed:?}");
    let run_b = seed_run(&h, "b", None, "w1:p1");
    let o = h.run(&["ledger", "status"]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o));
    assert!(out(&o).starts_with("Run b (active)."), "{}", text(&o));

    // The slug of a closed run can be used again.
    close_run(&h, &run_b, None).unwrap();
    git_in(&h, &h.project, &["branch", "-D", "fleet/a"]);
    seed_run(&h, "a", None, "w1:p1");
    let runs = json_of(&h, &["ledger", "run", "list", "--json"]);
    let status: Vec<(&str, &str)> = runs
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r["slug"].as_str().unwrap(), r["status"].as_str().unwrap()))
        .collect();
    assert_eq!(status, [("a", "active"), ("b", "done"), ("a", "done")]);
}

#[test]
fn close_abandon_cancels_the_open_tasks() {
    let h = world("tl-abandon");
    let run = seed_run(&h, "a", None, "w1:p1");
    queue(&h, "01-a", "opus", &[]);
    let closed = close_run(&h, &run, Some("operator dropped it")).unwrap();
    assert_eq!(closed.status, "abandoned", "{closed:?}");
    let cancelled: Vec<&str> = closed.cancelled.iter().map(|c| c.0.as_str()).collect();
    assert_eq!(cancelled, ["T-1"], "{closed:?}");
    let t = json_of(&h, &["ledger", "show", "T-1", "--run", "a", "--json"]);
    assert_eq!(t["task"]["status"], "cancelled", "{t}");
}

/// Phase 1: `horch ledger watch` starts no loop. It says that fleetd does
/// the watch of each run, and exits.
#[test]
fn ledger_watch_points_at_fleetd() {
    let h = world("tl-watch-fleetd");
    seed_run_with(&h, "a", StepName::ALL, Some(10));
    let said = ok(&h, &["ledger", "watch", "--run", "a"]);
    assert!(said.contains("fleetd does the watch of each run"), "{said}");
}

/// The run a ledger worker row of `persona` belongs to.
fn run_of_worker(h: &Harness, run: &str, persona: &str) -> usize {
    json_of(h, &["ledger", "status", "--run", run, "--json"])["workers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|w| w["worker"]["persona"] == persona)
        .count()
}

fn spawn_args<'a>(pane: &'a str, extra: &[&'a str]) -> Vec<&'a str> {
    let mut v = vec!["spawn"];
    v.extend_from_slice(extra);
    v.extend_from_slice(&["--from-pane", pane, "--no-tile", "--force"]);
    v
}

#[test]
fn a_spawn_is_refused_as_retired_and_adds_no_worker() {
    let h = fleet("tl-spawn-text", "default");
    seed_run(&h, "a", None, "w1:p1");
    for extra in [
        &["codex-terra", "Review src/x.rs"][..],
        &["codex-terra"][..],
    ] {
        let o = h.run(&spawn_args("w1:p1", extra));
        assert!(!o.status.success(), "{}", text(&o));
        assert!(
            text(&o).contains("horch spawn is retired. fleetd launches each attempt."),
            "{}",
            text(&o)
        );
        assert_eq!(run_of_worker(&h, "a", "codex-terra"), 0);
    }
}

// ─── #190: no pre-claim row outlives its process ─────────────────────────────

fn worker_status(h: &Harness, id: &str) -> (String, String) {
    let w = worker_json(h, id);
    (
        w["status"].as_str().unwrap_or_default().to_string(),
        w["end_reason"].as_str().unwrap_or_default().to_string(),
    )
}

/// The wakes recorded for the run, oldest first.
fn wakes(h: &Harness) -> Vec<String> {
    wakes_of(h, None)
}

/// [`wakes`] of the run `run`. A failed conformance job of the fake agents
/// adds a drift idea, whose feature has its own active run.
fn wakes_of(h: &Harness, run: Option<&str>) -> Vec<String> {
    let mut args = vec!["ledger", "events", "--json"];
    args.extend(run.iter().flat_map(|r| ["--run", *r]));
    json_of(h, &args)
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["kind"] == "woke")
        .map(|e| e["text"].as_str().unwrap().to_string())
        .collect()
}

/// The text of each outbox row, oldest first. The pass of fleetd writes
/// its wakes to the outbox and delivers them from there, so a wake that
/// `HORCH_NO_WAKE=1` keeps is a row, with no `woke` event.
fn outbox(h: &Harness) -> Vec<String> {
    let store = horch_core::taskledger::Store::open(&h.ledger_db()).unwrap();
    let mut stmt = store
        .conn()
        .prepare("SELECT text FROM outbox ORDER BY rowid")
        .unwrap();
    let rows = stmt.query_map([], |r| r.get::<_, String>(0)).unwrap();
    rows.map(|r| r.unwrap()).collect()
}

fn seed_parked_task(h: &Harness, short: &str) {
    let store = Store::open(&h.ledger_db()).unwrap();
    let moved = store
        .conn()
        .execute(
            "UPDATE tasks SET status = 'escalated', held_reason = 'unlanded', attempts = 1
             WHERE seq = ?1",
            [short.trim_start_matches("T-").parse::<i64>().unwrap()],
        )
        .unwrap();
    assert_eq!(moved, 1, "{short} is a task of the ledger");
}

fn needs_you(h: &Harness, kind: &str) -> Vec<String> {
    let store = Store::open(&h.ledger_db()).unwrap();
    let mut stmt = store
        .conn()
        .prepare("SELECT text FROM needs_you WHERE kind = ?1 AND state = 'open' ORDER BY key")
        .unwrap();
    let rows = stmt.query_map([kind], |r| r.get::<_, String>(0)).unwrap();
    rows.map(|r| r.unwrap()).collect()
}

fn pass(h: &Harness) {
    let slug: String = Store::open(&h.ledger_db())
        .unwrap()
        .conn()
        .query_row("SELECT slug FROM runs ORDER BY rowid LIMIT 1", [], |r| {
            r.get(0)
        })
        .unwrap();
    h.without_fleetd(|| ok(h, &["ledger", "watch", "--once", "--force", "--run", &slug]));
}

fn later(seconds: u64) -> String {
    horch_core::clock::stamp(horch_core::clock::now() + Duration::from_secs(seconds))
}

/// A process that stands in for a live supervisor and agent; killed on drop.
struct Stand(std::process::Child);

impl Stand {
    fn new() -> Stand {
        use std::os::unix::process::CommandExt;
        Stand(
            std::process::Command::new("sleep")
                .arg("300")
                .process_group(0)
                .spawn()
                .unwrap(),
        )
    }

    fn pid(&self) -> String {
        self.0.id().to_string()
    }
}

impl Drop for Stand {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn a_launch_of_an_unknown_teammate_leaves_no_live_row() {
    let mut h = world_without_fleetd("tl-unknown");
    push_env(&mut h, "default");
    let run = seed_run(&h, "a", Some(1), "w1:p1");
    let p = ticket(&h, "01-a", "# 01-a\n");
    seed_task(&h, &run, &p, "no-such-teammate");
    h.serve_fleetd();
    let rows = wait_push("a worker row that is not live", || {
        let ws = worker_rows(&h);
        (!ws.is_empty() && ws.iter().all(|w| w["status"] != "spawning")).then_some(ws)
    });
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["status"], "launch_failed", "{rows:?}");
    let unknown = horch_core::roster::Roster::builtin()
        .unwrap()
        .require("no-such-teammate")
        .unwrap_err();
    assert!(
        unknown
            .to_string()
            .starts_with("unknown teammate 'no-such-teammate'."),
        "{unknown}"
    );
    assert_eq!(
        rows[0]["end_reason"].as_str().unwrap(),
        format!("its spawn failed: {unknown}")
    );
}

#[test]
fn the_watchdog_ends_pre_claim_rows_whose_process_is_gone() {
    let h = world("tl-ghosts");
    let run = seed_run(&h, "a", None, "w1:p1");
    queue(&h, "01-a", "sonnet-x", &[]);
    let ghost = seed_worker(&h, &run, "sonnet-x");
    let ghost2 = seed_worker(&h, &run, "sonnet-x");
    // PIDs far above any pid_max: never a live process.
    let mut store = Store::open(&h.ledger_db()).unwrap();
    store
        .immediate(|tx| {
            workers::supervisor_started(tx, &ghost2, 99999991, None)?;
            workers::agent_started(tx, &ghost2, 99999992, None)
        })
        .unwrap();

    // 1 pass now: ghost2's supervisor is gone; ghost is in its launch grace.
    h.without_fleetd(|| ok(&h, &["ledger", "watch", "--once", "--force", "--run", "a"]));
    let (st, why) = worker_status(&h, &ghost2);
    assert_eq!(st, "no_task", "{why}");
    assert!(why.contains("supervisor is gone"), "{why}");
    assert_eq!(worker_status(&h, &ghost).0, "spawning");

    let o = run_env(
        &h,
        &[("HORCH_NOW", &later(200))],
        &["ledger", "status", "--run", "a"],
    );
    let line = out(&o)
        .lines()
        .find(|l| l.contains("spawning"))
        .unwrap_or_default()
        .to_string();
    assert!(line.ends_with("STALE"), "{}", text(&o));
    let before = outbox(&h).len();
    let o = h.without_fleetd(|| {
        run_env(
            &h,
            &[("HORCH_NOW", &later(200))],
            &["ledger", "watch", "--once", "--force", "--run", "a"],
        )
    });
    assert!(o.status.success(), "{}", text(&o));
    let (st, why) = worker_status(&h, &ghost);
    assert_eq!(st, "launch_failed", "{why}");
    assert!(why.contains("no horch worker checked in"), "{why}");
    assert_eq!(outbox(&h).len(), before, "{:?}", outbox(&h));
}

#[test]
fn a_live_worker_that_does_not_claim_is_reported_once() {
    let h = world("tl-noclaim");
    let run = seed_run(&h, "a", None, "w1:p1");
    seed_limits(&h, &run, None, Some(100000));
    queue(&h, "01-a", "sonnet-x", &[]);
    let stand = Stand::new();
    let me: u32 = stand.pid().parse().unwrap();
    let mut store = Store::open(&h.ledger_db()).unwrap();
    let id = store
        .immediate(|tx| {
            let id = workers::insert(
                tx,
                &NewWorker {
                    run_id: run.run_id.clone(),
                    persona: "sonnet-x".into(),
                    workspace_id: run.workspace_id.clone(),
                    pane_id: Some("w1:p7".into()),
                    ..Default::default()
                },
            )?;
            let started = horch_core::procid::start_time(me);
            workers::supervisor_started(tx, &id, me, started)?;
            workers::agent_started(tx, &id, me, started)?;
            Ok(id)
        })
        .unwrap();
    let watch = |at: Option<String>| {
        let env: Vec<(&str, &str)> = at.iter().map(|t| ("HORCH_NOW", t.as_str())).collect();
        let o = h.without_fleetd(|| {
            run_env(
                &h,
                &env,
                &["ledger", "watch", "--once", "--force", "--run", "a"],
            )
        });
        assert!(o.status.success(), "{}", text(&o));
    };
    // Within the 3 min claim grace (TRF-23) the worker is never touched.
    watch(Some(later(150)));
    assert_eq!(worker_status(&h, &id).0, "claiming");
    assert!(outbox(&h).is_empty(), "{:?}", outbox(&h));
    watch(Some(later(200)));
    watch(Some(later(300)));
    assert!(outbox(&h).is_empty(), "{:?}", outbox(&h));
    assert_eq!(worker_status(&h, &id).0, "claiming");
}

#[test]
fn replacing_a_live_worker_is_refused_as_retired_and_leaves_the_worker_live() {
    let h = fleet("tl-replaces-live", "default");
    let run = seed_run(&h, "a", None, "w1:p1");
    let live = seed_worker(&h, &run, "codex-terra");
    let o = h.run(&spawn_args("w1:p1", &["codex-terra", "--replaces", &live]));
    assert!(!o.status.success(), "{}", text(&o));
    assert!(
        text(&o).contains("horch spawn is retired. fleetd launches each attempt."),
        "{}",
        text(&o)
    );
    assert_eq!(worker_status(&h, &live).0, "spawning");
    assert_eq!(run_of_worker(&h, "a", "codex-terra"), 1);
}

// ─── #168: a task that kills every worker is held, not retried forever ──────

fn claim_and_fail(h: &Harness, persona: &str) -> String {
    let (not_before, clock): (Option<i64>, i64) = {
        let store = Store::open(&h.ledger_db()).unwrap();
        let not_before = store
            .conn()
            .query_row("SELECT not_before FROM tasks", [], |r| r.get(0))
            .unwrap();
        let clock = store
            .conn()
            .query_row("SELECT fleet_clock_s FROM fleetd", [], |r| r.get(0))
            .unwrap();
        (not_before, clock)
    };
    if let Some(nb) = not_before.filter(|nb| *nb > clock) {
        h.advance_fleet_clock(nb - clock);
    }
    let agent = Stand::new();
    let b = wait_push("the rescue of the tree of the last attempt", || {
        try_bind(h, persona, Some(agent.0.id())).ok()
    });
    drop(agent);
    pass(h);
    b.worker_id
}

#[test]
fn a_task_whose_attempts_all_fail_is_failed_and_its_replan_requeues_it() {
    let h = world_without_fleetd("tl-held");
    std::fs::create_dir_all(h.fleet_root()).unwrap();
    std::fs::write(h.fleet_root().join("fleet.toml"), "pass_period_s = 1\n").unwrap();
    h.serve_fleetd();
    let run = seed_run(&h, "a", None, "w1:p1");
    seed_limits(&h, &run, Some(2), None);
    queue(&h, "01-a", "sonnet-x", &[]);

    claim_and_fail(&h, "sonnet-x");
    let t = json_of(&h, &["ledger", "show", "T-1", "--run", "a", "--json"]);
    assert_eq!(t["task"]["status"], "queued", "{t}");
    assert!(t["task"]["held_reason"].is_null(), "{t}");
    let rows_before = outbox(&h).len();

    claim_and_fail(&h, "sonnet-x");
    let t = json_of(&h, &["ledger", "show", "T-1", "--run", "a", "--json"]);
    assert_eq!(t["task"]["status"], "failed", "{t}");
    assert!(t["task"]["held_reason"].is_null(), "{t}");
    assert!(try_bind(&h, "sonnet-x", None).is_err());
    let new = outbox(&h).split_off(rows_before);
    let questions: Vec<&String> = new.iter().filter(|w| w.contains("QUESTION:")).collect();
    assert_eq!(questions.len(), 1, "{new:?}");
    for part in [
        "replan T-1: requeue it 1 time, or cancel it",
        "horch ledger requeue T-1",
        "horch ledger cancel T-1",
        "2 attempts failed (dead: the process of attempt ",
        "A1 is gone; dead: the process of attempt ",
        "A2 is gone)",
    ] {
        assert!(questions[0].contains(part), "{part}: {new:?}");
    }
    assert!(!new.iter().any(|w| w.contains("--replaces")), "{new:?}");
    assert!(
        !new.iter().any(|w| w.contains("Spawn:")),
        "a failed task asks for no worker: {new:?}"
    );
    let st = out(&h.run(&["ledger", "status"]));
    assert!(st.contains("T-1   failed"), "{st}");
    assert!(
        st.contains("[2 attempts; ends: dead: the process of attempt "),
        "{st}"
    );
    // A failed task is open work: the run does not close.
    let err = close_run(&h, &the_run(&h), None).unwrap_err();
    assert!(format!("{err:#}").contains("T-1 (failed)"), "{err:#}");

    ok(&h, &["task", "requeue", "T-1", "ticket fixed"]);
    let t = json_of(&h, &["ledger", "show", "T-1", "--run", "a", "--json"]);
    assert_eq!(t["task"]["replans"], 1, "{t}");
    let agent = Stand::new();
    let b = wait_push("the rescue of the tree of T-1", || {
        try_bind(&h, "sonnet-x", Some(agent.0.id())).ok()
    });
    assert_eq!(b.task_id, t["task"]["task_id"], "{b:?}");
    assert_eq!(b.attempt, 3, "{b:?}");
    drop(agent);
    pass(&h);
    let t = json_of(&h, &["ledger", "show", "T-1", "--run", "a", "--json"]);
    assert_eq!(t["task"]["status"], "queued", "{t}");
    assert!(t["task"]["held_reason"].is_null(), "{t}");
    assert_eq!(t["task"]["attempts"], 3, "{t}");
}

#[test]
fn requeue_refuses_a_task_that_is_neither_held_nor_cancelled() {
    let h = world("tl-requeue-queued");
    seed_run(&h, "a", None, "w1:p1");
    queue(&h, "01-a", "opus", &[]);
    let o = h.run(&["ledger", "requeue", "T-1", "why"]);
    assert!(!o.status.success());
    assert!(
        text(&o).contains("only a held task is requeued"),
        "{}",
        text(&o)
    );
}

// ─── #119: dependents of a cancelled task are named, and can be freed ───────

#[test]
fn a_cancel_names_the_tasks_that_wait_for_it() {
    let h = world("tl-cancel-deps");
    seed_run(&h, "a", None, "w1:p1");
    queue(&h, "01-a", "opus-a", &[]);
    queue(&h, "02-b", "opus-b", &["--blocked-by", "T-1"]);
    queue(&h, "03-c", "opus-b", &["--blocked-by", "T-2"]);
    let o = ok(&h, &["ledger", "cancel", "T-1", "dropped"]);
    assert!(o.contains("Waiting for T-1: T-2, T-3."), "{o}");
    let st = out(&h.run(&["ledger", "status"]));
    assert!(
        st.contains("[T-1 was cancelled and cannot run again: drop the dependency, or cancel this task. A new task from its ticket does not end this wait]"),
        "{st}"
    );
    pass(&h);
    assert!(wakes(&h).is_empty(), "{:?}", wakes(&h));
    let items = needs_you(&h, "blocker");
    assert_eq!(items.len(), 1, "{items:?}");
    assert!(
        items[0].contains(
            "NOTE: T-1 was cancelled. T-2, T-3 wait for it. Cancel them with horch task cancel"
        ),
        "{items:?}"
    );
    assert!(try_bind(&h, "opus-b", None).is_err(), "T-2 waits for T-1");
}

#[test]
fn a_parked_unlanded_task_names_its_dependents_and_a_stuck_run_is_reported_once() {
    let h = world("tl-unlanded-deps");
    seed_run(&h, "a", None, "w1:p1");
    queue(&h, "01-a", "opus-a", &[]);
    queue(&h, "02-b", "opus-b", &["--blocked-by", "T-1"]);
    seed_parked_task(&h, "T-1");
    for _ in 0..3 {
        pass(&h);
    }
    assert!(
        wakes_of(&h, Some("a")).is_empty(),
        "{:?}",
        wakes_of(&h, Some("a"))
    );
    let blockers = needs_you(&h, "blocker");
    assert_eq!(blockers.len(), 1, "{blockers:?}");
    assert!(
        blockers[0].contains(
            "NOTE: T-2 waits: T-1 is parked unlanded. Run horch task requeue T-1 to resume it."
        ),
        "{blockers:?}"
    );
    let stuck = needs_you(&h, "stuck");
    assert_eq!(stuck.len(), 1, "{stuck:?}");
    assert!(stuck[0].contains("T-1 is held (unlanded)"), "{stuck:?}");
    assert!(
        stuck[0].contains("T-2: T-1 is parked unlanded"),
        "{stuck:?}"
    );
}

#[test]
fn a_stuck_run_wakes_from_the_watchdog_when_no_task_finishes() {
    let h = world("tl-stuck-watch");
    seed_run(&h, "a", None, "w1:p1");
    queue(&h, "01-a", "opus-a", &[]);
    queue(&h, "02-b", "opus-b", &["--blocked-by", "T-1"]);
    // A live worker holds T-1: the run is not stuck.
    let stand = Stand::new();
    let me = stand.0.id();
    let w = bind(&h, "opus-a", Some(me)).worker_id;
    {
        let mut store = Store::open(&h.ledger_db()).unwrap();
        store
            .immediate(|tx| {
                horch_core::taskledger::testing::record_supervisor(
                    tx,
                    &w,
                    me,
                    horch_core::procid::start_time(me),
                )
            })
            .unwrap();
    }
    pass(&h);
    assert!(
        needs_you(&h, "stuck").is_empty(),
        "{:?}",
        needs_you(&h, "stuck")
    );
    // Its worker is stopped and T-1 cancelled: nothing will start.
    ok(&h, &["ledger", "cancel", "T-1", "dropped"]);
    pass(&h);
    pass(&h);
    let stuck = needs_you(&h, "stuck");
    assert_eq!(stuck.len(), 1, "{stuck:?}");
    assert!(stuck[0].contains("the run is stuck:"), "{stuck:?}");
}

// ─── #136: a cancelled task never lands or turns done ───────────────────────

fn rev(h: &Harness, what: &str) -> String {
    git_in(h, &h.project.clone(), &["rev-parse", what])
        .trim()
        .to_string()
}

fn remote_refs(h: &Harness) -> String {
    git_in(h, &h.project.clone(), &["ls-remote", "origin"])
}

fn check_world(
    name: &str,
    steps: &[StepName],
    check: &str,
    fleet_toml: &str,
) -> (Harness, Claimed) {
    let h = push_finish_world(name, PUSH_SCENARIO);
    let toml = format!("pass_period_s = 1\n{fleet_toml}");
    std::fs::write(h.fleet_root().join("fleet.toml"), toml).unwrap();
    let run = seed_run_with(&h, "alpha", steps, None);
    let p = ticket(&h, "01-a", "# 01: Feature\n");
    seed_task_with(&h, &run, &p, "opus", |new| {
        new.land_check = Some(check.to_string());
    });
    let c = bind_claimed(&h, "opus", "c.txt");
    let pid = c.agent.pid();
    Store::open(&h.ledger_db())
        .unwrap()
        .immediate(|tx| {
            horch_core::taskledger::testing::record_supervisor(
                tx,
                &c.worker_id,
                pid,
                horch_core::procid::start_time(pid),
            )?;
            tx.execute(
                "UPDATE workers SET host = 'test-host' WHERE worker_id = ?1",
                [&c.worker_id],
            )?;
            Ok(())
        })
        .unwrap();
    h.serve_fleetd();
    c.ready(&h);
    (h, c)
}

fn worktree_status(h: &Harness, short: &str) -> String {
    let st = json_of(h, &["ledger", "status", "--json"]);
    st["worktrees"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["task"] == short)
        .map(|w| w["status"].as_str().unwrap().to_string())
        .unwrap_or_default()
}

#[test]
fn a_cancelled_task_does_not_land_or_turn_done() {
    let (h, mut c) = check_world(
        "tl-cancel-land",
        &[StepName::Land, StepName::Cleanup],
        "true",
        "stop_grace_s = 1\n",
    );
    let before = (rev(&h, "fleet/alpha"), remote_refs(&h));

    let o = ok(&h, &["ledger", "cancel", "T-1", "operator dropped it"]);
    assert!(
        o.contains("T-1 cancelled.") || o.contains("T-1 is not cancelled yet"),
        "{o}"
    );
    // fleetd stops the agent. The test reaps it, as the pane shell of a real
    // agent does: a zombie still has its pid, so fleetd sees it alive.
    wait_push("fleetd stops the agent of T-1", || {
        c.agent.child.try_wait().unwrap().map(|_| ())
    });
    wait_push("T-1 is cancelled", || {
        let t = json_of(&h, &["ledger", "show", "T-1", "--json"]);
        (t["task"]["status"] == "cancelled").then_some(())
    });
    assert_eq!(worker_status(&h, &c.worker_id).0, "stopped");

    wait_push("the worktree of T-1 is abandoned", || {
        (worktree_status(&h, "T-1") == "abandoned").then_some(())
    });
    assert_eq!((rev(&h, "fleet/alpha"), remote_refs(&h)), before);
    let t = json_of(&h, &["ledger", "show", "T-1", "--json"]);
    assert_eq!(t["task"]["status"], "cancelled", "{t}");

    let st = out(&h.run(&["ledger", "status"]));
    assert!(st.contains("T-1   abandoned"), "{st}");
    assert!(c.path.join("c.txt").exists());
    let unf = ok(&h, &["ledger", "unfinished"]);
    assert!(unf.contains("T-1 was cancelled with its worktree"), "{unf}");

    let o = h.run(&["ledger", "requeue", "T-1", "wanted after all"]);
    assert!(!o.status.success(), "{}", text(&o));
    assert!(text(&o).contains(": cancelled -> ready"), "{}", text(&o));
    assert!(c.path.join("c.txt").exists());
    let unf = ok(&h, &["ledger", "unfinished"]);
    assert!(unf.contains("T-1 was cancelled with its worktree"), "{unf}");
}

fn check_job_of(h: &Harness, task_id: &str) -> Option<i64> {
    let store = Store::open_read_only(&h.ledger_db()).unwrap();
    store
        .conn()
        .query_row(
            "SELECT id FROM jobs WHERE kind = 'task_check' AND subject = ?1 ORDER BY id LIMIT 1",
            [task_id],
            |r| r.get(0),
        )
        .ok()
}

#[test]
fn a_cancel_during_the_check_lands_nothing() {
    let hold = "/usr/bin/touch started; while [ ! -f go ]; do /bin/sleep 0.1; done";
    let (h, c) = check_world(
        "tl-cancel-lock",
        &[StepName::Land, StepName::Cleanup],
        hold,
        "stop_grace_s = 1\n",
    );
    let before = rev(&h, "fleet/alpha");
    let task_id = c.task_id.clone();
    let tree = c.path.clone();
    c.finish();
    wait_pipeline("the check of T-1 to start", || {
        tree.join("started").exists().then_some(())
    });
    ok(&h, &["ledger", "cancel", "T-1", "dropped during the check"]);
    std::fs::write(tree.join("go"), "").unwrap();
    let job = wait_pipeline("the check job of T-1", || check_job_of(&h, &task_id));
    wait_pipeline("the check job to end", || {
        let row = job_row(&h, job);
        (row.state != JobState::Queued && row.state != JobState::Running).then_some(())
    });
    wait_push("T-1 is cancelled", || {
        let t = json_of(&h, &["ledger", "show", "T-1", "--json"]);
        (t["task"]["status"] == "cancelled").then_some(())
    });
    wait_push("the worktree of T-1 is abandoned", || {
        (worktree_status(&h, "T-1") == "abandoned").then_some(())
    });
    assert_eq!(rev(&h, "fleet/alpha"), before);
    let t = json_of(&h, &["ledger", "show", "T-1", "--json"]);
    assert_eq!(t["task"]["status"], "cancelled", "{t}");
}

#[test]
fn a_finish_returns_at_once_while_a_long_check_runs() {
    let hold = "/usr/bin/touch started; while [ ! -f go ]; do /bin/sleep 0.1; done";
    let (h, c) = check_world("tl-detach", &[StepName::Check], hold, "");
    let tree = c.path.clone();
    c.finish();
    wait_pipeline("the check of T-1 to start", || {
        tree.join("started").exists().then_some(())
    });
    let t = json_of(&h, &["ledger", "show", "T-1", "--json"]);
    assert_ne!(t["task"]["status"], "done", "the check still runs: {t}");
    std::fs::write(tree.join("go"), "").unwrap();
    wait_pipeline("T-1 done", || {
        let t = json_of(&h, &["ledger", "show", "T-1", "--json"]);
        (t["task"]["status"] == "done").then_some(())
    });
}
