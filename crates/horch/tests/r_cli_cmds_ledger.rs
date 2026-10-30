//! `horch ledger legacy`: every subcommand arm of the legacy session ledger,
//! with a temp state dir.

use std::process::{Command, Output};

struct World {
    tmp: tempfile::TempDir,
}

impl World {
    fn run(&self, args: &[&str]) -> Output {
        let root = self.tmp.path();
        Command::new(env!("CARGO_BIN_EXE_horch"))
            .args(["ledger", "legacy"])
            .args(args)
            .current_dir(root)
            .env_clear()
            .env("HOME", root.join("home"))
            .env("XDG_DATA_HOME", root.join("data"))
            .env("HORCH_STATE_DIR", root.join("state"))
            .env("HORCH_PROJECT_DIR", "/oracle/project")
            .env("HORCH_GIT_BIN", "/nonexistent")
            .output()
            .unwrap()
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert!(out.status.success(), "{args:?}: {out:?}");
        String::from_utf8(out.stdout).unwrap()
    }
}

#[test]
fn the_legacy_subcommands_write_read_and_report_a_session() {
    let w = World {
        tmp: tempfile::tempdir().unwrap(),
    };
    w.ok(&[
        "add",
        "--record-id",
        "r1",
        "--agent",
        "claude",
        "--tier",
        "t",
        "--model",
        "m",
        "--role",
        "dev",
        "--task",
        "do it",
    ]);
    assert_eq!(w.run(&["has-session", "s1"]).status.code(), Some(1));
    w.ok(&["set-session", "r1", "s1"]);
    assert_eq!(w.run(&["has-session", "s1"]).status.code(), Some(0));
    w.ok(&["note", "r1", "hello"]);
    w.ok(&["done", "r1", "fin"]);
    let record: serde_json::Value = serde_json::from_str(&w.ok(&["get", "r1"])).unwrap();
    assert_eq!(record["session_id"], "s1");
    assert_eq!(record["status"], "done");
    let events: Vec<_> = record["history"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| (h["event"].as_str().unwrap(), h["text"].as_str().unwrap()))
        .collect();
    assert_eq!(
        events,
        [("spawned", "do it"), ("note", "hello"), ("done", "fin")]
    );
    let list = w.ok(&["list"]);
    assert!(
        list.starts_with("[done] t (dev)  session=s1  record=r1\n"),
        "{list}"
    );
    assert!(w.ok(&["path"]).trim_end().ends_with("-oracle-project.json"));
}

#[test]
fn the_legacy_resume_and_assign_arms_run() {
    let w = World {
        tmp: tempfile::tempdir().unwrap(),
    };
    w.ok(&[
        "add",
        "--record-id",
        "r1",
        "--agent",
        "claude",
        "--tier",
        "t",
        "--model",
        "m",
        "--role",
        "dev",
        "--task",
        "first",
    ]);
    w.ok(&["assign", "--role", "dev", "second"]);
    w.ok(&["resume", "r1", "dev", "again"]);
    let record: serde_json::Value = serde_json::from_str(&w.ok(&["get", "r1"])).unwrap();
    let texts: Vec<&str> = record["history"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["text"].as_str().unwrap())
        .collect();
    assert!(
        texts.contains(&"second") && texts.contains(&"again"),
        "{texts:?}"
    );
}
