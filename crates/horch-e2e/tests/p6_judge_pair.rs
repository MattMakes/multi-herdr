//! The e2e tests of the judge pair (`docs/specs/fleetd/design.md`: RR-3,
//! FM-9, FM-33, "A cancel"; `docs/tickets/fleetd-p6/59-e2e-judge.md`).
//!
//! Each test makes its own ledger, fleet root and git project in a temp
//! folder, and runs a test `horch fleetd` with the fake herdr and the fake
//! agents. `horch idea add --direct` adds 1 ticket of the judgment kind
//! `docs`, so fleetd plans 1 judged task and its judge task (MAP.md, Q8).
//! fleetd launches each attempt. The fake worker commits 1 file and says
//! done with `horch result`. The fake judge commits `review.md` with the
//! verdict of its scenario key (`verdict_pass`, `verdict_critical`).
//!
//! Each test reads the ledger and waits for an event, never for a time. No
//! test reads the load, the limits or the clock of this machine: the
//! harness gives the machine facts through `HORCH_MACHINE_FILE`.

#![cfg(unix)]

use std::path::PathBuf;
use std::process::Output;

use horch_e2e::harness::{fixtures, wait_for_or, Harness};
use rusqlite::{Connection, OpenFlags};

/// The `fleet.toml` of each test: 1 pass each second.
const FLEET_TOML: &str = "pass_period_s = 1\n";

/// The ticket of each test: 1 task of the judgment kind `docs`.
const TICKET: &str = "# 01: Write the queue guide\n\n\
<!-- horch-ticket: 1 -->\n\
**Persona:** primary `opus` · **Parallel:** yes\n\
**Kind:** docs\n\
**Review:** light\n\
**Check:** `true`\n\
**Own:** `fake-claude.txt`\n\n\
## Goal\n\nThe guide names the queue length.\n";

/// The kind of the needs-you item of a judged task past its fix rounds.
const ESCALATED_KIND: &str = "judge_escalated";

/// The reason of a refused cancel of a judge task.
const JUDGE_CANCEL_REFUSED: &str = "its judge follows";

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// The 2 tasks of the judge pair.
#[derive(Debug, Clone)]
struct Pair {
    judged: String,
    judge: String,
    run: String,
}

