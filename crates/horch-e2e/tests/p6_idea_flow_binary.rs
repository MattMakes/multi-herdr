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

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use horch_core::taskledger::model::{Actor, TaskKind};
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::{schema, util, Store};
use horch_e2e::harness::{fixtures, wait_for_or, Harness};
use rusqlite::{Connection, OpenFlags};

/// The `fleet.toml` of each test: 1 pass each second.
const FLEET_TOML: &str = "pass_period_s = 1\n";

/// The source of the idea.md of a `--direct` idea.
const DIRECT_SOURCE: &str = "horch idea add --direct";

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
        h.fleet_workspace();
        let w = World { h };
        // Plan validation refuses a plan when the project declares no gates.
        std::fs::write(
            w.h.project.join("fleet.toml"),
            "[gates]\nfast = \"true\"\nfull = \"true\"\n",
        )
        .unwrap();
        w.write_ticket("01-queue-guide.md", TICKET);
        w.git(&["add", "-A"]);
        w.git(&["commit", "-q", "-m", "base"]);

        w.h.serve_fleetd();
        w.add_direct("01-queue-guide.md");
        w
    }

    /// Write the ticket `name` of the project folder `tickets` with `text`.
    fn write_ticket(&self, name: &str, text: &str) {
        let tickets = self.h.project.join("tickets");
        std::fs::create_dir_all(&tickets).unwrap();
        std::fs::write(tickets.join(name), text).unwrap();
    }

    /// `horch idea add --direct` of the ticket `name` of the project.
    fn add_direct(&self, name: &str) {
        let ticket = self.h.project.join("tickets").join(name);
        let o = self
            .h
            .run(&["idea", "add", "--direct", &ticket.to_string_lossy()]);
        assert!(o.status.success(), "horch idea add --direct: {}", text(&o));
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
        let Ok(conn) =
            Connection::open_with_flags(self.h.ledger_db(), OpenFlags::SQLITE_OPEN_READ_ONLY)
        else {
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
        let show =
            |title: &str, sql: &str| format!("{title}:\n  {}\n", self.rows(sql).join("\n  "));
        [
            show(
                "features",
                "SELECT id, IFNULL(branch, '-'), state FROM features",
            ),
            show("ideas", "SELECT id, state FROM ideas"),
            show("needs-you items", "SELECT key, state, text FROM needs_you"),
            show(
                "idea sources",
                "SELECT entity_id, json_extract(data_json, '$.source') FROM events
                 WHERE entity = 'idea' AND kind = 'added'",
            ),
            show(
                "tasks",
                "SELECT task_id, kind, status, IFNULL(idea_id, '-') FROM tasks",
            ),
            format!("plan verdicts:\n{}\n", self.verdicts()),
            format!("git refs:\n{}\n", self.refs()),
            format!(
                "fleetd stderr, each line 1 time:\n{}\n",
                self.fleetd_stderr()
            ),
        ]
        .concat()
    }

    /// The plan.verdict.json of each idea folder that has one.
    fn verdicts(&self) -> String {
        let ideas = self.h.state.join("fleet").join("ideas");
        let mut dirs: Vec<_> = std::fs::read_dir(&ideas)
            .map(|d| d.filter_map(Result::ok).map(|e| e.path()).collect())
            .unwrap_or_default();
        dirs.sort();
        dirs.iter()
            .filter_map(|d| {
                let v = std::fs::read_to_string(d.join("plan.verdict.json")).ok()?;
                Some(format!("  {}: {v}", d.display()))
            })
            .collect::<Vec<_>>()
            .join("\n")
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

/// Ticket 58 of fleetd-build: 10 `--direct` ideas in a row, with 1 pass
/// each second. A pass can come between the commit of a request and its
/// files. Each idea reaches `ticketing` and gets its planned task, no idea
/// is ever in `clarifying`, and fleetd starts no clarifying process.
#[test]
fn ten_direct_ideas_in_a_row_all_reach_ticketing() {
    let w = World::new("p6-flow-ten-direct", "commit,result");
    for n in 2..=10 {
        let name = format!("{n:02}-queue-guide.md");
        let ticket = TICKET.replacen(
            "# 01: Write the queue guide",
            &format!("# {n:02}: Write the queue guide {n}"),
            1,
        );
        w.write_ticket(&name, &ticket);
        w.add_direct(&name);
    }
    w.wait(
        "10 --direct ideas that were in ticketing, each with its task",
        || {
            let ticketed = w.one(
                "SELECT COUNT(DISTINCT e.entity_id) FROM events e
             WHERE e.entity = 'idea' AND e.to_state = 'ticketing'
               AND EXISTS (SELECT 1 FROM tasks t WHERE t.idea_id = e.entity_id)",
            );
            (ticketed.as_deref() == Some("10")).then_some(())
        },
    );
    // fleetd can add other ideas, for example 1 for each agent CLI of the
    // host that fails its conformance check. The checks below read only
    // the `--direct` ideas.
    let direct = format!("SELECT entity_id FROM events WHERE entity = 'idea' AND kind = 'added' AND json_extract(data_json, '$.source') = '{DIRECT_SOURCE}'");
    assert_eq!(w.rows(&direct).len(), 10, "{}", w.context());
    let clarifying = w.rows(&format!(
        "SELECT entity_id FROM events WHERE entity = 'idea' AND to_state = 'clarifying'
           AND entity_id IN ({direct})"
    ));
    assert!(
        clarifying.is_empty(),
        "--direct ideas in clarifying: {clarifying:?}"
    );
    let leads = w.rows(&format!(
        "SELECT id FROM attempts WHERE id LIKE '%/clarifying/%' AND idea_id IN ({direct})"
    ));
    assert!(leads.is_empty(), "clarifying processes: {leads:?}");
}

// ─── fault 3: each fake plays only the keys of its role ─────────────────

/// The verdict file of the fake judge.
const VERDICT_FILE: &str = "review.md";

/// The scenario of each fake of a judge pair: the keys of the worker and
/// of the judge together, as the fleet gives them to each pane.
const PAIR_SCENARIO: &str = "commit,result,verdict_pass";

/// A git project with a ledger that holds 1 judged task and its judge
/// task. A stand-in `horch` before the real one on `PATH` logs the files
/// of HEAD at each `horch result`.
struct Pair {
    h: Harness,
    judged: String,
    judge: String,
    stub_log: PathBuf,
}

impl Pair {
    fn new(name: &str) -> Pair {
        let h = Harness::new(name).with_git();
        let root = h.project.canonicalize().unwrap();
        let mut store = Store::open(&h.ledger_db()).unwrap();
        let (judged, judge) = store
            .immediate(|tx| {
                let run = runs::create(
                    tx,
                    &root,
                    &NewRun {
                        slug: "r1".into(),
                        base_branch: Some("main".into()),
                        ..Default::default()
                    },
                )?;
                schema::run_idea(tx, &run.run_id, &util::now(), true)?;
                let judged = tasks::add(
                    tx,
                    &run,
                    &NewTask::work("Write the guide", "guide.md", "opus"),
                    Actor::Horch,
                )?;
                let judge = tasks::add(
                    tx,
                    &run,
                    &NewTask {
                        kind: TaskKind::Work,
                        parent_task_id: Some(judged.task_id.clone()),
                        ..NewTask::work("Judge: Write the guide", "guide.md", "codex")
                    },
                    Actor::Horch,
                )?;
                Ok((judged.task_id, judge.task_id))
            })
            .unwrap();
        let stub_log = h.root.join("stub-horch.log");
        Pair {
            h,
            judged,
            judge,
            stub_log,
        }
    }

    /// Run fake-claude with [`PAIR_SCENARIO`] as the attempt of `task_id`,
    /// in the project. The stand-in `horch` writes 1 line for each
    /// `horch result`: the files of HEAD then.
    fn launch(&mut self, task_id: &str) {
        let attempt = self.h.root.join(format!("attempt-{task_id}"));
        std::fs::create_dir_all(&attempt).unwrap();
        let task_yaml = attempt.join("task.yaml");
        std::fs::write(
            &task_yaml,
            format!(
                "task_id: {task_id}\nworktree: {}\n",
                self.h.project.display()
            ),
        )
        .unwrap();
        let stub_dir = self.h.root.join("stub-bin");
        std::fs::create_dir_all(&stub_dir).unwrap();
        let stub = stub_dir.join("horch");
        let script = format!(
            "#!/bin/sh\n[ \"$1\" = result ] || exit 0\n\
             printf 'result:' >> '{log}'\n\
             \"$HORCH_GIT_BIN\" ls-tree --name-only HEAD | \
             while read -r f; do printf ' %s' \"$f\"; done >> '{log}'\n\
             echo >> '{log}'\n",
            log = self.stub_log.display()
        );
        std::fs::write(&stub, script).unwrap();
        make_executable(&stub);
        self.h.set("HORCH_TASK_YAML", task_yaml.to_string_lossy());
        self.h.set("HORCH_FAKE_SCENARIO", PAIR_SCENARIO);
        let mut cmd = Command::new(self.h.bin.join("claude"));
        self.h.seal(&mut cmd);
        cmd.env(
            "PATH",
            format!("{}:{}", stub_dir.display(), self.h.bin.display()),
        );
        cmd.current_dir(&self.h.project);
        let o = cmd.output().unwrap();
        assert!(o.status.success(), "fake claude: {}", text(&o));
    }

    /// The files of HEAD of the project.
    fn head_files(&self) -> Vec<String> {
        let o = self
            .h
            .git_cmd(&["ls-tree", "--name-only", "HEAD"])
            .current_dir(&self.h.project)
            .output()
            .unwrap();
        String::from_utf8_lossy(&o.stdout)
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// The lines of the stand-in `horch`.
    fn results(&self) -> Vec<String> {
        std::fs::read_to_string(&self.stub_log)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }
}

fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// Fault 3: the fake worker of a judged task commits its work and says
/// done. It writes no `review.md`: that is the work of the judge.
#[test]
fn fake_worker_writes_no_review_md() {
    let mut p = Pair::new("p6-fake-worker");
    let judged = p.judged.clone();
    p.launch(&judged);
    let files = p.head_files();
    assert!(
        files.iter().any(|f| f == "fake-claude.txt"),
        "the worker commits its work: {files:?}"
    );
    assert!(
        !files.iter().any(|f| f == VERDICT_FILE),
        "the worker writes no {VERDICT_FILE}: {files:?}"
    );
    assert_eq!(p.results().len(), 1, "the worker says done 1 time");
}

/// Fault 3: the fake judge commits `review.md` before `horch result`, and
/// does not do the work of the worker.
#[test]
fn fake_judge_commits_review_md_before_its_result() {
    let mut p = Pair::new("p6-fake-judge");
    let judge = p.judge.clone();
    p.launch(&judge);
    let files = p.head_files();
    assert!(
        files.iter().any(|f| f == VERDICT_FILE),
        "the judge commits {VERDICT_FILE}: {files:?}"
    );
    assert!(
        !files.iter().any(|f| f == "fake-claude.txt"),
        "the judge does not do the work of the worker: {files:?}"
    );
    let results = p.results();
    assert_eq!(results.len(), 1, "the judge says done 1 time: {results:?}");
    assert!(
        results[0].split(' ').any(|f| f == VERDICT_FILE),
        "{VERDICT_FILE} is in HEAD at horch result: {results:?}"
    );
}
