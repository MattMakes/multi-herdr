#![cfg(unix)]

use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};

use horch_core::execution::records::{NewRecord, SessionLedger};
use horch_core::procid;
use horch_core::taskledger::Store;
use horch_e2e::harness::{repo_root, wait_for_or, Harness};
use horch_e2e::ledger_seed::{seed_run, seed_task, seed_worker};

const RECORD: &str = "r-ctx";
const ROLE: &str = "opus-1";
const SESSION: &str = "11111111-1111-4111-8111-111111111111";
const LINES: usize = 3;

const WORKER: &str = r#"read go
for n in 1 2 3; do
  "$1" ledger progress "step $n" >"$2/progress-$n.out" 2>&1
  echo $? >"$2/progress-$n.code"
done
read end
"#;

struct Worker {
    child: Child,
    stdin: Option<ChildStdin>,
    started: u64,
    out: PathBuf,
}

impl Worker {
    fn start(h: &Harness, worker_id: &str) -> Worker {
        let out = h.root.join("worker-out");
        std::fs::create_dir_all(&out).unwrap();
        let mut cmd = Command::new("/bin/sh");
        h.seal(&mut cmd);
        cmd.env("HORCH_WORKER_ID", worker_id)
            .env("HORCH_RECORD_ID", RECORD)
            .env("HORCH_ROLE", ROLE)
            .arg("-c")
            .arg(WORKER)
            .arg("worker")
            .arg(Harness::horch_bin())
            .arg(&out)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = cmd.spawn().unwrap();
        let stdin = child.stdin.take();
        let started = procid::start_time(child.id()).expect("the start time of sh");
        Worker {
            child,
            stdin,
            started,
            out,
        }
    }

