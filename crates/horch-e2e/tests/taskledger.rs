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
use horch_core::taskledger::workers::NewWorker;
use horch_core::taskledger::Store;
use horch_e2e::bind::{bind, bind_worker, try_bind};
use horch_e2e::fleetd::Fleetds;
use horch_e2e::harness::{copy_tree, fixtures, repo_root, Harness};
use horch_e2e::ledger_seed::{seed_run, seed_task};
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

/// [`seed_run`] with the finish steps `steps` and, when given, the check
/// period `check_every_s`, as `horch ledger run new <slug> --steps <steps>
/// [--check-every <s>]` made the run. Gives the run.
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

/// 1 queued task of `run` from `ticket` for `primary`, as `horch ledger add`
/// made it: [`seed_task`] with the `**Check:** none` of the ticket, and
/// the other options of `add` that `edit` sets (`--check`, `--shared`,
/// `--own`, `--blocked-by`, `--issue`).
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

/// [`seed_task_with`] that waits for the task `blocked_by`, as `horch ledger
/// add-map` made it from a map row.
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

/// [`wait_push`] for the verifier pipeline of a task with all its finish
/// steps: the check, the reviews, 2 more steps and a landing each wait for
/// a pass, and 16 parallel tests slow each pass, so the limit is longer.
fn wait_pipeline<T>(what: &str, f: impl FnMut() -> Option<T>) -> T {
    wait_within(Duration::from_secs(240), what, f)
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

fn worker_new(h: &Harness, persona: &str) -> String {
    ok(h, &["ledger", "worker", "new", "--persona", persona])
        .trim()
        .to_string()
}

/// The next task of `persona` is bound to a new worker with the role
/// `role` (FR-5). Gives the worker id.
fn bind_role(h: &Harness, persona: &str, role: &str) -> String {
    let worker = NewWorker {
        persona: persona.into(),
        role: Some(role.into()),
        ..Default::default()
    };
    bind_worker(h, worker, None).worker_id
}

#[test]
fn status_on_a_fresh_ledger_creates_it_and_says_no_runs() {
    let h = world_without_fleetd("tl-fresh");
    let db = h.root.join("ledger.sqlite3");
    assert!(!db.exists());
    // Since Phase 6 `status` only reads: it creates no ledger, and names
    // fleetd, which creates it at its start.
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
    // TRF-45: 1 path on stdout, the task ledger file of the harness.
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

    // Phase 6 retired the worker plan: fleetd dispatches each ready task.
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

    // The test is the agent: its process becomes the wrapper of the live
    // attempt, so the progress command finds its attempt. A process of an
    // attempt sends no operator request, so this comes last.
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
    // fleetd makes the folder of an attempt when it launches one.
    let attempt = h
        .fleet_root()
        .join("tasks")
        .join(&bound.task_id)
        .join("attempts")
        .join("A1");
    std::fs::create_dir_all(&attempt).unwrap();
    // Since Phase 5 the command writes 1 line to the outbox of the attempt,
    // and fleetd ingests it: the command writes no ledger row.
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

/// [`finish_world`] in push form: the herdr workspace, the teammates and the
/// gates of [`push_env`] with the fake agent scenario `scenario`. fleetd is
/// down: seed the run and its tasks, then call [`Harness::serve_fleetd`].
fn push_finish_world(name: &str, scenario: &str) -> Harness {
    let mut h = finish_world(name);
    h.stop_fleetd();
    push_env(&mut h, scenario);
    h
}

#[test]
fn a_task_finishes_through_review_land_pr_and_cleanup() {
    horch_e2e::expected::check(
        file!(),
        "a_task_finishes_through_review_land_pr_and_cleanup",
        || {
            let h = push_finish_world("tl-finish", "exec,stay,commit,result");
            let run = seed_run(&h, "alpha", None, "w1:p1");
            let p = ticket(&h, "01-a", "# 01: Feature\n");
            seed_task_with(&h, &run, &p, "opus", |new| {
                new.land_check = Some("test -f fake-claude.txt".into());
                new.issue_url = Some("https://github.com/o/r/issues/29".into());
            });
            h.serve_fleetd();
            // The fake agent commits fake-claude.txt and ends with horch
            // result. fleetd verifies the work, queues the code-sweep
            // reviews, lands the branch, opens the PR and removes the tree.
            let tree = wait_push("the worktree of T-1", || task_worktree(&h, "T-1"));
            let reviews = wait_done_playing_reviewers(&h, "T-1");
            assert!(reviews >= 2, "bugs and clarity at least: {reviews}");
            // The task branch landed on the run branch. Phase 4 retired the
            // push of the run branch and the section of the task in a PR
            // here: a feature reaches the primary branch through 1 pull
            // request, opened when the feature is ready (D11).
            wait_pipeline("the branch of T-1 landed", || task_branch_landed(&h, 1));
            let project = h.project.clone();
            let log = git_in(&h, &project, &["log", "--oneline", "fleet/alpha"]);
            assert!(log.contains("fake claude: commit"), "{log}");
            // Known fault (ticket 168, T-207): no code removes the tree of
            // landed work.
            wait_push("the worktree of T-1 is gone", || {
                (!tree.exists()).then_some(())
            });
            let st = json_of(&h, &["ledger", "status", "--json"]);
            assert_eq!(st["worktrees"][0]["status"], "cleaned", "{st}");
        },
    );
}

/// The review tasks that have no findings file yet, in the order they were
/// queued: the persona of each and its findings file.
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

/// Wait for the reviews of a code-sweep round, write each findings file
/// (the first review gets the Critical and Important items, the others
/// none), and give the personas and the files of the round.
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

/// The `fix_rounds` of the task `seq` and the count of its attempts.
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

    // Round 1 goes to the reviewer persona, and its Critical finding sends
    // the task back for a fix attempt (RR-18: a fix round, not an attempt).
    let round = review_round_with(&h, "- a.txt:1 data loss. Fix: x", "- none");
    assert!(
        round.iter().all(|(p, _)| p == "codex-reviewer"),
        "{round:?}"
    );
    wait_pipeline("the fix attempt", || {
        (fix_rounds_and_attempts(&h, 1) == (1, 2)).then_some(())
    });

    // The fix is a new commit: round 2 reviews it, and goes to the cheaper
    // reviewer persona (TRF-30). An unanswered Important finding fails.
    let round = review_round_with(&h, "- none", "- a.txt:1 slow. Fix: cache");
    assert!(
        round.iter().all(|(p, _)| p == "codex-reviewer-light"),
        "{round:?}"
    );
    wait_pipeline("the second fix attempt", || {
        (fix_rounds_and_attempts(&h, 1) == (2, 3)).then_some(())
    });

    // Finding ids are stable across rounds: round 1's Critical was F1, so
    // this Important finding is F2. The author answers it in the answers
    // file of the round, and an answered Important finding passes on WARN.
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

/// FM-16 and OR-3 (`taskledger/wakes.rs`): no wake goes to an orchestrator.
/// The wake `reviews_in` of today's orchestrator has the new owner fleetd,
/// which gets nothing: the verifier carries on when the reviews are in.
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
    let run = seed_run_with(&h, "zeta", &StepName::ALL, Some(1));
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
    // With no fast gate a pass of fleetd adds a needs-you item that names
    // fleetd, and `status` prints it.
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
    // An unlanded worktree is not removed: the import adopts it as
    // orphaned, and only a landed or rescued tree is removed.
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
        let events = json_of(&h, &["ledger", "events", "--json"]);
        assert!(
            events
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["kind"] == "woke"
                    && e["text"].as_str().unwrap().contains("Tasks can start: T-3")),
            "the orchestrator is woken for C: {events}"
        );
        // 5. B's checks and the stall ladder; A's worker is untouched.
        stall_by_the_ladder(&h, &wb, bpid, bstart);
        assert_ladder_texts(&json_of(&h, &["ledger", "events", "--json"]), "T-1");
        // The orchestrator got the wake for B.
        let wakes = outbox(&h);
        assert!(
            wakes
                .iter()
                .any(|w| w.contains("codex-terra-1 on T-1 shows no progress")),
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
        assert_eq!(rb["previous_worker_id"], wb.as_str(), "{rb}");

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
            "{st}"
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
    wait_push("T-1 done", || {
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
    wait_push("the branch of T-1 landed", || task_branch_landed(&h, 1));
}

/// Some when the branch of the task with the number `seq` is `landed`.
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

/// Wait until the task `short` is `done` and its worktree `tree` is gone.
/// Its agent ends with `horch result`, and fleetd runs the verifier, the
/// review steps and the land. The test plays each reviewer: it writes the
/// findings file of each review task with no findings. Gives the count of
/// findings files that it wrote.
fn land_by_push(h: &Harness, short: &str, tree: &Path) -> usize {
    let written = wait_done_playing_reviewers(h, short);
    wait_push(&format!("the worktree of {short} is gone"), || {
        (!tree.exists()).then_some(())
    });
    written
}

/// Wait until the task `short` is `done`, as [`land_by_push`] does, but
/// not for its worktree. Gives the count of findings files that the test
/// wrote.
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

/// The shell that plays the agent of a live attempt in a test: the `horch`
/// command that [`AgentShell::run`] gives it runs below its process.
struct AgentShell {
    child: std::process::Child,
    dir: PathBuf,
    horch: PathBuf,
}

impl AgentShell {
    /// Start the shell. It waits for the command file of [`AgentShell::run`].
    fn start(h: &Harness) -> AgentShell {
        let dir = h.root.join("agent-shell");
        std::fs::create_dir_all(&dir).unwrap();
        let mut cmd = std::process::Command::new("/bin/sh");
        h.seal(&mut cmd);
        let child = cmd
            .arg("-c")
            .arg("while [ ! -f \"$0/go.sh\" ]; do /bin/sleep 0.05; done; . \"$0/go.sh\"")
            .arg(&dir)
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

    /// Run `horch <args>` in the shell, once. Gives its exit code and its
    /// output.
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
    /// A shell that no command reached waits for ever: end it with the test.
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn a_shared_task_claims_on_the_run_branch_and_lands_only_new_clean_commits() {
    let h = push_finish_world("tl-shared", "exec,stay");
    // A shared checkout holds only committed tickets.
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

    // The check judges committed work only: a changed tracked file and an
    // untracked file are not the work of the task, and fail nothing.
    std::fs::write(&p, "# 01: Doc, edited\n").unwrap();
    std::fs::write(project.join("draft.md"), "draft\n").unwrap();
    commit_file(&h, &project, "doc.md");
    let head = git_in(&h, &project, &["rev-parse", "HEAD"]);
    // A task of the shared checkout has no worktree: the result names its
    // commit and its file.
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
    // The task has no worktree and no task branch: its commits are on the
    // run branch already, so nothing is left to land.
    let store = Store::open(&h.ledger_db()).unwrap();
    let count = |sql: &str| -> i64 { store.conn().query_row(sql, [], |r| r.get(0)).unwrap() };
    assert_eq!(count("SELECT COUNT(*) FROM worktrees"), 0);
    assert_eq!(
        count("SELECT COUNT(*) FROM branches WHERE task_id IS NOT NULL"),
        0
    );
    // The edited ticket and the draft stay as they were.
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "# 01: Doc, edited\n");
    assert_eq!(
        std::fs::read_to_string(project.join("draft.md")).unwrap(),
        "draft\n"
    );
}

/// The scenario of the fake agents that fleetd launches in a push world: a
/// reviewer commits and ends with `horch result`, as in the other tests.
const PUSH_SCENARIO: &str = "exec,stay,commit,result";

/// The work of a claimed task: its worker, its worktree and the shell that
/// plays its agent.
struct Claimed {
    task_id: String,
    path: PathBuf,
    file: String,
    /// The `worktree_add` job that makes the worktree.
    add_job: i64,
    agent: AgentShell,
}

impl Claimed {
    /// Wait until fleetd has made the worktree of the bound task, and commit
    /// the file of the task in it. fleetd runs: see [`Harness::serve_fleetd`].
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

    /// The agent ends with `horch ledger finish`: its `result.json` says
    /// `completed`, and fleetd verifies the work (Phase 4).
    fn finish(self) {
        let (code, said) = self.agent.run(&["ledger", "finish"]);
        assert_eq!(code, 0, "{said}");
    }
}

/// 1 task of `persona` of the run of `h`, bound to a new worker whose agent
/// is a shell of the test. fleetd is down (see [`push_finish_world`]): the
/// test sets its environment, starts fleetd, which makes the worktree, and
/// calls [`Claimed::ready`] and [`Claimed::finish`].
fn bind_claimed(h: &Harness, persona: &str, file: &str) -> Claimed {
    // The bind makes the worktree of the task (FR-5): no worker asks for
    // it since Phase 6 retired `horch ledger worktree new`.
    let agent = AgentShell::start(h);
    let bound = bind(h, persona, Some(agent.pid()));
    let tree = bound.worktree.expect("a worktree task has its tree");
    Claimed {
        task_id: bound.task_id,
        path: PathBuf::from(tree.path),
        file: file.to_string(),
        add_job: tree.add_job.expect("the bind queues the worktree_add job"),
        agent,
    }
}

/// A run with the finish steps `steps` and 1 task of `persona` with `check`
/// (`None`: the ticket says `Check: none`), bound as [`bind_claimed`] does.
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

/// (name, status, verdict, note) of the finish steps of task `seq` in the
/// ledger, in order. fleetd records them as the verifier runs each step.
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

/// Wait until the step `name` of T-1 is not `pending` or `running`. Gives
/// (status, verdict, note).
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

/// [`git_in`] that runs again while another process holds the index lock.
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

/// Queue 1 `task_check` job of `task` with the time limit `limit_s`, as a
/// pass of the verifier does with the fixed limit of 1200 s (`CHECK_TIMEOUT`):
/// the test writes the limit in the new row in the same transaction, before
/// a pass can start the job. Gives the id of the job.
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

/// The row of the job `id`.
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

/// The calls of the fake codex that run a check in `codex sandbox`.
fn sandbox_calls(h: &Harness) -> Vec<Value> {
    h.calls_of("codex")
        .into_iter()
        .filter(|c| c["argv"][0] == "sandbox")
        .collect()
}

/// A task of `persona` whose check never ends by itself, so that only the
/// time limit of its job ends it. The check runs in the sandbox of the
/// worker (SR-4), below the process that the sandbox starts, and writes its
/// own pid files in its worktree: `host.pid` is its parent.
fn assert_check_killed_at_its_limit(name: &str, persona: &str) -> (Claimed, Harness) {
    let h = push_finish_world(name, PUSH_SCENARIO);
    let check = "echo $PPID > host.pid; /bin/sleep 3600 & echo $! > sleep.pid; wait";
    let c = push_claimed(&h, persona, "check", Some(check), "a.txt");
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
    let row = wait_push("the job to end at its time limit", || {
        let row = job_row(&h, job);
        (row.state != JobState::Queued && row.state != JobState::Running).then_some(row)
    });
    // A job past its time limit is abandoned and has no verdict.
    assert_eq!(row.state, JobState::Abandoned, "{row:?}");
    for file in ["sleep.pid", "host.pid"] {
        let pid = std::fs::read_to_string(c.path.join(file))
            .unwrap_or_else(|e| panic!("the check wrote no {file}: {e}"))
            .trim()
            .to_string();
        wait_gone(file, &pid);
    }
    // The shell of the task ends before the harness checks for leaks.
    (c, h)
}

#[test]
fn a_check_past_its_timeout_is_killed_with_its_children() {
    assert_check_killed_at_its_limit("tl-check-timeout", "opus");
}

/// SR-4 for a Codex worker: its check runs inside `codex sandbox`, and a
/// check past its time limit leaves neither its sleep nor the sandbox.
#[test]
fn a_codex_workers_sandboxed_check_past_its_timeout_leaves_no_process() {
    let (c, h) = assert_check_killed_at_its_limit("tl-codex-check-timeout", "codex-sol");
    drop(c);
    assert_eq!(sandbox_calls(&h).len(), 1);
}

/// SR-4: a task check writes only in the writable paths of the sandbox of
/// its worker (its worktree, its attempt folder, its temp folder). A check
/// that writes outside fails, and the file is never made. The sandbox is
/// the same for each persona: the check of a Claude worker is refused too.
#[test]
fn a_codex_workers_check_cannot_write_outside_its_worktree() {
    for persona in ["codex-sol", "opus"] {
        let h = push_finish_world(&format!("tl-codex-check-{persona}"), PUSH_SCENARIO);
        // A folder outside each writable path of a check: not in the
        // worktree, not in the fleet root, not in the temp folder.
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
    // A's check holds until the test releases it, so B always passes its
    // check and lands inside A's check, however slow the host is. The loop
    // ends after 600 s so a failed test leaves no check behind.
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

/// [`push_finish_world`] with a code-sweep task whose agent ends with its
/// commit, and fleetd up: the verifier queues the reviews. The test sets
/// its environment in `set` before fleetd starts.
fn sweep_started(name: &str, set: impl FnOnce(&mut Harness)) -> Harness {
    let mut h = push_finish_world(name, PUSH_SCENARIO);
    set(&mut h);
    let c = sweep_pushed(&h);
    h.serve_fleetd();
    c.ready(&h);
    c.finish();
    h
}

/// Wait until the verifier of fleetd has queued the reviews of a round.
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
    // The reviewers of the other reviews find nothing. When they are in, the
    // verifier queues the cancelled review again: 1 new review task.
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
    // Round 1: the Critical finding F1 sends the task back for a fix attempt.
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

    // The fix attempt ends, and a reviewer of round 2 finds F1 again: the
    // waiver stands, and the sweep passes.
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

/// Link the system tools that the sandboxed script of the complexity-sweep
/// uses (`mktemp`, `cat` and `rm`) into the `bin/` of `h`: the sealed `PATH`
/// of fleetd and its jobs holds only the fakes.
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

/// A run with only the complexity-sweep step and 1 task whose agent ends
/// with its commit of the first of `files`, and then of the others. fleetd
/// runs from here on: set the environment of the test first.
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
    // The temp folder is writable for the analyser in its sandbox (SR-4).
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
    // The temp folder is writable for the analyser in its sandbox (SR-4).
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
    // The temp folder is writable for the check in its sandbox (SR-4).
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
    // fleetd starts with the outer repository in its environment.
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

/// The text of the task file of the first review task, once the verifier
/// queued the reviews of a code-sweep round.
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
        // The store layout: <data>/skills/<skill>/<source>+<hash>/.
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
    // The temp folder is writable for the analyser in its sandbox (SR-4).
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
        // <home>/.claude/plugins/<plugin>/<skill>/: a folder named like the
        // skill that holds no reviewers/ sits first in the sort order and is
        // skipped.
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

fn queue(h: &Harness, name: &str, persona: &str, extra: &[&str]) {
    let p = ticket(h, name, &format!("# {name}\n"));
    let p = p.to_string_lossy().into_owned();
    let mut args = vec!["ledger", "add", "--file", &p, "--primary", persona];
    args.extend_from_slice(extra);
    ok(h, &args);
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
    ok(&h, &["ledger", "run", "new", "a"]);
    ok(&h, &["ledger", "run", "new", "b"]);
    let in_b = ok(
        &h,
        &["ledger", "worker", "new", "--persona", "opus", "--run", "b"],
    )
    .trim()
    .to_string();

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
    let h = fleet("tl-which-ws", "default");
    ok(&h, &["ledger", "run", "new", "a"]);
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

#[test]
fn a_closed_run_frees_the_project_for_the_next_run() {
    let h = world("tl-close");
    ok(&h, &["ledger", "run", "new", "a"]);
    queue(&h, "01-a", "opus", &[]);

    // A queued task holds the run open, and the refusal names it.
    let o = h.run(&["ledger", "run", "close"]);
    assert!(!o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("T-1 (queued)"), "{}", text(&o));

    ok(&h, &["ledger", "cancel", "T-1", "not needed"]);
    // A settled run still needs a passing gate on its tip (TRF-20).
    let o = h.run(&["ledger", "run", "close"]);
    assert!(!o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("has no gate on its tip"), "{}", text(&o));
    ok(&h, &["ledger", "run", "gate", "--check", "true"]);
    let closed = ok(&h, &["ledger", "run", "close"]);
    assert!(closed.contains("Run a closed as done."), "{closed}");
    ok(&h, &["ledger", "run", "new", "b"]);
    let o = h.run(&["ledger", "status"]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o));
    assert!(out(&o).starts_with("Run b (active)."), "{}", text(&o));

    // The slug of a closed run can be used again.
    ok(
        &h,
        &["ledger", "run", "gate", "--run", "b", "--check", "true"],
    );
    ok(&h, &["ledger", "run", "close", "--run", "b"]);
    ok(&h, &["ledger", "run", "new", "a"]);
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
    ok(&h, &["ledger", "run", "new", "a"]);
    queue(&h, "01-a", "opus", &[]);
    let o = ok(
        &h,
        &["ledger", "run", "close", "--abandon", "operator dropped it"],
    );
    assert!(o.contains("T-1 cancelled."), "{o}");
    assert!(o.contains("Run a closed as abandoned."), "{o}");
    let t = json_of(&h, &["ledger", "show", "T-1", "--run", "a", "--json"]);
    assert_eq!(t["task"]["status"], "cancelled", "{t}");
}

#[test]
fn run_new_refuses_an_old_run_branch_behind_the_main_branch() {
    let h = world("tl-oldbranch");
    ok(&h, &["ledger", "run", "new", "a"]);
    ok(&h, &["ledger", "run", "gate", "--check", "true"]);
    ok(&h, &["ledger", "run", "close"]);
    project_file(&h, "new.txt", "x");
    git_commit_all(&h, "main moves on");
    let o = h.run(&["ledger", "run", "new", "a"]);
    assert!(!o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("branch fleet/a exists"), "{}", text(&o));
}

/// Phase 1: `horch ledger watch` starts no loop. It says that fleetd does
/// the watch of each run, and exits.
#[test]
fn ledger_watch_points_at_fleetd() {
    let h = world("tl-watch-fleetd");
    ok(&h, &["ledger", "run", "new", "a", "--check-every", "10"]);
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

fn second_workspace(h: &Harness) {
    let mut create = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut create);
    let o = create
        .args(["workspace", "create", "--label", "second"])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", text(&o));
}

/// Queue 1 task for `persona` in `run`, so a claiming spawn of it passes
/// the spawn guard (TRF-31: a task-less spawn needs a startable task).
fn queue_for(h: &Harness, env: &[(&str, &str)], run: &str, persona: &str) {
    let file = ticket(
        h,
        &format!("{run}-{persona}"),
        "# Work\n**Check:** `true`\n",
    );
    let file = file.to_string_lossy().into_owned();
    let o = run_env(
        h,
        env,
        &[
            "ledger",
            "add",
            "--file",
            &file,
            "--primary",
            persona,
            "--run",
            run,
        ],
    );
    assert!(o.status.success(), "{}", text(&o));
}

fn spawn_args<'a>(pane: &'a str, extra: &[&'a str]) -> Vec<&'a str> {
    let mut v = vec!["spawn"];
    v.extend_from_slice(extra);
    v.extend_from_slice(&["--from-pane", pane, "--no-tile", "--force"]);
    v
}

#[test]
fn spawn_run_names_the_run_the_worker_joins() {
    let h = fleet("tl-spawn-run", "default");
    // A workspace holds 1 planned run (TRF-34): run a is of workspace w2.
    let o = run_env(
        &h,
        &[("HORCH_WORKSPACE_ID", "w2")],
        &["ledger", "run", "new", "a"],
    );
    assert!(o.status.success(), "{}", text(&o));
    ok(&h, &["ledger", "run", "new", "b"]);
    // Without --run, 2 active runs in no workspace of theirs: the spawn joins
    // no run (TRF-34), and its NOTE names the flag that joins one.
    let o = run_env(
        &h,
        &[("HORCH_WORKSPACE_ID", "w9")],
        &spawn_args("w1:p1", &["codex-terra"]),
    );
    assert!(o.status.success(), "{}", text(&o));
    assert!(
        text(&o).contains("NOTE: this spawn joins no task-ledger run."),
        "{}",
        text(&o)
    );
    assert!(text(&o).contains("Give --run <slug>"), "{}", text(&o));
    assert_eq!(run_of_worker(&h, "a", "codex-terra"), 0);
    assert_eq!(run_of_worker(&h, "b", "codex-terra"), 0);
    queue_for(&h, &[], "b", "codex-terra");
    let o = h.run(&spawn_args("w1:p1", &["codex-terra", "--run", "b"]));
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(run_of_worker(&h, "b", "codex-terra"), 1);
    assert_eq!(run_of_worker(&h, "a", "codex-terra"), 0);
}

#[test]
fn a_spawn_joins_the_run_of_its_own_workspace() {
    let h = fleet("tl-spawn-ws", "default");
    second_workspace(&h);
    ok(&h, &["ledger", "run", "new", "a"]);
    // Only W1 owns a run: a task-less spawn from W2 joins no run (TRF-34),
    // and its NOTE names that run and the flag that joins it.
    let w2 = [("HORCH_WORKSPACE_ID", "w2")];
    queue_for(&h, &[], "a", "codex-terra");
    let o = run_env(&h, &w2, &spawn_args("w2:p1", &["codex-terra"]));
    assert!(o.status.success(), "{}", text(&o));
    assert!(
        text(&o).contains(
            "NOTE: this spawn joins no task-ledger run. Run a (workspace w1) is not of this workspace w2. Give --run <slug> to join it."
        ),
        "{}",
        text(&o)
    );
    assert_eq!(run_of_worker(&h, "a", "codex-terra"), 0);
    // --run reaches the run of another workspace.
    let o = run_env(
        &h,
        &w2,
        &spawn_args("w2:p1", &["codex-terra", "--run", "a"]),
    );
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(run_of_worker(&h, "a", "codex-terra"), 1);

    // W2 owns a run of its own now: the spawn goes there.
    let o = run_env(&h, &w2, &["ledger", "run", "new", "b"]);
    assert!(o.status.success(), "{}", text(&o));
    queue_for(&h, &w2, "b", "opus");
    let o = run_env(&h, &w2, &spawn_args("w2:p1", &["opus"]));
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(run_of_worker(&h, "b", "opus"), 1);
    assert_eq!(run_of_worker(&h, "a", "opus"), 0);
}

#[test]
fn trf_43_a_spawn_with_task_text_refuses_with_the_ledger_task_command() {
    let h = fleet("tl-spawn-text", "default");
    ok(&h, &["ledger", "run", "new", "a"]);
    let o = h.run(&spawn_args("w1:p1", &["codex-terra", "Review src/x.rs"]));
    assert!(!o.status.success(), "{}", text(&o));
    assert!(
        text(&o).contains("horch ledger task 'Review src/x.rs' --persona codex-terra"),
        "{}",
        text(&o)
    );
    assert_eq!(run_of_worker(&h, "a", "codex-terra"), 0);
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
    json_of(h, &["ledger", "events", "--json"])
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

fn later(seconds: u64) -> String {
    horch_core::clock::stamp(horch_core::clock::now() + Duration::from_secs(seconds))
}

/// A process that stands in for a live supervisor and agent; killed on drop.
struct Stand(std::process::Child);

impl Stand {
    fn new() -> Stand {
        Stand(
            std::process::Command::new("sleep")
                .arg("300")
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

fn plan_line(h: &Harness) -> String {
    ok(h, &["ledger", "plan-workers"]).trim().to_string()
}

#[test]
fn a_refused_spawn_ends_its_worker_row() {
    let mut h = fleet("tl-refused", "default");
    h.set(
        "HORCH_QUOTA_FILE",
        fixtures()
            .join("quota/all-exhausted.json")
            .to_string_lossy(),
    );
    // The fixture's 7-day window is exhausted at its own time.
    h.set("HORCH_NOW", "2026-09-28T18:00:00Z");
    ok(&h, &["ledger", "run", "new", "a"]);
    queue(&h, "01-a", "opus", &[]);
    let o = h.run(&["spawn", "opus", "--from-pane", "w1:p1", "--no-tile"]);
    assert_eq!(o.status.code(), Some(3), "{}", text(&o));
    let st = json_of(&h, &["ledger", "status", "--json"]);
    let rows: Vec<&Value> = st["workers"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|w| w["worker"]["kind"] == "worker")
        .collect();
    assert_eq!(rows.len(), 1, "{st}");
    assert_eq!(rows[0]["worker"]["status"], "launch_failed", "{st}");
    let reason = rows[0]["worker"]["end_reason"].as_str().unwrap();
    assert!(
        reason.starts_with("REFUSED: opus cannot start."),
        "{reason}"
    );
    assert_eq!(plan_line(&h), "opus: spawn 1 (1 can start, 0 on the way).");
}

#[test]
fn a_spawn_of_an_unknown_teammate_leaves_no_live_row() {
    let h = fleet("tl-unknown", "default");
    ok(&h, &["ledger", "run", "new", "a"]);
    let o = h.run(&[
        "spawn",
        "no-such-teammate",
        "--from-pane",
        "w1:p1",
        "--no-tile",
    ]);
    assert!(!o.status.success(), "{}", text(&o));
    let st = json_of(&h, &["ledger", "status", "--json"]);
    for w in st["workers"].as_array().unwrap() {
        if w["worker"]["kind"] == "worker" {
            assert_eq!(w["worker"]["status"], "launch_failed", "{st}");
        }
    }
}

#[test]
fn the_watchdog_ends_pre_claim_rows_whose_process_is_gone() {
    let h = world("tl-ghosts");
    ok(&h, &["ledger", "run", "new", "a"]);
    queue(&h, "01-a", "sonnet-x", &[]);
    let ghost = worker_new(&h, "sonnet-x");
    let ghost2 = worker_new(&h, "sonnet-x");
    // PIDs far above any pid_max: never a live process.
    ok(
        &h,
        &[
            "ledger",
            "worker",
            "pid",
            &ghost2,
            "--supervisor",
            "99999991",
        ],
    );
    ok(
        &h,
        &["ledger", "worker", "pid", &ghost2, "--agent", "99999992"],
    );
    assert_eq!(
        plan_line(&h),
        "sonnet-x: spawn 0 (1 can start, 2 on the way)."
    );

    // 1 pass now: ghost2's supervisor is gone; ghost is in its launch grace.
    h.without_fleetd(|| ok(&h, &["ledger", "watch", "--once", "--force"]));
    let (st, why) = worker_status(&h, &ghost2);
    assert_eq!(st, "no_task", "{why}");
    assert!(why.contains("supervisor is gone"), "{why}");
    assert_eq!(worker_status(&h, &ghost).0, "spawning");
    assert_eq!(
        plan_line(&h),
        "sonnet-x: spawn 0 (1 can start, 1 on the way)."
    );

    // Past the grace, ghost never had a supervisor: status marks it STALE,
    // and 1 watchdog pass ends it launch_failed with 1 wake.
    let o = run_env(&h, &[("HORCH_NOW", &later(200))], &["ledger", "status"]);
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
            &["ledger", "watch", "--once", "--force"],
        )
    });
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(worker_status(&h, &ghost).0, "launch_failed");
    let new: Vec<String> = outbox(&h).split_off(before);
    assert_eq!(new.len(), 1, "{new:?}");
    assert!(
        new[0].contains("ended before they claimed a task"),
        "{new:?}"
    );
    assert!(new[0].contains("Spawn: sonnet-x x1"), "{new:?}");
    assert_eq!(
        plan_line(&h),
        "sonnet-x: spawn 1 (1 can start, 0 on the way)."
    );
}

#[test]
fn a_live_worker_that_does_not_claim_is_reported_once() {
    let h = world("tl-noclaim");
    ok(
        &h,
        &["ledger", "run", "new", "a", "--heartbeat-stale", "100000"],
    );
    queue(&h, "01-a", "sonnet-x", &[]);
    let id = ok(
        &h,
        &[
            "ledger",
            "worker",
            "new",
            "--persona",
            "sonnet-x",
            "--pane",
            "w1:p7",
        ],
    )
    .trim()
    .to_string();
    let stand = Stand::new();
    let me = stand.pid();
    ok(&h, &["ledger", "worker", "pid", &id, "--supervisor", &me]);
    ok(&h, &["ledger", "worker", "pid", &id, "--agent", &me]);
    let watch = |at: Option<String>| {
        let env: Vec<(&str, &str)> = at.iter().map(|t| ("HORCH_NOW", t.as_str())).collect();
        let o = h.without_fleetd(|| run_env(&h, &env, &["ledger", "watch", "--once", "--force"]));
        assert!(o.status.success(), "{}", text(&o));
    };
    // Within the 3 min claim grace (TRF-23) the worker is never touched.
    watch(Some(later(150)));
    assert_eq!(worker_status(&h, &id).0, "claiming");
    assert!(outbox(&h).is_empty(), "{:?}", outbox(&h));
    // Past it: 1 wake that names the pane, and only 1.
    watch(Some(later(200)));
    assert_eq!(outbox(&h).len(), 1, "{:?}", outbox(&h));
    watch(Some(later(300)));
    let w = outbox(&h);
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].contains("has not claimed a task"), "{w:?}");
    assert!(w[0].contains("pane w1:p7"), "{w:?}");
    assert_eq!(worker_status(&h, &id).0, "claiming");
}

#[test]
fn replacing_a_live_worker_names_the_stop_command() {
    let h = fleet("tl-replaces-live", "default");
    ok(&h, &["ledger", "run", "new", "a"]);
    let live = worker_new(&h, "codex-terra");
    let o = h.run(&spawn_args("w1:p1", &["codex-terra", "--replaces", &live]));
    assert!(!o.status.success(), "{}", text(&o));
    assert!(
        text(&o).contains(&format!("Run horch ledger stop {live}")),
        "{}",
        text(&o)
    );
}

// ─── #168: a task that kills every worker is held, not retried forever ──────

/// T-1 is bound to a new worker of `persona` (FR-5), which is reaped.
fn claim_and_reap(h: &Harness, persona: &str, why: &str) -> String {
    let b = bind(h, persona, None);
    let t1 = json_of(h, &["ledger", "show", "T-1", "--json"]);
    assert_eq!(b.task_id, t1["task"]["task_id"], "{b:?}");
    ok(h, &["ledger", "reap", &b.worker_id, why]);
    b.worker_id
}

#[test]
fn a_task_reaped_max_attempts_times_fails_and_its_replan_requeues_it() {
    let h = world("tl-held");
    ok(&h, &["ledger", "run", "new", "a", "--max-attempts", "2"]);
    queue(&h, "01-a", "sonnet-x", &[]);

    // Reaped once: queued again, as before.
    claim_and_reap(&h, "sonnet-x", "hung in check");
    let t = json_of(&h, &["ledger", "show", "T-1", "--json"]);
    assert_eq!(t["task"]["status"], "queued", "{t}");
    assert!(t["task"]["held_reason"].is_null(), "{t}");
    assert!(
        plan_line(&h).starts_with("sonnet-x: spawn 1"),
        "{}",
        plan_line(&h)
    );
    let before = wakes(&h).len();
    let rows_before = outbox(&h).len();

    // Reaped twice: failed (rung 6). No worker is asked for, and 1
    // QUESTION asks the orchestrator for the replan and names both ends.
    claim_and_reap(&h, "sonnet-x", "overflowed at claim");
    let t = json_of(&h, &["ledger", "show", "T-1", "--json"]);
    assert_eq!(t["task"]["status"], "failed", "{t}");
    assert!(t["task"]["held_reason"].is_null(), "{t}");
    assert_eq!(plan_line(&h), "No task can start now. Spawn no worker.");
    let mut new = wakes(&h).split_off(before);
    // The replan QUESTION is an outbox row; the pass of fleetd delivers it.
    new.extend(outbox(&h).split_off(rows_before));
    let questions: Vec<&String> = new.iter().filter(|w| w.contains("QUESTION:")).collect();
    assert_eq!(questions.len(), 1, "{new:?}");
    for part in [
        "replan T-1: requeue it 1 time, or cancel it",
        "horch ledger requeue T-1",
        "horch ledger cancel T-1",
        "dead: hung in check",
        "dead: overflowed at claim",
    ] {
        assert!(questions[0].contains(part), "{part}: {new:?}");
    }
    assert!(!new.iter().any(|w| w.contains("--replaces")), "{new:?}");
    assert!(
        !new.iter().any(|w| w.contains("Spawn:")),
        "a failed task asks for no worker: {new:?}"
    );
    let st = out(&h.run(&["ledger", "status"]));
    assert!(st.contains("failed"), "{st}");
    assert!(
        st.contains("[2 attempts; ends: dead: hung in check; dead: overflowed at claim]"),
        "{st}"
    );
    // A failed task is open work: the run does not close.
    let o = h.run(&["ledger", "run", "close"]);
    assert!(text(&o).contains("T-1 (failed)"), "{}", text(&o));

    // The orchestrator requeues it, its 1 replan: claimable again, its
    // history kept.
    ok(&h, &["ledger", "requeue", "T-1", "ticket fixed"]);
    let t = json_of(&h, &["ledger", "show", "T-1", "--json"]);
    assert_eq!(t["task"]["replans"], 1, "{t}");
    let b = bind(&h, "sonnet-x", None);
    assert_eq!(b.task_id, t["task"]["task_id"], "{b:?}");
    assert_eq!(b.attempt, 3, "{b:?}");
    let w = b.worker_id;
    // The limit counts attempts since the requeue: 1 reap queues it again.
    ok(&h, &["ledger", "reap", &w, "once more"]);
    let t = json_of(&h, &["ledger", "show", "T-1", "--json"]);
    assert!(t["task"]["held_reason"].is_null(), "{t}");
    assert_eq!(t["task"]["attempts"], 3, "{t}");
}

#[test]
fn requeue_refuses_a_task_that_is_neither_held_nor_cancelled() {
    let h = world("tl-requeue-queued");
    ok(&h, &["ledger", "run", "new", "a"]);
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
    ok(&h, &["ledger", "run", "new", "a"]);
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
    let w = wakes(&h);
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(
        w[0].contains("NOTE: T-1 was cancelled. T-2, T-3 wait for it. Run horch ledger unblock T-2 --from T-1"),
        "{w:?}"
    );

    assert_eq!(plan_line(&h), "No task can start now. Spawn no worker.");
    ok(&h, &["ledger", "unblock", "T-2", "--from", "T-1"]);
    assert_eq!(
        plan_line(&h),
        "opus-b: spawn 1 (1 can start, 0 on the way)."
    );
    // A dependency that does not exist is refused.
    let o = h.run(&["ledger", "unblock", "T-2", "--from", "T-1"]);
    assert!(
        text(&o).contains("T-2 does not wait for T-1"),
        "{}",
        text(&o)
    );
}

#[test]
fn an_unlanded_done_names_its_dependents_and_a_stuck_run_wakes_once() {
    let h = world("tl-unlanded-deps");
    ok(&h, &["ledger", "run", "new", "a"]);
    queue(&h, "01-a", "opus-a", &[]);
    queue(&h, "02-b", "opus-b", &["--blocked-by", "T-1"]);
    let w = bind(&h, "opus-a", None).worker_id;
    ok_as(
        &h,
        &w,
        &["ledger", "done", "--unlanded", "a red check", "parked"],
    );
    let all = wakes(&h);
    assert!(
        all.iter().any(|w| w.contains(
            "NOTE: T-2 waits: T-1 is parked unlanded. Run horch ledger requeue T-1 to resume it."
        )),
        "{all:?}"
    );
    // No worker is live, nothing can start, T-2 is open: the run is stuck.
    let stuck = |h: &Harness| {
        wakes(h)
            .into_iter()
            .filter(|w| w.contains("the run is stuck:"))
            .collect::<Vec<_>>()
    };
    let s = stuck(&h);
    assert_eq!(s.len(), 1, "{all:?}");
    assert!(s[0].contains("T-1 is held (unlanded)"), "{s:?}");
    assert!(s[0].contains("T-2: T-1 is parked unlanded"), "{s:?}");
    // The watchdog sees the same stuck run: no second wake.
    h.without_fleetd(|| ok(&h, &["ledger", "watch", "--once", "--force"]));
    h.without_fleetd(|| ok(&h, &["ledger", "watch", "--once", "--force"]));
    assert_eq!(stuck(&h).len(), 1, "{:?}", wakes(&h));
}

#[test]
fn a_stuck_run_wakes_from_the_watchdog_when_no_task_finishes() {
    let h = world("tl-stuck-watch");
    ok(&h, &["ledger", "run", "new", "a"]);
    queue(&h, "01-a", "opus-a", &[]);
    queue(&h, "02-b", "opus-b", &["--blocked-by", "T-1"]);
    // A live worker holds T-1: the run is not stuck.
    let w = bind(&h, "opus-a", None).worker_id;
    let stand = Stand::new();
    let me = stand.pid();
    ok(&h, &["ledger", "worker", "pid", &w, "--supervisor", &me]);
    ok(&h, &["ledger", "worker", "pid", &w, "--agent", &me]);
    h.without_fleetd(|| ok(&h, &["ledger", "watch", "--once", "--force"]));
    assert!(
        !outbox(&h).iter().any(|w| w.contains("stuck")),
        "{:?}",
        outbox(&h)
    );
    // Its worker is stopped and T-1 cancelled: nothing will start.
    ok(&h, &["ledger", "cancel", "T-1", "dropped"]);
    h.without_fleetd(|| ok(&h, &["ledger", "watch", "--once", "--force"]));
    h.without_fleetd(|| ok(&h, &["ledger", "watch", "--once", "--force"]));
    let stuck: Vec<String> = outbox(&h)
        .into_iter()
        .filter(|w| w.contains("the run is stuck:"))
        .collect();
    assert_eq!(stuck.len(), 1, "{:?}", outbox(&h));
}

// ─── #136: a cancelled task never lands or turns done ───────────────────────

/// Bind the next task to a new `persona` worker, make worktree `name` and
/// commit `file` in it. Returns the worker and the worktree path.
fn claim_and_commit(h: &Harness, persona: &str, name: &str, file: &str) -> (String, PathBuf) {
    let w = bind(h, persona, None).worker_id;
    let wt = ok_as(h, &w, &["ledger", "worktree", "new", name]);
    let path = PathBuf::from(wt.lines().find_map(|l| l.strip_prefix("path: ")).unwrap());
    std::fs::write(path.join(file), "x\n").unwrap();
    git_in(h, &path, &["add", file]);
    git_in(
        h,
        &path,
        &["commit", "--quiet", "--no-gpg-sign", "-m", file],
    );
    (w, path)
}

fn rev(h: &Harness, what: &str) -> String {
    git_in(h, &h.project.clone(), &["rev-parse", what])
        .trim()
        .to_string()
}

#[test]
fn a_cancelled_task_does_not_land_or_turn_done() {
    let h = finish_world("tl-cancel-land");
    ok(
        &h,
        &["ledger", "run", "new", "r2", "--steps", "land,cleanup"],
    );
    queue(&h, "01-a", "opus", &[]);
    let (w, path) = claim_and_commit(&h, "opus", "canc", "c.txt");
    let before = (rev(&h, "fleet/r2"), rev(&h, "origin/fleet/r2"));

    let o = ok(&h, &["ledger", "cancel", "T-1", "operator dropped it"]);
    assert!(o.contains("T-1 cancelled."), "{o}");
    assert_eq!(worker_status(&h, &w).0, "stopped");

    let mut cmd = h.horch(&["ledger", "finish"]);
    cmd.env("HORCH_WORKER_ID", &w);
    let o = cmd.output().unwrap();
    assert!(!o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("T-1 was cancelled"), "{}", text(&o));
    let mut cmd = h.horch(&["ledger", "done", "finished anyway"]);
    cmd.env("HORCH_WORKER_ID", &w);
    let o = cmd.output().unwrap();
    assert!(!o.status.success(), "{}", text(&o));
    git_in(&h, &h.project.clone(), &["fetch", "--quiet", "origin"]);
    assert_eq!((rev(&h, "fleet/r2"), rev(&h, "origin/fleet/r2")), before);
    let t = json_of(&h, &["ledger", "show", "T-1", "--json"]);
    assert_eq!(t["task"]["status"], "cancelled", "{t}");

    // The worktree is abandoned, kept on disk, and listed as unfinished.
    let st = out(&h.run(&["ledger", "status"]));
    assert!(st.contains("T-1   abandoned"), "{st}");
    assert!(path.join("c.txt").exists());
    let unf = ok(&h, &["ledger", "unfinished"]);
    assert!(unf.contains("T-1 was cancelled with its worktree"), "{unf}");

    // `cancelled` is terminal (fleetd-p2 MAP.md, Q22): the door refuses a
    // requeue, and the kept worktree stays unfinished.
    let o = h.run(&["ledger", "requeue", "T-1", "wanted after all"]);
    assert!(!o.status.success(), "{}", text(&o));
    assert!(text(&o).contains(": cancelled -> ready"), "{}", text(&o));
    assert!(path.join("c.txt").exists());
    let unf = ok(&h, &["ledger", "unfinished"]);
    assert!(unf.contains("T-1 was cancelled with its worktree"), "{unf}");
}

#[test]
fn a_cancel_during_the_land_lands_nothing() {
    let h = finish_world("tl-cancel-lock");
    ok(
        &h,
        &["ledger", "run", "new", "r2", "--steps", "land,cleanup"],
    );
    let go = h.root.join("go");
    // T-1's land check runs until the test says go. The land takes the lock
    // after the check, so the cancel arrives before the fast-forward.
    let hold = format!("while [ ! -f '{}' ]; do /bin/sleep 0.1; done", go.display());
    queue(&h, "01-a", "opus", &["--check", &hold]);
    let (w, _) = claim_and_commit(&h, "opus", "a", "a.txt");
    let before = rev(&h, "fleet/r2");
    let worktree_of = |seq: &str| -> String {
        let st = json_of(&h, &["ledger", "status", "--json"]);
        st["worktrees"]
            .as_array()
            .unwrap()
            .iter()
            .find(|w| w["task"] == seq)
            .map(|w| w["status"].as_str().unwrap().to_string())
            .unwrap_or_default()
    };
    let mut cmd = h.horch(&["ledger", "finish"]);
    cmd.env("HORCH_WORKER_ID", &w)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let finish = cmd.spawn().unwrap();
    wait_for("T-1 to start its land", || {
        (worktree_of("T-1") == "landing").then_some(())
    });
    ok(&h, &["ledger", "cancel", "T-1", "dropped during the land"]);
    std::fs::write(&go, "").unwrap();
    let o = finish.wait_with_output().unwrap();
    assert!(!o.status.success(), "{}", text(&o));
    assert!(
        text(&o).contains("T-1 is cancelled: nothing landed"),
        "{}",
        text(&o)
    );
    assert_eq!(rev(&h, "fleet/r2"), before);
    let t = json_of(&h, &["ledger", "show", "T-1", "--json"]);
    assert_eq!(t["task"]["status"], "cancelled", "{t}");
    assert_eq!(worktree_of("T-1"), "abandoned");
}

/// A worker `opus-1` in its registered pane `w1:p1`, with 1 claimed task
/// whose check is `check`, and 1 commit in its worktree (#160).
fn pane_world(name: &str, check: &str, timeout: Option<&str>) -> (Harness, String) {
    let mut h = finish_world(name);
    h.set("HORCH_WORKSPACE_ID", "w1");
    restart_fleetd(&h);
    let herdr = |args: &[&str]| {
        let mut cmd = std::process::Command::new(h.bin.join("herdr"));
        h.seal(&mut cmd);
        let o = cmd.args(args).output().unwrap();
        assert!(o.status.success(), "{}", text(&o));
    };
    herdr(&["workspace", "create", "--label", "author"]);
    herdr(&[
        "pane",
        "test-state",
        "w1:p1",
        "--status",
        "idle",
        "--agent",
        "claude",
    ]);
    let mailbox = h.state.join("herdr-orchestration-w1");
    std::fs::create_dir_all(&mailbox).unwrap();
    std::fs::write(mailbox.join("opus-1.id"), "w1:p1").unwrap();
    ok(&h, &["ledger", "run", "new", "alpha", "--steps", "check"]);
    let p = ticket(&h, "01-a", "# 01: Feature\n");
    let p = p.to_string_lossy();
    let mut add = vec![
        "ledger",
        "add",
        "--file",
        &p,
        "--primary",
        "opus",
        "--check",
        check,
    ];
    if let Some(t) = timeout {
        add.extend(["--check-timeout", t]);
    }
    ok(&h, &add);
    let w = bind_role(&h, "opus", "opus-1");
    let wt = ok_as(&h, &w, &["ledger", "worktree", "new", "f"]);
    let path = PathBuf::from(wt.lines().find_map(|l| l.strip_prefix("path: ")).unwrap());
    commit_file(&h, &path, "a.txt");
    (h, w)
}

/// `horch ledger finish` as worker `w` in its pane.
fn finish_in_pane(h: &Harness, w: &str) -> Output {
    let mut cmd = h.horch(&["ledger", "finish"]);
    cmd.env("HORCH_WORKER_ID", w)
        .env("HERDR_PANE_ID", "w1:p1")
        .env("HORCH_ROLE", "opus-1");
    cmd.output().unwrap()
}

/// The PID in the "finish runs (PID <n>)" line of a detached finish.
fn detached_pid(o: &Output) -> String {
    let s = out(o);
    s.split("finish runs (PID ")
        .nth(1)
        .and_then(|r| r.split(')').next())
        .unwrap_or_else(|| panic!("no detached finish: {}", text(o)))
        .to_string()
}

/// The pending mail of `opus-1` that reports the end of a finish.
fn finish_mail(h: &Harness) -> Option<String> {
    use horch_core::messaging::mail::Mail;
    Mail::to_role(&h.state, "w1", "opus-1")
        .pending()
        .ok()?
        .into_iter()
        .map(|m| m.text)
        .find(|t| t.contains("The finish of T-1 ended."))
}

/// #160: in its pane, a worker's finish returns at once while a long check
/// runs; the result arrives as durable mail. Elsewhere it runs in the
/// foreground and says why.
#[test]
fn a_detached_finish_returns_at_once_and_mails_the_result() {
    let (h, w) = pane_world("tl-detach", "/bin/sleep 20", Some("3"));
    let started = Instant::now();
    let o = finish_in_pane(&h, &w);
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "finish returned after {:?}",
        started.elapsed()
    );
    assert_eq!(o.status.code(), Some(0), "{}", text(&o));
    assert!(
        out(&o).contains("horch tells you the result. End your turn now"),
        "{}",
        text(&o)
    );
    detached_pid(&o);
    let mail = wait_for("the finish mail", || finish_mail(&h));
    assert!(
        mail.starts_with("[horch] NOTE: The finish of T-1 ended."),
        "{mail}"
    );
    assert!(mail.contains("timed out after 3 s"), "{mail}");
    assert!(mail.ends_with("Steps: check failed."), "{mail}");
    let st = json_of(&h, &["ledger", "steps", "T-1", "--json"]);
    assert_eq!(st[0]["verdict"], "timed out", "{st}");

    // Outside its pane, finish runs in the foreground and says why.
    let mut cmd = h.horch(&["ledger", "finish"]);
    cmd.env("HORCH_WORKER_ID", &w);
    let o = cmd.output().unwrap();
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert!(
        text(&o).contains(
            "finish runs in the foreground: this command does not run in the registered pane of opus-1"
        ),
        "{}",
        text(&o)
    );
}

/// #160: a detached finish that is killed leaves no result; the next finish
/// reports it and runs again.
#[test]
fn a_killed_detached_finish_is_reported_by_the_next_finish() {
    let tmp = tempfile::tempdir().unwrap();
    let started = tmp.path().join("started");
    let go = tmp.path().join("go");
    let check = format!(
        "/usr/bin/touch {}; test -f {} || /bin/sleep 20",
        started.display(),
        go.display()
    );
    let (h, w) = pane_world("tl-detach-killed", &check, None);
    let o = finish_in_pane(&h, &w);
    let pid = detached_pid(&o);
    wait_for("the check to start", || started.exists().then_some(()));
    let kill = |args: &[&str]| {
        std::process::Command::new("kill")
            .args(args)
            .status()
            .unwrap()
            .success()
    };
    assert!(kill(&["-TERM", &pid]), "the detached finish runs");
    wait_for("the detached finish to end", || {
        (!kill(&["-0", &pid])).then_some(())
    });
    assert_eq!(finish_mail(&h), None, "a killed run sends no result");

    std::fs::write(&go, "").unwrap();
    let o = finish_in_pane(&h, &w);
    assert!(
        out(&o).contains(&format!(
            "the detached finish (PID {pid}) ended without a result; finish runs again"
        )),
        "{}",
        text(&o)
    );
    assert_ne!(detached_pid(&o), pid, "a new run starts");
    let mail = wait_for("the finish mail", || finish_mail(&h));
    assert!(mail.contains("every finish step passed"), "{mail}");
}
