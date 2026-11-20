//! The e2e tests of questions, replans and the needs-you list
//! (`docs/specs/fleetd/design.md`: RR-12, OR-3, FM-21, FM-35;
//! `docs/tickets/fleetd-p6/60-e2e-questions.md`).
//!
//! Each test makes its own ledger, fleet root and git project in a temp
//! folder, and runs a test `horch fleetd` with the fake herdr and the fake
//! agents. `horch idea add --direct` adds 1 ticket, so the idea `I-0001` has
//! its folder, its feature and 1 planned task, which fleetd dispatches to
//! the fake claude.
//!
//! - A question comes from a real ask: the fake worker runs `horch tell
//!   orchestrator "QUESTION: ..."` (key `ask`). The sub-orchestrator of the
//!   feature is the fake idea-lead; key `answer_own` answers the oldest open
//!   question of the feature. When a test plays the sub-orchestrator, it
//!   runs the same `horch idea note --question` in the idea folder.
//! - A task fails when each attempt ends with no result: the fake exits.
//!
//! No test reads the load, the limits or the clock of this machine: the
//! harness gives the machine facts through `HORCH_MACHINE_FILE`, the test
//! `fleet.toml` gives the keys, and the test moves the fleet clock in place
//! of a wait for it, at most 1 time for each pass of fleetd. Each wait is
//! for an event of the ledger, never for a time. `HORCH_TEST_DOCS_FIXTURE`
//! keeps the docs sync off the network.

#![cfg(unix)]

use std::cell::RefCell;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::process::Output;

use horch_core::fleetd::ideas::application::question_flow::NEEDS_YOU_KIND as QUESTION_KIND;
use horch_core::fleetd::task_execution::application::planned_tasks;
use horch_core::taskledger::tasks::TASK_ESCALATED;
use horch_e2e::harness::{fixtures, wait_for_or, Harness};
use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

/// The `fleet.toml` of each test: 1 pass each second.
const FLEET_TOML: &str = "pass_period_s = 1\n";

/// The ticket of each test: 1 task of the code kind for the fake claude.
const TICKET: &str = "# 01: Write the queue guide\n\n\
<!-- horch-ticket: 1 -->\n\
**Persona:** primary `opus` · **Parallel:** yes\n\
**Kind:** feature\n\
**Review:** none\n\
**Check:** `true`\n\
**Own:** `fake-claude.txt`\n\n\
## Goal\n\nThe guide names the queue length.\n";

/// The idea of `horch idea add --direct` in each test.
const IDEA: &str = "I-0001";

/// `question_limit_s` of the design: 15 min with no answer from a running
/// owner.
const QUESTION_LIMIT_S: i64 = 900;

/// The move of the fleet clock in 1 pass while a test waits for a task to
/// use its attempts: more than the lease of an attempt (120 s) and than
/// each backoff of rung 6 (30 s, 120 s), less than `replan_limit_s`.
const STEP_S: i64 = 500;

/// The most attempts of a task that [`World::drive`] waits through.
const MAX_ATTEMPTS_SEEN: usize = 20;

