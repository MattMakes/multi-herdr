//! `horch ledger add` maps `--check none` to "no check", the same as the
//! ticket line `**Check:** none`. It never stores the shell command `none`.
//! Every test runs the binary on a temp git project, with the watchdog off
//! and no herdr pane. A test fleetd serves the requests of the commands.

mod support;

use std::path::PathBuf;
use std::process::Command;

use support::fleetd::{short_tempdir, TestFleetd};

const PATH: &str = "/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin";

/// The fleetd drops first: fields drop in order.
struct World {
    _fleetd: Option<TestFleetd>,
    tmp: tempfile::TempDir,
}

impl World {
    fn new() -> World {
        let tmp = short_tempdir();
        let proj = tmp.path().join("proj");
        std::fs::create_dir_all(proj.join("tickets")).unwrap();
        for args in [
            &["init", "-q", "-b", "main"][..],
            &["commit", "-q", "--allow-empty", "-m", "init"][..],
        ] {
            let st = Command::new("git")
                .args(args)
                .current_dir(&proj)
                .env_clear()
                .env("PATH", PATH)
                .env("HOME", tmp.path().join("home"))
                .env("GIT_AUTHOR_NAME", "n")
                .env("GIT_AUTHOR_EMAIL", "a@b")
                .env("GIT_COMMITTER_NAME", "n")
                .env("GIT_COMMITTER_EMAIL", "a@b")
                .status()
                .unwrap();
            assert!(st.success(), "git {args:?}");
        }
        let mut w = World { _fleetd: None, tmp };
        let fleet_root = w.tmp.path().join("home/.horch/fleet");
        w._fleetd = Some(TestFleetd::start(w.horch(), &fleet_root));
        w
    }

    fn proj(&self) -> PathBuf {
        self.tmp.path().join("proj")
    }

    fn ticket(&self, name: &str, check: &str) {
        let text = format!(
            "# 01: {name}\n\n<!-- horch-ticket: 1 -->\n**Persona:** primary `dev`\n**Kind:** quality\n**Check:** {check}\n**Own:** `src/a.rs`\n\n## Goal\n\nDo {name}.\n"
        );
        std::fs::write(self.proj().join("tickets").join(name), text).unwrap();
    }

    fn ok(&self, args: &[&str]) -> String {
        let (ok, out) = self.run(args);
        assert!(ok, "{args:?}: {out}");
        out
    }

    /// The `horch` binary with the environment of the test.
    fn horch(&self) -> Command {
        let root = self.tmp.path();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_horch"));
        cmd.current_dir(self.proj())
            .env_clear()
            .env("PATH", PATH)
            .env("HOME", root.join("home"))
            .env("XDG_DATA_HOME", root.join("data"))
            .env("HORCH_STATE_DIR", root.join("state"))
            .env("HORCH_PROJECT_DIR", self.proj())
            .env("HORCH_NO_WATCHDOG", "1");
        cmd
    }

    /// Run horch. It returns success and stdout followed by stderr.
    fn run(&self, args: &[&str]) -> (bool, String) {
        let o = self.horch().args(args).output().unwrap();
        let out = String::from_utf8(o.stdout).unwrap();
        let err = String::from_utf8(o.stderr).unwrap();
        (o.status.success(), format!("{out}{err}"))
    }

    /// The `task` object of each queued task, in queue order. `status`
    /// exits non-zero with no orchestrator, so only its JSON counts.
    fn tasks(&self) -> Vec<serde_json::Value> {
        let o = self
            .horch()
            .args(["ledger", "status", "--json"])
            .output()
            .unwrap();
        let st: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
        st["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["task"].clone())
            .collect()
    }
}

#[test]
fn check_none_flag_on_add_stores_no_check() {
    let w = World::new();
    w.ok(&["ledger", "run", "new", "r1"]);
    // The incident: the ticket says none and the flag says none.
    w.ticket("a.md", "none");
    // The flag overrides a ticket command, with spaces around it.
    w.ticket("b.md", "`cargo test`");
    w.ok(&[
        "ledger",
        "add",
        "--file",
        "tickets/a.md",
        "--primary",
        "dev",
        "--check",
        "none",
        "--run",
        "r1",
    ]);
    w.ok(&[
        "ledger",
        "add",
        "--file",
        "tickets/b.md",
        "--primary",
        "dev",
        "--check",
        " none ",
        "--run",
        "r1",
    ]);
    let tasks = w.tasks();
    for t in &tasks {
        assert_eq!(t["check_none"], true, "{t}");
        assert_eq!(
            t["land_check"],
            serde_json::Value::Null,
            "none is not a command: {t}"
        );
    }
}

#[test]
fn check_none_in_ticket_on_add_without_flag_stores_no_check() {
    let w = World::new();
    w.ok(&["ledger", "run", "new", "r1"]);
    w.ticket("a.md", "none");
    w.ticket("b.md", "`cargo test`");
    w.ok(&[
        "ledger",
        "add",
        "--file",
        "tickets/a.md",
        "--primary",
        "dev",
        "--run",
        "r1",
    ]);
    w.ok(&[
        "ledger",
        "add",
        "--file",
        "tickets/b.md",
        "--primary",
        "dev",
        "--run",
        "r1",
    ]);
    let tasks = w.tasks();
    assert_eq!(tasks[0]["check_none"], true, "{}", tasks[0]);
    assert_eq!(tasks[0]["land_check"], serde_json::Value::Null);
    assert_eq!(tasks[1]["check_none"], false, "{}", tasks[1]);
    assert_eq!(tasks[1]["land_check"], "cargo test");
}
