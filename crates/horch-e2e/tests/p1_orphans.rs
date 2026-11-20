//! FM-22 through `horch fleetd` (`docs/specs/fleetd/design.md`, FM-22 and
//! "Ledger", I5): an orphan process or pane after a reap is stopped by the
//! process-group kill and the exact pane close, within 1 pass of 5 s plus
//! `stop_grace_s`. fleetd stops a group only while the recorded PID leads
//! it with the recorded start time, and closes a pane only when the process
//! in it is a recorded process of the row. A process that no row names is
//! not touched (`docs/tickets/fleetd-p1/MAP.md`, Q12).
//!
//! The fake herdr answers `pane process-info` with the field names of real
//! herdr 0.9.4. Its real answer for a pane that runs `horch worker`:
//!
//! ```json
//! {"id":"cli:pane:process_info","result":{"process_info":{
//!   "pane_id":"wG1:p9S","shell_pid":50111,"foreground_process_group_id":50111,
//!   "foreground_processes":[{"pid":50111,"name":"horch","argv0":"horch",
//!   "cwd":"/Users/MASCOTT/projects/mh/multi-herdr"}]}}}
//! ```
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use horch_core::taskledger::{
    model::WorkerStatus,
    runs::{self, NewRun},
    workers::{self, NewWorker},
    Store,
};
use horch_e2e::fleetd::Fleetds;
use horch_e2e::harness::{wait_for, wait_for_or, Harness};
use serde_json::Value;

/// The grace of a polite stop, in s, in the `fleet.toml` of each test.
const GRACE_S: u64 = 1;

/// 1 pass of 5 s plus the grace, plus 4 s for the start of fleetd and
/// a slow machine.
const STOPPED_IN: Duration = Duration::from_secs(5 + GRACE_S + 4);

/// A process group that the test started, by the pid and start time of
/// its leader. Killed on drop while it is still that group, so that a
/// stand-in worker that ignores SIGTERM leaks no process.
struct Group((u32, u64));

impl Drop for Group {
    fn drop(&mut self) {
        let (pgid, started) = self.0;
        if horch_core::procid::is_same_group(pgid, started) {
            // SAFETY: plain syscall to the group that this test started.
            unsafe { libc::kill(-(pgid as i32), libc::SIGKILL) };
        }
    }
}

/// A harness whose fake herdr runs pane commands, a ledger with 1 run in
/// workspace `w1`, and a `fleet.toml` with a grace of [`GRACE_S`].
fn fleet(name: &str) -> (Harness, Store, String) {
    let mut h = Harness::new(name);
    h.set("HORCH_WORKSPACE_ID", "w1");
    h.set("HORCH_NO_WATCHDOG", "1");
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_FAKE_SCENARIO", "exec");
    remove_agent_clis_so_fleetd_queues_no_conformance_job(&h);
    let db = h.root.join("ledger.sqlite3");
    h.set("HORCH_LEDGER_DB", db.to_string_lossy());
    let root = h.root.join("fleet");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("fleet.toml"),
        format!("stop_grace_s = {GRACE_S}\n"),
    )
    .unwrap();
    // The first workspace of the fake herdr is `w1`, with the pane `w1:p1`.
    herdr(&h, &["workspace", "create", "--label", "orphans"]);
    let mut store = Store::open(&db).unwrap();
    let run = store
        .immediate(|tx| {
            runs::create(
                tx,
                &h.project,
                &NewRun {
                    slug: "orphans".into(),
                    workspace_id: Some("w1".into()),
                    ..Default::default()
                },
            )
        })
        .unwrap();
    (h, store, run.run_id)
}

fn remove_agent_clis_so_fleetd_queues_no_conformance_job(h: &Harness) {
    for cli in ["claude", "codex", "agy"] {
        std::fs::remove_file(h.bin.join(cli)).unwrap();
    }
}

