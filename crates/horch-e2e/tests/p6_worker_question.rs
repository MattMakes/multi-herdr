#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::Duration;

use horch_e2e::harness::{fixtures, wait_for_or, Harness};
use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, OpenFlags};

const FLEET_TOML: &str = "pass_period_s = 1\n";

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

fn shell_word(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

struct World {
    h: Harness,
}

impl World {
    fn new(name: &str, scenario: &str, first: &str) -> World {
        World::with(name, scenario, first, Refuse::Never)
    }

    fn with(name: &str, scenario: &str, first: &str, refuse: Refuse) -> World {
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
        let git = shell_word(&h.git_bin().expect("a git").to_string_lossy());
        let wrapper = wrapper(&h.root, &first.replace("@GIT@", &git), refuse);
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

    fn feature(&self) -> String {
        self.one(&format!(
            "SELECT feature_id FROM ideas WHERE id = {}",
            quoted(IDEA)
        ))
        .expect("the idea has a feature")
    }

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

    fn blocked(&self, task: &str) {
        self.wait("the task in blocked_on_user", || {
            (self.status(task) == "blocked_on_user").then_some(())
        });
        self.passes(QUIET_PASSES);
    }

    fn attempt_dir(&self, attempt: &str) -> PathBuf {
        let (task, n) = attempt
            .rsplit_once('/')
            .expect("an attempt id names its task");
        self.h
            .fleet_root()
            .join("tasks")
            .join(task)
            .join("attempts")
            .join(n)
    }

    fn start_md(&self, attempt: &str) -> Option<String> {
        std::fs::read_to_string(self.attempt_dir(attempt).join("inbox/START.md")).ok()
    }

    fn needs_you(&self) -> Vec<String> {
        self.rows("SELECT key || ' ' || text FROM needs_you WHERE state = 'open' ORDER BY key")
    }

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

    fn wait<T>(&self, what: &str, f: impl FnMut() -> Option<T>) -> T {
        wait_for_or(what, f, || self.context())
    }

    /// The attempts of `task` that its retry budget counts:
    /// `attempts - attempt_floor`.
    fn used(&self, task: &str) -> i64 {
        self.one(&format!(
            "SELECT attempts - attempt_floor FROM tasks WHERE task_id = {}",
            quoted(task)
        ))
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("task {task} has a row\n{}", self.context()))
    }

    /// The run of `task` allows 1 more attempt than it used.
    fn one_attempt_left(&self, task: &str) -> i64 {
        let max = self.used(task) + 1;
        let conn = Connection::open(self.h.ledger_db()).unwrap();
        conn.busy_timeout(Duration::from_secs(30)).unwrap();
        let n = conn
            .execute(
                "UPDATE runs SET max_attempts = ?1
                 WHERE run_id = (SELECT run_id FROM tasks WHERE task_id = ?2)",
                rusqlite::params![max, task],
            )
            .unwrap();
        assert_eq!(n, 1, "the task has a run\n{}", self.context());
        max
    }
}

/// Which launches of the fake CLI exit at once with no result.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Refuse {
    Never,
    /// A resume: the CLI says that the session is gone, as claude does.
    Resume,
    /// A resume, and each fresh launch after the first.
    Every,
}

fn wrapper(root: &Path, first: &str, refuse: Refuse) -> String {
    let fake = Harness::horch_bin().with_file_name("fake-claude");
    let refused = shell_word(&root.join(REFUSED_LOG).to_string_lossy());
    let log = shell_word(&root.join(FIRST_LOG).to_string_lossy());
    let resume = if refuse == Refuse::Never {
        String::new()
    } else {
        format!(
            "*--resume*) echo \"$*\" >> {refused}; \
             echo 'No conversation found with session ID' >&2; exit 1 ;;\n"
        )
    };
    let fresh = if refuse == Refuse::Every {
        format!("; echo \"$*\" >> {refused}; exit 1")
    } else {
        String::new()
    };
    format!(
        "#!/bin/sh\n[ \"$1\" = warm ] && exit 0\n\
         case \"$*\" in\n*horch-conformance*) ;;\n{resume}*/inbox/START.md*)\n\
         if [ ! -e {log} ]; then {{ {first}\n}} > {log} 2>&1; exit 0; fi{fresh} ;;\nesac\n\
         exec {fake} \"$@\"\n",
        fake = shell_word(&fake.to_string_lossy()),
    )
}

