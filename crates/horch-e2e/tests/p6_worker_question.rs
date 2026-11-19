//! A question of a worker in a push attempt becomes 1 `questions` row with
//! its owner, through the ingest of fleetd (`docs/specs/fleetd/design.md`:
//! RR-12, FM-35, OR-3 and FM-8; T-897).
//!
//! Each test makes its own ledger, fleet root and git project in a temp
//! folder, and runs a test `horch fleetd` with the fake herdr and the fake
//! agents. `horch idea add --direct` adds 1 ticket, so the idea `I-0001` has
//! its feature and 1 planned task, which fleetd dispatches by push.
//!
//! The claude of each pane is a wrapper script. At the first launch of a
//! worker (its argument names `inbox/START.md`), the wrapper runs the shell
//! line of the test, for example `horch ask ...`, and exits with no result.
//! Each other launch, of a later worker or of the sub-orchestrator, runs the
//! fake claude with the keys of the scenario.
//!
//! No test reads the load, the limits or the clock of this machine. Each
//! wait is for an event of the ledger, never for a time.
//! `HORCH_TEST_DOCS_FIXTURE` keeps the docs sync off the network.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Output;

use horch_e2e::harness::{fixtures, wait_for_or, Harness};
use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, OpenFlags};

/// The `fleet.toml` of each test: 1 pass each second.
const FLEET_TOML: &str = "pass_period_s = 1\n";

/// The ticket of each test: 1 task of the code kind for the fake claude.
const TICKET: &str = "# 01: Write the port guide\n\n\
<!-- horch-ticket: 1 -->\n\
**Persona:** primary `opus` · **Parallel:** yes\n\
**Kind:** feature\n\
**Review:** none\n\
**Check:** `true`\n\
**Own:** `fake-claude.txt`\n\n\
## Goal\n\nThe guide names the port.\n";

/// The idea of `horch idea add --direct` in each test.
const IDEA: &str = "I-0001";

/// The question of the worker in each test.
const QUESTION: &str = "Which port does the guide name?";

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