/// Call the fake herdr of `h`; gives its stdout.
fn herdr(h: &Harness, args: &[&str]) -> String {
    let mut cmd = std::process::Command::new(h.bin.join("herdr"));
    h.seal(&mut cmd);
    let o = cmd.args(args).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// A new pane of workspace `w1`.
fn new_pane(h: &Harness) -> String {
    let v: Value = serde_json::from_str(&herdr(h, &["pane", "split", "w1:p1"])).unwrap();
    v["result"]["pane"]["pane_id"].as_str().unwrap().to_string()
}

/// The pane ids of workspace `w1`.
fn panes(h: &Harness) -> Vec<String> {
    let v: Value = serde_json::from_str(&herdr(h, &["pane", "list", "--workspace", "w1"])).unwrap();
    v["result"]["panes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["pane_id"].as_str().unwrap().to_string())
        .collect()
}

/// The shell of a stand-in worker in its own process group: the leader
/// writes `<tag>.leader`, starts 1 child that writes `<tag>.child`, and
/// each touches `<tag>.<leader|child>.term` on SIGTERM and lives on.
fn worker_script(dir: &Path, tag: &str) -> String {
    let file = |name: &str| dir.join(format!("{tag}.{name}")).display().to_string();
    let leader_pid = file("leader");
    let child_pid = file("child");
    let leader_term = file("leader.term");
    let child_term = file("child.term");
    // Each pid file is written to a temp file and renamed, so a reader never
    // sees half a pid. The inner `sh` prints its parent: the child.
    format!(
        "trap '/usr/bin/touch {leader_term}' TERM
(
  trap '/usr/bin/touch {child_term}' TERM
  /bin/sh -c 'echo $PPID' > {child_pid}.tmp
  /bin/mv {child_pid}.tmp {child_pid}
  while :; do /bin/sleep 1; done
) &
echo $$ > {leader_pid}.tmp
/bin/mv {leader_pid}.tmp {leader_pid}
while :; do /bin/sleep 1; done
"
    )
}

/// The pid that the stand-in worker `tag` wrote to `<tag>.<who>`, with its
/// start time.
fn pid_of(dir: &Path, tag: &str, who: &str) -> (u32, u64) {
    let path = dir.join(format!("{tag}.{who}"));
    wait_for("the pid of the stand-in worker", || {
        let pid: u32 = std::fs::read_to_string(&path).ok()?.trim().parse().ok()?;
        Some((pid, horch_core::procid::start_time(pid)?))
    })
}

/// Run the stand-in worker `tag` in `pane`. Gives its group, and the pids
/// and start times of its leader and of its child.
fn run_in_pane(h: &Harness, pane: &str, tag: &str) -> (Group, (u32, u64), (u32, u64)) {
    herdr(h, &["pane", "run", pane, &worker_script(&h.root, tag)]);
    let leader = pid_of(&h.root, tag, "leader");
    (Group(leader), leader, pid_of(&h.root, tag, "child"))
}

/// Run the stand-in worker `tag` outside herdr, in a new process group.
fn run_outside(h: &Harness, tag: &str) -> (Group, (u32, u64)) {
    use std::os::unix::process::CommandExt;
    let child = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(worker_script(&h.root, tag))
        .stdin(Stdio::null())
        .process_group(0)
        .spawn()
        .unwrap();
    // Reaped by a thread, so that a killed leader is no zombie.
    let mut child = child;
    std::thread::spawn(move || child.wait());
    let leader = pid_of(&h.root, tag, "leader");
    (Group(leader), leader)
}

/// A worker of `run` that a reap ended, whose row still names the
/// supervisor `supervisor` and the pane `pane`.
fn reaped_worker(store: &mut Store, run: &str, pane: &str, supervisor: (u32, u64)) -> String {
    store
        .immediate(|tx| {
            let w = workers::insert(
                tx,
                &NewWorker {
                    run_id: run.to_string(),
                    persona: "pi-worker".into(),
                    harness: Some("pi".into()),
                    workspace_id: Some("w1".into()),
                    pane_id: Some(pane.into()),
                    ..Default::default()
                },
            )?;
            workers::supervisor_started(tx, &w, supervisor.0, Some(supervisor.1))?;
            workers::end(tx, &w, WorkerStatus::Incomplete, "reaped", None)?;
            Ok(w)
        })
        .unwrap()
}

fn alive(p: (u32, u64)) -> bool {
    horch_core::procid::alive(p.0, Some(p.1))
}

fn got_term(h: &Harness, tag: &str, who: &str) -> bool {
    h.root.join(format!("{tag}.{who}.term")).exists()
}

/// The open needs-you items of the ledger.
fn open_items(store: &Store) -> Vec<String> {
    let mut stmt = store
        .conn()
        .prepare("SELECT key FROM needs_you WHERE state = 'open' ORDER BY key")
        .unwrap();
    stmt.query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// Wait until fleetd wrote `passes` more heartbeats: `passes` more passes.
fn wait_passes(h: &Harness, passes: usize) {
    let path: PathBuf = h.root.join("fleet").join("run").join("heartbeat.json");
    let at = || -> Option<u64> {
        let v: Value = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
        v["awake"]["awake_ms"].as_u64()
    };
    let mut seen = Vec::new();
    wait_for("fleetd passes", || {
        let now = at();
        if now.is_some() && seen.last() != Some(&now) {
            seen.push(now);
        }
        (seen.len() > passes).then_some(())
    });
}

#[test]
fn fm22_orphan_process_and_pane_after_a_reap_are_stopped() {
    let (h, mut store, run) = fleet("p1-fm22");
    let (_group, leader, child) = run_in_pane(&h, "w1:p1", "w");
    reaped_worker(&mut store, &run, "w1:p1", leader);
    // A process that no row names.
    let (_stranger, stranger) = run_outside(&h, "s");
    let mut fleetds = Fleetds::new(&h);
    fleetds.start(&h, "fleetd");
    let started = Instant::now();
    // The polite stop goes to each process of the group of the leader. Each
    // process writes its mark when it handles the signal, in any order.
    wait_for("the polite stop of the child and the leader", || {
        (got_term(&h, "w", "child") && got_term(&h, "w", "leader")).then_some(())
    });
    assert!(got_term(&h, "w", "child"));
    assert!(got_term(&h, "w", "leader"));
    wait_for_or(
        "the kill of the group and the close of the pane",
        || (!alive(leader) && !alive(child) && !panes(&h).contains(&"w1:p1".into())).then_some(()),
        || {
            format!(
                "leader alive {}, child alive {}, panes {:?}\n{}",
                alive(leader),
                alive(child),
                panes(&h),
                fleetds.output("fleetd", "stderr")
            )
        },
    );
    assert!(
        started.elapsed() < STOPPED_IN,
        "the orphans were stopped {} s after fleetd started; FM-22 stops them \
         within 1 pass of 5 s plus stop_grace_s",
        started.elapsed().as_secs()
    );
    assert_eq!(open_items(&store), Vec::<String>::new());
    assert!(
        alive(stranger),
        "fleetd touched a process that no row names"
    );
    assert!(!got_term(&h, "s", "leader"));
    assert!(!got_term(&h, "s", "child"));
}

#[test]
fn fm22_a_pid_that_does_not_lead_its_group_is_stopped_alone() {
    let (h, mut store, run) = fleet("p1-fm22-member");
    let pane = new_pane(&h);
    let (_group, leader, child) = run_in_pane(&h, &pane, "m");
    // The row names the child, which does not lead its group.
    reaped_worker(&mut store, &run, &pane, child);
    let mut fleetds = Fleetds::new(&h);
    fleetds.start(&h, "fleetd");
    wait_for_or(
        "the kill of the child",
        || (!alive(child)).then_some(()),
        || fleetds.output("fleetd", "stderr"),
    );
    assert!(got_term(&h, "m", "child"));
    assert!(
        !got_term(&h, "m", "leader") && alive(leader),
        "the stop of a pid that does not lead its group reached its group"
    );
    // The process in the pane is the leader, which the row does not name:
    // no proof, so the pane is kept and the operator is asked.
    let key = format!("pane_kept:{pane}");
    wait_for_or(
        "the needs-you item of the kept pane",
        || open_items(&store).contains(&key).then_some(()),
        || {
            format!(
                "{:?}\n{}",
                open_items(&store),
                fleetds.output("fleetd", "stderr")
            )
        },
    );
    assert!(panes(&h).contains(&pane));
}

#[test]
fn fm22_another_start_time_stops_nothing() {
    let (h, mut store, run) = fleet("p1-fm22-start");
    let pane = new_pane(&h);
    let (_group, leader, child) = run_in_pane(&h, &pane, "t");
    reaped_worker(&mut store, &run, &pane, (leader.0, leader.1 + 1));
    let mut fleetds = Fleetds::new(&h);
    fleetds.start(&h, "fleetd");
    // A polite stop and its kill would come within 2 passes.
    wait_passes(&h, 3);
    assert!(alive(leader) && alive(child));
    assert!(!got_term(&h, "t", "leader") && !got_term(&h, "t", "child"));
    assert!(panes(&h).contains(&pane));
    assert_eq!(open_items(&store), Vec::<String>::new());
}

#[test]
fn fm22_a_failed_process_info_keeps_the_pane() {
    let (h, mut store, run) = fleet("p1-fm22-info");
    // A pane that runs no command: the fake herdr answers no process-info.
    let pane = new_pane(&h);
    let (_group, leader) = run_outside(&h, "i");
    reaped_worker(&mut store, &run, &pane, leader);
    let mut fleetds = Fleetds::new(&h);
    fleetds.start(&h, "fleetd");
    wait_for_or(
        "the kill of the orphan",
        || (!alive(leader)).then_some(()),
        || fleetds.output("fleetd", "stderr"),
    );
    let key = format!("pane_kept:{pane}");
    wait_for_or(
        "the needs-you item of the kept pane",
        || open_items(&store).contains(&key).then_some(()),
        || {
            format!(
                "{:?}\n{}",
                open_items(&store),
                fleetds.output("fleetd", "stderr")
            )
        },
    );
    assert!(panes(&h).contains(&pane));
}
