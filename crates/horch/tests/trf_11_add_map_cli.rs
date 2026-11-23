mod support;

use std::path::PathBuf;
use std::process::Command;

use horch_core::taskledger::Store;
use support::fleetd::{short_tempdir, TestFleetd};

struct Out {
    code: i32,
    out: String,
    err: String,
}

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

    fn task_count(&self) -> i64 {
        let ledger = PathBuf::from(self.ok(&["ledger", "path"]).trim());
        let store = Store::open_read_only(&ledger).unwrap();
        store
            .conn()
            .query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))
            .unwrap()
    }
}

fn c1(title: &str, fields: &str) -> String {
    format!("# 01: {title}\n\n<!-- horch-ticket: 1 -->\n{fields}\n\n## Goal\n\nDo {title}.\n")
}

const FIELDS: &str = "**Persona:** primary `dev` · **Parallel:** yes\n**Kind:** feature\n**Check:** `true`\n**Own:** `src/a.rs`";

const MAP: &str =
    "# Map\n\n| # | Title | Blocked by | Wave |\n|---|---|---|---|\n| 01 | [A](a.md) | | 1 |\n";

#[test]
fn trf_11_dry_run_names_each_problem_and_writes_nothing() {
    let w = World::new();
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
    assert_eq!(w.task_count(), 0);
}
