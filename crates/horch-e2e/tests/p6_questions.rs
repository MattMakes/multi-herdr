//! The e2e tests of questions, replans and the needs-you list
//! (`docs/specs/fleetd/design.md`: RR-12, OR-3, FM-21, FM-35;
//! `docs/tickets/fleetd-p6/60-e2e-questions.md`).

#![cfg(unix)]

use std::cell::RefCell;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::Output;

use horch_core::fleetd::fleet_runtime::domain::pass::START_TIMEOUT_S;
use horch_core::fleetd::ideas::application::question_flow::NEEDS_YOU_KIND as QUESTION_KIND;
use horch_core::fleetd::task_execution::application::planned_tasks;
use horch_core::taskledger::tasks::TASK_ESCALATED;
use horch_e2e::harness::{fixtures, wait_for_or, Harness};
use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;

const FLEET_TOML: &str = "pass_period_s = 1\n";

const TICKET: &str = "# 01: Write the queue guide\n\n\
<!-- horch-ticket: 1 -->\n\
**Persona:** primary `opus` · **Parallel:** yes\n\
**Kind:** feature\n\
**Review:** none\n\
**Check:** `true`\n\
**Own:** `fake-claude.txt`\n\n\
## Goal\n\nThe guide names the queue length.\n";

const IDEA: &str = "I-0001";

const QUESTION_LIMIT_S: i64 = 900;

const STEP_S: i64 = 500;

const DRIVE_STEP_S: i64 = 60;
const _: () = assert!(DRIVE_STEP_S < START_TIMEOUT_S);

const MAX_ATTEMPTS_SEEN: usize = 20;