/// A fleet with a test fleetd and 1 `--direct` idea of the kind `docs`.
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
        let o = self.h.git_cmd(args).current_dir(&self.h.project).output().unwrap();
        assert!(o.status.success(), "git {}: {}", args.join(" "), text(&o));
    }

    fn ledger(&self) -> PathBuf {
        self.h.ledger_db()
    }

    /// The rows of `sql`, each row its columns joined with 1 space. No rows
    /// while the ledger does not exist yet.
    fn rows(&self, sql: &str) -> Vec<String> {
        let Ok(conn) = Connection::open_with_flags(self.ledger(), OpenFlags::SQLITE_OPEN_READ_ONLY)
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

    /// What a failed wait shows: the tasks, the attempts, the verdicts, the
    /// needs-you items and the task moves.
    fn context(&self) -> String {
        let show = |title: &str, sql: &str| format!("{title}:\n  {}\n", self.rows(sql).join("\n  "));
        [
            show(
                "tasks",
                "SELECT task_id, kind, IFNULL(parent_task_id, '-'), status, \
                 fix_rounds, attempt_floor, IFNULL(idea_id, '-') FROM tasks",
            ),
            show(
                "attempts",
                "SELECT id, IFNULL(task_id, '-'), state, IFNULL(outcome, '-') FROM attempts",
            ),
            show(
                "needs_you",
                "SELECT key, kind, state, text FROM needs_you",
            ),
            show(
                "ideas",
                "SELECT id, state, IFNULL(plan_sha256, '-') FROM ideas",
            ),
            show(
                "task moves",
                "SELECT event_id, entity_id, from_state, to_state, IFNULL(cause, '-') \
                 FROM events WHERE kind = 'transition' AND entity = 'task' ORDER BY event_id",
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

    /// The judge pair that fleetd planned for the idea.
    fn pair(&self) -> Pair {
        self.wait("the judged task and its judge task", || {
            let row = self.one(
                "SELECT j.parent_task_id, j.task_id, j.run_id FROM tasks j \
                 JOIN tasks w ON w.task_id = j.parent_task_id \
                 WHERE j.idea_id IS NOT NULL AND w.idea_id = j.idea_id",
            )?;
            let mut it = row.split(' ').map(str::to_string);
            Some(Pair {
                judged: it.next()?,
                judge: it.next()?,
                run: it.next()?,
            })
        })
    }

    fn status(&self, task: &str) -> String {
        self.one(&format!(
            "SELECT status FROM tasks WHERE task_id = '{task}'"
        ))
        .unwrap_or_default()
    }

    /// Wait until `task` is in `status`.
    fn wait_status(&self, task: &str, status: &str) {
        self.wait(&format!("task {task} in {status}"), || {
            (self.status(task) == status).then_some(())
        });
    }

    /// The event id of the first move of `task` to `to`.
    fn moved_at(&self, task: &str, to: &str) -> Option<i64> {
        self.one(&format!(
            "SELECT MIN(event_id) FROM events WHERE kind = 'transition' \
             AND entity = 'task' AND entity_id = '{task}' AND to_state = '{to}'"
        ))
        .and_then(|v| v.parse().ok())
    }

    /// The attempts of `task` with the outcome `outcome`.
    fn attempts_with(&self, task: &str, outcome: &str) -> usize {
        self.one(&format!(
            "SELECT COUNT(*) FROM attempts WHERE task_id = '{task}' AND outcome = '{outcome}'"
        ))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
    }

    /// The plan of the idea: its plan sha256 and its task ids.
    fn plan(&self) -> (String, Vec<String>) {
        let sha = self
            .one("SELECT IFNULL(plan_sha256, '-') FROM ideas")
            .unwrap_or_default();
        let tasks = self.rows("SELECT task_id FROM tasks WHERE idea_id IS NOT NULL ORDER BY task_id");
        (sha, tasks)
    }

    /// `horch task cancel <task> <reason> --run <run>`, the real client.
    fn cancel(&self, task: &str, run: &str) -> Output {
        self.h
            .run(&["task", "cancel", task, "the test cancels it", "--run", run])
    }
}

/// RR-3: a judgment task and its judge task both reach `done`, and the plan
/// does not change.
#[test]
fn rr3_judgment_and_judge_reach_done_with_no_plan_change() {
    let w = World::new("p6-judge-done", "commit,result,verdict_pass");
    let pair = w.pair();
    let planned = w.plan();
    w.wait_status(&pair.judge, "done");
    w.wait_status(&pair.judged, "done");
    assert_eq!(w.plan(), planned, "the plan does not change\n{}", w.context());
    let fix_rounds = w.one(&format!(
        "SELECT fix_rounds FROM tasks WHERE task_id = '{}'",
        pair.judged
    ));
    assert_eq!(fix_rounds.as_deref(), Some("0"), "{}", w.context());
}

/// RR-3: a CRITICAL item gives 1 fix round. The fix attempt takes no
/// attempt of `max_attempts`, and the judge task waits in `pending`.
#[test]
fn rr3_critical_gives_1_fix_round() {
    let w = World::new("p6-judge-critical", "commit,result,verdict_critical");
    let pair = w.pair();
    let snapshot = |w: &World| {
        w.one(&format!(
            "SELECT t.fix_rounds, t.attempt_floor, \
             (SELECT COUNT(*) FROM attempts a WHERE a.task_id = '{judge}' \
              AND a.outcome = 'critical_open') \
             FROM tasks t WHERE t.task_id = '{judged}'",
            judge = pair.judge,
            judged = pair.judged,
        ))
    };
    let first = w.wait("the first fix round", || {
        let s = snapshot(&w)?;
        (s.split(' ').next() != Some("0")).then_some(s)
    });
    let cols: Vec<&str> = first.split(' ').collect();
    let (rounds, floor, critical) = (cols[0], cols[1], cols[2]);
    assert_eq!(
        rounds, critical,
        "each critical_open judge attempt gives 1 fix round\n{}",
        w.context()
    );
    assert_eq!(
        floor, rounds,
        "a fix attempt takes no attempt of max_attempts\n{}",
        w.context()
    );
    assert!(
        w.moved_at(&pair.judged, "ready").is_some() && w.moved_at(&pair.judge, "pending").is_some(),
        "the judged task gets a fix attempt and the judge task waits\n{}",
        w.context()
    );
    assert_ne!(w.status(&pair.judged), "done", "{}", w.context());
    assert_ne!(w.status(&pair.judge), "done", "{}", w.context());
}

/// FM-9: a judgment task is `done` only after its judge task is `done`, and
/// it waits in `verifying` while its judge runs.
#[test]
fn fm9_judgment_task_is_done_only_after_its_judge() {
    let w = World::new("p6-judge-fm9", "commit,result,verdict_pass");
    let pair = w.pair();
    w.wait_status(&pair.judged, "done");
    let judged_done = w.moved_at(&pair.judged, "done").unwrap();
    let judge_done = w.moved_at(&pair.judge, "done");
    assert!(
        judge_done.is_some_and(|j| j < judged_done),
        "the judge task is done before the judged task\n{}",
        w.context()
    );
    let verifying = w.moved_at(&pair.judged, "verifying").unwrap();
    let judge_attempt = w.one(&format!(
        "SELECT MIN(event_id) FROM events WHERE kind = 'transition' AND entity = 'attempt' \
         AND entity_id IN (SELECT id FROM attempts WHERE task_id = '{}')",
        pair.judge
    ));
    let judge_attempt: i64 = judge_attempt.and_then(|v| v.parse().ok()).unwrap_or(i64::MAX);
    assert!(
        verifying < judge_attempt,
        "the judge runs only after the judged task is in verifying\n{}",
        w.context()
    );
    let moves = w.rows(&format!(
        "SELECT from_state || '>' || to_state FROM events WHERE kind = 'transition' \
         AND entity = 'task' AND entity_id = '{}' AND event_id > {verifying} \
         AND event_id < {judged_done}",
        pair.judged
    ));
    assert!(
        moves.is_empty(),
        "the judged task waits in verifying until it is done: {moves:?}\n{}",
        w.context()
    );
}

/// FM-33: after 2 fix rounds with an open CRITICAL item, both tasks are
/// `escalated` with 1 needs-you item, and neither one is `done`.
#[test]
fn fm33_two_fix_rounds_escalate() {
    let w = World::new("p6-judge-fm33", "commit,result,verdict_critical");
    let pair = w.pair();
    w.wait_status(&pair.judged, "escalated");
    w.wait_status(&pair.judge, "escalated");
    let rounds = w.one(&format!(
        "SELECT fix_rounds FROM tasks WHERE task_id = '{}'",
        pair.judged
    ));
    assert_eq!(rounds.as_deref(), Some("2"), "{}", w.context());
    assert_eq!(
        w.attempts_with(&pair.judge, "critical_open"),
        3,
        "3 judge attempts with an open CRITICAL item\n{}",
        w.context()
    );
    let items = w.rows(&format!(
        "SELECT key FROM needs_you WHERE state = 'open' AND kind = '{ESCALATED_KIND}'"
    ));
    assert_eq!(
        items,
        vec![format!("{ESCALATED_KIND}:{}", pair.judged)],
        "1 needs-you item\n{}",
        w.context()
    );
    assert!(
        w.moved_at(&pair.judged, "done").is_none() && w.moved_at(&pair.judge, "done").is_none(),
        "neither task was done\n{}",
        w.context()
    );
}

/// "A cancel": the cancel of a judged task cancels its judge task.
#[test]
fn cancel_of_a_judged_task_cancels_its_judge_task() {
    // `stay`: the fake worker stays, so the judged task is not done first.
    let w = World::new("p6-judge-cancel", "stay");
    let pair = w.pair();
    let o = w.cancel(&pair.judged, &pair.run);
    assert!(o.status.success(), "horch task cancel: {}", text(&o));
    w.wait_status(&pair.judged, "cancelled");
    w.wait_status(&pair.judge, "cancelled");
}

/// "A cancel": the cancel of a judge task alone is refused, and both tasks
/// stay.
#[test]
fn cancel_of_a_judge_task_alone_is_refused() {
    let w = World::new("p6-judge-refused", "stay");
    let pair = w.pair();
    let o = w.cancel(&pair.judge, &pair.run);
    let said = text(&o);
    assert!(!o.status.success(), "the cancel is refused: {said}");
    assert!(
        said.contains(JUDGE_CANCEL_REFUSED) && said.contains(&pair.judged),
        "the refusal names the judged task: {said}"
    );
    assert_ne!(w.status(&pair.judge), "cancelled", "{}", w.context());
    assert_ne!(w.status(&pair.judged), "cancelled", "{}", w.context());
    assert!(
        w.one(&format!(
            "SELECT 1 FROM tasks WHERE task_id = '{}' AND cancel_requested IS NOT NULL",
            pair.judge
        ))
        .is_none(),
        "no cancel is recorded on the judge task\n{}",
        w.context()
    );
}
