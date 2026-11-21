use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use horch_core::taskledger::Store;
use horch_platform::channel::Stream;

use crate::harness::{wait_for, wait_for_or, Harness};

const SIGKILL: i32 = 9;
const SIGTERM: i32 = 15;

pub struct Fleetds {
    db: PathBuf,
    root: PathBuf,
    children: Vec<Child>,
}

impl Fleetds {
    pub fn new(h: &Harness) -> Fleetds {
        Fleetds::on(h, h.root.join("ledger.sqlite3"))
    }

    pub fn on(h: &Harness, db: PathBuf) -> Fleetds {
        Fleetds {
            db,
            root: h.root.clone(),
            children: Vec::new(),
        }
    }

    pub fn row(&self) -> Option<(u32, u64)> {
        let store = Store::open(&self.db).ok()?;
        let (pid, started): (i64, i64) = store
            .conn()
            .query_row("SELECT pid, pid_start FROM fleetd WHERE id = 1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .ok()?;
        Some((u32::try_from(pid).ok()?, u64::try_from(started).ok()?))
    }

    pub fn spawn(&mut self, h: &Harness, name: &str) -> u32 {
        let file = |ext: &str| std::fs::File::create(self.root.join(format!("{name}.{ext}")));
        let mut cmd = Command::new(Harness::horch_bin());
        h.seal(&mut cmd);
        let child = cmd
            .arg("fleetd")
            .stdin(Stdio::null())
            .stdout(file("stdout").unwrap())
            .stderr(file("stderr").unwrap())
            .spawn()
            .unwrap();
        let pid = child.id();
        self.children.push(child);
        pid
    }

    pub fn start(&mut self, h: &Harness, name: &str) -> u32 {
        let pid = self.spawn(h, name);
        wait_for("the fleetd row to name the new fleetd", || {
            self.row().filter(|&(p, _)| p == pid).map(|_| ())
        });
        pid
    }

    pub fn serve(&mut self, h: &Harness, name: &str) -> u32 {
        let pid = self.spawn(h, name);
        let socket = h.fleet_root().join("run").join("fleetd.sock");
        let stderr = self.root.join(format!("{name}.stderr"));
        let read_stderr = || std::fs::read_to_string(&stderr).unwrap_or_default();
        let child = self
            .children
            .iter_mut()
            .find(|c| c.id() == pid)
            .expect("the new fleetd");
        wait_for_or(
            "the test fleetd to serve its channel",
            || {
                let probe = Instant::now() + Duration::from_millis(100);
                if Stream::connect(&socket, probe).is_ok() {
                    return Some(());
                }
                let exited = child.try_wait().ok().flatten();
                assert!(
                    exited.is_none(),
                    "the test fleetd exited ({exited:?}): {}",
                    read_stderr()
                );
                None
            },
            read_stderr,
        );
        pid
    }

    pub fn output(&self, name: &str, ext: &str) -> String {
        std::fs::read_to_string(self.root.join(format!("{name}.{ext}"))).unwrap_or_default()
    }

    pub fn wait_exit(&mut self, pid: u32) -> ExitStatus {
        let child = self
            .children
            .iter_mut()
            .find(|c| c.id() == pid)
            .expect("a fleetd of this test");
        wait_for("fleetd to exit", || child.try_wait().unwrap())
    }

    pub fn stop_current(&mut self) {
        let (pid, started) = self.row().expect("the fleetd row");
        assert!(horch_core::procid::signal_same(pid, started, SIGTERM));
        for child in self.children.iter_mut().filter(|c| c.id() == pid) {
            let _ = child.wait();
        }
        wait_for("fleetd to end", || {
            (!horch_core::procid::alive(pid, Some(started))).then_some(())
        });
    }

    pub fn kill_current(&mut self) -> (u32, u64) {
        let (pid, started) = self.row().expect("the fleetd row");
        assert!(horch_core::procid::signal_same(pid, started, SIGKILL));
        for child in self.children.iter_mut().filter(|c| c.id() == pid) {
            let _ = child.wait();
        }
        wait_for("fleetd to end", || {
            (!horch_core::procid::alive(pid, Some(started))).then_some(())
        });
        (pid, started)
    }
}

impl Drop for Fleetds {
    fn drop(&mut self) {
        if let Some((pid, started)) = self.row() {
            horch_core::procid::signal_same(pid, started, SIGKILL);
        }
        for child in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
