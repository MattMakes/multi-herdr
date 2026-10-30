//! The task ledger at the CLI seam (#28): the real `horch` binary, the fake
//! herdr and fake agents, a temporary ledger file per test.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::{Duration, Instant};

use horch_e2e::harness::{fixtures, Harness};
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
    let mut h = Harness::new(name).with_git();
    let db = h.root.join("ledger.sqlite3");
    h.set("HORCH_LEDGER_DB", db.to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
    h
}

/// A harness in workspace `w1` (root pane `w1:p1`) whose panes run their
/// commands, like `horch fleet` leaves it.
fn fleet(name: &str, scenario: &str) -> Harness {
    let mut h = world(name);
    let quota = fixtures().join("quota/all-ok.json");
    h.set("HORCH_QUOTA_FILE", quota.to_string_lossy());
    h.set("HORCH_WORKSPACE_ID", "w1");
    h.set("HORCH_FAKE_SCENARIO", scenario);
    let mut create = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut create);
    let o = create
        .args(["workspace", "create", "--label", name])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", text(&o));
    h
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

fn wait_for<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let until = Instant::now() + Duration::from_secs(30);
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

#[test]
fn status_on_a_fresh_ledger_creates_it_and_says_no_runs() {
    let h = world("tl-fresh");
    let db = h.root.join("ledger.sqlite3");
    assert!(!db.exists());
    let o = h.run(&["ledger", "status"]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(out(&o).trim(), "No runs.");
    assert!(db.exists(), "status creates the ledger file");
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
    assert!(p.trim().ends_with(".json"), "{p}");
}

#[test]
fn queue_claim_and_status_through_the_cli() {
    let h = world("tl-queue");
    ok(&h, &["ledger", "run", "new", "alpha"]);
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
    let a = a.to_string_lossy().into_owned();
    let b = b.to_string_lossy().into_owned();
    let added = ok(
        &h,
        &[
            "ledger",
            "add",
            "--file",
            &a,
            "--primary",
            "opus",
            "--own",
            "src/parse",
        ],
    );
    assert!(added.contains("T-1 queued for opus: Parser."), "{added}");
    ok(
        &h,
        &[
            "ledger",
            "add",
            "--file",
            &b,
            "--primary",
            "opus",
            "--blocked-by",
            "T-1",
        ],
    );

    let plan = json_of(&h, &["ledger", "plan-workers", "--json"]);
    assert_eq!(plan[0]["persona"], "opus");
    assert_eq!(plan[0]["spawn"], 1, "{plan}");

    let o = h.run(&["ledger", "status"]);
    assert_eq!(o.status.code(), Some(1), "work remains: {}", text(&o));
    assert!(out(&o).contains("blocked by T-1 (queued)"), "{}", out(&o));

    let w1 = worker_new(&h, "opus");
    let claim = ok_as(&h, &w1, &["ledger", "claim"]);
    assert!(claim.contains("Claimed T-1 (attempt 1): Parser"), "{claim}");
    // This ticket is not committed, so a worktree would not hold it: the
    // claim names it by its absolute path.
    let ticket = ticket_line(&claim);
    assert!(Path::new(&ticket).is_absolute(), "{claim}");
    assert!(ticket.ends_with("tickets/01-a.md"), "{claim}");
    assert!(claim.contains("Own: src/parse"), "{claim}");
    assert!(
        claim.contains("horch ledger worktree new parser"),
        "{claim}"
    );

    let w2 = worker_new(&h, "opus");
    let none = ok_as(&h, &w2, &["ledger", "claim"]);
    assert!(
        none.contains("NOTE: No task for opus. I exit. PID"),
        "{none}"
    );

    ok_as(&h, &w1, &["ledger", "progress", "parser compiles"]);
    let shown = json_of(&h, &["ledger", "show", "T-1", "--json"]);
    let kinds: Vec<&str> = shown["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, vec!["added", "claimed", "progress"]);

    ok(&h, &["ledger", "cancel", "T-2", "not needed"]);
    let st = json_of(&h, &["ledger", "status", "--json"]);
    assert_eq!(st["counts"]["cancelled"], 1);
    assert_eq!(st["complete"], false);
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

/// The path on the claim's `Ticket:` line.
fn ticket_line(claim: &str) -> String {
    let line = claim
        .lines()
        .find_map(|l| l.strip_prefix("Ticket: "))
        .unwrap_or_else(|| panic!("no Ticket line: {claim}"));
    line.strip_suffix(". Read it in full and follow it exactly.")
        .unwrap_or(line)
        .to_string()
}

#[test]
fn a_claim_names_an_ignored_ticket_by_an_absolute_path_that_opens_from_the_worktree() {
    let h = world("tl-ignored");
    project_file(&h, ".gitignore", "ai_docs/\n");
    git_commit_all(&h, "ignore ai_docs");
    ok(&h, &["ledger", "run", "new", "gamma"]);
    project_file(&h, "ai_docs/tickets/01-x.md", "# 01: Ignored\n");
    ok(
        &h,
        &[
            "ledger",
            "add",
            "--file",
            "ai_docs/tickets/01-x.md",
            "--primary",
            "opus",
        ],
    );
    let w = worker_new(&h, "opus");
    let claim = ok_as(&h, &w, &["ledger", "claim"]);
    let ticket = ticket_line(&claim);
    let made = ok_as(&h, &w, &["ledger", "worktree", "new", "ignored"]);
    let worktree = made
        .lines()
        .find_map(|l| l.strip_prefix("path: "))
        .unwrap_or_else(|| panic!("no worktree path: {made}"));
    let worktree = Path::new(worktree.trim());
    assert!(
        !worktree.join("ai_docs/tickets/01-x.md").exists(),
        "the ignored ticket is not in the worktree"
    );
    let opened = worktree.join(&ticket);
    assert!(
        opened.is_file(),
        "Ticket {ticket} from {}: {claim}",
        worktree.display()
    );
}

#[test]
fn a_claim_names_a_tracked_ticket_by_its_relative_path() {
    let h = world("tl-tracked");
    project_file(&h, "tickets/01-t.md", "# 01: Tracked\n");
    git_commit_all(&h, "add a ticket");
    ok(&h, &["ledger", "run", "new", "delta"]);
    ok(
        &h,
        &[
            "ledger",
            "add",
            "--file",
            "tickets/01-t.md",
            "--primary",
            "opus",
        ],
    );
    let w = worker_new(&h, "opus");
    let claim = ok_as(&h, &w, &["ledger", "claim"]);
    assert_eq!(ticket_line(&claim), "tickets/01-t.md", "{claim}");
}

#[test]
fn add_map_queues_a_ticket_map_in_one_step() {
    let h = world("tl-map");
    ok(&h, &["ledger", "run", "new", "beta"]);
    ticket(
        &h,
        "01-a",
        "# 01: Store\n\n**Persona:** primary `backend-developer`, fallback `opus` · **Parallel:** yes\n",
    );
    ticket(
        &h,
        "02-b",
        "# 02: Claim\n\n**Persona:** primary `opus` · **Parallel:** no\n",
    );
    std::fs::write(
        h.project.join("tickets/MAP.md"),
        "# Map: x\nSpec: [#28](https://example/28)\n\n| # | Issue | Title | Blocked by | Wave |\n|---|---|---|---|---|\n\
         | [01](01-a.md) | [#29](https://example/29) | Store | - | 1 |\n\
         | [02](02-b.md) | [#30](https://example/30) | Claim | #29 | 2 |\n",
    )
    .unwrap();
    let o = ok(&h, &["ledger", "add-map", "tickets/MAP.md"]);
    assert!(o.contains("2 tasks queued"), "{o}");
    let st = json_of(&h, &["ledger", "status", "--json"]);
    let tasks = st["tasks"].as_array().unwrap();
    assert_eq!(tasks.len(), 2);
    let claim = &tasks[1]["task"];
    assert_eq!(claim["claimable_by"], "opus");
    assert_eq!(claim["parallel"], false);
    assert_eq!(claim["issue_url"], "https://example/30");
    assert_eq!(claim["spec_ref"], "https://example/28");
    assert!(tasks[1]["waits"]
        .as_str()
        .unwrap()
        .contains("blocked by T-1"));

    // A map with an unknown persona line fails as a whole.
    ticket(&h, "03-c", "# 03: No persona\n");
    std::fs::write(
        h.project.join("tickets/BAD.md"),
        "| # | Title | Blocked by |\n|---|---|---|\n| [03](03-c.md) | C | - |\n",
    )
    .unwrap();
    let o = h.run(&["ledger", "add-map", "tickets/BAD.md"]);
    assert!(!o.status.success());
    assert!(
        text(&o).contains("names no primary persona"),
        "{}",
        text(&o)
    );
}

#[test]
fn eight_parallel_claim_processes_take_exactly_three_tasks() {
    let h = world("tl-race");
    ok(&h, &["ledger", "run", "new", "race"]);
    for n in 0..3 {
        let p = ticket(&h, &format!("t{n}"), &format!("# T{n}\n"));
        ok(
            &h,
            &[
                "ledger",
                "add",
                "--file",
                &p.to_string_lossy(),
                "--primary",
                "opus",
            ],
        );
    }
    let ids: Vec<String> = (0..8).map(|_| worker_new(&h, "opus")).collect();
    let children: Vec<_> = ids
        .iter()
        .map(|id| {
            let mut cmd = h.horch(&["ledger", "claim", "--json"]);
            cmd.env("HORCH_WORKER_ID", id);
            cmd.stdout(std::process::Stdio::piped());
            cmd.spawn().unwrap()
        })
        .collect();
    let mut claimed = 0;
    for c in children {
        let o = c.wait_with_output().unwrap();
        assert!(o.status.success(), "{}", text(&o));
        let v: Value = serde_json::from_slice(&o.stdout).unwrap();
        if v["outcome"] == "claimed" {
            claimed += 1;
        }
    }
    assert_eq!(claimed, 3);
    let st = json_of(&h, &["ledger", "status", "--json"]);
    assert_eq!(st["counts"]["running"], 3);
}

fn worker_json(h: &Harness, id: &str) -> Value {
    json_of(h, &["ledger", "show", id, "--json"])["worker"].clone()
}

#[test]
fn a_spawned_worker_records_its_pids_session_and_checkins() {
    let mut h = fleet("tl-spawn", "exec,stay");
    h.set("HORCH_HOSTNAME", "test-host");
    ok(
        &h,
        &["ledger", "run", "new", "gamma", "--heartbeat-every", "1"],
    );
    let p = ticket(&h, "01-a", "# 01: A\n");
    ok(
        &h,
        &[
            "ledger",
            "add",
            "--file",
            &p.to_string_lossy(),
            "--primary",
            "codex-terra",
        ],
    );

    let o = h.run(&[
        "spawn",
        "codex-terra",
        "--from-pane",
        "w1:p1",
        "--no-tile",
        "--force",
    ]);
    assert!(o.status.success(), "{}", text(&o));
    let pane = out(&o).lines().last().unwrap().trim().to_string();
    let st = json_of(&h, &["ledger", "status", "--json"]);
    let w = st["workers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["worker"]["kind"] == "worker")
        .expect("spawn wrote a worker row")["worker"]
        .clone();
    let id = w["worker_id"].as_str().unwrap().to_string();
    assert_eq!(w["persona"], "codex-terra");
    assert_eq!(w["pane_id"], pane.as_str());
    assert_eq!(w["host"], "test-host");

    // The supervisor and the agent record their PIDs; the start times match.
    let w = wait_for("both PIDs", || {
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
    // The agent was told to claim. The fake writes its call record after
    // the PIDs are in, so wait for it.
    wait_for("the agent's task names horch ledger claim", || {
        h.calls_of("codex")
            .into_iter()
            .rev()
            .find(|c| c["argv"].to_string().contains("horch ledger claim"))
    });

    // Checkins advance while the agent runs.
    let first = w["last_checkin_at"].as_str().unwrap().to_string();
    wait_for("2 more checkins", || {
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
    let w = wait_for("the session", || {
        let w = worker_json(&h, &id);
        w["session_id"].is_string().then_some(w)
    });
    let w = if w["transcript_path"].is_string() {
        w
    } else {
        wait_for("the transcript", || {
            let w = worker_json(&h, &id);
            w["transcript_path"].is_string().then_some(w)
        })
    };
    assert!(
        Path::new(w["transcript_path"].as_str().unwrap()).is_file(),
        "{w}"
    );

    // The agent claims, then is killed: the task is queued again.
    ok_as(&h, &id, &["ledger", "claim"]);
    let agent = w["agent_pid"].as_i64().unwrap() as u32;
    let started = w["agent_started"].as_i64().unwrap() as u64;
    assert!(horch_core::procid::signal_same(
        agent,
        started,
        libc_sigterm()
    ));
    let w = wait_for("the exit", || {
        let w = worker_json(&h, &id);
        (w["status"] == "incomplete").then_some(w)
    });
    assert!(
        w["end_reason"]
            .as_str()
            .unwrap()
            .contains("before horch ledger done"),
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
/// the body. Calls go to `<dir>/calls`.
const FAKE_GH: &str = r#"#!/bin/sh
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
    h.write_bin("gh", FAKE_GH.as_bytes());
    h.set("HORCH_GH_BIN", h.bin.join("gh").to_string_lossy());
    h.set("FAKE_GH_DIR", gh_dir.to_string_lossy());
    h.set("FAKE_GH_BRANCH", "fleet/alpha");
    let cli = h.write_bin(
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
    h
}

fn finish_json(h: &Harness, worker: &str) -> (Option<i32>, Value) {
    let mut cmd = h.horch(&["ledger", "finish", "--json"]);
    cmd.env("HORCH_WORKER_ID", worker);
    let o = cmd.output().unwrap();
    let v: Value =
        serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&o)));
    (o.status.code(), v)
}

#[test]
fn a_task_finishes_through_review_land_pr_and_cleanup() {
    let h = finish_world("tl-finish");
    ok(&h, &["ledger", "run", "new", "alpha"]);
    let p = ticket(&h, "01-a", "# 01: Feature\n");
    ok(
        &h,
        &[
            "ledger",
            "add",
            "--file",
            &p.to_string_lossy(),
            "--primary",
            "opus",
            "--check",
            "test -f worker.txt",
            "--issue",
            "https://github.com/o/r/issues/29",
        ],
    );
    let w = worker_new(&h, "opus");
    ok_as(&h, &w, &["ledger", "claim"]);
    let wt = ok_as(&h, &w, &["ledger", "worktree", "new", "feature"]);
    let path = PathBuf::from(wt.lines().find_map(|l| l.strip_prefix("path: ")).unwrap());
    assert!(path.ends_with(".worktrees/T1-feature"), "{wt}");

    // Uncommitted work is refused.
    std::fs::write(path.join("worker.txt"), "work\n").unwrap();
    let mut cmd = h.horch(&["ledger", "finish"]);
    cmd.env("HORCH_WORKER_ID", &w);
    let o = cmd.output().unwrap();
    assert!(!o.status.success());
    assert!(text(&o).contains("uncommitted"), "{}", text(&o));
    // done refuses before the steps pass.
    let mut cmd = h.horch(&["ledger", "done", "early"]);
    cmd.env("HORCH_WORKER_ID", &w);
    let o = cmd.output().unwrap();
    assert!(
        !o.status.success() && text(&o).contains("is not finished"),
        "{}",
        text(&o)
    );

    // A 50 MB ignored build folder is removed with the worktree.
    std::fs::write(path.join(".gitignore"), "target/\n").unwrap();
    std::fs::create_dir_all(path.join("target")).unwrap();
    std::fs::write(path.join("target/big.bin"), vec![0u8; 50 * 1024 * 1024]).unwrap();
    std::fs::write(
        path.join("lib.ts"),
        "export function feature(): number {\n  return 1;\n}\n",
    )
    .unwrap();
    git_in(&h, &path, &["add", "worker.txt", ".gitignore", "lib.ts"]);
    git_in(
        &h,
        &path,
        &["commit", "--quiet", "--no-gpg-sign", "-m", "feature"],
    );

    // Round 1: code-sweep queues reviewer tasks and the pipeline waits.
    let (code, r) = finish_json(&h, &w);
    assert_eq!(code, Some(1));
    assert_eq!(r["stopped_at"], "code-sweep", "{r}");
    let reviews = r["queued_reviews"].as_array().unwrap().clone();
    assert!(reviews.len() >= 2, "bugs and clarity at least: {r}");
    let plan = json_of(&h, &["ledger", "plan-workers", "--json"]);
    assert_eq!(plan[0]["persona"], "codex-reviewer", "{plan}");

    // Each reviewer claims its task, writes findings, and finishes.
    for _ in &reviews {
        let rw = worker_new(&h, "codex-reviewer");
        let c: Value =
            serde_json::from_str(&ok_as(&h, &rw, &["ledger", "claim", "--json"])).unwrap();
        let findings = c["task"]["spec_ref"].as_str().unwrap().to_string();
        std::fs::write(
            &findings,
            "## Critical\n- none\n## Important\n- none\n## Minor\n- worker.txt:1 name it better\n",
        )
        .unwrap();
        ok_as(
            &h,
            &rw,
            &["ledger", "done", "0 critical, 0 important, 1 minor"],
        );
    }

    // Round 2: every step passes.
    let (code, r) = finish_json(&h, &w);
    assert_eq!(code, Some(0), "{r}");
    let names: Vec<(&str, &str)> = r["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["name"].as_str().unwrap(), s["status"].as_str().unwrap()))
        .collect();
    assert_eq!(
        names,
        vec![
            ("check", "passed"),
            ("code-sweep", "passed"),
            ("complexity-sweep", "passed"),
            ("recheck", "passed"),
            ("land", "passed"),
            ("pr", "passed"),
            ("cleanup", "passed"),
        ]
    );
    assert!(!path.exists(), "the worktree is gone");
    let st = json_of(&h, &["ledger", "status", "--json"]);
    let wtl = &st["worktrees"][0];
    assert_eq!(wtl["status"], "cleaned");
    assert!(
        wtl["bytes_on_disk"].as_i64().unwrap() >= 50 * 1024 * 1024,
        "{wtl}"
    );
    // The remote run branch has the work; the PR has the task's section.
    let remote = h.root.join("remote.git");
    let log = git_in(&h, &remote, &["log", "--oneline", "fleet/alpha"]);
    assert!(log.contains("feature"), "{log}");
    let body = std::fs::read_to_string(h.root.join("gh/body")).unwrap();
    assert!(body.contains("<!-- horch:task T-1 -->"), "{body}");
    assert!(body.contains("Closes #29"), "{body}");
    assert_eq!(st["run"]["pr_number"], 7);

    ok_as(
        &h,
        &w,
        &["ledger", "done", "Feature landed. worker.txt added."],
    );
    let o = h.run(&["ledger", "status"]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o));
}

#[test]
fn a_critical_finding_fails_until_a_fix_is_reviewed_again() {
    let h = finish_world("tl-critical");
    ok(
        &h,
        &["ledger", "run", "new", "alpha", "--steps", "code-sweep"],
    );
    let p = ticket(&h, "01-a", "# 01: Feature\n");
    ok(
        &h,
        &[
            "ledger",
            "add",
            "--file",
            &p.to_string_lossy(),
            "--primary",
            "opus",
        ],
    );
    let w = worker_new(&h, "opus");
    ok_as(&h, &w, &["ledger", "claim"]);
    let wt = ok_as(&h, &w, &["ledger", "worktree", "new", "f"]);
    let path = PathBuf::from(wt.lines().find_map(|l| l.strip_prefix("path: ")).unwrap());
    std::fs::write(path.join("a.txt"), "1\n").unwrap();
    git_in(&h, &path, &["add", "a.txt"]);
    git_in(
        &h,
        &path,
        &["commit", "--quiet", "--no-gpg-sign", "-m", "a"],
    );

    let review_round = |critical: &str, important: &str| {
        let (_, r) = finish_json(&h, &w);
        let n = r["queued_reviews"].as_array().unwrap().len();
        for i in 0..n {
            let rw = worker_new(&h, "codex-reviewer");
            let c: Value =
                serde_json::from_str(&ok_as(&h, &rw, &["ledger", "claim", "--json"])).unwrap();
            let f = c["task"]["spec_ref"].as_str().unwrap().to_string();
            let (crit, imp) = if i == 0 {
                (critical, important)
            } else {
                ("- none", "- none")
            };
            std::fs::write(
                &f,
                format!("## Critical\n{crit}\n## Important\n{imp}\n## Minor\n- none\n"),
            )
            .unwrap();
            ok_as(&h, &rw, &["ledger", "done", "reviewed"]);
        }
    };
    review_round("- a.txt:1 data loss. Fix: x", "- none");
    let (code, r) = finish_json(&h, &w);
    assert_eq!(code, Some(1));
    assert!(r["message"].as_str().unwrap().contains("Critical"), "{r}");

    // A fix is a new commit: the next finish reviews it again.
    std::fs::write(path.join("a.txt"), "2\n").unwrap();
    git_in(
        &h,
        &path,
        &["commit", "--quiet", "--no-gpg-sign", "-am", "fix"],
    );
    review_round("- none", "- a.txt:1 slow. Fix: cache");
    let (code, r) = finish_json(&h, &w);
    assert_eq!(code, Some(1), "an unanswered Important finding fails");
    let msg = r["message"].as_str().unwrap().to_string();
    let answers = msg
        .split_whitespace()
        .find(|t| t.ends_with("answers.md"))
        .expect("the message names the answers file")
        .to_string();
    std::fs::write(&answers, "F1: the input is 3 items at most\n").unwrap();
    let (code, r) = finish_json(&h, &w);
    assert_eq!(
        code,
        Some(0),
        "an answered Important finding passes on WARN: {r}"
    );
    assert_eq!(r["steps"][0]["verdict"], "WARN");
}

#[test]
fn a_failed_reviews_in_tell_wakes_the_orchestrator_once() {
    let h = finish_world("tl-reviews-in");
    ok(
        &h,
        &["ledger", "run", "new", "alpha", "--steps", "code-sweep"],
    );
    let p = ticket(&h, "01-a", "# 01: Feature\n");
    ok(
        &h,
        &[
            "ledger",
            "add",
            "--file",
            &p.to_string_lossy(),
            "--primary",
            "opus",
        ],
    );
    // The author has a role but no pane: the tell to it fails.
    let w = ok(
        &h,
        &[
            "ledger",
            "worker",
            "new",
            "--persona",
            "opus",
            "--role",
            "opus-1",
        ],
    )
    .trim()
    .to_string();
    ok_as(&h, &w, &["ledger", "claim"]);
    let wt = ok_as(&h, &w, &["ledger", "worktree", "new", "f"]);
    let path = PathBuf::from(wt.lines().find_map(|l| l.strip_prefix("path: ")).unwrap());
    std::fs::write(path.join("a.txt"), "1\n").unwrap();
    git_in(&h, &path, &["add", "a.txt"]);
    git_in(
        &h,
        &path,
        &["commit", "--quiet", "--no-gpg-sign", "-m", "a"],
    );
    let (_, r) = finish_json(&h, &w);
    let n = r["queued_reviews"].as_array().unwrap().len();
    let last = review_all(
        &h,
        n,
        "## Critical\n- none\n## Important\n- none\n## Minor\n- none\n",
    );
    let fallbacks = || {
        let events = json_of(&h, &["ledger", "events", "--json"]);
        events
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| {
                e["kind"] == "woke"
                    && e["text"]
                        .as_str()
                        .unwrap()
                        .contains("The reviews of T-1 are in, but opus-1 was not told")
            })
            .count()
    };
    assert_eq!(fallbacks(), 1, "the orchestrator gets the missed tell");
    // A retried done skips the notification, and its fallback too.
    let mut cmd = h.horch(&["ledger", "done", "reviewed"]);
    cmd.env("HORCH_WORKER_ID", &last);
    cmd.output().unwrap();
    assert_eq!(fallbacks(), 1, "a retry does not send it again");
}

#[test]
fn stopping_one_of_three_workers_leaves_the_others_running() {
    let mut h = fleet("tl-stop", "exec,stay");
    h.set("HORCH_STOP_GRACE_MS", "2000");
    ok(
        &h,
        &["ledger", "run", "new", "delta", "--heartbeat-every", "1"],
    );
    for n in 0..3 {
        let p = ticket(&h, &format!("t{n}"), &format!("# T{n}\n"));
        ok(
            &h,
            &[
                "ledger",
                "add",
                "--file",
                &p.to_string_lossy(),
                "--primary",
                "codex-terra",
            ],
        );
    }
    let mut ids = Vec::new();
    for _ in 0..3 {
        let o = h.run(&[
            "spawn",
            "codex-terra",
            "--from-pane",
            "w1:p1",
            "--no-tile",
            "--force",
        ]);
        assert!(o.status.success(), "{}", text(&o));
    }
    let st = json_of(&h, &["ledger", "status", "--json"]);
    for w in st["workers"].as_array().unwrap() {
        if w["worker"]["kind"] == "worker" {
            ids.push(w["worker"]["worker_id"].as_str().unwrap().to_string());
        }
    }
    assert_eq!(ids.len(), 3);
    let pids: Vec<(u32, u64)> = ids
        .iter()
        .map(|id| {
            let w = wait_for("the agent PID", || {
                let w = worker_json(&h, id);
                w["agent_started"].is_i64().then_some(w)
            });
            (
                w["agent_pid"].as_i64().unwrap() as u32,
                w["agent_started"].as_i64().unwrap() as u64,
            )
        })
        .collect();
    for id in &ids {
        ok_as(&h, id, &["ledger", "claim"]);
    }
    let o = ok(&h, &["ledger", "stop", &ids[0], "the operator dropped it"]);
    assert!(o.contains("Stopped"), "{o}");
    assert!(
        !horch_core::procid::alive(pids[0].0, Some(pids[0].1)),
        "the stopped agent is gone"
    );
    assert!(horch_core::procid::alive(pids[1].0, Some(pids[1].1)));
    assert!(horch_core::procid::alive(pids[2].0, Some(pids[2].1)));
    let shown = json_of(&h, &["ledger", "show", &ids[0], "--json"]);
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
    // Its task is queued again; the others still run.
    let st = json_of(&h, &["ledger", "status", "--json"]);
    assert_eq!(st["counts"]["queued"], 1);
    assert_eq!(st["counts"]["running"], 2);
    // A second stop finds the processes gone and sends nothing.
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
fn three_checks_without_progress_reap_the_worker_by_its_pid() {
    let mut h = fleet("tl-watch", "exec,stay");
    h.set("HORCH_STOP_GRACE_MS", "2000");
    ok(
        &h,
        &["ledger", "run", "new", "eps", "--heartbeat-every", "1"],
    );
    let p = ticket(&h, "t0", "# T0\n");
    ok(
        &h,
        &[
            "ledger",
            "add",
            "--file",
            &p.to_string_lossy(),
            "--primary",
            "codex-terra",
        ],
    );
    let o = h.run(&[
        "spawn",
        "codex-terra",
        "--from-pane",
        "w1:p1",
        "--no-tile",
        "--force",
    ]);
    assert!(o.status.success(), "{}", text(&o));
    let st = json_of(&h, &["ledger", "status", "--json"]);
    let id = st["workers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["worker"]["kind"] == "worker")
        .unwrap()["worker"]["worker_id"]
        .as_str()
        .unwrap()
        .to_string();
    let w = wait_for("the agent PID", || {
        let w = worker_json(&h, &id);
        w["agent_started"].is_i64().then_some(w)
    });
    let (pid, started) = (
        w["agent_pid"].as_i64().unwrap() as u32,
        w["agent_started"].as_i64().unwrap() as u64,
    );
    ok_as(&h, &id, &["ledger", "claim"]);
    // The fake agent runs, but shows no timer and writes nothing more.
    let mut verdicts = Vec::new();
    for _ in 0..3 {
        std::thread::sleep(Duration::from_millis(1100));
        let v = json_of(&h, &["ledger", "watch", "--once", "--force", "--json"]);
        verdicts.push(v[0]["verdict"].as_str().unwrap().to_string());
    }
    // The first check may still see the session's transcript appear.
    assert_eq!(verdicts.last().unwrap(), "dead", "{verdicts:?}");
    assert!(
        !horch_core::procid::alive(pid, Some(started)),
        "the agent was stopped"
    );
    let shown = json_of(&h, &["ledger", "show", &id, "--json"]);
    assert_eq!(shown["worker"]["status"], "incomplete");
    let stops = shown["stops"].as_array().unwrap();
    assert!(
        stops.iter().all(|s| s["requested_by"] == "watchdog"),
        "{shown}"
    );
    assert!(stops
        .iter()
        .any(|s| s["target"] == "agent" && s["pid"].as_i64() == Some(pid as i64)));
    let t = json_of(&h, &["ledger", "show", "T-1", "--json"]);
    assert_eq!(t["task"]["status"], "queued");
    // The replacement claims the same task and gets the old transcript path.
    let plan = json_of(&h, &["ledger", "plan-workers", "--json"]);
    assert_eq!(plan[0]["replaces"][0], id.as_str(), "{plan}");
    let r = ok(
        &h,
        &[
            "ledger",
            "worker",
            "new",
            "--persona",
            "codex-terra",
            "--replaces",
            &id,
        ],
    );
    let rid = r.trim().to_string();
    let claim: Value =
        serde_json::from_str(&ok_as(&h, &rid, &["ledger", "claim", "--json"])).unwrap();
    assert_eq!(claim["task"]["seq"], 1);
    assert_eq!(claim["prior"][0]["worker_id"], id.as_str());
    assert_eq!(claim["attempt"], 2);
}

/// Kills the stand-in agent and the watchdog that holds a lock in `locks`
/// when dropped, so a failed assertion leaks neither.
struct WatchdogTeardown {
    agent: std::process::Child,
    locks: PathBuf,
}

impl Drop for WatchdogTeardown {
    fn drop(&mut self) {
        let locks = std::fs::read_dir(&self.locks).into_iter().flatten();
        locks
            .flatten()
            .for_each(|lock| kill_lock_owner(&lock.path()));
        let _ = self.agent.kill();
        let _ = self.agent.wait();
    }
}

/// Kill the process that the `owner` file of `lock` names, if it still runs.
fn kill_lock_owner(lock: &Path) {
    let owner = std::fs::read(lock.join("owner")).unwrap_or_default();
    let v: Value = serde_json::from_slice(&owner).unwrap_or_default();
    if let (Some(pid), Some(started)) = (v["pid"].as_u64(), v["started"].as_u64()) {
        horch_core::procid::signal_same(pid as u32, started, 9);
    }
}

#[test]
fn a_dead_watchdog_is_restarted_and_a_second_one_is_refused() {
    let mut h = world("tl-wdog");
    h.unset("HORCH_NO_WATCHDOG");
    // The watchdog checks the worker every second. Many stall rounds keep a
    // stalled worker from being reaped, which would end the running task.
    ok(
        &h,
        &[
            "ledger",
            "run",
            "new",
            "zeta",
            "--check-every",
            "1",
            "--stall-rounds",
            "1000",
        ],
    );
    let p = ticket(&h, "t0", "# T0\n");
    ok(
        &h,
        &[
            "ledger",
            "add",
            "--file",
            &p.to_string_lossy(),
            "--primary",
            "opus",
        ],
    );
    let w = worker_new(&h, "opus");
    // The worker's agent is a child this test owns, so the watchdog sees it
    // alive and does not reap the worker. A reap would stop only the child.
    let teardown = WatchdogTeardown {
        agent: std::process::Command::new("/bin/sleep")
            .arg("600")
            .spawn()
            .unwrap(),
        locks: h.root.join("locks"),
    };
    horch_core::taskledger::supervisor::agent_started(
        &h.root.join("ledger.sqlite3"),
        &w,
        teardown.agent.id(),
    )
    .unwrap();
    // The first horch command after a task runs starts the missing watchdog.
    let mut cmd = h.horch(&["ledger", "claim"]);
    cmd.env("HORCH_WORKER_ID", &w);
    let o = cmd.output().unwrap();
    assert!(
        text(&o).contains("the watchdog of run zeta was not running; started it"),
        "{}",
        text(&o)
    );
    let run = || json_of(&h, &["ledger", "status", "--json"])["run"].clone();
    let r = wait_for("the watchdog's checkin", || {
        let r = run();
        r["watchdog_pid"].is_i64().then_some(r)
    });
    let pid = r["watchdog_pid"].as_i64().unwrap() as u32;
    let started = r["watchdog_started"].as_i64().unwrap() as u64;
    // A second watchdog for the run is refused.
    let o = h.run(&["ledger", "watch"]);
    assert!(
        !o.status.success() && text(&o).contains("already runs"),
        "{}",
        text(&o)
    );
    // Stop it by its recorded PID; the next command restarts it.
    assert!(horch_core::procid::signal_same(pid, started, 9));
    wait_for("the watchdog to end", || {
        (!horch_core::procid::alive(pid, Some(started))).then_some(())
    });
    // Checkin stamps are whole seconds, so the 2-interval staleness can take
    // up to 3 s to show; poll instead of guessing a sleep.
    wait_for("the restart", || {
        let o = h.run(&["ledger", "status"]);
        text(&o).contains("started it").then_some(())
    });
    let r2 = wait_for("the new watchdog", || {
        let r = run();
        (r["watchdog_pid"].as_i64() != Some(pid as i64) && r["watchdog_pid"].is_i64()).then_some(r)
    });
    let (p2, s2) = (
        r2["watchdog_pid"].as_i64().unwrap() as u32,
        r2["watchdog_started"].as_i64().unwrap() as u64,
    );
    assert!(horch_core::procid::signal_same(p2, s2, 9), "teardown");
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
    // An unlanded worktree is not removed: its commits are not on main.
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
        !o.status.success() && text(&o).contains("not on main"),
        "{}",
        text(&o)
    );
    assert!(un.exists());

    // Reopen it: the new task's claim gets the old worktree.
    ok(&h, &["ledger", "run", "new", "again"]);
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
    let w = worker_new(&h, "opus");
    let c = ok_as(&h, &w, &["ledger", "claim"]);
    assert!(c.contains("Reopened from unfinished past work"), "{c}");
    let un_claimed = link.join(un.strip_prefix(&proj).unwrap());
    assert!(
        c.contains(&format!("Worktree: {}", un_claimed.display())),
        "{c}"
    );
    // The inherited worktree lacks the untracked ticket: the claim names it
    // by its absolute path.
    let ticket = ticket_line(&c);
    assert!(Path::new(&ticket).is_absolute(), "{c}");
    assert!(ticket.ends_with("tickets/finish-it.md"), "{c}");
}

#[test]
fn assign_refuses_in_a_ledger_run() {
    let mut h = fleet("tl-assign", "default");
    h.set("HORCH_NO_WATCHDOG", "1");
    ok(&h, &["ledger", "run", "new", "eta"]);
    let o = h.run(&["assign", "opus-1", "do it"]);
    assert!(!o.status.success());
    assert!(
        text(&o).contains("assignment is queue plus claim"),
        "{}",
        text(&o)
    );
}

/// Review every queued reviewer task of a finish round with `findings`.
/// Returns the last reviewer's worker id.
fn review_all(h: &Harness, n: usize, findings: &str) -> String {
    let mut last = String::new();
    for _ in 0..n {
        let rw = worker_new(h, "codex-reviewer");
        let c: Value =
            serde_json::from_str(&ok_as(h, &rw, &["ledger", "claim", "--json"])).unwrap();
        assert_eq!(c["outcome"], "claimed", "{c}");
        let f = c["task"]["spec_ref"].as_str().unwrap().to_string();
        std::fs::write(&f, findings).unwrap();
        ok_as(h, &rw, &["ledger", "done", "reviewed"]);
        last = rw;
    }
    last
}

/// A claimed task's worktree, with 1 committed file, finished through every
/// step. Returns the finish report.
fn work_and_finish(h: &Harness, w: &str, name: &str) -> Value {
    let wt = ok_as(h, w, &["ledger", "worktree", "new", name]);
    let path = PathBuf::from(wt.lines().find_map(|l| l.strip_prefix("path: ")).unwrap());
    std::fs::write(
        path.join(format!("{name}.rs")),
        format!("fn {name}() {{}}\n"),
    )
    .unwrap();
    git_in(h, &path, &["add", "."]);
    git_in(
        h,
        &path,
        &["commit", "--quiet", "--no-gpg-sign", "-m", name],
    );
    let (code, r) = finish_json(h, w);
    assert_eq!(code, Some(1), "round 1 waits for reviews: {r}");
    let n = r["queued_reviews"].as_array().unwrap().len();
    review_all(
        h,
        n,
        "## Critical\n- none\n## Important\n- none\n## Minor\n- none\n",
    );
    let (code, r) = finish_json(h, w);
    assert_eq!(code, Some(0), "{r}");
    assert!(!path.exists(), "{name}: the worktree is gone");
    r
}

#[test]
fn the_whole_flow_on_the_fake_herdr() {
    let mut h = finish_world("tl-e2e");
    let quota = fixtures().join("quota/all-ok.json");
    h.set("HORCH_QUOTA_FILE", quota.to_string_lossy());
    h.set("HORCH_WORKSPACE_ID", "w1");
    h.set("HORCH_FAKE_SCENARIO", "exec,stay");
    h.set("HORCH_STOP_GRACE_MS", "2000");
    let mut create = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut create);
    assert!(create
        .args(["workspace", "create", "--label", "e2e"])
        .output()
        .unwrap()
        .status
        .success());

    // 1. The orchestrator queues 3 tasks: A and B parallel, C blocked by A.
    ok(
        &h,
        &["ledger", "run", "new", "alpha", "--heartbeat-every", "1"],
    );
    for (n, t, persona) in [
        ("a", "A", "opus"),
        ("b", "B", "codex-terra"),
        ("c", "C", "opus"),
    ] {
        ticket(
            &h,
            n,
            &format!("# {t}\n\n**Persona:** primary `{persona}`\n"),
        );
    }
    std::fs::write(
        h.project.join("tickets/MAP.md"),
        "| # | Title | Blocked by |\n|---|---|---|\n| [1](a.md) | A | - |\n| [2](b.md) | B | - |\n| [3](c.md) | C | 1 |\n",
    )
    .unwrap();
    ok(&h, &["ledger", "add-map", "tickets/MAP.md"]);

    // 2. plan-workers says 2: 1 opus (A) and 1 codex-terra (B).
    let plan = json_of(&h, &["ledger", "plan-workers", "--json"]);
    let spawn: i64 = plan
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["spawn"].as_i64().unwrap())
        .sum();
    assert_eq!(spawn, 2, "{plan}");
    let wa = worker_new(&h, "opus");
    assert!(ok_as(&h, &wa, &["ledger", "claim"]).contains("Claimed T-1"));
    // B runs in a real pane: its agent is the fake codex that stays.
    let o = h.run(&[
        "spawn",
        "codex-terra",
        "--from-pane",
        "w1:p1",
        "--no-tile",
        "--force",
    ]);
    assert!(o.status.success(), "{}", text(&o));
    let st = json_of(&h, &["ledger", "status", "--json"]);
    let wb = st["workers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|w| w["worker"]["persona"] == "codex-terra")
        .unwrap()["worker"]["worker_id"]
        .as_str()
        .unwrap()
        .to_string();
    let b = wait_for("B's agent PID", || {
        let w = worker_json(&h, &wb);
        w["agent_started"].is_i64().then_some(w)
    });
    let (bpid, bstart) = (
        b["agent_pid"].as_i64().unwrap() as u32,
        b["agent_started"].as_i64().unwrap() as u64,
    );
    assert!(ok_as(&h, &wb, &["ledger", "claim"]).contains("Claimed T-2"));

    // 3. B stops making progress: its fake agent runs, but shows no timer
    // and writes nothing.
    // 4. A lands: every step passes, its folder is gone, and C can start.
    work_and_finish(&h, &wa, "alpha_a");
    ok_as(&h, &wa, &["ledger", "done", "A done."]);
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
    // 5. After 3 checks the watchdog reaps B's worker by its PIDs; A's and
    // C's workers are untouched.
    let mut last = String::new();
    for _ in 0..3 {
        std::thread::sleep(Duration::from_millis(1100));
        let v = json_of(&h, &["ledger", "watch", "--once", "--force", "--json"]);
        last = v
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["worker_id"] == wb.as_str())
            .map(|c| c["verdict"].as_str().unwrap().to_string())
            .unwrap_or(last);
    }
    assert_eq!(last, "dead");
    assert!(!horch_core::procid::alive(bpid, Some(bstart)));
    // C is claimed now (a worker row without a live process would be judged
    // dead by the checks above).
    let wc = worker_new(&h, "opus");
    assert!(ok_as(&h, &wc, &["ledger", "claim"]).contains("Claimed T-3"));
    let rb = ok(
        &h,
        &[
            "ledger",
            "worker",
            "new",
            "--persona",
            "codex-terra",
            "--replaces",
            &wb,
        ],
    )
    .trim()
    .to_string();
    let claim: Value =
        serde_json::from_str(&ok_as(&h, &rb, &["ledger", "claim", "--json"])).unwrap();
    assert_eq!(claim["task"]["seq"], 2);
    assert_eq!(claim["prior"][0]["worker_id"], wb.as_str());
    assert!(
        claim["prior"][0]["transcript_path"].is_string(),
        "the old transcript path is handed over: {claim}"
    );

    // 6. A spare worker finds no task and names its PID.
    let spare = worker_new(&h, "opus");
    ok(
        &h,
        &[
            "ledger",
            "worker",
            "pid",
            &spare,
            "--supervisor",
            &std::process::id().to_string(),
        ],
    );
    let none = ok_as(&h, &spare, &["ledger", "claim"]);
    assert!(
        none.contains(&format!(
            "No task for opus. I exit. PID {}.",
            std::process::id()
        )),
        "{none}"
    );

    // 7. B (replaced) and C finish; everything is done and cleaned.
    work_and_finish(&h, &rb, "alpha_b");
    ok_as(&h, &rb, &["ledger", "done", "B done by the replacement."]);
    work_and_finish(&h, &wc, "alpha_c");
    ok_as(&h, &wc, &["ledger", "done", "C done."]);
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
}

/// The output of `horch ledger <args>` as `worker`, success or not.
fn as_worker(h: &Harness, worker: &str, args: &[&str]) -> Output {
    let mut cmd = h.horch(args);
    cmd.env("HORCH_WORKER_ID", worker);
    cmd.output().unwrap()
}

#[test]
fn a_shared_task_claims_on_the_run_branch_and_lands_only_new_clean_commits() {
    let h = finish_world("tl-shared");
    // A shared checkout holds only committed tickets: check needs it clean.
    let p = ticket(&h, "01-a", "# 01: Doc\n");
    git_commit_all(&h, "ticket");
    ok(
        &h,
        &["ledger", "run", "new", "alpha", "--steps", "check,land"],
    );
    ok(
        &h,
        &[
            "ledger",
            "add",
            "--file",
            &p.to_string_lossy(),
            "--primary",
            "opus",
            "--shared",
            "--check",
            "true",
        ],
    );
    let w = worker_new(&h, "opus");
    // The main checkout is on main: the claim refuses and names the switch.
    let o = as_worker(&h, &w, &["ledger", "claim"]);
    assert!(!o.status.success(), "{}", text(&o));
    assert!(text(&o).contains("git switch fleet/alpha"), "{}", text(&o));
    let st = json_of(&h, &["ledger", "status", "--json"]);
    assert_eq!(
        st["tasks"][0]["task"]["status"], "queued",
        "the refusal claims nothing: {st}"
    );

    let project = h.project.clone();
    git_in(&h, &project, &["switch", "--quiet", "fleet/alpha"]);
    ok_as(&h, &w, &["ledger", "claim"]);

    // check judges committed work only, in a shared checkout too.
    std::fs::write(project.join("doc.md"), "draft\n").unwrap();
    let (code, r) = finish_json(&h, &w);
    assert_eq!(code, Some(1));
    assert_eq!(r["stopped_at"], "check", "{r}");
    assert!(
        r["message"].as_str().unwrap().contains("uncommitted"),
        "{r}"
    );

    // No commit since the claim: land refuses.
    std::fs::remove_file(project.join("doc.md")).unwrap();
    let (code, r) = finish_json(&h, &w);
    assert_eq!(code, Some(1));
    assert_eq!(r["stopped_at"], "land", "{r}");
    assert!(
        r["message"]
            .as_str()
            .unwrap()
            .contains("no commit since the claim"),
        "{r}"
    );

    std::fs::write(project.join("doc.md"), "done\n").unwrap();
    git_in(&h, &project, &["add", "doc.md"]);
    git_in(
        &h,
        &project,
        &["commit", "--quiet", "--no-gpg-sign", "-m", "doc"],
    );
    let (code, r) = finish_json(&h, &w);
    assert_eq!(code, Some(0), "{r}");
    let remote = h.root.join("remote.git");
    let log = git_in(&h, &remote, &["log", "--oneline", "fleet/alpha"]);
    assert!(log.contains("doc"), "{log}");
}

/// A run with `steps`, 1 task with `check`, claimed by a new worker that
/// commits `file` in its worktree. Returns (worker, worktree path).
fn claimed_with_commit(
    h: &Harness,
    steps: &str,
    check: Option<&str>,
    file: &str,
) -> (String, PathBuf) {
    ok(h, &["ledger", "run", "new", "alpha", "--steps", steps]);
    let p = ticket(h, "01-a", "# 01: Feature\n");
    let mut add = vec![
        "ledger".to_string(),
        "add".into(),
        "--file".into(),
        p.to_string_lossy().into_owned(),
        "--primary".into(),
        "opus".into(),
    ];
    if let Some(c) = check {
        add.extend(["--check".into(), c.to_string()]);
    }
    ok(h, &add.iter().map(|s| s.as_str()).collect::<Vec<_>>());
    let w = worker_new(h, "opus");
    ok_as(h, &w, &["ledger", "claim"]);
    let wt = ok_as(h, &w, &["ledger", "worktree", "new", "feat"]);
    let path = PathBuf::from(wt.lines().find_map(|l| l.strip_prefix("path: ")).unwrap());
    commit_file(h, &path, file);
    (w, path)
}

fn commit_file(h: &Harness, dir: &Path, file: &str) {
    std::fs::write(dir.join(file), format!("{file}\n")).unwrap();
    git_in(h, dir, &["add", file]);
    git_in(h, dir, &["commit", "--quiet", "--no-gpg-sign", "-m", file]);
}

/// The (status, verdict) of step `name` in a finish report.
fn step_of(r: &Value, name: &str) -> (String, String) {
    let s = r["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == name)
        .unwrap_or_else(|| panic!("no step {name}: {r}"));
    (
        s["status"].as_str().unwrap_or("").to_string(),
        s["verdict"].as_str().unwrap_or("").to_string(),
    )
}

fn lock_worktree(h: &Harness, path: &Path, on: bool) {
    let project = h.project.clone();
    let p = path.to_string_lossy();
    let verb = if on { "lock" } else { "unlock" };
    git_in(h, &project, &["worktree", verb, &p]);
}

fn reject_pushes(h: &Harness, on: bool) {
    let hook = h.root.join("remote.git/hooks/pre-receive");
    if on {
        std::fs::write(&hook, "#!/bin/sh\necho rejected >&2\nexit 1\n").unwrap();
        horch_core::runtime::process::make_executable(&hook).unwrap();
    } else {
        let _ = std::fs::remove_file(&hook);
    }
}

#[test]
fn a_failed_push_after_a_land_is_retried_by_the_next_finish() {
    let h = finish_world("tl-push-retry");
    let (w, _) = claimed_with_commit(&h, "land,cleanup", None, "a.txt");
    reject_pushes(&h, true);
    let (code, r) = finish_json(&h, &w);
    assert_eq!(code, Some(1));
    assert_eq!(
        step_of(&r, "land"),
        ("failed".into(), "landed, not pushed".into()),
        "{r}"
    );
    // A 2nd failure keeps the step failed.
    let (code, r) = finish_json(&h, &w);
    assert_eq!(code, Some(1));
    assert_eq!(
        step_of(&r, "land"),
        ("failed".into(), "landed, not pushed".into()),
        "{r}"
    );
    reject_pushes(&h, false);
    let (code, r) = finish_json(&h, &w);
    assert_eq!(code, Some(0), "{r}");
    let project = h.project.clone();
    let remote = h.root.join("remote.git");
    assert_eq!(
        git_in(&h, &remote, &["rev-parse", "fleet/alpha"]),
        git_in(&h, &project, &["rev-parse", "fleet/alpha"])
    );
    assert_eq!(step_of(&r, "cleanup").0, "passed");
}

#[test]
fn a_replacement_commit_after_the_land_lands_and_is_pushed() {
    let h = finish_world("tl-reland");
    let (w1, path) = claimed_with_commit(&h, "land,cleanup", None, "a.txt");
    // A locked worktree is not removed: cleanup fails after the land.
    lock_worktree(&h, &path, true);
    let (code, r) = finish_json(&h, &w1);
    assert_eq!(code, Some(1));
    assert_eq!(step_of(&r, "land").0, "passed", "{r}");
    assert_eq!(step_of(&r, "cleanup").0, "failed", "{r}");
    ok(&h, &["ledger", "stop", &w1, "reaped in the test"]);
    let w2 = ok(
        &h,
        &[
            "ledger",
            "worker",
            "new",
            "--persona",
            "opus",
            "--replaces",
            &w1,
        ],
    )
    .trim()
    .to_string();
    let claim = ok_as(&h, &w2, &["ledger", "claim"]);
    assert!(claim.contains("already landed"), "{claim}");
    assert!(claim.contains("cleanup"), "the steps left: {claim}");
    lock_worktree(&h, &path, false);
    commit_file(&h, &path, "b.txt");
    let (code, r) = finish_json(&h, &w2);
    assert_eq!(code, Some(0), "{r}");
    let project = h.project.clone();
    assert_eq!(
        git_in(&h, &project, &["show", "fleet/alpha:b.txt"]),
        "b.txt"
    );
    let remote = h.root.join("remote.git");
    assert_eq!(git_in(&h, &remote, &["show", "fleet/alpha:b.txt"]), "b.txt");
    assert!(!path.exists(), "cleanup removed the worktree");
}

#[test]
fn a_commit_after_the_land_is_checked_and_reviewed_again() {
    let h = finish_world("tl-reland-review");
    let (w, path) = claimed_with_commit(&h, "check,code-sweep,land,cleanup", Some("true"), "a.txt");
    let clean = "## Critical\n- none\n## Important\n- none\n## Minor\n- none\n";
    let (_, r) = finish_json(&h, &w);
    review_all(&h, r["queued_reviews"].as_array().unwrap().len(), clean);
    lock_worktree(&h, &path, true);
    let (_, r) = finish_json(&h, &w);
    assert_eq!(step_of(&r, "land").0, "passed", "{r}");
    assert_eq!(r["stopped_at"], "cleanup", "{r}");
    let check_rounds = r["steps"][0]["rounds"].as_i64().unwrap();

    lock_worktree(&h, &path, false);
    commit_file(&h, &path, "b.txt");
    let (code, r) = finish_json(&h, &w);
    assert_eq!(code, Some(1));
    assert_eq!(r["stopped_at"], "code-sweep", "a new round: {r}");
    assert!(
        r["steps"][0]["rounds"].as_i64().unwrap() > check_rounds,
        "check ran again: {r}"
    );
    review_all(&h, r["queued_reviews"].as_array().unwrap().len(), clean);
    let (code, r) = finish_json(&h, &w);
    assert_eq!(code, Some(0), "{r}");
    let remote = h.root.join("remote.git");
    assert_eq!(git_in(&h, &remote, &["show", "fleet/alpha:b.txt"]), "b.txt");
}

#[test]
fn a_landed_worktree_with_no_new_commit_passes_land_without_a_push() {
    let h = finish_world("tl-landed-again");
    let (w, path) = claimed_with_commit(&h, "land,cleanup", None, "a.txt");
    lock_worktree(&h, &path, true);
    let (_, r) = finish_json(&h, &w);
    let (_, landed) = step_of(&r, "land");
    // Any push now fails: a land that pushed again would fail.
    reject_pushes(&h, true);
    let (code, r) = finish_json(&h, &w);
    assert_eq!(code, Some(1));
    assert_eq!(step_of(&r, "land"), ("passed".into(), landed), "{r}");
    assert_eq!(r["stopped_at"], "cleanup", "{r}");
}

#[test]
fn a_landed_worktree_passes_land_at_once_in_a_run_with_no_remote() {
    let h = world("tl-landed-local");
    let (w, path) = claimed_with_commit(&h, "land,cleanup", None, "a.txt");
    lock_worktree(&h, &path, true);
    let (_, r) = finish_json(&h, &w);
    let (_, landed) = step_of(&r, "land");
    assert_eq!(r["stopped_at"], "cleanup", "{r}");
    let (_, r) = finish_json(&h, &w);
    assert_eq!(step_of(&r, "land"), ("passed".into(), landed), "{r}");
    lock_worktree(&h, &path, false);
    let (code, r) = finish_json(&h, &w);
    assert_eq!(code, Some(0), "{r}");
}

#[test]
fn a_check_past_its_timeout_is_killed_with_its_children() {
    let h = finish_world("tl-check-timeout");
    ok(&h, &["ledger", "run", "new", "alpha", "--steps", "check"]);
    let p = ticket(&h, "01-a", "# 01: Feature\n");
    let pidfile = h.root.join("sleep.pid");
    // The check never ends by itself: only the timeout ends it, so no
    // wall-clock bound on the finish is needed to prove the kill.
    let check = format!("/bin/sleep 3600 & echo $! > {}; wait", pidfile.display());
    ok(
        &h,
        &[
            "ledger",
            "add",
            "--file",
            &p.to_string_lossy(),
            "--primary",
            "opus",
            "--check",
            &check,
            "--check-timeout",
            "2",
        ],
    );
    let w = worker_new(&h, "opus");
    ok_as(&h, &w, &["ledger", "claim"]);
    let wt = ok_as(&h, &w, &["ledger", "worktree", "new", "feat"]);
    let path = PathBuf::from(wt.lines().find_map(|l| l.strip_prefix("path: ")).unwrap());
    commit_file(&h, &path, "a.txt");
    let (code, r) = finish_json(&h, &w);
    assert_eq!(code, Some(1), "{r}");
    assert_eq!(
        step_of(&r, "check"),
        ("failed".into(), "timed out".into()),
        "{r}"
    );
    assert!(
        r["message"]
            .as_str()
            .unwrap()
            .contains("timed out after 2 s"),
        "{r}"
    );
    let pid = std::fs::read_to_string(&pidfile)
        .unwrap()
        .trim()
        .to_string();
    // The killed sleep is reaped by init, not by finish: wait for that.
    wait_for("the check's sleep to be gone", || {
        let alive = std::process::Command::new("kill")
            .args(["-0", &pid])
            .status()
            .unwrap()
            .success();
        (!alive).then_some(())
    });
}

#[test]
fn a_land_checks_outside_the_lock_and_lands_after_a_faster_task() {
    let h = finish_world("tl-land-race");
    ok(&h, &["ledger", "run", "new", "alpha", "--steps", "land"]);
    let started = h.root.join("a-started");
    let release = h.root.join("a-release");
    // A's check holds until the test releases it, so B's land always falls
    // inside A's check, however slow the host is. The loop ends after 600 s
    // so a failed test leaves no check behind.
    let slow = format!(
        "/usr/bin/touch {}; i=0; while [ $i -lt 6000 ]; do [ -e {} ] && exit 0; /bin/sleep 0.1; i=$((i + 1)); done; exit 1",
        started.display(),
        release.display()
    );
    for (name, check) in [("01-a", slow.as_str()), ("02-b", "true")] {
        let p = ticket(&h, name, &format!("# {name}\n"));
        ok(
            &h,
            &[
                "ledger",
                "add",
                "--file",
                &p.to_string_lossy(),
                "--primary",
                "opus",
                "--check",
                check,
            ],
        );
    }
    let wa = worker_new(&h, "opus");
    ok_as(&h, &wa, &["ledger", "claim"]);
    let wb = worker_new(&h, "opus");
    ok_as(&h, &wb, &["ledger", "claim"]);
    let mut paths = Vec::new();
    for (w, name, file) in [(&wa, "a", "a.txt"), (&wb, "b", "b.txt")] {
        let wt = ok_as(&h, w, &["ledger", "worktree", "new", name]);
        let path = PathBuf::from(wt.lines().find_map(|l| l.strip_prefix("path: ")).unwrap());
        commit_file(&h, &path, file);
        paths.push(path);
    }
    let mut a = h.horch(&["ledger", "finish", "--json"]);
    a.env("HORCH_WORKER_ID", &wa)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut a = a.spawn().unwrap();
    wait_for("A's land check", || started.exists().then_some(()));
    let (code, r) = finish_json(&h, &wb);
    assert_eq!(code, Some(0), "B lands while A checks: {r}");
    assert!(a.try_wait().unwrap().is_none(), "A still checks");
    std::fs::write(&release, "").unwrap();
    let out = a.wait_with_output().unwrap();
    assert!(out.status.success(), "{}", text(&out));
    let project = h.project.clone();
    let log = git_in(&h, &project, &["log", "--format=%s", "fleet/alpha"]);
    let order: Vec<&str> = log.lines().take(2).collect();
    assert_eq!(order, vec!["a.txt", "b.txt"], "A lands after B: {log}");
}

/// A run with only the code-sweep step and 1 claimed task with 1 commit.
fn sweep_world(h: &Harness) -> (String, PathBuf) {
    claimed_with_commit(h, "code-sweep", None, "a.txt")
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

const CLEAN: &str = "## Critical\n- none\n## Important\n- none\n## Minor\n- none\n";

#[test]
fn a_reviewer_that_cannot_run_hands_the_round_to_its_fallback() {
    let mut h = finish_world("tl-sweep-down");
    h.set("HORCH_CODEX_BIN", h.root.join("no-codex").to_string_lossy());
    let (w, _) = sweep_world(&h);
    let (code, r) = finish_json(&h, &w);
    assert_eq!(code, Some(1));
    let msg = r["message"].as_str().unwrap();
    assert!(msg.contains("codex-reviewer cannot run here"), "{msg}");
    assert!(
        msg.contains("the reviews go to architect-reviewer"),
        "{msg}"
    );
    let all = reviews(&h);
    assert!(!all.is_empty());
    assert!(
        all.iter().all(|(_, _, p)| p == "architect-reviewer"),
        "{all:?}"
    );
}

#[test]
fn a_queued_review_moves_to_its_fallback_persona() {
    let h = finish_world("tl-sweep-fallback");
    let (w, _) = sweep_world(&h);
    finish_json(&h, &w);
    let (first, _, persona) = reviews(&h)[0].clone();
    assert_eq!(persona, "codex-reviewer");
    ok(&h, &["ledger", "fallback", &first, "codex quota spent"]);
    let after = reviews(&h);
    assert_eq!(after[0].2, "architect-reviewer", "{after:?}");
}

#[test]
fn a_cancelled_review_is_queued_again_and_the_sweep_then_passes() {
    let h = finish_world("tl-sweep-cancel");
    let (w, _) = sweep_world(&h);
    let (_, r) = finish_json(&h, &w);
    let n = r["queued_reviews"].as_array().unwrap().len();
    let (first, _, _) = reviews(&h)[0].clone();
    ok(
        &h,
        &["ledger", "cancel", &first, "the reviewer cannot start"],
    );
    let (code, r) = finish_json(&h, &w);
    assert_eq!(code, Some(1));
    assert_eq!(
        step_of(&r, "code-sweep").0,
        "running",
        "the step waits: {r}"
    );
    assert!(
        r["message"]
            .as_str()
            .unwrap()
            .contains("1 cancelled reviews are queued again"),
        "{r}"
    );
    let all = reviews(&h);
    assert_eq!(all.len(), n + 1, "1 new review task: {all:?}");
    assert_eq!(all.iter().filter(|r| r.1 == "queued").count(), n, "{all:?}");
    review_all(&h, n, CLEAN);
    let (code, r) = finish_json(&h, &w);
    assert_eq!(code, Some(0), "{r}");
    assert_eq!(step_of(&r, "code-sweep"), ("passed".into(), "PASS".into()));
}

#[test]
fn only_the_orchestrator_waives_a_critical_finding() {
    let h = finish_world("tl-sweep-waive");
    let (w, _) = sweep_world(&h);
    let (_, r) = finish_json(&h, &w);
    let n = r["queued_reviews"].as_array().unwrap().len();
    review_all(&h, 1, "## Critical\n- a.txt:1 SQL injection. Fix: bind it\n## Important\n- none\n## Minor\n- none\n");
    review_all(&h, n - 1, CLEAN);
    let (code, r) = finish_json(&h, &w);
    assert_eq!(code, Some(1), "{r}");
    assert!(
        r["message"]
            .as_str()
            .unwrap()
            .contains("1 Critical findings (F1)"),
        "{r}"
    );

    let o = as_worker(&h, &w, &["ledger", "waive", "T-1", "F1", "it is mine"]);
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

    let (code, r) = finish_json(&h, &w);
    assert_eq!(code, Some(0), "{r}");
    let steps = &r["steps"].as_array().unwrap();
    let ev = steps[0]["evidence_path"].as_str().unwrap();
    let report = std::fs::read_to_string(ev).unwrap();
    assert!(
        report.contains("F1 [")
            && report.contains("waived by the orchestrator: the query binds its parameters"),
        "{report}"
    );
}

#[test]
fn a_new_tree_supersedes_the_open_reviews_of_the_old_round() {
    let h = finish_world("tl-sweep-supersede");
    let (w, path) = sweep_world(&h);
    let (_, r) = finish_json(&h, &w);
    let n = r["queued_reviews"].as_array().unwrap().len();
    // 1 reviewer holds a round-1 review when the tree changes.
    let rw = worker_new(&h, "codex-reviewer");
    ok_as(&h, &rw, &["ledger", "claim"]);
    commit_file(&h, &path, "b.txt");
    let (code, r) = finish_json(&h, &w);
    assert_eq!(code, Some(1));
    assert!(r["message"].as_str().unwrap().contains("round 2"), "{r}");
    assert_eq!(
        worker_json(&h, &rw)["status"],
        "stopped",
        "its review is superseded"
    );
    let all = reviews(&h);
    let (old, new) = all.split_at(n);
    assert!(
        old.iter().all(|r| r.1 == "cancelled"),
        "round 1 is cancelled: {all:?}"
    );
    assert!(
        new.iter().all(|r| r.1 == "queued"),
        "round 2 is queued: {all:?}"
    );
    // A superseded review is never queued again.
    finish_json(&h, &w);
    assert_eq!(reviews(&h).len(), all.len(), "{:?}", reviews(&h));
}

#[test]
fn a_replacement_with_the_same_tree_reuses_the_finished_round() {
    let h = finish_world("tl-sweep-replace");
    let (w1, _) = sweep_world(&h);
    let (_, r) = finish_json(&h, &w1);
    let n = r["queued_reviews"].as_array().unwrap().len();
    review_all(&h, n, CLEAN);
    ok(&h, &["ledger", "stop", &w1, "reaped in the test"]);
    let w2 = ok(
        &h,
        &[
            "ledger",
            "worker",
            "new",
            "--persona",
            "opus",
            "--replaces",
            &w1,
        ],
    )
    .trim()
    .to_string();
    ok_as(&h, &w2, &["ledger", "claim"]);
    let (code, r) = finish_json(&h, &w2);
    assert_eq!(code, Some(0), "{r}");
    assert_eq!(reviews(&h).len(), n, "no new review task");
}

#[test]
fn a_missing_reviewer_prompt_queues_no_review() {
    let h = finish_world("tl-sweep-noprompt");
    std::fs::remove_file(h.root.join("code-sweep/reviewers/clarity-checker.md")).unwrap();
    let (w, _) = sweep_world(&h);
    let (code, r) = finish_json(&h, &w);
    assert_eq!(code, Some(1));
    assert!(
        r["message"].as_str().unwrap().contains("clarity-checker"),
        "{r}"
    );
    assert!(reviews(&h).is_empty(), "{:?}", reviews(&h));
}

#[test]
fn a_rust_only_change_is_not_sent_to_the_complexity_analyser() {
    let mut h = finish_world("tl-complexity-rs");
    let called = h.root.join("analyser-called");
    let cli = h.write_bin(
        "marking-complexity",
        format!(
            "#!/bin/sh\n/usr/bin/touch {}\necho '{{\"files\":[],\"violations\":[]}}'\n",
            called.display()
        )
        .as_bytes(),
    );
    h.set("HORCH_COMPLEXITY_CLI", cli.to_string_lossy());
    let (w, path) = claimed_with_commit(&h, "complexity-sweep", None, "lib.rs");
    commit_file(&h, &path, "node.gd");
    let (code, r) = finish_json(&h, &w);
    assert_eq!(code, Some(0), "{r}");
    assert_eq!(step_of(&r, "complexity-sweep").0, "not_applicable", "{r}");
    assert!(!called.exists(), "the analyser parses TypeScript only");
}

#[test]
fn finish_finds_the_complexity_analyser_in_a_local_marketplace_of_the_store() {
    let mut h = finish_world("tl-complexity-store");
    // The store installed code-sweep from a local marketplace checkout. The
    // analyser sits in the same checkout, outside the store.
    let market = h.root.join("skill-marketplace/plugins");
    let dist = market.join("code/skills/complexity-sweep/code/cli/dist");
    std::fs::create_dir_all(&dist).unwrap();
    let called = h.root.join("analyser-called");
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
    let (w, _) = claimed_with_commit(&h, "complexity-sweep", None, "a.ts");
    let (code, r) = finish_json(&h, &w);
    assert_eq!(code, Some(0), "{r}");
    assert_eq!(step_of(&r, "complexity-sweep").0, "passed", "{r}");
    assert!(called.exists(), "the marketplace analyser ran");
}

#[test]
fn a_complexity_report_past_64_kib_is_read_whole() {
    let mut h = finish_world("tl-complexity-big");
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
    let cli = h.write_bin(
        "node-like-complexity",
        format!(
            "#!/bin/sh\nif [ -p /dev/stdout ]; then /usr/bin/head -c 65536 {r}; else /bin/cat {r}; fi\nexit 0\n",
            r = report_path.display()
        )
        .as_bytes(),
    );
    h.set("HORCH_COMPLEXITY_CLI", cli.to_string_lossy());
    let (w, _) = claimed_with_commit(&h, "complexity-sweep", None, "a.ts");
    let (code, r) = finish_json(&h, &w);
    assert_eq!(code, Some(1), "{r}");
    let (status, summary) = step_of(&r, "complexity-sweep");
    assert_eq!(
        (status.as_str(), summary.as_str()),
        ("failed", "1 violations"),
        "{r}"
    );
}

#[test]
fn an_outer_git_dir_reaches_no_child_of_finish() {
    let mut h = finish_world("tl-scrub");
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
    let dumps = h.root.join("env");
    std::fs::create_dir_all(&dumps).unwrap();
    let gh = h.bin.join("gh").to_string_lossy().into_owned();
    let gh_env = h.write_bin(
        "gh-env",
        format!(
            "#!/bin/sh\n/usr/bin/env > {}/gh.env\nexec {gh} \"$@\"\n",
            dumps.display()
        )
        .as_bytes(),
    );
    h.set("HORCH_GH_BIN", gh_env.to_string_lossy());
    let cli = h.write_bin(
        "complexity-env",
        format!(
            "#!/bin/sh\n/usr/bin/env > {}/complexity.env\necho '{{\"files\":[],\"violations\":[]}}'\n",
            dumps.display()
        )
        .as_bytes(),
    );
    h.set("HORCH_COMPLEXITY_CLI", cli.to_string_lossy());
    let check = format!("/usr/bin/env > {}/check.env", dumps.display());
    let (w, _) = claimed_with_commit(&h, "check,complexity-sweep,land,pr", Some(&check), "a.ts");
    h.set("GIT_DIR", other.join(".git").to_string_lossy());
    h.set("GIT_WORK_TREE", other.to_string_lossy());
    let (code, r) = finish_json(&h, &w);
    assert_eq!(code, Some(0), "{r}");
    let project = h.project.clone();
    assert_eq!(git_in(&h, &project, &["show", "fleet/alpha:a.ts"]), "a.ts");
    let branches = git_in(&h, &other, &["branch", "--list"]);
    assert_eq!(
        branches.trim(),
        "* main",
        "the other repository is untouched"
    );
    for name in ["check", "complexity", "gh"] {
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

#[test]
fn finish_finds_the_code_sweep_skill_in_the_skill_store() {
    let mut h = finish_world("tl-sweep-store");
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
    let (w, _) = sweep_world(&h);
    let (code, r) = finish_json(&h, &w);
    assert_eq!(code, Some(1));
    assert_eq!(
        step_of(&r, "code-sweep").0,
        "running",
        "reviews are queued: {r}"
    );
    let ticket = std::fs::read_to_string(
        json_of(&h, &["ledger", "status", "--json"])["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["task"]["kind"] == "review")
            .unwrap()["task"]["task_file"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert!(
        ticket.contains("You are bug-hunter."),
        "the store prompt: {ticket}"
    );
}