/// `s` as 1 shell word in single quotes.
fn shell_word(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// A fleet with a test fleetd and 1 `--direct` idea.
struct World {
    h: Harness,
}

impl World {
    /// The fleet, with `scenario` as the keys of each fake agent, and
    /// `first` as the shell line of the first launch of a worker.
    fn new(name: &str, scenario: &str, first: &str) -> World {
        let mut h = Harness::new(name).with_git();
        // The ledger and the fleet root are outside the state folder. The
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
        // The worker runs `horch` from PATH, as in a pane.
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
        // Plan validation refuses a plan when the project declares no gates.
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

    /// The feature of the idea.
    fn feature(&self) -> String {
        self.one(&format!(
            "SELECT feature_id FROM ideas WHERE id = {}",
            quoted(IDEA)
        ))
        .expect("the idea has a feature")
    }

    /// The attempts of the task, oldest first: (id, state, outcome).
    fn attempts(&self, task: &str) -> Vec<(String, String, String)> {
        self.rows(&format!(
            "SELECT id || char(9) || state || char(9) || IFNULL(outcome, '-') FROM attempts
             WHERE task_id = {} ORDER BY rowid",
            quoted(task)
        ))
        .into_iter()
        .map(|row| {
            let mut parts = row.split('\t').map(str::to_string);
            (
                parts.next().unwrap_or_default(),
                parts.next().unwrap_or_default(),
                parts.next().unwrap_or_default(),
            )
        })
        .collect()
    }

    /// The `questions` rows of the task, oldest first: (id, state, owner).
    fn questions(&self, task: &str) -> Vec<(String, String, String)> {
        self.rows(&format!(
            "SELECT id || char(9) || state || char(9) || owner FROM questions
             WHERE task_id = {} ORDER BY id",
            quoted(task)
        ))
        .into_iter()
        .map(|row| {
            let mut parts = row.split('\t').map(str::to_string);
            (
                parts.next().unwrap_or_default(),
                parts.next().unwrap_or_default(),
                parts.next().unwrap_or_default(),
            )
        })
        .collect()
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

    /// Wait until the task is `blocked_on_user`, then for [`QUIET_PASSES`]
    /// more passes, so a second ingest of the same file has its chance.
    fn blocked(&self, task: &str) {
        self.wait("the task in blocked_on_user", || {
            (self.status(task) == "blocked_on_user").then_some(())
        });
        self.passes(QUIET_PASSES);
    }

    /// The folder of attempt `attempt` of the task.
    fn attempt_dir(&self, attempt: &str) -> PathBuf {
        let (task, n) = attempt.rsplit_once('/').expect("an attempt id names its task");
        self.h
            .fleet_root()
            .join("tasks")
            .join(task)
            .join("attempts")
            .join(n)
    }

    /// The `START.md` of attempt `attempt`, when fleetd wrote it.
    fn start_md(&self, attempt: &str) -> Option<String> {
        std::fs::read_to_string(self.attempt_dir(attempt).join("inbox/START.md")).ok()
    }

    /// The text of each needs-you item that is open.
    fn needs_you(&self) -> Vec<String> {
        self.rows("SELECT key || ' ' || text FROM needs_you WHERE state = 'open' ORDER BY key")
    }

    /// What a failed wait shows.
    fn context(&self) -> String {
        let show =
            |title: &str, sql: &str| format!("{title}:\n  {}\n", self.rows(sql).join("\n  "));
        let stderr = std::fs::read_to_string(self.h.root.join("fleetd.stderr")).unwrap_or_default();
        let mut tail: Vec<&str> = stderr.lines().rev().take(40).collect();
        tail.reverse();
        let first = std::fs::read_to_string(self.h.root.join(FIRST_LOG)).unwrap_or_default();
        format!(
            "first launch of the worker:\n{first}\n{}{}{}{}{}{}{}fleetd.stderr:\n{}\n",
            show(
                "ideas",
                "SELECT id || ' ' || state || ' ' || IFNULL(feature_id, '-') FROM ideas"
            ),
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
                "ingested",
                "SELECT path || ' ' || outcome || ' ' || IFNULL(reason, '-') FROM ingested"
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

    /// Wait for `f` to give a value, with [`World::context`] on a failure.
    fn wait<T>(&self, what: &str, f: impl FnMut() -> Option<T>) -> T {
        wait_for_or(what, f, || self.context())
    }
}

/// The claude wrapper: the first launch of a worker runs `first`, with its
/// output in [`FIRST_LOG`] under `root`, and exits with no result; each
/// other launch runs the fake claude. A conformance run of fleetd also has
/// an `inbox/START.md`: it always runs the fake claude.
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

/// The output of the shell line of the first launch of a worker.
const FIRST_LOG: &str = "first-launch.log";

/// The shell line of a worker that runs `horch ask` with `text`.
fn horch_ask(text: &str) -> String {
    format!("horch ask {}", shell_word(text))
}

/// The shell line of a worker that sends a QUESTION line with `text`.
fn question_tell(text: &str) -> String {
    format!(
        "horch tell orchestrator {}",
        shell_word(&format!("QUESTION: {text}"))
    )
}

/// Assert the 1 row of a question of the first attempt: its task, its
/// attempt, its owner and its state; the task is `blocked_on_user`, and
/// the attempt has the outcome `asked`.
fn assert_1_row_of_the_first_attempt(w: &World, task: &str) -> String {
    let attempts = w.attempts(task);
    let (attempt, state, outcome) = attempts.first().cloned().unwrap_or_default();
    let rows = w.questions(task);
    assert_eq!(
        rows.len(),
        1,
        "1 question file gives exactly 1 questions row\n{}",
        w.context()
    );
    let (id, q_state, owner) = rows[0].clone();
    assert_eq!(
        id,
        format!("{attempt}/Q1"),
        "the row is of the attempt\n{}",
        w.context()
    );
    assert_eq!(q_state, "open", "{}", w.context());
    assert_eq!(
        owner,
        format!("idea-lead:{}", w.feature()),
        "the owner is the sub-orchestrator of the feature\n{}",
        w.context()
    );
    assert_eq!(w.status(task), "blocked_on_user", "{}", w.context());
    assert_eq!(
        (state.as_str(), outcome.as_str()),
        ("ended", "asked"),
        "{}",
        w.context()
    );
    id
}

/// RR-12, step 1 of T-897: `horch ask` of a worker in a push attempt gives
/// 1 `questions` row with its owner, in the move of the task to
/// `blocked_on_user`. More passes give no second row.
#[test]
fn horch_ask_of_a_push_worker_gives_1_row_with_its_owner() {
    let w = World::new("p6-wq-ask", "stay", &horch_ask(QUESTION));
    let task = w.task();
    w.blocked(&task);
    assert_1_row_of_the_first_attempt(&w, &task);
}

/// RR-12: a QUESTION line of `horch tell` from a worker in a push attempt
/// gives 1 `questions` row with its owner, not 0 and not 2 (the question
/// file and the mail are 1 question). More passes give no second row.
#[test]
fn question_tell_of_a_push_worker_gives_1_row_with_its_owner() {
    let w = World::new("p6-wq-tell", "stay", &question_tell(QUESTION));
    let task = w.task();
    w.blocked(&task);
    assert_1_row_of_the_first_attempt(&w, &task);
}

/// RR-12: the answer of the owner (the fake sub-orchestrator answers with
/// its own decision) answers the row, the task leaves `blocked_on_user`,
/// and `START.md` of the next attempt holds the question and the answer.
#[test]
fn answer_of_the_owner_reaches_the_next_attempt() {
    let w = World::new("p6-wq-owner", "stay,answer_own", &horch_ask(QUESTION));
    let task = w.task();
    w.wait("the answered question", || {
        let rows = w.questions(&task);
        (rows.len() == 1 && rows[0].1 == "answered").then_some(())
    });
    let next = w.wait("the next attempt of the task", || {
        w.attempts(&task).get(1).map(|a| a.0.clone())
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

/// RR-12: the answer by `horch tell <role> --answers <id>` to the QUESTION
/// line of a worker answers its row, the task leaves `blocked_on_user`, and
/// `START.md` of the next attempt holds the question and the answer.
#[test]
fn answer_by_tell_answers_reaches_the_next_attempt() {
    let w = World::new("p6-wq-answers", "stay", &question_tell(QUESTION));
    let task = w.task();
    w.blocked(&task);
    let id = assert_1_row_of_the_first_attempt(&w, &task);
    let mail_id = w
        .one(&format!(
            "SELECT json_extract(data_json, '$.mail_id') FROM events
             WHERE kind = 'question' AND entity_id = {}
               AND json_extract(data_json, '$.question_id') = {}",
            quoted(&task),
            quoted(&id)
        ))
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| panic!("the row holds the mail id\n{}", w.context()));
    let role = w
        .one(&format!(
            "SELECT w.role FROM attempts a JOIN workers w ON w.worker_id = a.worker_id
             WHERE a.id = {}",
            quoted(id.rsplit_once('/').map_or("", |(a, _)| a))
        ))
        .filter(|r| !r.is_empty())
        .unwrap_or_else(|| panic!("the worker of the attempt has a role\n{}", w.context()));
    let answer = "The guide names port 8080.";
    let o = w.h.run(&["tell", &role, "--answers", &mail_id, answer]);
    assert!(o.status.success(), "horch tell --answers: {}", text(&o));
    assert_eq!(w.questions(&task)[0].1, "answered", "{}", w.context());
    let next = w.wait("the next attempt of the task", || {
        w.attempts(&task).get(1).map(|a| a.0.clone())
    });
    let start = w.wait("START.md of the next attempt", || w.start_md(&next));
    assert!(
        start.contains(QUESTION) && start.contains(answer),
        "START.md of the next attempt holds the question and the answer:\n{start}"
    );
}

/// RR-12: a second question of the same attempt gives a second row.
#[test]
fn second_question_of_the_same_attempt_gives_a_second_row() {
    let second = "Which host does the guide name?";
    let first = format!("{}; {}", horch_ask(QUESTION), horch_ask(second));
    let w = World::new("p6-wq-second", "stay", &first);
    let task = w.task();
    w.blocked(&task);
    let attempt = w.attempts(&task)[0].0.clone();
    let ids: Vec<String> = w.questions(&task).into_iter().map(|q| q.0).collect();
    assert_eq!(
        ids,
        [format!("{attempt}/Q1"), format!("{attempt}/Q2")],
        "2 question files of 1 attempt give 2 rows\n{}",
        w.context()
    );
}

/// SR-6, OR-3: a question file of a bad form gives no row and 1 needs-you
/// item, and fleetd stays healthy.
#[test]
fn question_file_of_a_bad_form_gives_no_row_and_1_needs_you_item() {
    // The PATH of a pane holds only the fakes and horch.
    let first = "q=\"$(/usr/bin/dirname \"$HORCH_TASK_YAML\")/outbox/questions\"; \
                 /bin/mkdir -p \"$q\" && printf '{\"text\": 1}' > \"$q/1.json\"";
    let w = World::new("p6-wq-bad", "stay", first);
    let task = w.task();
    w.wait("the rejected question file", || {
        w.one("SELECT path FROM ingested WHERE outcome = 'rejected' AND path LIKE '%/questions/1.json'")
    });
    w.passes(QUIET_PASSES);
    assert!(w.questions(&task).is_empty(), "{}", w.context());
    let items: Vec<String> = w
        .needs_you()
        .into_iter()
        .filter(|i| i.contains("questions/1.json"))
        .collect();
    assert_eq!(items.len(), 1, "{}", w.context());
}

/// FM-8 and T-894: the fake sub-orchestrator does not play the worker key
/// `ask`, so it sends no QUESTION line, and no idea holds a question.
#[test]
fn fake_sub_orchestrator_does_not_ask_and_makes_no_idea() {
    let w = World::new("p6-wq-fake", "stay,ask", &horch_ask(QUESTION));
    let task = w.task();
    w.blocked(&task);
    w.wait("a launch of the sub-orchestrator", || {
        w.one("SELECT id FROM attempts WHERE task_id IS NULL")
    });
    w.passes(QUIET_PASSES);
    let asks: Vec<String> = w
        .h
        .calls_of("claude")
        .iter()
        .filter(|c| c.get("ask").is_some())
        .map(|c| c.to_string())
        .collect();
    assert!(asks.is_empty(), "the sub-orchestrator asked: {asks:?}");
    // fleetd can make an idea of its own (a failed adapter conformance
    // run), but no idea holds the text of a question.
    for idea in w.rows("SELECT id FROM ideas") {
        let path = w.h.fleet_root().join("ideas").join(&idea).join("idea.md");
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(
            !text.contains("QUESTION:") && !text.contains(QUESTION),
            "idea {idea} holds a question:\n{text}\n{}",
            w.context()
        );
    }
}
