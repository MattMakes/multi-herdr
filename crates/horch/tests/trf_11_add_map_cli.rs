//! `horch ledger add-map` as the strict consumer of the ticket contract C1
//! (trace-fixes ticket 10): `--dry-run` (TRF-11), the ticket snapshot in the
//! claim (TRF-12), the refusal of a ticket already queued (TRF-13) and the
//! `Kind`, `Review` and `Check none` fields (TRF-14). Every test runs the
//! binary on a temp git project, with the watchdog off and no herdr pane. A
//! test fleetd serves the requests of the commands.

mod support;

use std::path::PathBuf;
use std::process::Command;

use support::fleetd::{short_tempdir, TestFleetd};

struct Out {
    code: i32,
    out: String,
    err: String,
}

/// The fleetd drops first: fields drop in order.
struct World {
    _fleetd: Option<TestFleetd>,
    tmp: tempfile::TempDir,
}

const PATH: &str = "/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin";

impl World {
    fn new() -> World {
        let mut w = World::without_fleetd();
        w.start_fleetd();
        w
    }

    /// The temp git project, and no fleetd: no ledger exists yet.
    fn without_fleetd() -> World {
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
        World { _fleetd: None, tmp }
    }

    fn start_fleetd(&mut self) {
        let fleet_root = self.tmp.path().join("home/.horch/fleet");
        self._fleetd = Some(TestFleetd::start(self.horch(), &fleet_root));
    }

    fn proj(&self) -> PathBuf {
        self.tmp.path().join("proj")
    }

