#![cfg(unix)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use horch_core::fleetd::fleet_runtime::adapters::scans::FolderItem;
use horch_core::fleetd::fleet_runtime::application::pass::{
    Done, NoDuties, Outside, Pass, PassReport, Ports, Settings,
};
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::health::Hold;
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::task_execution::domain::task::{AttemptRow, InPane, Recorded};
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::ports::LaunchSpec;
use horch_core::runtime::fault::Faults;
use horch_core::taskledger::model::WorkerKind;
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::{schema, util};
use rusqlite::{params, Connection, OptionalExtension};

const PROBE_DB: &str = "POLITE_PROBE_DB";
const PROBE_OUT: &str = "POLITE_PROBE_OUT";

struct World {
    dir: PathBuf,
    _temp: tempfile::TempDir,
    conn: Connection,
    run: Run,
}

fn world() -> World {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().to_path_buf();
    let mut conn = Connection::open(dir.join("ledger.sqlite3")).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    conn.execute(
        "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
         VALUES (1, 1, ?1, 0, ?2, 0, ?2)",
        params![std::process::id(), util::now()],
    )
    .unwrap();
    let new_run = NewRun {
        slug: "r1".into(),
        ..Default::default()
    };
    let run = runs::create(&conn, &dir, &new_run).unwrap();
    std::fs::create_dir_all(dir.join("fleet")).unwrap();
    World {
        dir,
        _temp: temp,
        conn,
        run,
    }
}

impl World {
    fn root(&self) -> PathBuf {
        self.dir.join("fleet")
    }

    fn db(&self) -> PathBuf {
        self.dir.join("ledger.sqlite3")
    }

    fn file(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    fn stops_dir(&self) -> PathBuf {
        self.root().join("run").join("stops")
    }

    fn old_stop_file(&self, p: Recorded) -> PathBuf {
        self.stops_dir()
            .join(format!("{}-{}.json", p.pid, p.pid_start.unwrap()))
    }

    fn settings(&self) -> Settings {
        Settings {
            fleet_root: self.root(),
            generation: Generation(1),
            config: FleetConfig::default(),
            horch: PathBuf::from("/bin/false"),
            faults: Faults::default(),
        }
    }

    fn pass<'a>(&self, fake: &'a Fake) -> Pass<'a> {
        Pass::new(
            self.settings(),
            Ports {
                outside: fake,
                duties: &NoDuties,
                needs_you: &NeedsYouList,
                host: &Launcher,
            },
        )
    }

    fn orphan(&self, p: &Proc) {
        let new = NewWorker {
            run_id: self.run.run_id.clone(),
            kind: Some(WorkerKind::Worker),
            persona: "dev".into(),
            ..Default::default()
        };
        let id = workers::insert(&self.conn, &new).unwrap();
        let r = p.recorded();
        self.conn
            .execute(
                "UPDATE workers SET status = 'done', agent_pid = ?2, agent_started = ?3
                 WHERE worker_id = ?1",
                params![id, r.pid, r.pid_start.map(|s| s as i64)],
            )
            .unwrap();
    }

    fn clock(&self) -> i64 {
        self.conn
            .query_row("SELECT fleet_clock_s FROM fleetd", [], |r| r.get(0))
            .unwrap()
    }

    fn set_clock(&self, s: i64) {
        self.conn
            .execute("UPDATE fleetd SET fleet_clock_s = ?1 WHERE id = 1", [s])
            .unwrap();
    }

    fn row(&self, p: Recorded) -> Option<i64> {
        self.conn
            .query_row(
                "SELECT at_s FROM polite_stops WHERE pid = ?1 AND pid_start = ?2",
                params![p.pid, p.pid_start.unwrap() as i64],
                |r| r.get(0),
            )
            .optional()
            .unwrap()
    }

    fn run(&mut self, pass: &mut Pass<'_>) -> PassReport {
        pass.run(&mut self.conn).unwrap()
    }
}

struct Fake;

impl Outside for Fake {
    fn panes(&self) -> Option<BTreeMap<String, InPane>> {
        None
    }

    fn close_pane(&self, _pane: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn holds(&self) -> Vec<Hold> {
        Vec::new()
    }

    fn worktree_folders(&self, _conn: &Connection) -> anyhow::Result<Vec<FolderItem>> {
        Ok(Vec::new())
    }

    fn attempt_spec(&self, _attempt: &AttemptRow) -> anyhow::Result<LaunchSpec> {
        anyhow::bail!("no attempt launches in these tests")
    }
}

struct Proc(Child);

impl Proc {
    fn start(w: &World, on_term: &str, env: &[(&str, &Path)]) -> Proc {
        let ready = w.file("ready");
        let _ = std::fs::remove_file(&ready);
        let script = format!(
            "trap '{on_term}' TERM; : > \"{}\"; while :; do sleep 0.1; done",
            ready.display()
        );
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c").arg(script);
        for (k, v) in env {
            cmd.env(k, v);
        }
        let p = Proc(cmd.spawn().unwrap());
        assert!(wait_for(|| ready.exists()), "the shell set no trap");
        p
    }

    fn stubborn(w: &World) -> Proc {
        Proc::start(w, "", &[])
    }

    fn polite(w: &World) -> Proc {
        Proc::start(w, "exit 0", &[])
    }

    fn recorded(&self) -> Recorded {
        let pid = self.0.id();
        Recorded {
            pid,
            pid_start: horch_platform::process::start_time(pid),
        }
    }