const QUIET_PASSES: usize = 3;

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn quoted(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

struct World {
    h: Harness,
    moved_at: RefCell<Option<String>>,
}

impl World {
    fn new(name: &str, scenario: &str) -> World {
        World::build(name, scenario, None)
    }

    fn with_answering_owner(name: &str, scenario: &str) -> World {
        World::build(name, scenario, Some(answering_owner))
    }

    fn with_dead_first_launch(name: &str, scenario: &str) -> World {
        World::build(name, scenario, Some(dead_first_launch))
    }

    fn build(name: &str, scenario: &str, setup: Option<fn(&mut Harness, &Path)>) -> World {
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
        if let Some(setup) = setup {
            setup(&mut h, &fleet);
        }
        h.fleet_workspace();
        let w = World {
            h,
            moved_at: RefCell::new(None),
        };
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

    fn idea_folder(&self) -> PathBuf {
        self.h.fleet_root().join("ideas").join(IDEA)
    }

    fn conn(&self) -> Connection {
        Connection::open_with_flags(self.h.ledger_db(), OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
    }

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

    fn replans(&self, task: &str) -> Option<String> {
        self.one(&format!(
            "SELECT replans FROM tasks WHERE task_id = {}",
            quoted(task)
        ))
    }

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

    fn pass_mark(&self) -> Option<String> {
        self.one("SELECT clock_ticked_at FROM fleetd WHERE id = 1")
    }

    fn passes(&self, n: usize) {
        for _ in 0..n {
            let before = self.pass_mark();
            self.wait("1 more pass of fleetd", || {
                (self.pass_mark() != before).then_some(())
            });
        }
    }

    fn step_clock(&self, seconds: i64) {
        let mark = self.pass_mark();
        if mark.is_some() && *self.moved_at.borrow() != mark {
            self.h.advance_fleet_clock(seconds);
            *self.moved_at.borrow_mut() = mark;
        }
    }

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
                    self.step_clock(DRIVE_STEP_S);
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

    fn live_owner(&self) -> Option<(String, i64)> {
        self.owners()
            .into_iter()
            .find_map(|(id, state, pid)| (state == "live").then_some((id, pid?)))
    }

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

    fn asked(&self, task: &str) -> String {
        self.wait("the question of the worker", || {
            self.one(&format!(
                "SELECT id FROM questions WHERE task_id = {} AND state = 'open'",
                quoted(task)
            ))
        })
    }

    fn items(&self, key: &str) -> Vec<String> {
        self.rows(&format!(
            "SELECT state FROM needs_you WHERE key = {}",
            quoted(key)
        ))
    }

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

    fn answer_as_owner(&self, q: &str) {
        let file = self.h.root.join(OWNER_ANSWER);
        let tmp = file.with_extension("tmp");
        std::fs::write(&tmp, q).unwrap();
        std::fs::rename(&tmp, &file).unwrap();
    }

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

        let key = self.plan_key(task);
        let folder = self.idea_folder();
        let tmp = folder.join(format!("replan-{key}.yaml.tmp"));
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

    fn attempts_of(&self, task: &str) -> usize {
        self.rows(&format!(
            "SELECT id FROM attempts WHERE task_id = {}",
            quoted(task)
        ))
        .len()
    }

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
        let answer = self.h.root.join(format!("{OWNER_ANSWER}.log"));
        let answer = std::fs::read_to_string(answer).unwrap_or_default();
        format!(
            "needs-you/:\n  {}\nidea folder:\n  {}\n{}{}{}{}{}{}calls:\n{}\nfleetd.stderr:\n{}\n\
             answers of the owner:\n{answer}\n",
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

    fn wait<T>(&self, what: &str, f: impl FnMut() -> Option<T>) -> T {
        wait_for_or(what, f, || self.context())
    }
}

const OWNER_ANSWER: &str = "owner-answer";

fn shell_word(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn owner_wrapper(answer: &Path, folder: &Path) -> String {
    let horch = Harness::horch_bin();
    let fake = horch.with_file_name("fake-claude");
    format!(
        "#!/bin/sh\n[ \"$1\" = warm ] && exit 0\n\
         case \"$*\" in\n*horch-conformance*|*/inbox/START.md*) ;;\n*/ideas/{IDEA}*)\n\
         launch=$$\n\
         ( while kill -0 \"$launch\" 2>/dev/null; do\n\
         if [ -s {answer} ]; then\n\
         read -r q < {answer}\n\
         cd {folder} && {horch} idea note {IDEA} --question \"$q\" \
         --case own_decision {text} >> {answer}.log 2>&1\n\
         exit 0\nfi\n/bin/sleep 0.2\ndone ) &\n;;\nesac\n\
         exec {fake} \"$@\"\n",
        answer = shell_word(&answer.to_string_lossy()),
        folder = shell_word(&folder.to_string_lossy()),
        horch = shell_word(&horch.to_string_lossy()),
        text = shell_word("The guide uses the queue length of the status bar."),
        fake = shell_word(&fake.to_string_lossy()),
    )
}

fn answering_owner(h: &mut Harness, fleet: &Path) {
    let folder = fleet.join("ideas").join(IDEA);
    let wrapper = owner_wrapper(&h.root.join(OWNER_ANSWER), &folder);
    let path = h.write_warmed("claude-wrapper", wrapper.as_bytes());
    h.set("HORCH_CLAUDE_BIN", path.to_string_lossy());
}

const DEAD_LAUNCH_LOG: &str = "dead-launch.log";

fn dead_first_launch(h: &mut Harness, _fleet: &Path) {
    let herdr = h.bin.join("herdr");
    let wrapper = format!(
        "#!/bin/sh\n\
         case \"$*\" in\n\
         \"pane run \"*\"'worker' 'opus-\"*)\n\
         if [ ! -e {log} ]; then echo \"$*\" > {log}; \
         echo 'fake-herdr: pane run refused once' >&2; exit 1; fi ;;\n\
         esac\n\
         exec {herdr} \"$@\"\n",
        log = shell_word(&h.root.join(DEAD_LAUNCH_LOG).to_string_lossy()),
        herdr = shell_word(&herdr.to_string_lossy()),
    );
    let path = h.write_bin("herdr-wrapper", wrapper.as_bytes());
    h.set("HORCH_HERDR_BIN", path.to_string_lossy());
}

fn escalated_key(task: &str) -> String {
    horch_core::taskledger::tasks::escalated_key(task)
}

fn question_key(q: &str) -> String {
    horch_core::fleetd::ideas::application::question_flow::needs_you_key(q)
}

#[test]
fn rr12_question_owner_is_started_and_answers() {
    let w = World::new("p6-q-started", "stay,ask,answer_own");
    let task = w.task();
    let q = w.asked(&task);
    w.wait("the answered question", || {
        (w.question_state(&q) == "answered").then_some(())
    });
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
    w.wait("the task leaves blocked_on_user", || {
        (w.status(&task) != "blocked_on_user").then_some(())
    });
    assert!(w.items(&question_key(&q)).is_empty(), "no needs-you item");
}

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

    for _ in 0..10 {
        w.passes(1);
        w.h.advance_fleet_clock(QUESTION_LIMIT_S);
    }
    w.passes(1);
    assert_eq!(w.question_state(&q), "escalated");
    assert_eq!(w.status(&task), "blocked_on_user");
    assert_eq!(w.items(&question_key(&q)), ["open"]);
}

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

#[test]
fn fm21_one_replan_then_escalation() {
    let w = World::new("p6-q-fm21", "");
    let task = w.task();
    w.escalate(&task);
    assert_eq!(w.items(&escalated_key(&task)), ["open"]);

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

#[test]
fn fm21_launch_timeout_gives_the_task_a_retry() {
    let w = World::with_dead_first_launch("p6-q-launch-timeout", "");
    let task = w.task();
    let first = w.wait("the first attempt in launching", || {
        w.one(&format!(
            "SELECT id FROM attempts WHERE task_id = {} AND state = 'launching'",
            quoted(&task)
        ))
    });
    w.wait("the first launch, which herdr refuses", || {
        w.h.root.join(DEAD_LAUNCH_LOG).exists().then_some(())
    });
    w.passes(1);
    w.h.advance_fleet_clock(START_TIMEOUT_S);
    let outcome = w.wait("the end of the first attempt", || {
        w.one(&format!(
            "SELECT outcome FROM attempts WHERE id = {} AND state = 'ended'",
            quoted(&first)
        ))
    });
    assert_eq!(outcome, "timed_out", "{}", w.context());
    w.drive("a second attempt of the task", || {
        (w.attempts_of(&task) >= 2).then_some(())
    });
    let lost = w.rows(
        "SELECT task_id FROM tasks t WHERE status IN ('leased', 'running')
           AND NOT EXISTS (SELECT 1 FROM attempts a
                           WHERE a.task_id = t.task_id AND a.state != 'ended')
           AND NOT EXISTS (SELECT 1 FROM leases l WHERE l.task_id = t.task_id)",
    );
    assert!(lost.is_empty(), "lost tasks: {lost:?}\n{}", w.context());
}

#[test]
fn fm35_delivery_repeats_until_an_answer() {
    let w = World::with_answering_owner("p6-q-fm35", "stay,ask");
    let task = w.task();
    let q = w.asked(&task);
    let (first, pid) = w.wait("a live sub-orchestrator", || w.live_owner());
    w.wait("the doorbell of the first process", || {
        w.rung_for(&q).contains(&first).then_some(())
    });

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

    const FIRST_LOG: &str = "first-launch.log";

    fn shell_word(s: &str) -> String {
        format!("'{}'", s.replace('\'', "'\\''"))
    }

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

        fn attempts(&self, task: &str) -> Vec<String> {
            self.rows(&format!(
                "SELECT id FROM attempts WHERE task_id = {} ORDER BY rowid",
                quoted(task)
            ))
        }

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
