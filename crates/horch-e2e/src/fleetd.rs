//! The `horch fleetd` processes of an end-to-end test.
//!
//! [`Fleetds`] starts `horch fleetd` in the sealed environment of a
//! [`Harness`], reads the `fleetd` row of its ledger, and stops each fleetd
//! of the test when dropped, also when the test fails: the children it
//! started, and the fleetd that the row names (a check before a command can
//! start one detached).

use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};

use horch_core::taskledger::Store;

use crate::harness::{wait_for, Harness};

const SIGKILL: i32 = 9;

/// The fleetd processes of 1 test, on the ledger `<root>/ledger.sqlite3` of
/// its harness.
pub struct Fleetds {
    db: PathBuf,
    root: PathBuf,
    children: Vec<Child>,
}

impl Fleetds {
    pub fn new(h: &Harness) -> Fleetds {
        Fleetds {
            db: h.root.join("ledger.sqlite3"),
            root: h.root.clone(),
            children: Vec::new(),
        }
    }

    /// The PID and start time of the fleetd that the `fleetd` row names.
    /// `None` while no row exists.
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

    /// Start `horch fleetd` in the sealed environment of `h`, with its
    /// output in `<root>/<name>.stdout` and `<root>/<name>.stderr`. Gives
    /// its PID. It does not wait for the row.
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

    /// [`Fleetds::spawn`], then wait until the `fleetd` row names it.
    pub fn start(&mut self, h: &Harness, name: &str) -> u32 {
        let pid = self.spawn(h, name);
        wait_for("the fleetd row to name the new fleetd", || {
            self.row().filter(|&(p, _)| p == pid).map(|_| ())
        });
        pid
    }

    /// The text that the fleetd `name` wrote to `ext` (`stdout` or
    /// `stderr`).
    pub fn output(&self, name: &str, ext: &str) -> String {
        std::fs::read_to_string(self.root.join(format!("{name}.{ext}"))).unwrap_or_default()
    }

    /// Wait until the child `pid` of [`Fleetds::spawn`] exits, and give its
    /// exit status.
    pub fn wait_exit(&mut self, pid: u32) -> ExitStatus {
        let child = self
            .children
            .iter_mut()
            .find(|c| c.id() == pid)
            .expect("a fleetd of this test");
        wait_for("fleetd to exit", || child.try_wait().unwrap())
    }

    /// Kill the fleetd that the row names, and wait until it is gone. Gives
    /// its PID and start time.
    pub fn kill_current(&mut self) -> (u32, u64) {
        let (pid, started) = self.row().expect("the fleetd row");
        assert!(horch_core::procid::signal_same(pid, started, SIGKILL));
        // A child of the test stays a zombie, which counts as alive, until
        // it is waited for.
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