    fn wait_exit(&mut self) -> bool {
        wait_for(|| matches!(self.0.try_wait(), Ok(Some(_))))
    }
}

impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_for(mut cond: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(30);
    while Instant::now() < end {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

#[test]
fn probe_rows() {
    let Some(db) = std::env::var_os(PROBE_DB) else {
        return;
    };
    let out = PathBuf::from(std::env::var_os(PROBE_OUT).unwrap());
    let conn = Connection::open(db).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    let n: i64 = conn
        .query_row("SELECT count(*) FROM polite_stops", [], |r| r.get(0))
        .unwrap();
    let tmp = out.with_extension("tmp");
    std::fs::write(&tmp, n.to_string()).unwrap();
    std::fs::rename(tmp, out).unwrap();
}

#[test]
fn polite_stop_is_a_row_before_the_signal() {
    let mut w = world();
    let out = w.file("rows-at-signal");
    let exe = std::env::current_exe().unwrap();
    let db = w.db();
    let on_term = "\"$PROBE\" --exact probe_rows --nocapture --test-threads=1 >/dev/null 2>&1";
    let p = Proc::start(
        &w,
        on_term,
        &[("PROBE", &exe), (PROBE_DB, &db), (PROBE_OUT, &out)],
    );
    w.orphan(&p);
    let pid = p.recorded().pid;

    let fake = Fake;
    let mut pass = w.pass(&fake);
    let clock = w.clock();
    assert_eq!(
        w.run(&mut pass).done,
        vec![Done::Stop(format!("process {pid}"))]
    );
    assert_eq!(
        w.row(p.recorded()),
        Some(clock),
        "the row has the fleet clock"
    );
    assert!(wait_for(|| out.exists()), "the process got no polite stop");
    assert_eq!(
        std::fs::read_to_string(&out).unwrap(),
        "1",
        "the row was committed when the signal came"
    );
    assert!(!w.stops_dir().exists(), "no file in run/stops");
}

#[test]
fn kill_comes_after_the_grace_across_a_restart() {
    let mut w = world();
    let mut p = Proc::stubborn(&w);
    w.orphan(&p);
    let pid = p.recorded().pid;
    let fake = Fake;

    let mut pass = w.pass(&fake);
    assert_eq!(
        w.run(&mut pass).done,
        vec![Done::Stop(format!("process {pid}"))]
    );
    let at = w.row(p.recorded()).expect("the row of the polite stop");
    drop(pass);

    w.set_clock(at + 10);
    let mut pass = w.pass(&fake);
    assert_eq!(w.run(&mut pass).done, Vec::new(), "within its grace");
    assert_eq!(w.row(p.recorded()), Some(at), "the time stays");
    drop(pass);

    w.set_clock(at + 25);
    let mut pass = w.pass(&fake);
    assert_eq!(
        w.run(&mut pass).done,
        vec![Done::Stop(format!("kill process {pid}"))]
    );
    assert!(p.wait_exit(), "the kill ended it");
}

#[test]
fn row_is_deleted_when_the_process_is_gone() {
    let mut w = world();
    let mut p = Proc::polite(&w);
    w.orphan(&p);
    let r = p.recorded();
    let fake = Fake;
    let mut pass = w.pass(&fake);

    w.run(&mut pass);
    assert!(w.row(r).is_some(), "the row of the polite stop");
    assert!(p.wait_exit(), "the polite stop ended it");
    w.run(&mut pass);
    assert_eq!(w.row(r), None, "the row of a gone process is deleted");
}

#[test]
fn old_stop_file_becomes_a_row_once() {
    let mut w = world();
    let mut p = Proc::stubborn(&w);
    w.orphan(&p);
    let r = p.recorded();
    let pid = r.pid;
    std::fs::create_dir_all(w.stops_dir()).unwrap();
    std::fs::write(w.old_stop_file(r), r#"{"polite_at":0}"#).unwrap();
    w.set_clock(5);
    let fake = Fake;
    let mut pass = w.pass(&fake);

    let report = w.run(&mut pass);
    assert_eq!(report.done, Vec::new(), "the grace from the file holds");
    assert_eq!(w.row(r), Some(0), "the file is a row");
    assert!(!w.old_stop_file(r).exists(), "the file is removed");
    assert!(!w.stops_dir().exists(), "the empty folder is removed");

    assert_eq!(w.run(&mut pass).done, Vec::new());
    assert_eq!(w.row(r), Some(0), "the row stays");

    w.set_clock(25);
    assert_eq!(
        w.run(&mut pass).done,
        vec![Done::Stop(format!("kill process {pid}"))]
    );
    assert!(p.wait_exit(), "the kill ended it");
}

#[test]
fn symlinked_old_stop_file_is_removed_and_logged() {
    let mut w = world();
    let p = Proc::stubborn(&w);
    w.orphan(&p);
    let r = p.recorded();
    let pid = r.pid;
    let target = w.file("forged.json");
    std::fs::write(&target, r#"{"polite_at":0}"#).unwrap();
    std::fs::create_dir_all(w.stops_dir()).unwrap();
    let link = w.old_stop_file(r);
    std::os::unix::fs::symlink(&target, &link).unwrap();
    w.set_clock(100);
    let fake = Fake;
    let mut pass = w.pass(&fake);

    let report = w.run(&mut pass);
    assert_eq!(
        report.done,
        vec![Done::Stop(format!("process {pid}"))],
        "a new grace: the polite stop, not the kill"
    );
    assert_eq!(w.row(r), Some(100), "the row has the time of the new stop");
    assert!(
        std::fs::symlink_metadata(&link).is_err(),
        "the link is removed"
    );
    assert!(target.exists(), "the target of the link stays");
    let name = link.display().to_string();
    assert!(
        report
            .failed
            .iter()
            .any(|l| l.contains(&name) && l.contains("not a regular file")),
        "the log names the file and the reason: {:?}",
        report.failed
    );
}