    fn write(&self, name: &str, text: &str) {
        std::fs::write(self.proj().join("tickets").join(name), text).unwrap();
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

    fn run_as(&self, worker: Option<&str>, args: &[&str]) -> Out {
        let mut cmd = self.horch();
        cmd.args(args);
        if let Some(w) = worker {
            cmd.env("HORCH_WORKER_ID", w);
        }
        let o = cmd.output().unwrap();
        Out {
            code: o.status.code().unwrap(),
            out: String::from_utf8(o.stdout).unwrap(),
            err: String::from_utf8(o.stderr).unwrap(),
        }
    }

    fn run(&self, args: &[&str]) -> Out {
        self.run_as(None, args)
    }

    fn ok(&self, args: &[&str]) -> String {
        let o = self.run(args);
        assert_eq!(o.code, 0, "{args:?}: {}{}", o.out, o.err);
        o.out
    }

    fn task_count(&self) -> usize {
        let o = self.run(&["ledger", "status", "--json"]);
        let v: serde_json::Value = serde_json::from_str(&o.out).unwrap();
        v["tasks"].as_array().unwrap().len()
    }
}

/// A C1 ticket with `fields` after its version line.
fn c1(title: &str, fields: &str) -> String {
    format!("# 01: {title}\n\n<!-- horch-ticket: 1 -->\n{fields}\n\n## Goal\n\nDo {title}.\n")
}

const FIELDS: &str = "**Persona:** primary `dev` · **Parallel:** yes\n**Kind:** feature\n**Check:** `true`\n**Own:** `src/a.rs`";

const MAP: &str =
    "# Map\n\n| # | Title | Blocked by | Wave |\n|---|---|---|---|\n| 01 | [A](a.md) | | 1 |\n";

#[test]
fn trf_11_dry_run_names_each_problem_and_writes_nothing() {
    let w = World::new();
    w.ok(&["ledger", "run", "new", "r1"]);
    w.write("a.md", &c1("A", &FIELDS.replace("feature", "chore")));
    w.write(
        "MAP.md",
        &format!("{MAP}| 02 | [B](b.md) | 01 | 2 |\n| 03 | [C](a.md) | 07 | 2 |\n"),
    );
    w.write("b.md", "# 02: B\n\n**Persona:** primary `dev`\n");
    let o = w.run(&["ledger", "add-map", "--dry-run", "tickets/MAP.md"]);
    assert_eq!(o.code, 1, "{}{}", o.out, o.err);
    let lines: Vec<&str> = o.out.lines().collect();
    let kind = "tickets/a.md:5: Kind 'chore' is not one of: feature, quality, refactor, security, docs, report";
    assert_eq!(
        &lines[..3],
        &[
            kind,
            "tickets/b.md:3: the field block does not start with <!-- horch-ticket: 1 -->",
            kind,
        ],
        "{}",
        o.out
    );
    assert!(
        lines[3].starts_with("3 problems in ") && lines[3].ends_with("Nothing was queued."),
        "{}",
        o.out
    );
    assert_eq!(w.task_count(), 0);

    // A clean map passes the dry run, needs no run, and still queues nothing.
    // fleetd creates the ledger at its start, so it starts after the dry run.
    let mut w = World::without_fleetd();
    w.write("a.md", &c1("A", FIELDS));
    w.write("MAP.md", MAP);
    let ledger = PathBuf::from(w.ok(&["ledger", "path"]).trim());
    let out = w.ok(&["ledger", "add-map", "--dry-run", "tickets/MAP.md"]);
    assert!(!ledger.exists(), "the dry run created {}", ledger.display());
    assert!(
        out.starts_with("1 tickets in ")
            && out.ends_with("follow the ticket format. Nothing was queued.\n"),
        "{out}"
    );
    w.start_fleetd();
    w.ok(&["ledger", "run", "new", "r1"]);
    assert_eq!(w.task_count(), 0);
}

#[test]
fn trf_13_add_map_refuses_a_ticket_already_queued_unless_again() {
    let w = World::new();
    w.ok(&["ledger", "run", "new", "r1"]);
    w.write("a.md", &c1("A", FIELDS));
    w.write("MAP.md", MAP);
    w.ok(&["ledger", "add-map", "tickets/MAP.md"]);
    let o = w.run(&["ledger", "add-map", "tickets/MAP.md"]);
    assert_ne!(o.code, 0);
    assert!(
        o.err
            .contains("tickets/a.md is already T-1 (queued) in this run."),
        "{}",
        o.err
    );
    assert!(o.err.contains("add --again"), "{}", o.err);
    assert_eq!(w.task_count(), 1);
    let out = w.ok(&["ledger", "add-map", "--again", "tickets/MAP.md"]);
    assert!(out.starts_with("T-2 queued for dev: A."), "{out}");
    assert_eq!(w.task_count(), 2);
}

#[test]
fn trf_14_add_map_stores_kind_review_and_check_none() {
    let w = World::new();
    w.ok(&["ledger", "run", "new", "r1"]);
    w.write(
        "a.md",
        &c1(
            "A",
            "**Persona:** primary `dev`\n**Kind:** quality\n**Check:** none\n**Own:** `src/a.rs`",
        ),
    );
    w.write(
        "b.md",
        &c1(
            "B",
            &FIELDS.replace("**Check:**", "**Review:** light\n**Check:**"),
        ),
    );
    w.write("MAP.md", &format!("{MAP}| 02 | [B](b.md) | | 1 |\n"));
    w.ok(&["ledger", "add-map", "tickets/MAP.md"]);
    let st: serde_json::Value =
        serde_json::from_str(&w.run(&["ledger", "status", "--json"]).out).unwrap();
    let tasks = st["tasks"].as_array().unwrap();
    let (a, b) = (&tasks[0]["task"], &tasks[1]["task"]);
    assert_eq!(a["work_kind"], "quality");
    assert_eq!(a["review_mode"], "full");
    assert_eq!(a["check_none"], true);
    assert_eq!(a["land_check"], serde_json::Value::Null);
    assert_eq!(b["work_kind"], "feature");
    assert_eq!(b["review_mode"], "light");
    assert_eq!(b["check_none"], false);
    assert_eq!(b["land_check"], "true");
    assert!(a.get("ticket_snapshot").is_none(), "{a}");
}