/// The passes that a test watches after an event, to see that nothing more
/// comes.
const QUIET_PASSES: usize = 3;

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// SQL text of `s`, in single quotes.
fn quoted(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// A fleet with a test fleetd and 1 `--direct` idea.
struct World {
    h: Harness,
    /// The last pass that moved the fleet clock: `fleetd.clock_ticked_at`.
    moved_at: RefCell<Option<String>>,
}

impl World {
    /// The fleet, with `scenario` as the keys of each fake agent.
    fn new(name: &str, scenario: &str) -> World {
        let mut h = Harness::new(name).with_git();
        // The ledger and the fleet root are outside the state folder, so the
        // test can move the fleet clock of `<root>/ledger.sqlite3`. The
        // launch gives each pane the ledger and the fleet root.
        let fleet = h.root.join("fleet");
        std::fs::create_dir_all(&fleet).unwrap();
        std::fs::write(fleet.join("fleet.toml"), FLEET_TOML).unwrap();
        h.set(
            "HORCH_LEDGER_DB",
            h.root.join("ledger.sqlite3").to_string_lossy(),
        );
        h.set("FLEET_ROOT", fleet.to_string_lossy());
        h.set("HORCH_NO_WAKE", "1");
        h.set("HORCH_NO_WATCHDOG", "1");
        h.set(
            "HORCH_QUOTA_FILE",
            fixtures().join("quota/all-ok.json").to_string_lossy(),
        );
        let docs = h.root.join("docs-fixture");
        std::fs::create_dir_all(&docs).unwrap();
        h.set("HORCH_TEST_DOCS_FIXTURE", docs.to_string_lossy());
        // `exec`: each pane of the fake herdr runs its command.
        h.set("HORCH_FAKE_SCENARIO", format!("exec,{scenario}"));
        h.fleet_workspace();
        let w = World {
            h,
            moved_at: RefCell::new(None),
        };
        // Plan validation refuses a plan when the project declares no gates.
        std::fs::write(
            w.h.project.join("fleet.toml"),
            "[gates]\nfast = \"true\"\nfull = \"true\"\n",
        )
        .unwrap();
        let tickets = w.h.project.join("tickets");
        std::fs::create_dir_all(&tickets).unwrap();
        let ticket = tickets.join("01-queue-guide.md");
        std::fs::write(&ticket, TICKET).unwrap();
        w.git(&["add", "-A"]);
        w.git(&["commit", "-q", "-m", "base"]);
        w.h.serve_fleetd();
        let o =
            w.h.run(&["idea", "add", "--direct", &ticket.to_string_lossy()]);
        assert!(o.status.success(), "horch idea add --direct: {}", text(&o));
        w
    }

    fn git(&self, args: &[&str]) {
        let o = self.h.git_cmd(args).output().unwrap();
        assert!(o.status.success(), "git {}: {}", args.join(" "), text(&o));
    }

    /// The folder of the idea [`IDEA`].
    fn idea_folder(&self) -> PathBuf {
        self.h.fleet_root().join("ideas").join(IDEA)
    }

    fn conn(&self) -> Connection {
        Connection::open_with_flags(self.h.ledger_db(), OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
    }

    /// Rows of 1 column of the ledger, as text.
    fn rows(&self, sql: &str) -> Vec<String> {
        let conn = self.conn();
        let mut stmt = conn.prepare(sql).unwrap();
        stmt.query_map([], |r| r.get::<_, SqlValue>(0))
            .unwrap()
            .map(|r| match r.unwrap() {
                SqlValue::Null => String::new(),
                SqlValue::Integer(n) => n.to_string(),
                SqlValue::Real(x) => x.to_string(),
                SqlValue::Text(t) => t,
                SqlValue::Blob(b) => String::from_utf8_lossy(&b).into_owned(),
            })
            .collect()
    }

    /// The first row of [`World::rows`].
    fn one(&self, sql: &str) -> Option<String> {
        self.rows(sql).into_iter().next()
    }

    /// The 1 planned task of the idea.
    fn task(&self) -> String {
        self.wait("the planned task of the idea", || {
            self.one(&format!(
                "SELECT task_id FROM tasks WHERE idea_id = {}",
                quoted(IDEA)
            ))
        })
    }

    fn status(&self, task: &str) -> String {
        self.one(&format!(
            "SELECT status FROM tasks WHERE task_id = {}",
            quoted(task)
        ))
        .unwrap_or_default()
    }

    fn replans(&self, task: &str) -> Option<String> {
        self.one(&format!(
            "SELECT replans FROM tasks WHERE task_id = {}",
            quoted(task)
        ))
    }

    /// The key of the task in the committed plan of its idea: the name of
    /// its replan file.
    fn plan_key(&self, task: &str) -> String {
        let committed = planned_tasks::committed_task(&self.conn(), task)
            .unwrap()
            .expect("the task of a committed plan");
        let spec: Value = serde_json::from_str(&committed.spec_json).unwrap();
        spec["key"]
            .as_str()
            .expect("the key of the task")
            .to_string()
    }

    /// The mark of the last pass of fleetd.
    fn pass_mark(&self) -> Option<String> {
        self.one("SELECT clock_ticked_at FROM fleetd WHERE id = 1")
    }

    /// Wait until fleetd ran `n` more passes.
    fn passes(&self, n: usize) {
        for _ in 0..n {
            let before = self.pass_mark();
            self.wait("1 more pass of fleetd", || {
                (self.pass_mark() != before).then_some(())
            });
        }
    }

    /// Move the fleet clock by `seconds`, at most 1 time for each pass.
    fn step_clock(&self, seconds: i64) {
        let mark = self.pass_mark();
        if mark.is_some() && *self.moved_at.borrow() != mark {
            self.h.advance_fleet_clock(seconds);
            *self.moved_at.borrow_mut() = mark;
        }
    }

    /// Wait for `f`, and move the fleet clock by [`STEP_S`] in each pass
    /// until `f` gives a value. The clock moves only while no attempt of a
    /// task is in `launching`: the move stands in for the lease of a silent
    /// attempt and for the backoff, never for the time of a launch. Each
    /// new attempt of a task starts the time limit of the wait again, so a
    /// slow host only makes the chain of attempts slower.
    fn drive<T>(&self, what: &str, mut f: impl FnMut() -> Option<T>) -> T {
        let count = || {
            self.rows("SELECT id FROM attempts WHERE task_id IS NOT NULL")
                .len()
        };
        for _ in 0..MAX_ATTEMPTS_SEEN {
            let seen = count();
            let step = self.wait(what, || {
                if let Some(v) = f() {
                    return Some(Some(v));
                }
                if count() != seen {
                    return Some(None);
                }
                let busy = self.one(
                    "SELECT id FROM attempts WHERE task_id IS NOT NULL AND state = 'launching'",
                );
                if busy.is_none() {
                    self.step_clock(STEP_S);
                }
                None
            });
            if let Some(v) = step {
                return v;
            }
        }
        panic!(
            "{what}: no end after {MAX_ATTEMPTS_SEEN} attempts\n{}",
            self.context()
        );
    }

    /// The sub-orchestrator attempts of the supervising phase, oldest
    /// first: (id, state, pid).
    fn owners(&self) -> Vec<(String, String, Option<i64>)> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare(
                "SELECT id, state, pid FROM attempts
                 WHERE task_id IS NULL AND id LIKE '%/supervising/%' ORDER BY rowid",
            )
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    }

    /// The live sub-orchestrator with its process: (attempt, pid).
    fn live_owner(&self) -> Option<(String, i64)> {
        self.owners()
            .into_iter()
            .find_map(|(id, state, pid)| (state == "live").then_some((id, pid?)))
    }

    /// The attempts that a `doorbell` event rang for the question `q`.
    fn rung_for(&self, q: &str) -> Vec<String> {
        self.rows(&format!(
            "SELECT entity_id FROM events WHERE kind = 'doorbell' AND entity = 'attempt'
             AND data_json LIKE {} ORDER BY event_id",
            quoted(&format!("%\"{q}\"%"))
        ))
    }

    fn question_state(&self, q: &str) -> String {
        self.one(&format!(
            "SELECT state FROM questions WHERE id = {}",
            quoted(q)
        ))
        .unwrap_or_default()
    }

    /// The 1 question of the worker of the task, from its real ask.
    fn asked(&self, task: &str) -> String {
        self.wait("the question of the worker", || {
            self.one(&format!(
                "SELECT id FROM questions WHERE task_id = {} AND state = 'open'",
                quoted(task)
            ))
        })
    }

    /// The rows of the needs-you item `key`: (state) for each row.
    fn items(&self, key: &str) -> Vec<String> {
        self.rows(&format!(
            "SELECT state FROM needs_you WHERE key = {}",
            quoted(key)
        ))
    }

    /// The files of `<fleet root>/needs-you/` of the items of kind `kind`
    /// (the file name starts with `<kind>-`) whose text names `id`.
    fn needs_you_files(&self, kind: &str, id: &str) -> Vec<PathBuf> {
        let dir = self.h.fleet_root().join("needs-you");
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Vec::new();
        };
        let prefix = format!("{kind}-");
        let mut out: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().starts_with(&prefix))
            .map(|e| e.path())
            .filter(|p| {
                std::fs::read_to_string(p)
                    .map(|t| t.contains(id))
                    .unwrap_or(false)
            })
            .collect();
        out.sort();
        out
    }

    /// The keys of the "Needs you" list of `horch ideas --json`.
    fn board_keys(&self) -> Vec<String> {
        let o = self.h.run(&["ideas", "--json"]);
        assert!(o.status.success(), "horch ideas --json: {}", text(&o));
        let board: Value = serde_json::from_slice(&o.stdout).unwrap();
        board["needs_you"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|n| n["key"].as_str().map(str::to_string))
            .collect()
    }

    /// The herdr notifications of the needs-you list: each `blocked`
    /// report on the pane `fleet-needs-you`.
    fn notifications(&self) -> usize {
        self.h
            .calls_of("herdr")
            .iter()
            .filter(|c| {
                let argv: Vec<&str> = c["argv"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect();
                argv.starts_with(&["pane", "report-agent", "fleet-needs-you"])
                    && argv.windows(2).any(|p| p == ["--state", "blocked"])
            })
            .count()
    }

    /// The answer of the sub-orchestrator to the question `q`, case
    /// `own_decision`, as the fake idea-lead gives it.
    fn answer_as_owner(&self, q: &str) {
        let o = self
            .h
            .horch(&[
                "idea",
                "note",
                IDEA,
                "--question",
                q,
                "--case",
                "own_decision",
                "The guide uses the queue length of the status bar.",
            ])
            .current_dir(self.idea_folder())
            .output()
            .unwrap();
        assert!(
            o.status.success(),
            "horch idea note --question: {}",
            text(&o)
        );
    }

    /// Drive the task to `escalated`: its attempts end with no result, the
    /// task fails, the test writes its 1 replan as the sub-orchestrator,
    /// and the task fails again. Each step asserts its fact on the way.
    fn escalate(&self, task: &str) {
        self.drive("the task fails after its attempts", || {
            (self.status(task) == "failed").then_some(())
        });
        let used = self.attempts_of(task);
        assert!(
            used >= 1,
            "a failed task used its attempts\n{}",
            self.context()
        );
        assert_eq!(self.replans(task).as_deref(), Some("0"));
        assert!(
            self.items(&escalated_key(task)).is_empty(),
            "a failed task waits for its replan, not for you\n{}",
            self.context()
        );

        // The sub-orchestrator replans the failed task 1 time: a retry.
        let key = self.plan_key(task);
        let folder = self.idea_folder();
        let tmp = folder.join(format!("replan-{key}.yaml.tmp"));
        // A file of the sub-orchestrator has mode 0600 (SR-6).
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)
            .and_then(|mut f| f.write_all(b"retry: the ticket of the task is fixed\n"))
            .unwrap();
        std::fs::rename(&tmp, folder.join(format!("replan-{key}.yaml"))).unwrap();
        self.wait("the replan of the failed task", || {
            (self.replans(task).as_deref() == Some("1")).then_some(())
        });
        let verdict = folder.join(format!("replan-{key}.verdict.json"));
        let verdict: Value = self.wait("the verdict of the replan", || {
            serde_json::from_str(&std::fs::read_to_string(&verdict).ok()?).ok()
        });
        assert_eq!(verdict["valid"], true, "{verdict}");

        // The retry starts the attempts again; the second failure is the
        // end: escalated, and no second replan.
        self.drive("the task is escalated after its replan", || {
            (self.status(task) == "escalated").then_some(())
        });
        assert_eq!(self.replans(task).as_deref(), Some("1"));
        let again = self.attempts_of(task) - used;
        assert_eq!(
            again,
            used,
            "the retry gives the task its attempts again, and no more\n{}",
            self.context()
        );
    }

    /// The count of the attempts of the task.
    fn attempts_of(&self, task: &str) -> usize {
        self.rows(&format!(
            "SELECT id FROM attempts WHERE task_id = {}",
            quoted(task)
        ))
        .len()
    }

    /// What a failed wait shows.
    fn context(&self) -> String {
        let show =
            |title: &str, sql: &str| format!("{title}:\n  {}\n", self.rows(sql).join("\n  "));
        let mut calls: Vec<String> = self
            .h
            .calls()
            .iter()
            .map(|c| format!("{} {}", c["fake"], c["argv"]))
            .map(|c| c.chars().take(300).collect())
            .collect();
        calls.dedup();
        let cut = calls.len().saturating_sub(40);
        calls.drain(..cut);
        let stderr = std::fs::read_to_string(self.h.root.join("fleetd.stderr")).unwrap_or_default();
        let mut tail: Vec<&str> = stderr.lines().rev().take(40).collect();
        tail.reverse();
        let folder: Vec<String> = std::fs::read_dir(self.idea_folder())
            .map(|d| {
                d.filter_map(Result::ok)
                    .map(|e| {
                        let name = e.file_name().to_string_lossy().into_owned();
                        match name.ends_with(".verdict.json") {
                            true => format!(
                                "{name}: {}",
                                std::fs::read_to_string(e.path()).unwrap_or_default()
                            ),
                            false => name,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        let mirror: Vec<String> = std::fs::read_dir(self.h.fleet_root().join("needs-you"))
            .map(|d| {
                d.filter_map(Result::ok)
                    .map(|e| {
                        let text = std::fs::read_to_string(e.path()).unwrap_or_default();
                        format!(
                            "{}: {}",
                            e.file_name().to_string_lossy(),
                            text.replace('\n', " | ")
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        format!(
            "needs-you/:\n  {}\nidea folder:\n  {}\n{}{}{}{}{}{}calls:\n{}\nfleetd.stderr:\n{}\n",
            mirror.join("\n  "),
            folder.join("\n  "),
            show(
                "ideas",
                "SELECT id || ' ' || state || ' ' || IFNULL(feature_id, '-') FROM ideas"
            ),
            show(
                "tasks",
                "SELECT task_id || ' ' || status || ' replans ' || replans FROM tasks"
            ),
            show(
                "attempts",
                "SELECT id || ' ' || state || ' ' || IFNULL(outcome, '-') FROM attempts"
            ),
            show(
                "questions",
                "SELECT id || ' ' || state || ' ' || owner || ' due ' || due_at FROM questions"
            ),
            show(
                "needs-you",
                "SELECT key || ' ' || state || ' ' || text FROM needs_you"
            ),
            show(
                "the events of the tasks, the fleet clock and the last jobs",
                "SELECT * FROM (SELECT IFNULL(entity, '-') || ' ' || IFNULL(entity_id, '-') || ' ' || kind \
                 || ' ' || IFNULL(to_state, '-') || ' ' || text FROM events e WHERE EXISTS (SELECT 1 FROM tasks t \
                 WHERE e.entity_id = t.task_id OR e.entity_id LIKE t.task_id || '/%') \
                 ORDER BY event_id DESC LIMIT 40) \
                 UNION ALL SELECT 'clock ' || fleet_clock_s FROM fleetd \
                 UNION ALL SELECT * FROM (SELECT 'job ' || id || ' ' || kind || ' ' || state \
                 FROM jobs ORDER BY id DESC LIMIT 6)"
            ),
            calls.join("\n"),
            tail.join("\n"),
        )
    }

    /// Wait for `f` to give a value, with [`World::context`] on a failure.
    fn wait<T>(&self, what: &str, f: impl FnMut() -> Option<T>) -> T {
        wait_for_or(what, f, || self.context())
    }
}

/// The key of the needs-you item of the escalated task `task`.
fn escalated_key(task: &str) -> String {
    horch_core::taskledger::tasks::escalated_key(task)
}

/// The key of the needs-you item of the escalated question `q`.
fn question_key(q: &str) -> String {
    horch_core::fleetd::ideas::application::question_flow::needs_you_key(q)
}

/// RR-12: a question whose owner is not running is answered after fleetd
/// starts it.
#[test]
fn rr12_question_owner_is_started_and_answers() {
    let w = World::new("p6-q-started", "stay,ask,answer_own");
    let task = w.task();
    let q = w.asked(&task);
    w.wait("the answered question", || {
        (w.question_state(&q) == "answered").then_some(())
    });
    // The owner is the sub-orchestrator of the feature, and fleetd started
    // it for the question: it answered with its own decision.
    let feature = w
        .one(&format!(
            "SELECT feature_id FROM ideas WHERE id = {}",
            quoted(IDEA)
        ))
        .unwrap();
    let owner = w
        .one(&format!(
            "SELECT owner FROM questions WHERE id = {}",
            quoted(&q)
        ))
        .unwrap();
    assert_eq!(owner, format!("idea-lead:{feature}"));
    assert!(
        !w.owners().is_empty(),
        "fleetd started no sub-orchestrator\n{}",
        w.context()
    );
    let case = w
        .one(&format!(
            "SELECT answer_case FROM questions WHERE id = {}",
            quoted(&q)
        ))
        .unwrap_or_default();
    assert!(
        case.is_empty() || case == "own_decision",
        "the case of the answer: {case}"
    );
    // The task waits no more for the answer.
    w.wait("the task leaves blocked_on_user", || {
        (w.status(&task) != "blocked_on_user").then_some(())
    });
    assert!(w.items(&question_key(&q)).is_empty(), "no needs-you item");
}

/// RR-12: an unanswered question reaches `needs-you/` at its limit, 15 min
/// of the fleet clock with no answer from a running owner. The wait for you
/// has no limit.
#[test]
fn rr12_unanswered_question_reaches_needs_you_at_its_limit() {
    let w = World::new("p6-q-limit", "stay,ask");
    let task = w.task();
    let q = w.asked(&task);
    w.wait("a live sub-orchestrator that owns the question", || {
        w.live_owner()
    });
    w.wait("the doorbell of the question", || {
        (!w.rung_for(&q).is_empty()).then_some(())
    });
    assert_eq!(w.question_state(&q), "open");
    assert!(w.items(&question_key(&q)).is_empty());

    // The owner runs and does not answer: at the limit the question goes to
    // you.
    w.h.advance_fleet_clock(QUESTION_LIMIT_S);
    w.wait("the escalated question", || {
        (w.question_state(&q) == "escalated").then_some(())
    });
    assert_eq!(w.items(&question_key(&q)), ["open"]);
    w.wait("the file of the question in needs-you/", || {
        (!w.needs_you_files(QUESTION_KIND, &q).is_empty()).then_some(())
    });
    assert!(
        w.board_keys().contains(&question_key(&q)),
        "horch ideas shows the question\n{}",
        w.context()
    );
    w.wait("the herdr notification", || {
        (w.notifications() > 0).then_some(())
    });

    // The wait for you has no limit: 10 more limits of the fleet clock
    // change nothing.
    for _ in 0..10 {
        w.passes(1);
        w.h.advance_fleet_clock(QUESTION_LIMIT_S);
    }
    w.passes(1);
    assert_eq!(w.question_state(&q), "escalated");
    assert_eq!(w.status(&task), "blocked_on_user");
    assert_eq!(w.items(&question_key(&q)), ["open"]);
}

/// OR-3: the same escalation, seen on each pass after it, gives 1 item: 1
/// row, 1 file in `needs-you/`, 1 line of `horch ideas`, and a herdr
/// notification. Its item is `resolved` when its cause ends: your requeue
/// of the escalated task.
#[test]
fn or3_same_escalation_gives_1_item_and_it_resolves() {
    let w = World::new("p6-q-or3", "");
    let task = w.task();
    w.escalate(&task);
    let key = escalated_key(&task);
    let file = w.wait("the file of the escalated task in needs-you/", || {
        let files = w.needs_you_files(TASK_ESCALATED, &task);
        (!files.is_empty()).then_some(files)
    });
    w.wait("the herdr notification", || {
        (w.notifications() > 0).then_some(())
    });

    // More passes, with the task still escalated: still 1 item.
    for _ in 0..QUIET_PASSES {
        w.passes(1);
        w.step_clock(STEP_S);
    }
    w.passes(1);
    assert_eq!(w.status(&task), "escalated");
    assert_eq!(w.items(&key), ["open"], "1 row for the key");
    assert_eq!(
        w.needs_you_files(TASK_ESCALATED, &task),
        file,
        "1 file for the item"
    );
    let shown = w.board_keys().iter().filter(|k| **k == key).count();
    assert_eq!(shown, 1, "horch ideas shows the item 1 time");

    // You requeue the task: the cause ends, and so does the item. The run
    // of the task is the run of its feature (Q5); the fleet can have more
    // than 1 feature, so the command names it.
    let feature = w
        .one(&format!(
            "SELECT feature_id FROM ideas WHERE id = {}",
            quoted(IDEA)
        ))
        .unwrap();
    let o = w.h.run(&[
        "task",
        "requeue",
        "--run",
        &feature,
        &task,
        "the ticket of the task is fixed",
    ]);
    assert!(o.status.success(), "horch task requeue: {}", text(&o));
    w.wait("the resolved item", || {
        (w.items(&key) == ["resolved"]).then_some(())
    });
    w.wait("the file of the item is removed", || {
        w.needs_you_files(TASK_ESCALATED, &task)
            .is_empty()
            .then_some(())
    });
    assert!(
        !w.board_keys().contains(&key),
        "horch ideas shows only open items\n{}",
        w.context()
    );
    assert_ne!(w.status(&task), "escalated");
}

/// FM-21: bounded attempts and backoff, 1 replan, then escalation with 1
/// needs-you item.
#[test]
fn fm21_one_replan_then_escalation() {
    let w = World::new("p6-q-fm21", "");
    let task = w.task();
    w.escalate(&task);
    assert_eq!(w.items(&escalated_key(&task)), ["open"]);

    // Escalated is the end of the retries: the clock moves on and no new
    // attempt starts.
    let attempts = w.attempts_of(&task);
    for _ in 0..QUIET_PASSES {
        w.passes(1);
        w.step_clock(STEP_S);
    }
    w.passes(1);
    assert_eq!(w.status(&task), "escalated");
    assert_eq!(
        w.attempts_of(&task),
        attempts,
        "no attempt after escalation"
    );
    assert_eq!(w.replans(&task).as_deref(), Some("1"));
}

/// FM-35: fleetd repeats the delivery of a question to each new process of
/// the owner until an answer is recorded, and stops after it.
#[test]
fn fm35_delivery_repeats_until_an_answer() {
    let w = World::new("p6-q-fm35", "stay,ask");
    let task = w.task();
    let q = w.asked(&task);
    let (first, pid) = w.wait("a live sub-orchestrator", || w.live_owner());
    w.wait("the doorbell of the first process", || {
        w.rung_for(&q).contains(&first).then_some(())
    });

    // The first process dies with no answer. fleetd starts a new one and
    // rings it for the same question.
    let pid = libc::pid_t::try_from(pid).unwrap();
    // SAFETY: SIGKILL to the 1 recorded process of the attempt.
    assert_eq!(unsafe { libc::kill(pid, libc::SIGKILL) }, 0);
    let (second, _) = w.wait("a second live sub-orchestrator", || {
        w.live_owner().filter(|(a, _)| *a != first)
    });
    w.wait("the doorbell of the second process", || {
        w.rung_for(&q).contains(&second).then_some(())
    });
    assert_eq!(w.question_state(&q), "open");

    // The owner answers: the delivery ends.
    w.answer_as_owner(&q);
    w.wait("the answered question", || {
        (w.question_state(&q) == "answered").then_some(())
    });
    let rung = w.rung_for(&q);
    let owners = w.owners().len();
    for _ in 0..QUIET_PASSES {
        w.passes(1);
    }
    assert_eq!(w.rung_for(&q), rung, "no doorbell after the answer");
    assert_eq!(w.owners().len(), owners, "no new process for the question");
}

/// The owner-note test moved from T-897. It runs a push worker that asks 1
/// question through `horch ask`, with the setup of `p6_worker_question.rs`.
mod owner_note {
    use std::path::{Path, PathBuf};

    use horch_e2e::harness::{fixtures, wait_for_or, Harness};
    use rusqlite::types::Value as SqlValue;
    use rusqlite::{Connection, OpenFlags};

    use super::{quoted, text, FLEET_TOML};

    const TICKET: &str = "# 01: Write the port guide\n\n\
<!-- horch-ticket: 1 -->\n\
**Persona:** primary `opus` · **Parallel:** yes\n\
**Kind:** feature\n\
**Review:** none\n\
**Check:** `true`\n\
**Own:** `fake-claude.txt`\n\n\
## Goal\n\nThe guide names the port.\n";

    const IDEA: &str = "I-0001";

    const QUESTION: &str = "Which port does the guide name?";

    /// The log of the first launch of the worker, under the harness root.
    const FIRST_LOG: &str = "first-launch.log";

    fn shell_word(s: &str) -> String {
        format!("'{}'", s.replace('\'', "'\\''"))
    }

    /// A claude wrapper: the first worker launch runs `first`, and each
    /// other launch runs the fake claude.
    fn wrapper(root: &Path, first: &str) -> String {
        let fake = Harness::horch_bin().with_file_name("fake-claude");
        format!(
            "#!/bin/sh\n[ \"$1\" = warm ] && exit 0\n\
             case \"$*\" in\n*horch-conformance*) ;;\n*/inbox/START.md*)\n\
             if [ ! -e {log} ]; then {{ {first}\n}} > {log} 2>&1; exit 0; fi ;;\nesac\n\
             exec {fake} \"$@\"\n",
            log = shell_word(&root.join(FIRST_LOG).to_string_lossy()),
            fake = shell_word(&fake.to_string_lossy()),
        )
    }

    struct World {
        h: Harness,
    }

    impl World {
        fn new(name: &str, scenario: &str, first: &str) -> World {
            let mut h = Harness::new(name).with_git();
            let fleet = h.root.join("fleet");
            std::fs::create_dir_all(&fleet).unwrap();
            std::fs::write(fleet.join("fleet.toml"), FLEET_TOML).unwrap();
            h.set(
                "HORCH_LEDGER_DB",
                h.root.join("ledger.sqlite3").to_string_lossy(),
            );
            h.set("FLEET_ROOT", fleet.to_string_lossy());
            h.set("HORCH_NO_WAKE", "1");
            h.set("HORCH_NO_WATCHDOG", "1");
            h.set(
                "HORCH_QUOTA_FILE",
                fixtures().join("quota/all-ok.json").to_string_lossy(),
            );
            let docs = h.root.join("docs-fixture");
            std::fs::create_dir_all(&docs).unwrap();
            h.set("HORCH_TEST_DOCS_FIXTURE", docs.to_string_lossy());
            h.set("HORCH_FAKE_SCENARIO", format!("exec,{scenario}"));
            let horch = h.bin.join("horch");
            if !horch.exists() {
                std::os::unix::fs::symlink(Harness::horch_bin(), horch).unwrap();
            }
            let wrapper = wrapper(&h.root, first);
            let path = h.write_warmed("claude-wrapper", wrapper.as_bytes());
            h.set("HORCH_CLAUDE_BIN", path.to_string_lossy());
            let workspace = h.fleet_workspace();
            h.set("HORCH_WORKSPACE_ID", workspace);
            let w = World { h };
            std::fs::write(
                w.h.project.join("fleet.toml"),
                "[gates]\nfast = \"true\"\nfull = \"true\"\n",
            )
            .unwrap();
            let tickets = w.h.project.join("tickets");
            std::fs::create_dir_all(&tickets).unwrap();
            let ticket = tickets.join("01-port-guide.md");
            std::fs::write(&ticket, TICKET).unwrap();
            w.git(&["add", "-A"]);
            w.git(&["commit", "-q", "-m", "base"]);
            w.h.serve_fleetd();
            let o =
                w.h.run(&["idea", "add", "--direct", &ticket.to_string_lossy()]);
            assert!(o.status.success(), "horch idea add --direct: {}", text(&o));
            w
        }

        fn git(&self, args: &[&str]) {
            let o = self.h.git_cmd(args).output().unwrap();
            assert!(o.status.success(), "git {}: {}", args.join(" "), text(&o));
        }

        fn rows(&self, sql: &str) -> Vec<String> {
            let conn =
                Connection::open_with_flags(self.h.ledger_db(), OpenFlags::SQLITE_OPEN_READ_ONLY)
                    .unwrap();
            let mut stmt = conn.prepare(sql).unwrap();
            stmt.query_map([], |r| r.get::<_, SqlValue>(0))
                .unwrap()
                .map(|r| match r.unwrap() {
                    SqlValue::Null => String::new(),
                    SqlValue::Integer(n) => n.to_string(),
                    SqlValue::Real(x) => x.to_string(),
                    SqlValue::Text(t) => t,
                    SqlValue::Blob(b) => String::from_utf8_lossy(&b).into_owned(),
                })
                .collect()
        }

        fn one(&self, sql: &str) -> Option<String> {
            self.rows(sql).into_iter().next()
        }

        fn task(&self) -> String {
            self.wait("the planned task of the idea", || {
                self.one(&format!(
                    "SELECT task_id FROM tasks WHERE idea_id = {}",
                    quoted(IDEA)
                ))
            })
        }

        fn status(&self, task: &str) -> String {
            self.one(&format!(
                "SELECT status FROM tasks WHERE task_id = {}",
                quoted(task)
            ))
            .unwrap_or_default()
        }

        /// The attempt ids of `task`, oldest first.
        fn attempts(&self, task: &str) -> Vec<String> {
            self.rows(&format!(
                "SELECT id FROM attempts WHERE task_id = {} ORDER BY rowid",
                quoted(task)
            ))
        }

        /// The states of the question rows of `task`, by id.
        fn question_states(&self, task: &str) -> Vec<String> {
            self.rows(&format!(
                "SELECT state FROM questions WHERE task_id = {} ORDER BY id",
                quoted(task)
            ))
        }

        fn start_md(&self, attempt: &str) -> Option<String> {
            let (task, n) = attempt
                .rsplit_once('/')
                .expect("an attempt id names its task");
            let dir = self
                .h
                .fleet_root()
                .join("tasks")
                .join(task)
                .join("attempts")
                .join(n);
            std::fs::read_to_string(dir.join("inbox/START.md")).ok()
        }

        fn context(&self) -> String {
            let show =
                |title: &str, sql: &str| format!("{title}:\n  {}\n", self.rows(sql).join("\n  "));
            let stderr: PathBuf = self.h.root.join("fleetd.stderr");
            let stderr = std::fs::read_to_string(stderr).unwrap_or_default();
            let mut tail: Vec<&str> = stderr.lines().rev().take(40).collect();
            tail.reverse();
            let first = std::fs::read_to_string(self.h.root.join(FIRST_LOG)).unwrap_or_default();
            format!(
                "first launch of the worker:\n{first}\n{}{}{}{}{}fleetd.stderr:\n{}\n",
                show("tasks", "SELECT task_id || ' ' || status FROM tasks"),
                show(
                    "attempts",
                    "SELECT id || ' ' || state || ' ' || IFNULL(outcome, '-') FROM attempts"
                ),
                show(
                    "questions",
                    "SELECT id || ' ' || state || ' ' || owner FROM questions"
                ),
                show(
                    "needs-you",
                    "SELECT key || ' ' || state || ' ' || text FROM needs_you"
                ),
                show(
                    "the last events",
                    "SELECT IFNULL(entity, '-') || ' ' || IFNULL(entity_id, '-') || ' ' || kind \
                     || ' ' || IFNULL(to_state, '-') || ' ' || text FROM events \
                     ORDER BY event_id DESC LIMIT 30"
                ),
                tail.join("\n"),
            )
        }

        fn wait<T>(&self, what: &str, f: impl FnMut() -> Option<T>) -> T {
            wait_for_or(what, f, || self.context())
        }
    }

    /// RR-12: the answer of the owner (the fake sub-orchestrator answers with
    /// its own decision) answers the row, the task leaves `blocked_on_user`,
    /// and `START.md` of the next attempt holds the question and the answer.
    #[test]
    fn answer_of_the_owner_reaches_the_next_attempt() {
        let ask = format!("horch ask {}", shell_word(QUESTION));
        let w = World::new("p6-q-owner-note", "stay,answer_own", &ask);
        let task = w.task();
        w.wait("the answered question", || {
            (w.question_states(&task) == ["answered"]).then_some(())
        });
        let next = w.wait("the next attempt of the task", || {
            w.attempts(&task).get(1).cloned()
        });
        assert_ne!(w.status(&task), "blocked_on_user", "{}", w.context());
        let start = w.wait("START.md of the next attempt", || w.start_md(&next));
        assert!(
            start.contains(QUESTION),
            "START.md of the next attempt holds the question:\n{start}"
        );
        // The fake sub-orchestrator answers with this text (crate::idea_agent).
        let answer = w
            .one(&format!(
                "SELECT json_extract(data_json, '$.text') FROM events
                 WHERE kind = 'answered' AND entity_id = {}",
                quoted(&task)
            ))
            .expect("the answered event holds the answer");
        assert!(
            !answer.is_empty() && start.contains(&answer),
            "START.md of the next attempt holds the answer {answer:?}:\n{start}"
        );
    }
}
