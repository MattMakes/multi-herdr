//! The idea flow of Phase 6 through the binary
//! (`ai_docs/plans/fleetd-build/tickets/56-phase-6-idea-flow-through-the-binary.md`).
//!
//! Each test makes its own ledger, fleet root and git project in a temp
//! folder, and runs a test `horch fleetd` with the fake herdr and the fake
//! agents. `horch idea add --direct` adds 1 ticket. fleetd makes the feature
//! row and then the feature branch in the project `.git`.
//!
//! Each test waits for an event of the ledger or of git, never for a time.
//! Each test sets `HORCH_TEST_DOCS_FIXTURE` to an empty local folder, so no
//! test uses the network.

#![cfg(unix)]

use std::process::Output;

use horch_e2e::harness::{fixtures, wait_for_or, Harness};
use rusqlite::{Connection, OpenFlags};

/// The `fleet.toml` of each test: 1 pass each second.
const FLEET_TOML: &str = "pass_period_s = 1\n";

/// The ticket of each test: 1 task of the code kind.
const TICKET: &str = "# 01: Write the queue guide\n\n\
<!-- horch-ticket: 1 -->\n\
**Persona:** primary `opus` · **Parallel:** yes\n\
**Kind:** feature\n\
**Review:** none\n\
**Check:** `true`\n\
**Own:** `fake-claude.txt`\n\n\
## Goal\n\nThe guide names the queue length.\n";

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// A fleet with a test fleetd and 1 `--direct` idea.
struct World {
    h: Harness,
}

impl World {
    /// The fleet, with `scenario` as the keys of each fake agent.
    fn new(name: &str, scenario: &str) -> World {
        let mut h = Harness::new(name).with_git();
        // The ledger is in the state folder and the fleet root is next to
        // it, with no FLEET_ROOT: a pane gets only HORCH_STATE_DIR and
        // HORCH_DATA_DIR, and its worker finds the same paths there.
        let fleet = h.state.join("fleet");
        std::fs::create_dir_all(&fleet).unwrap();
        std::fs::write(fleet.join("fleet.toml"), FLEET_TOML).unwrap();
        h.set(
            "HORCH_LEDGER_DB",
            h.state.join("ledger.sqlite3").to_string_lossy(),
        );
        h.set("HORCH_NO_WAKE", "1");
        h.set("HORCH_NO_WATCHDOG", "1");
        h.set(
            "HORCH_QUOTA_FILE",
            fixtures().join("quota/all-ok.json").to_string_lossy(),
        );
        // No test uses the network for the docs sync of fleetd.
        let docs = h.root.join("docs-fixture");
        std::fs::create_dir_all(&docs).unwrap();
        h.set("HORCH_TEST_DOCS_FIXTURE", docs.to_string_lossy());
        // `exec`: each pane of the fake herdr runs its command.
        h.set("HORCH_FAKE_SCENARIO", format!("exec,{scenario}"));
        let label = h
            .project
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        let mut create = std::process::Command::new(h.bin.join("herdr"));
        h.seal(&mut create);
        let o = create
            .args(["workspace", "create", "--label", &label])
            .output()
            .unwrap();
        assert!(o.status.success(), "herdr workspace create: {}", text(&o));

        let tickets = h.project.join("tickets");
        std::fs::create_dir_all(&tickets).unwrap();
        let ticket = tickets.join("01-queue-guide.md");
        std::fs::write(&ticket, TICKET).unwrap();
        let w = World { h };
        w.git(&["add", "-A"]);
        w.git(&["commit", "-q", "-m", "base"]);

        w.h.serve_fleetd();
        let o = w
            .h
            .run(&["idea", "add", "--direct", &ticket.to_string_lossy()]);
        assert!(o.status.success(), "horch idea add --direct: {}", text(&o));
        w
    }

    fn git(&self, args: &[&str]) {
        let o = self
            .h
            .git_cmd(args)
            .current_dir(&self.h.project)
            .output()
            .unwrap();
        assert!(o.status.success(), "git {}: {}", args.join(" "), text(&o));
    }

    /// The rows of `sql`, each row its columns joined with 1 space. No rows
    /// while the ledger does not exist yet.
    fn rows(&self, sql: &str) -> Vec<String> {
        let Ok(conn) = Connection::open_with_flags(
            self.h.ledger_db(),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        ) else {
            return Vec::new();
        };
        let _ = conn.busy_timeout(std::time::Duration::from_secs(5));
        let Ok(mut stmt) = conn.prepare(sql) else {
            return Vec::new();
        };
        let n = stmt.column_count();
        stmt.query_map([], |r| {
            let mut cols = Vec::new();
            for i in 0..n {
                let v: rusqlite::types::Value = r.get(i)?;
                cols.push(match v {
                    rusqlite::types::Value::Null => "-".to_string(),
                    rusqlite::types::Value::Integer(i) => i.to_string(),
                    rusqlite::types::Value::Real(f) => f.to_string(),
                    rusqlite::types::Value::Text(t) => t,
                    rusqlite::types::Value::Blob(_) => "<blob>".to_string(),
                });
            }
            Ok(cols.join(" "))
        })
        .map(|rows| rows.filter_map(Result::ok).collect())
        .unwrap_or_default()
    }

    fn one(&self, sql: &str) -> Option<String> {
        self.rows(sql).into_iter().next()
    }

    /// What a failed wait shows: the features, the ideas, the tasks, the
    /// git refs and the fleetd stderr.
    fn context(&self) -> String {
        let show = |title: &str, sql: &str| format!("{title}:\n  {}\n", self.rows(sql).join("\n  "));
        [
            show(
                "features",
                "SELECT id, IFNULL(branch, '-'), state FROM features",
            ),
            show("ideas", "SELECT id, state FROM ideas"),
            show(
                "tasks",
                "SELECT task_id, kind, status, IFNULL(idea_id, '-') FROM tasks",
            ),
            format!("git refs:\n{}\n", self.refs()),
            format!("fleetd stderr, each line 1 time:\n{}\n", self.fleetd_stderr()),
        ]
        .concat()
    }

    /// The refs of the project repository.
    fn refs(&self) -> String {
        let o = self
            .h
            .git_cmd(&["for-each-ref", "--format=%(refname) %(objectname:short)"])
            .current_dir(&self.h.project)
            .output()
            .unwrap();
        String::from_utf8_lossy(&o.stdout).into_owned()
    }

    /// The lines of the fleetd stderr, each 1 time, in order.
    fn fleetd_stderr(&self) -> String {
        let all = std::fs::read_to_string(self.h.root.join("fleetd.stderr")).unwrap_or_default();
        let mut seen = std::collections::BTreeSet::new();
        all.lines()
            .filter(|l| seen.insert(*l))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Wait for `what`: `f` gives a value.
    fn wait<T>(&self, what: &str, f: impl FnMut() -> Option<T>) -> T {
        wait_for_or(what, f, || self.context())
    }
}

/// Fault 1: fleetd makes the feature branch of `horch idea add --direct`
/// in the project `.git`. The git sandbox of fleetd lets it write the git
/// folder of the project.
#[test]
fn idea_add_direct_makes_the_feature_and_its_branch() {
    let w = World::new("p6-flow-branch", "commit,result");
    let branch = w.wait("the feature row with its branch", || {
        w.one("SELECT branch FROM features WHERE branch IS NOT NULL")
    });
    let name = format!("refs/heads/{branch}");
    w.wait("the feature branch in git", || {
        w.refs()
            .lines()
            .any(|l| l.split(' ').next() == Some(name.as_str()))
            .then_some(())
    });
}
