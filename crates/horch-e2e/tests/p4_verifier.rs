//! The e2e tests of the verifier of Phase 4 (`docs/specs/fleetd/design.md`,
//! FM-9 (the part test of Phase 4), RR-3, SR-4;
//! `docs/tickets/fleetd-p4/51-p4-verifier-e2e.md`).
//!
//! Each test queues 1 task in a temp ledger and starts a real `horch fleetd`.
//! Fleetd dispatches the task by push: it binds the attempt `A1`, makes the
//! worktree with its `worktree_add` job, writes `task.yaml` and
//! `inbox/START.md`, and launches the fake agent `claude` in a pane of the
//! fake herdr. The fake commits in the worktree and runs `horch result`. Only
//! the verifier of fleetd moves the task, so each test reads the ledger, the
//! file `verdict-<n>.json` and the files of the check, and never a time.
//!
//! Each process runs with temporary `HORCH_DATA_DIR`, `HORCH_STATE_DIR`,
//! `HORCH_LEDGER_DB` and `FLEET_ROOT`. The load, the limits and the clock of
//! this machine are not read: the harness gives the machine facts through
//! `HORCH_MACHINE_FILE`, and the test `fleet.toml` gives the pass period.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Output;

use horch_core::fleetd::task_execution::adapters::task_files::attempt_dir;
use horch_core::taskledger::model::{Actor, VerifyProfile, WorkKind, WorkerKind};
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::{schema, Store};
use horch_e2e::harness::{fixtures, wait_for_or, Harness};
use rusqlite::Connection;
use serde_json::Value;

/// The persona of the worker: a Claude persona, so its check runs in the
/// platform sandbox and the fake agent is `claude`.
const PERSONA: &str = "opus";

/// The 1 file that the fake agent commits, `fake-<fake>.txt` (`agent.rs`).
const FAKE_FILE: &str = "fake-claude.txt";

/// Link `bwrap` of this host into the `bin/` of `h`, so that fleetd and its
/// jobs find the Linux sandbox on their sealed `PATH`. macOS needs no link:
/// its sandbox is `/usr/bin/sandbox-exec`.
fn link_bwrap(h: &Harness) {
    let found = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join("bwrap"))
            .find(|p| p.is_file())
    });
    if let Some(bwrap) = found {
        std::os::unix::fs::symlink(bwrap, h.bin.join("bwrap")).unwrap();
    }
}

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// What a test gives a task: its check, its kind, its owned files, and the
/// file that the agent commits before the fake agent runs.
struct Spec<'a> {
    check: &'a str,
    kind: WorkKind,
    own: &'a [&'a str],
    /// A file `(path, body)` that the agent writes and commits in its
    /// worktree, as a docs author does. The fake agent commits only
    /// `fake-claude.txt`.
    writes: Option<(&'a str, &'a str)>,
}

/// 1 test: a harness with its own ledger, fleet root and fleet.toml, a run
/// with its orchestrator in the pane `w1:p1`, and 1 queued task. The fleetd
/// stops before the harness removes its folder.
struct World {
    h: Harness,
    task_id: String,
}

impl World {
    fn new(name: &str, spec: &Spec) -> World {
        let mut h = Harness::new(name).with_git();
        link_bwrap(&h);
        let db = h.root.join("ledger.sqlite3");
        h.set("HORCH_LEDGER_DB", db.to_string_lossy());
        h.set("HORCH_NO_WAKE", "1");
        h.set("HORCH_NO_WATCHDOG", "1");
        h.set(
            "HORCH_QUOTA_FILE",
            fixtures().join("quota/all-ok.json").to_string_lossy(),
        );
        h.set("HORCH_FAKE_SCENARIO", "exec,commit,result");
        // A pass each second. The load of the build host does not hold
        // dispatch in this test.
        let fleet = h.fleet_root();
        std::fs::create_dir_all(&fleet).unwrap();
        std::fs::write(
            fleet.join("fleet.toml"),
            "health_max_load_per_cpu = 1000000.0\npass_period_s = 1\n",
        )
        .unwrap();
        // The project declares its gates, so no hold of a missing fast gate
        // stops the dispatch.
        std::fs::write(
            h.project.join("fleet.toml"),
            "[gates]\nfast = \"true\"\nfull = \"true\"\n",
        )
        .unwrap();
        World::git(&h, &h.project, &["add", "fleet.toml"]);
        World::git(
            &h,
            &h.project,
            &["commit", "--quiet", "--no-gpg-sign", "-m", "gates"],
        );
        if let Some((path, body)) = spec.writes {
            World::docs_author(&mut h, path, body);
        }
        // The workspace of the fleet has the label of the project folder, so
        // fleetd launches an attempt with no recorded pane in it.
        let workspace = h.fleet_workspace();
        assert_eq!(workspace, "w1");
        h.set("HORCH_WORKSPACE_ID", workspace);
        let task_id = World::queue(&h, spec);
        World { h, task_id }
    }

