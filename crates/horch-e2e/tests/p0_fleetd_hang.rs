//! Phase 0 red test for a hung fleetd (`docs/specs/fleetd/design.md`, FM-12,
//! PL-5, and preservation row 06 S14). It turns green in Phase 1.
//!
//! This file has no `#![cfg(unix)]`: the design wants the result on macOS,
//! Linux and Windows. It uses only `std` and `horch_core` functions that
//! compile on each OS.
//!
//! The test stands in for the service manager: it calls `horch fleetd check`
//! itself, as the 30 s timer of the design does. It reads the PID of fleetd
//! from the `fleetd` row of the ledger of the harness.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Child, Output, Stdio};
use std::time::{Duration, Instant};

use horch_core::procid;
use horch_core::taskledger::Store;
use horch_e2e::harness::Harness;

/// The hang limit of fleetd, in awake time (PL-5).
const HANG_LIMIT: Duration = Duration::from_secs(30);

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// The PID and start time of the fleetd that the `fleetd` row of the ledger
/// names (design, "Ledger"). `None` while no row exists.
fn fleetd_row(db: &Path) -> Option<(u32, Option<u64>)> {
    if !db.exists() {
        return None;
    }
    let store = Store::open(db).ok()?;
    let (pid, started): (i64, i64) = store
        .conn()
        .query_row("SELECT pid, pid_start FROM fleetd WHERE id = 1", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .ok()?;
    Some((u32::try_from(pid).ok()?, u64::try_from(started).ok()))
}

/// The hung fleetd that the test started, and each fleetd that the `fleetd`
/// row names. Drop stops them all, also when the test fails.
struct Fleetds {
    db: PathBuf,
    child: Option<Child>,
    seen: Vec<(u32, Option<u64>)>,
}

impl Fleetds {
    /// The fleetd that the `fleetd` row names, recorded for the teardown.
    fn current(&mut self) -> Option<(u32, Option<u64>)> {
        let row = fleetd_row(&self.db)?;
        if !self.seen.contains(&row) {
            self.seen.push(row);
        }
        Some(row)
    }
}

impl Drop for Fleetds {
    fn drop(&mut self) {
        let _ = self.current();
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
        for &(pid, started) in &self.seen {
            if let Some(started) = started.or_else(|| procid::start_time(pid)) {
                procid::stop_same(pid, started, procid::Stop::Kill);
            }
        }
    }
}

#[test]
fn fm12_hung_fleetd_is_restarted_on_each_os() {
    horch_e2e::expected::check(file!(), "fm12_hung_fleetd_is_restarted_on_each_os", || {
        let mut h = Harness::new("p0-fm12");
        let db = h.root.join("ledger.sqlite3");
        h.set("HORCH_LEDGER_DB", db.to_string_lossy());
        h.set("HORCH_NO_WAKE", "1");
        let mut fleetds = Fleetds {
            db: db.clone(),
            child: None,
            seen: Vec::new(),
        };
        let out = h.root.join("fleetd.stdout");
        let err = h.root.join("fleetd.stderr");
        // Only this fleetd hangs: the one that `horch fleetd check` starts
        // gets the environment of the check.
        let mut cmd = h.horch(&["fleetd"]);
        cmd.env("HORCH_FAULT", "hang-pass")
            .stdin(Stdio::null())
            .stdout(File::create(&out).unwrap())
            .stderr(File::create(&err).unwrap());
        let hung = fleetds.child.insert(cmd.spawn().unwrap()).id();
        let exited = |child: &mut Child, what: &str| {
            child.try_wait().unwrap().map(|status| {
                format!(
                    "horch fleetd {what}: status: {status}\nstdout: {}\nstderr: {}",
                    std::fs::read_to_string(&out).unwrap_or_default(),
                    std::fs::read_to_string(&err).unwrap_or_default()
                )
            })
        };
        let until = Instant::now() + Duration::from_secs(30);
        while fleetds.current().is_none_or(|(pid, _)| pid != hung) {
            if let Some(why) = exited(fleetds.child.as_mut().unwrap(), "exited at start") {
                panic!("{why}");
            }
            assert!(
                Instant::now() < until,
                "horch fleetd (PID {hung}) runs, but after 30 s the fleetd row of the ledger names {:?}",
                fleetd_row(&db)
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        // Its heartbeat is older than the hang limit after this sleep.
        std::thread::sleep(HANG_LIMIT + Duration::from_secs(2));
        if let Some(why) = exited(fleetds.child.as_mut().unwrap(), "with hang-pass exited") {
            panic!("{why}");
        }
        let o = h.run(&["fleetd", "check"]);
        assert!(o.status.success(), "horch fleetd check: {}", text(&o));
        let until = Instant::now() + Duration::from_secs(10);
        while fleetds
            .child
            .as_mut()
            .unwrap()
            .try_wait()
            .unwrap()
            .is_none()
        {
            assert!(
                Instant::now() < until,
                "the hung fleetd (PID {hung}) still runs 10 s after horch fleetd check: {}",
                text(&o)
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        let (new, started) = fleetds.current().unwrap_or_else(|| {
            panic!(
                "the fleetd row is gone after horch fleetd check: {}",
                text(&o)
            )
        });
        assert!(
            new != hung && procid::alive(new, started),
            "no new fleetd runs after horch fleetd check: the fleetd row names PID {new}: {}",
            text(&o)
        );
        // A fresh heartbeat: a check past 1 more hang limit leaves the new
        // fleetd in place, so its passes write the heartbeat.
        std::thread::sleep(HANG_LIMIT + Duration::from_secs(2));
        let o = h.run(&["fleetd", "check"]);
        assert!(o.status.success(), "horch fleetd check: {}", text(&o));
        assert_eq!(
            fleetds.current(),
            Some((new, started)),
            "horch fleetd check restarted a fresh fleetd: {}",
            text(&o)
        );
        assert!(
            procid::alive(new, started),
            "the new fleetd (PID {new}) ended"
        );
    });
}