const FIRST_LOG: &str = "first-launch.log";

const REFUSED_LOG: &str = "refused-resume.log";

fn horch_ask(text: &str) -> String {
    format!("horch ask {}", shell_word(text))
}

fn question_tell(text: &str) -> String {
    format!(
        "horch tell orchestrator {}",
        shell_word(&format!("QUESTION: {text}"))
    )
}

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

fn answer_by_tell(w: &World, task: &str, id: &str, answer: &str) {
    let mail_id = w
        .one(&format!(
            "SELECT json_extract(data_json, '$.mail_id') FROM events
             WHERE kind = 'question' AND entity_id = {}
               AND json_extract(data_json, '$.question_id') = {}",
            quoted(task),
            quoted(id)
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
    let o = w.h.run(&["tell", &role, "--answers", &mail_id, answer]);
    assert!(o.status.success(), "horch tell --answers: {}", text(&o));
    assert_eq!(w.questions(task)[0].1, "answered", "{}", w.context());
}

#[test]
fn horch_ask_of_a_push_worker_gives_1_row_with_its_owner() {
    let w = World::new("p6-wq-ask", "stay", &horch_ask(QUESTION));
    let task = w.task();
    w.blocked(&task);
    assert_1_row_of_the_first_attempt(&w, &task);
}

#[test]
fn question_tell_of_a_push_worker_gives_1_row_with_its_owner() {
    let w = World::new("p6-wq-tell", "stay", &question_tell(QUESTION));
    let task = w.task();
    w.blocked(&task);
    assert_1_row_of_the_first_attempt(&w, &task);
}

#[test]
fn answer_by_tell_answers_reaches_the_next_attempt() {
    let w = World::new("p6-wq-answers", "stay", &question_tell(QUESTION));
    let task = w.task();
    w.blocked(&task);
    let id = assert_1_row_of_the_first_attempt(&w, &task);
    let answer = "The guide names port 8080.";
    answer_by_tell(&w, &task, &id, answer);
    let next = w.wait("the next attempt of the task", || {
        w.attempts(&task).get(1).map(|a| a.0.clone())
    });
    let start = w.wait("START.md of the next attempt", || w.start_md(&next));
    assert!(
        start.contains(QUESTION) && start.contains(answer),
        "START.md of the next attempt holds the question and the answer:\n{start}"
    );
}

fn worker_of(w: &World, attempt: &str, col: &str) -> String {
    w.one(&format!(
        "SELECT w.{col} FROM attempts a JOIN workers w ON w.worker_id = a.worker_id
         WHERE a.id = {}",
        quoted(attempt)
    ))
    .unwrap_or_default()
}

fn argv_of(w: &World, attempt: &str) -> Vec<Vec<String>> {
    let start = w.attempt_dir(attempt).join("inbox/START.md");
    let start = start.to_string_lossy();
    w.h.calls_of("claude")
        .into_iter()
        .map(|c| {
            c["argv"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|a| a.as_str().unwrap_or_default().to_string())
                .collect::<Vec<String>>()
        })
        .filter(|argv| argv.iter().any(|a| a.contains(start.as_ref())))
        .collect()
}

fn resumes(argv: &[String], session: &str) -> bool {
    argv.windows(2)
        .any(|p| p[0] == "--resume" && p[1] == session)
}

fn session_of(w: &World, attempt: &str) -> String {
    w.wait("the session id of the first attempt", || {
        Some(worker_of(w, attempt, "session_id")).filter(|s| !s.is_empty())
    })
}

/// RR-12, design line 1015: fleetd resumes the session of the worker with
/// the answer, through the resume form of the adapter, and records it.
#[test]
fn answered_question_resumes_the_session_of_the_worker() {
    let w = World::new("p6-wq-resume", "stay", &question_tell(QUESTION));
    let task = w.task();
    w.blocked(&task);
    let id = assert_1_row_of_the_first_attempt(&w, &task);
    let first = w.attempts(&task)[0].0.clone();
    let session = session_of(&w, &first);
    answer_by_tell(&w, &task, &id, "The guide names port 8080.");
    let next = w.wait("the next attempt of the task", || {
        w.attempts(&task).get(1).map(|a| a.0.clone())
    });
    let argv = w.wait("the CLI of the next attempt", || {
        argv_of(&w, &next).into_iter().next()
    });
    assert!(
        resumes(&argv, &session),
        "the next attempt runs claude --resume {session}: {argv:?}\n{}",
        w.context()
    );
    assert_eq!(
        worker_of(&w, &next, "resumed_from_session"),
        session,
        "the worker row records the resumed session\n{}",
        w.context()
    );
    // T-15 retired the manual resume: no CLI user can start one.
    let o = w.h.run(&["spawn", "--resume", &session]);
    assert!(!o.status.success(), "horch spawn --resume: {}", text(&o));
    assert_eq!(w.attempts(&task).len(), 2, "{}", w.context());
}

/// The first launch commits work in progress, then asks [`QUESTION`].
fn first_commits_and_asks() -> String {
    let commit = "printf 'work\\n' > work.txt && @GIT@ add work.txt && \
                  @GIT@ -c user.name=t -c user.email=t@t -c commit.gpgSign=false \
                  commit -q --no-verify -m 'work in progress'";
    let tree =
        "cd \"$(/usr/bin/sed -n 's/^worktree: //p' \"$HORCH_TASK_YAML\" | /usr/bin/tr -d '\"')\"";
    format!("{tree} && {commit} && {}", question_tell(QUESTION))
}

/// The lines of [`REFUSED_LOG`]: the argv of each refused launch.
fn refused_argvs(w: &World) -> Vec<Vec<String>> {
    std::fs::read_to_string(w.h.root.join(REFUSED_LOG))
        .unwrap_or_default()
        .lines()
        .map(|l| l.split_whitespace().map(str::to_string).collect())
        .collect()
}

/// A task with 1 attempt left whose resume the CLI refuses: the refused
/// attempt (a2), and the fresh attempt after it (a3). `max` is the
/// attempt limit of the run.
struct Refused {
    w: World,
    task: String,
    session: String,
    answer: &'static str,
    a2: String,
    a3: String,
    max: i64,
}

fn refused_resume_at_the_last_attempt(name: &str, refuse: Refuse) -> Refused {
    let w = World::with(name, "stay", &first_commits_and_asks(), refuse);
    let task = w.task();
    w.blocked(&task);
    let id = assert_1_row_of_the_first_attempt(&w, &task);
    let a1 = w.attempts(&task)[0].0.clone();
    let session = session_of(&w, &a1);
    let max = w.one_attempt_left(&task);
    let answer = "The guide names port 8080.";
    answer_by_tell(&w, &task, &id, answer);
    // RR-12: no backoff, so the fresh attempt comes within 1 wait.
    let a3 = w.wait("the fresh attempt after the refused resume", || {
        w.attempts(&task).get(2).map(|a| a.0.clone())
    });
    let a2 = w.attempts(&task)[1].clone();
    assert_eq!(
        (a2.1.as_str(), a2.2.as_str()),
        ("ended", "resume_refused"),
        "the refused resume ends resume_refused\n{}",
        w.context()
    );
    assert_eq!(worker_of(&w, &a2.0, "resumed_from_session"), session);
    assert!(
        refused_argvs(&w).iter().any(|argv| resumes(argv, &session)),
        "the CLI of the resumed attempt got --resume {session}\n{}",
        w.context()
    );
    assert_eq!(
        worker_of(&w, &a3, "resumed_from_session"),
        "",
        "the fresh attempt records no resume\n{}",
        w.context()
    );
    Refused {
        w,
        task,
        session,
        answer,
        a2: a2.0,
        a3,
        max,
    }
}

/// RR-12, design line 1015: a resume that the CLI refuses because the
/// session is gone gives a fresh attempt from the question, the answer and
/// the committed work, also at the last attempt: the refusal burns no
/// attempt and waits no backoff (D244-F3).
#[test]
fn refused_resume_at_the_last_attempt_gives_a_fresh_attempt_with_the_answer_and_the_work() {
    let r = refused_resume_at_the_last_attempt("p6-wq-refused", Refuse::Resume);
    let w = &r.w;
    assert_eq!(
        w.used(&r.task),
        r.max,
        "the refused resume gives its attempt back\n{}",
        w.context()
    );
    let argv = w.wait("the CLI of the fresh attempt", || {
        argv_of(w, &r.a3).into_iter().next()
    });
    assert!(
        !argv.iter().any(|a| a == "--resume"),
        "the fresh attempt does not resume: {argv:?}"
    );
    let start = w.wait("START.md of the fresh attempt", || w.start_md(&r.a3));
    assert!(
        start.contains(QUESTION) && start.contains(r.answer),
        "START.md of the fresh attempt holds the question and the answer:\n{start}"
    );
    let tree = w
        .one(&format!(
            "SELECT path FROM worktrees WHERE task_id = {}",
            quoted(&r.task)
        ))
        .unwrap_or_else(|| panic!("the task has a tree\n{}", w.context()));
    let log =
        w.h.git_cmd(&["-C", &tree, "log", "--format=%s"])
            .output()
            .unwrap();
    assert!(
        String::from_utf8_lossy(&log.stdout).contains("work in progress")
            && Path::new(&tree).join("work.txt").is_file(),
        "the fresh attempt keeps the committed work: {}\n{}",
        text(&log),
        w.context()
    );
}

/// RR-12 bound: 1 fresh launch for each answer. When the fresh attempt
/// also exits with no result, it ends `no_result` and burns the last
/// attempt as today, and no attempt resumes again.
#[test]
fn fresh_attempt_after_a_refused_resume_that_fails_burns_the_attempt_and_does_not_loop() {
    let r = refused_resume_at_the_last_attempt("p6-wq-refused-twice", Refuse::Every);
    let w = &r.w;
    w.wait("the end of the fresh attempt", || {
        let a = w.attempts(&r.task).get(2).cloned()?;
        (a.1 == "ended").then_some(())
    });
    let a3 = w.attempts(&r.task)[2].clone();
    assert_eq!(
        a3.2,
        "no_result",
        "the failed fresh attempt is a normal failure\n{}",
        w.context()
    );
    w.wait("the task out of its retry budget", || {
        matches!(w.status(&r.task).as_str(), "failed" | "escalated").then_some(())
    });
    w.passes(QUIET_PASSES);
    assert_eq!(
        w.attempts(&r.task).len(),
        3,
        "no attempt after the burned last attempt\n{}",
        w.context()
    );
    // The wrapper of a2 also resumes its session 1 time with no START.md
    // (the repair pass of FM-6). No attempt after a2 resumes it.
    let refused = refused_argvs(w);
    let a3_dir = w.attempt_dir(&r.a3).to_string_lossy().into_owned();
    let names_a3 = |argv: &Vec<String>| argv.iter().any(|a| a.contains(&a3_dir));
    assert!(
        !refused
            .iter()
            .any(|argv| resumes(argv, &r.session) && names_a3(argv)),
        "the attempt after the refused resume {} does not resume {}: {refused:?}",
        r.a2,
        r.session
    );
    assert!(
        refused.iter().any(names_a3),
        "the fresh attempt {} launched: {refused:?}",
        r.a3
    );
}

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

#[test]
fn question_file_of_a_bad_form_gives_no_row_and_1_needs_you_item() {
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

#[test]
fn fake_sub_orchestrator_does_not_ask_and_makes_no_idea() {
    let w = World::new("p6-wq-fake", "stay,ask", &horch_ask(QUESTION));
    let task = w.task();
    w.blocked(&task);
    w.wait("a launch of the sub-orchestrator", || {
        w.one("SELECT id FROM attempts WHERE task_id IS NULL")
    });
    let lead_calls = || -> Vec<serde_json::Value> {
        w.h.calls_of("claude")
            .into_iter()
            .filter(|c| {
                c["argv"].as_array().into_iter().flatten().any(|a| {
                    a.as_str()
                        .unwrap_or_default()
                        .split_whitespace()
                        .any(|p| std::path::Path::new(p).join("idea.md").is_file())
                })
            })
            .collect()
    };
    // The attempt row comes before its CLI starts: wait for the call.
    w.wait("a call of the sub-orchestrator CLI", || {
        (!lead_calls().is_empty()).then_some(())
    });
    w.passes(QUIET_PASSES);
    let leads = lead_calls();
    let asks: Vec<String> = leads
        .iter()
        .filter(|c| c.get("ask").is_some())
        .map(|c| c.to_string())
        .collect();
    assert!(asks.is_empty(), "the sub-orchestrator asked: {asks:?}");
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