    /// Name a `claude` that writes and commits `body` as the file `path` in
    /// the worktree of its attempt, then runs the fake agent. It acts only
    /// in a pane with a `task.yaml`; each other run goes to the fake agent
    /// at once. The sealed `PATH` holds no tool, so the script uses only the
    /// builtins of `sh` and `$HORCH_GIT_BIN`.
    fn docs_author(h: &mut Harness, path: &str, body: &str) {
        let fake = h.bin.join("claude");
        let script = h.root.join("docs-author.sh");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\n\
                 if [ -n \"${{HORCH_TASK_YAML:-}}\" ] && [ -f \"$HORCH_TASK_YAML\" ]; then\n\
                 while IFS= read -r line; do\n\
                 case \"$line\" in worktree:*) tree=${{line#worktree: }};; esac\n\
                 done < \"$HORCH_TASK_YAML\"\n\
                 tree=${{tree#\\\"}}; tree=${{tree%\\\"}}\n\
                 cd \"$tree\" || exit 1\n\
                 printf '%s\\n' '{body}' > '{path}' || exit 1\n\
                 \"$HORCH_GIT_BIN\" add -- '{path}' || exit 1\n\
                 \"$HORCH_GIT_BIN\" -c core.hooksPath=/dev/null -c commit.gpgsign=false \
                 -c user.name=author -c user.email=author@horch.invalid \
                 commit -q -m 'the work' -- '{path}' || exit 1\n\
                 fi\n\
                 exec '{fake}' \"$@\"\n",
                fake = fake.display()
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        h.set("HORCH_CLAUDE_BIN", script.to_string_lossy());
    }

    fn git(h: &Harness, dir: &Path, args: &[&str]) -> String {
        let mut c = h.git_cmd(args);
        c.current_dir(dir);
        let o = c.output().unwrap();
        assert!(o.status.success(), "git {args:?}: {}", text(&o));
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    }

    /// Write what `horch idea` and `horch ledger add` write: the run (the
    /// verification profile `minimal`: the check, then land, so no review
    /// step runs), its idea, its orchestrator in the root pane `w1:p1`, and
    /// the queued task. Dispatch binds the task, which is not done here.
    /// Gives the task id.
    fn queue(h: &Harness, spec: &Spec) -> String {
        let dir = h.project.join("tickets");
        std::fs::create_dir_all(&dir).unwrap();
        let ticket = dir.join("t0.md");
        std::fs::write(&ticket, "# t0\n").unwrap();
        // The run branch exists before the run, as `horch idea` makes it.
        World::git(h, &h.project, &["branch", "fleet/r1", "main"]);
        let store = Store::open(&h.root.join("ledger.sqlite3")).unwrap();
        let conn = store.conn();
        let run = runs::create(
            conn,
            &h.project,
            &NewRun {
                slug: "r1".into(),
                workspace_id: Some("w1".into()),
                base_branch: Some("main".into()),
                run_branch: Some("fleet/r1".into()),
                finish_steps: VerifyProfile::Minimal.steps(),
                ..Default::default()
            },
        )
        .unwrap();
        // FR-12: a task needs an idea in flight of the feature of its run.
        schema::run_idea(conn, &run.run_id, &horch_core::clock::now_stamp(), true)
            .unwrap()
            .expect("the idea of r1");
        let orchestrator = workers::insert(
            conn,
            &NewWorker {
                run_id: run.run_id.clone(),
                kind: Some(WorkerKind::Orchestrator),
                persona: "orchestrator".into(),
                workspace_id: Some("w1".into()),
                pane_id: Some("w1:p1".into()),
                ..Default::default()
            },
        )
        .unwrap();
        runs::set_orchestrator(conn, &run.run_id, &orchestrator).unwrap();
        let mut new = NewTask::work("t0", &ticket.to_string_lossy(), PERSONA);
        new.ticket_snapshot = Some("# t0\n".into());
        new.land_check = Some(spec.check.into());
        new.work_kind = Some(spec.kind);
        new.own = spec.own.iter().map(|s| s.to_string()).collect();
        tasks::add(conn, &run, &new, Actor::Orchestrator)
            .unwrap()
            .task_id
    }

    fn conn(&self) -> Connection {
        let conn = Connection::open(self.h.root.join("ledger.sqlite3")).unwrap();
        conn.busy_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        conn
    }

    fn attempt_folder(&self) -> PathBuf {
        attempt_dir(&self.h.fleet_root(), &self.task_id, "A1")
    }

    /// Start `horch fleetd` on the ledger. It dispatches the task by push.
    /// The harness stops it.
    fn start_fleetd(&self) {
        self.h.serve_fleetd();
    }

    /// Wait until fleetd wrote `task.yaml` of attempt `A1`, then give the
    /// path of the worktree that it names.
    fn worktree(&self) -> PathBuf {
        let yaml = self.attempt_folder().join("task.yaml");
        let found = wait_for_or(
            "fleetd to write the task.yaml of attempt A1",
            || std::fs::read_to_string(&yaml).ok(),
            || self.evidence(),
        );
        let path = found
            .lines()
            .find_map(|l| l.trim().strip_prefix("worktree:"))
            .map(|v| v.trim().trim_matches('"').to_string())
            .unwrap_or_else(|| panic!("no worktree in {found}"));
        PathBuf::from(path)
    }

    fn task_status(&self) -> String {
        self.conn()
            .query_row(
                "SELECT status FROM tasks WHERE task_id = ?1",
                [&self.task_id],
                |r| r.get(0),
            )
            .unwrap()
    }

    /// The state and the outcome of attempt `A1`, when it exists.
    fn attempt(&self) -> Option<(String, Option<String>)> {
        self.conn()
            .query_row(
                "SELECT state, outcome FROM attempts WHERE id = ?1",
                [format!("{}/A1", self.task_id)],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok()
    }

    /// Each status that the task had, in order, with the text of its event.
    fn moves(&self) -> Vec<(String, String)> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(
                "SELECT to_state, COALESCE(cause, '') FROM events
                 WHERE entity = 'task' AND entity_id = ?1 AND to_state IS NOT NULL
                 ORDER BY event_id",
            )
            .unwrap();
        let rows = stmt
            .query_map([&self.task_id], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        rows
    }

    /// The path of `verdict-<n>.json` of the task.
    fn verdict_path(&self, n: u32) -> PathBuf {
        self.h
            .fleet_root()
            .join("tasks")
            .join(&self.task_id)
            .join(format!("verdict-{n}.json"))
    }

    /// Wait for `verdict-<n>.json` and read it.
    fn verdict(&self, n: u32) -> Value {
        let path = self.verdict_path(n);
        wait_for_or(
            &format!("verdict-{n}.json of the task"),
            || std::fs::read(&path).ok(),
            || self.evidence(),
        );
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap()
    }

    /// Whether a `task_check` job ended: it is neither queued nor running.
    fn check_job_ended(&self) -> bool {
        self.conn()
            .query_row(
                "SELECT count(*) FROM jobs
                 WHERE kind = 'task_check' AND state NOT IN ('queued', 'running')",
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap()
            > 0
    }

    /// The facts of the ledger and of fleetd, for a failure message.
    fn evidence(&self) -> String {
        let conn = self.conn();
        let jobs: Vec<String> = conn
            .prepare("SELECT kind || ' ' || state || ' ' || COALESCE(result_json, '') FROM jobs")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        format!(
            "task status: {}\nattempt: {:?}\nmoves: {:?}\njobs: {jobs:?}\nclaude calls: {:?}\nfleetd stderr: {}",
            self.task_status(),
            self.attempt(),
            self.moves(),
            self.h.calls_of("claude"),
            std::fs::read_to_string(self.h.root.join("fleetd.stderr")).unwrap_or_default()
        )
    }
}

/// The task is not done, and no event moved it to `done`.
fn assert_not_done(w: &World) {
    assert_ne!(w.task_status(), "done", "{}", w.evidence());
    assert!(
        !w.moves().iter().any(|(to, _)| to == "done"),
        "{}",
        w.evidence()
    );
}

#[test]
fn passing_code_task_reaches_done_only_through_the_verifier() {
    let w = World::new(
        "p4v-pass",
        &Spec {
            check: "test -s fake-claude.txt",
            kind: WorkKind::Feature,
            own: &[],
            writes: None,
        },
    );
    w.start_fleetd();
    // The fake agent said "done". Only the verifier moves the task.
    let verdict = w.verdict(1);
    assert_eq!(verdict["decision"], "passed", "{verdict}");
    wait_for_or(
        "the task to be done",
        || (w.task_status() == "done").then_some(()),
        || w.evidence(),
    );
    // The only move to `done` names the verdict that judged the tip.
    let to_done: Vec<_> = w
        .moves()
        .into_iter()
        .filter(|(to, _)| to == "done")
        .collect();
    assert_eq!(to_done.len(), 1, "{}", w.evidence());
    assert!(
        to_done[0].1.contains("the verifier: verdict-1.json judged"),
        "{}",
        w.evidence()
    );
    assert!(w.worktree().join(FAKE_FILE).is_file());
}

#[test]
fn push_dispatched_task_launches_writes_task_files_and_reaches_done_through_verifier() {
    let w = World::new(
        "p4v-chain",
        &Spec {
            check: "test -s fake-claude.txt",
            kind: WorkKind::Feature,
            own: &[],
            writes: None,
        },
    );
    // The test binds no attempt and writes no file of the attempt: fleetd
    // does each step of the chain.
    assert!(w.attempt().is_none(), "{}", w.evidence());
    w.start_fleetd();
    let verdict = w.verdict(1);
    assert_eq!(verdict["decision"], "passed", "{verdict}");
    wait_for_or(
        "the task to be done",
        || (w.task_status() == "done").then_some(()),
        || w.evidence(),
    );

    // Dispatch bound the task to attempt A1 by push.
    let moves = w.moves();
    assert!(
        moves
            .iter()
            .any(|(to, why)| to == "leased" && why.contains("dispatch binds T-1 to attempt")),
        "{}",
        w.evidence()
    );
    let (state, outcome) = w.attempt().expect("attempt A1");
    assert_eq!(state, "ended", "{}", w.evidence());
    assert_eq!(outcome.as_deref(), Some("verified"), "{}", w.evidence());

    // The `worktree_add` job made the worktree on the branch of the task.
    let jobs: Vec<String> = w
        .conn()
        .prepare("SELECT state FROM jobs WHERE kind = 'worktree_add'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(jobs, ["passed"], "{}", w.evidence());
    let tree = w.worktree();
    assert!(tree.join(".git").exists(), "{}", w.evidence());
    assert!(tree.join(FAKE_FILE).is_file(), "{}", w.evidence());

    // Fleetd wrote the 2 files of the attempt before the launch.
    let dir = w.attempt_folder();
    let yaml = std::fs::read_to_string(dir.join("task.yaml")).unwrap();
    assert!(yaml.lines().any(|l| l.trim() == "attempt: A1"), "{yaml}");
    assert!(yaml.lines().any(|l| l.trim() == "kind: feature"), "{yaml}");
    let start = std::fs::read_to_string(dir.join("inbox").join("START.md")).unwrap();
    assert!(start.starts_with("# t0\n"), "{start}");
    assert!(start.contains("horch result"), "{start}");

    // The launcher started the fake agent, which committed and wrote
    // `outbox/result.json` through `horch result`.
    let result: Value =
        serde_json::from_slice(&std::fs::read(dir.join("outbox").join("result.json")).unwrap())
            .unwrap();
    assert_eq!(result["status"], "completed", "{result}");
    let agent =
        w.h.calls_of("claude")
            .into_iter()
            .find(|c| c["commit"]["head"].is_string())
            .unwrap_or_else(|| panic!("no launch of the fake agent\n{}", w.evidence()));
    assert_eq!(agent["result"]["exit"], 0, "{agent}");

    // The verifier judged the tip: its check ran in the worker sandbox and
    // `verdict-1.json` names the job. The only move to `done` names it.
    assert!(verdict["job"].is_number(), "{verdict}");
    let to_done: Vec<_> = moves.into_iter().filter(|(to, _)| to == "done").collect();
    assert_eq!(to_done.len(), 1, "{}", w.evidence());
    assert!(
        to_done[0].1.contains("the verifier: verdict-1.json judged"),
        "{}",
        w.evidence()
    );
}

#[test]
fn fm9_code_task_with_a_failing_check_stays_not_done() {
    let w = World::new(
        "p4v-fm9-code",
        &Spec {
            check: "test -f the-file-that-the-worker-never-wrote",
            kind: WorkKind::Feature,
            own: &[],
            writes: None,
        },
    );
    w.start_fleetd();
    let verdict = w.verdict(1);
    assert_eq!(verdict["decision"], "failed", "{verdict}");
    wait_for_or(
        "attempt A1 to end rejected",
        || (w.attempt().and_then(|a| a.1).as_deref() == Some("rejected")).then_some(()),
        || w.evidence(),
    );
    assert_not_done(&w);
}

#[test]
fn fm9_docs_task_without_its_section_stays_not_done() {
    let w = World::new(
        "p4v-fm9-docs",
        &Spec {
            // The check passes, so only the structural check can fail.
            check: "true",
            kind: WorkKind::Docs,
            own: &["guide.md"],
            // The guide has text and no section.
            writes: Some(("guide.md", "The guide has text and no section.")),
        },
    );
    w.start_fleetd();
    wait_for_or(
        "attempt A1 to end rejected",
        || (w.attempt().and_then(|a| a.1).as_deref() == Some("rejected")).then_some(()),
        || w.evidence(),
    );
    // The check passed (verdict 1), so the structural check gave the
    // failure (verdict 2).
    let checked = w.verdict(1);
    assert_eq!(checked["decision"], "passed", "{checked}");
    let verdict = w.verdict(2);
    assert_eq!(verdict["decision"], "failed", "{verdict}");
    assert!(
        verdict["reason"].as_str().unwrap().contains("guide.md"),
        "{verdict}"
    );
    assert_not_done(&w);
}

#[test]
fn sr4_check_that_writes_outside_its_paths_is_refused() {
    // A folder outside each writable path of a check: not in the worktree,
    // not in the fleet root, not in the temp folder of the harness.
    let outside = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let escaped = outside.path().join("escaped.txt");
    let check = format!(
        "echo inside > written-inside.txt && echo outside > {}",
        escaped.display()
    );
    let w = World::new(
        "p4v-sr4",
        &Spec {
            check: &check,
            kind: WorkKind::Feature,
            own: &[],
            writes: None,
        },
    );
    w.start_fleetd();
    // The sandbox of the check on the `PATH` that fleetd and its jobs get.
    if let Some(gap) = horch_platform::sandbox::host_problem(Some(w.h.bin.as_os_str())) {
        // Orchestrator answer Q-A, rule 3: a host with no sandbox (Linux
        // without `bwrap`) gives NotSupported, and the check does not run at
        // all. SR-4 holds by refusal, not by a denied write.
        wait_for_or(
            "the task_check job to end with no sandbox",
            || w.check_job_ended().then_some(()),
            || w.evidence(),
        );
        assert!(
            !escaped.exists() && !w.worktree().join("written-inside.txt").exists(),
            "SR-4 on a host with NotSupported ({gap:?}): the check ran with no sandbox\n{}",
            w.evidence()
        );
        assert_not_done(&w);
        return;
    }
    let verdict = w.verdict(1);
    assert_eq!(verdict["decision"], "failed", "{verdict}");
    // The check ran in its sandbox: its write inside the worktree stands,
    // and its write outside is refused.
    assert!(
        w.worktree().join("written-inside.txt").is_file(),
        "the check did not run, so its refusal proves nothing\n{verdict}\n{}",
        w.evidence()
    );
    assert!(
        !escaped.exists(),
        "SR-4: the check wrote {} outside its paths\n{}",
        escaped.display(),
        w.evidence()
    );
    assert_not_done(&w);
}

/// The address of this host that is not loopback, found by the route to a
/// documentation address (no packet is sent), or that documentation address
/// when the host has no route.
fn off_host_ip() -> std::net::IpAddr {
    let doc: std::net::IpAddr = [192, 0, 2, 1].into();
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| s.connect((doc, 9)).and_then(|()| s.local_addr()))
        .map(|a| a.ip())
        .ok()
        .filter(|ip| !ip.is_loopback() && !ip.is_unspecified())
        .unwrap_or(doc)
}

/// Serve 1 HTTP reply on loopback to each connection until `stop` is set.
/// Gives the port and the request texts that the server read.
fn loopback_server(
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> (u16, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    use std::io::{Read as _, Write as _};
    use std::sync::atomic::Ordering;
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = std::sync::Arc::clone(&seen);
    std::thread::spawn(move || {
        while !stop.load(Ordering::SeqCst) {
            let Ok((mut stream, _)) = listener.accept() else {
                std::thread::sleep(std::time::Duration::from_millis(20));
                continue;
            };
            let _ = stream.set_nonblocking(false);
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
            let mut buf = [0u8; 1024];
            let n = stream.read(&mut buf).unwrap_or(0);
            log.lock()
                .unwrap()
                .push(String::from_utf8_lossy(&buf[..n]).into_owned());
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        }
    });
    (port, seen)
}

#[test]
fn sr3_check_that_connects_off_host_is_refused() {
    use std::sync::atomic::{AtomicBool, Ordering};
    // A listener on the address of this host that is not loopback: a check
    // with off-host network would reach it. Without a route the address is
    // a documentation address that nobody serves.
    let ip = off_host_ip();
    let off_host = std::net::TcpListener::bind((ip, 0))
        .or_else(|_| std::net::TcpListener::bind(("127.0.0.1", 0)))
        .unwrap();
    off_host.set_nonblocking(true).unwrap();
    let off_url = format!("http://{ip}:{}/", off_host.local_addr().unwrap().port());
    let stop = std::sync::Arc::new(AtomicBool::new(false));
    let (port, loopback_requests) = loopback_server(std::sync::Arc::clone(&stop));
    // The sealed `PATH` of the check holds no `curl`.
    let curl = ["/usr/bin/curl", "/bin/curl"]
        .into_iter()
        .find(|p| Path::new(p).is_file())
        .expect("curl on this host");
    let get = |url: &str| format!("{curl} --silent --show-error --max-time 5 --data hello {url}");
    // The check connects to loopback first and leaves a file when it
    // reached it. Then it connects off host. The sandbox allows the first
    // and refuses the second, so the check fails.
    let check = format!(
        "{} && echo reached > loopback-ok.txt; {}",
        get(&format!("http://127.0.0.1:{port}/")),
        get(&off_url)
    );
    let w = World::new(
        "p4v-sr3",
        &Spec {
            check: &check,
            kind: WorkKind::Feature,
            own: &[],
            writes: None,
        },
    );
    w.start_fleetd();
    // The sandbox of the check on the `PATH` that fleetd and its jobs get.
    if let Some(gap) = horch_platform::sandbox::host_problem(Some(w.h.bin.as_os_str())) {
        // A host with no sandbox gives NotSupported, and the check does not
        // run at all. SR-3 holds by refusal, not by a denied connect.
        wait_for_or(
            "the task_check job to end with no sandbox",
            || w.check_job_ended().then_some(()),
            || w.evidence(),
        );
        stop.store(true, Ordering::SeqCst);
        assert!(
            off_host.accept().is_err() && loopback_requests.lock().unwrap().is_empty(),
            "SR-3 on a host with NotSupported ({gap:?}): the check ran with no sandbox\n{}",
            w.evidence()
        );
        assert_not_done(&w);
        return;
    }
    let verdict = w.verdict(1);
    stop.store(true, Ordering::SeqCst);
    assert_eq!(verdict["decision"], "failed", "{verdict}");
    // The check ran in its sandbox: its loopback request stands, and its
    // off-host request is refused.
    assert!(
        w.worktree().join("loopback-ok.txt").is_file(),
        "the check did not reach loopback, so its refusal proves nothing\n{verdict}\n{}",
        w.evidence()
    );
    assert!(
        loopback_requests
            .lock()
            .unwrap()
            .iter()
            .any(|r| r.contains("hello")),
        "{}",
        w.evidence()
    );
    assert!(
        off_host.accept().is_err(),
        "SR-3: the check reached the listener at {off_url}\n{}",
        w.evidence()
    );
    assert_not_done(&w);
}