    fn run_progress(&mut self) -> Vec<String> {
        let stdin = self.stdin.as_mut().expect("the stdin of the worker");
        stdin.write_all(b"go\n").unwrap();
        stdin.flush().unwrap();
        (1..=LINES)
            .map(|n| {
                let read = |ext: &str| {
                    std::fs::read_to_string(self.out.join(format!("progress-{n}.{ext}")))
                };
                let code = wait_for_or(
                    &format!("progress line {n}"),
                    || read("code").ok()?.trim().parse::<i32>().ok(),
                    || read("out").unwrap_or_default(),
                );
                let output = read("out").unwrap_or_default();
                assert_eq!(code, 0, "horch ledger progress {n}: {output}");
                output
            })
            .collect()
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn world(name: &str) -> Harness {
    let mut h = Harness::new(name).with_git();
    h.set(
        "HORCH_LEDGER_DB",
        h.state.join("ledger.sqlite3").to_string_lossy(),
    );
    h.set("HORCH_NO_WAKE", "1").set("HORCH_NO_WATCHDOG", "1");
    h.set("HORCH_WORKSPACE_ID", "w1");
    assert_eq!(h.fleet_workspace(), "w1");
    let fixture = repo_root().join("crates/horch-core/tests/fixtures/context/home");
    h.copy_in(&fixture, &h.home);
    h.serve_fleetd();
    h
}

fn seed_record(h: &Harness) {
    let ledger = SessionLedger::for_project(&h.state, &h.project.to_string_lossy());
    ledger
        .add(NewRecord {
            record_id: RECORD,
            agent: "claude",
            tier: "opus",
            model: "opus",
            role: ROLE,
            session_id: Some(SESSION),
            task: "work",
            phase: None,
        })
        .unwrap();
    ledger
        .store()
        .update(|rows| {
            let r = rows.iter_mut().find(|r| r.record_id == RECORD).unwrap();
            r.workspace_id = Some("w1".into());
            Ok(())
        })
        .unwrap();
}

fn seed_attempt(h: &Harness, task: &str, worker_id: &str, w: &Worker) {
    let mut store = Store::open(&h.ledger_db()).unwrap();
    store
        .immediate(|tx| {
            let now = horch_core::taskledger::util::now();
            tx.execute(
                "UPDATE workers SET status = 'working', task_id = ?2, attempt = 1,
                    agent_pid = ?3, agent_started = ?4, legacy_record_id = ?5,
                    role = ?6, started_at = ?7, claimed_at = ?7, last_checkin_at = ?7
                 WHERE worker_id = ?1",
                rusqlite::params![worker_id, task, w.child.id(), w.started, RECORD, ROLE, &now],
            )?;
            tx.execute(
                "UPDATE tasks SET status = 'running', attempts = 1, current_worker_id = ?2
                 WHERE task_id = ?1",
                [task, worker_id],
            )?;
            let attempt = format!("{task}/A1");
            tx.execute(
                "INSERT INTO attempts (id, task_id, worker_id, pid, pid_start, state, started_at,
                        active_at, via)
                    VALUES (?1, ?2, ?3, ?4, ?5, 'live', ?6, 0, 'claim')",
                rusqlite::params![&attempt, task, worker_id, w.child.id(), w.started, &now],
            )?;
            tx.execute(
                "INSERT INTO leases (task_id, attempt_id, expires_at, renewed_at, renew_basis)
                    VALUES (?1, ?2, ?3, ?4, 'claim')",
                rusqlite::params![task, &attempt, i64::MAX / 2, &now],
            )?;
            Ok(())
        })
        .unwrap();
    use std::os::unix::fs::PermissionsExt;
    let outbox = attempt_dir(h, task).join("outbox");
    std::fs::create_dir_all(&outbox).unwrap();
    std::fs::set_permissions(&outbox, std::fs::Permissions::from_mode(0o700)).unwrap();
}

fn attempt_dir(h: &Harness, task: &str) -> PathBuf {
    h.fleet_root()
        .join("tasks")
        .join(task)
        .join("attempts")
        .join("A1")
}

fn warned_events(h: &Harness) -> usize {
    SessionLedger::for_project(&h.state, &h.project.to_string_lossy())
        .get(RECORD)
        .unwrap()
        .history
        .iter()
        .filter(|e| e.event == "context-warned")
        .count()
}

fn ingested(h: &Harness, task: &str) -> Option<(u64, u64)> {
    let conn = rusqlite::Connection::open(h.ledger_db()).unwrap();
    let path: String = conn
        .query_row(
            "SELECT path FROM ingested WHERE path LIKE ?1",
            [format!("%{task}/attempts/A1/outbox/progress.jsonl")],
            |r| r.get(0),
        )
        .ok()?;
    let offset: i64 = conn
        .query_row(
            "SELECT log_offset FROM ingested WHERE path = ?1",
            [&path],
            |r| r.get(0),
        )
        .ok()?;
    let file = attempt_dir(h, task).join("outbox/progress.jsonl");
    let size = std::fs::metadata(file).ok()?.len();
    Some((size, u64::try_from(offset).ok()?))
}

#[test]
fn a_worker_in_a_live_attempt_gets_the_context_warning_1_time() {
    let h = world("ctx-warn-1");
    let run = seed_run(&h, "ctx", None, "w1:p1");
    let ticket = h.project.join("ctx.md");
    std::fs::write(&ticket, "# Work\n\n**Check:** `true`\n").unwrap();
    let task = seed_task(&h, &run, &ticket, "opus").task_id;
    let worker_id = seed_worker(&h, &run, "opus");
    seed_record(&h);
    let mut w = Worker::start(&h, &worker_id);
    seed_attempt(&h, &task, &worker_id, &w);

    let outputs = w.run_progress();
    let all = outputs.join("\n----\n");
    let warnings: usize = outputs
        .iter()
        .map(|o| o.matches("Context warning").count())
        .sum();
    assert_eq!(warnings, 1, "the warning shows 1 time:\n{all}");
    assert!(
        outputs[0].contains("Context warning"),
        "the first progress line warns:\n{all}"
    );

    wait_for_or(
        "fleetd to ingest each line of progress.jsonl",
        || ingested(&h, &task).filter(|(size, offset)| size == offset),
        || format!("{:?}", ingested(&h, &task)),
    );
    assert_eq!(warned_events(&h), 1, "the event is recorded 1 time:\n{all}");
}
