//! The release test of Phase 6 (`docs/specs/fleetd/design.md`, "What a
//! release does to a run in flight", row Phase 6; ticket 62 of
//! `docs/tickets/fleetd-p6/`; Q5, Q6 and Q11 of its MAP.md).
//!
//! A run with an open question starts on Phase 5. After the update, it has
//! 1 idea row and 1 feature row, and its question has 1 owner, the
//! sub-orchestrator `idea-lead:<feature id>`. The old orchestrator gets no
//! message, no task is lost, and the run's `MAP.md` stays readable by
//! `horch ledger add-map --dry-run`.
//!
//! The Phase 5 ledger is built by the real migration steps: the project and
//! the run `r1` are written before the step of [`schema::PHASE_4_VERSION`],
//! so its backfill gives the run its feature row, then the ledger goes to
//! [`schema::PHASE_5_VERSION`]. The other rows of the run in flight are
//! written as a Phase 5 binary left them:
//! - the orchestrator of the run, in the pane `w1:p1`;
//! - [`RUNNING_TASK`] `running`, with its live push attempt, its lease and
//!   its task folder;
//! - [`QUEUED_TASK`] `pending`: it waits for [`RUNNING_TASK`];
//! - [`DONE_TASK`] `done`;
//! - 1 open question of [`RUNNING_TASK`]: its mail question, delivered to
//!   the orchestrator before the update, its `questions` row owned by the
//!   orchestrator, and its `question` event;
//! - the `fleetd` row of the Phase 5 fleetd.
//!
//! The orchestrator, the worker of the live attempt and the old fleetd are
//! stand-ins: `sleep` processes that the rows name.
//!
//! The update: the test stops the old fleetd, migrates the ledger to
//! [`schema::PHASE_6_VERSION`] and starts `horch fleetd` of this branch,
//! with the fake herdr and the fake agents. Each process uses the temporary
//! folders of its harness, and each wait is for an event.

#![cfg(unix)]

use std::process::{Child, Command, Output};

use horch_core::fleetd::task_execution::adapters::task_files::attempt_dir;
use horch_core::messaging::mail::Mail;
use horch_core::procid;
use horch_core::taskledger::model::{Actor, EventKind, TaskStatus};
use horch_core::taskledger::runs::root_key;
use horch_core::taskledger::{schema, tasks, util};
use horch_e2e::fleetd::Fleetds;
use horch_e2e::harness::{fixtures, wait_for_or, Harness};
use rusqlite::Connection;
use serde_json::{json, Value};

/// The fleet clock of the Phase 5 `fleetd` row at the update.
const CLOCK: i64 = 5000;
/// The lease of the live attempt and the time limit of the question end 1
/// day of the fleet clock after the update: neither ends in a test.
const LATER: i64 = CLOCK + 86_400;

/// The `fleet.toml` of each test: 1 pass each second.
const FLEET_TOML: &str = "pass_period_s = 1\n";

/// The run in flight, its slug and its branch.
const RUN: &str = "r1";
const SLUG: &str = "zeta";
const RUN_BRANCH: &str = "fleet/zeta";
/// The herdr workspace of the run, and the pane of its orchestrator.
const WORKSPACE: &str = "w1";
const ORCHESTRATOR_PANE: &str = "w1:p1";
/// The orchestrator of the run.
const ORCHESTRATOR: &str = "w-orchestrator";

/// The feature and the idea that the run gets: `F-<run rowid>` and
/// `I-<run rowid>` (Q5).
const FEATURE: &str = "F-0001";
const IDEA: &str = "I-0001";

/// The task with the live attempt, its attempt and its worker.
const RUNNING_TASK: &str = "T-1";
const RUNNING_ATTEMPT: &str = "T-1/A1";
const WORKER: &str = "w-running";
/// The task that waits for [`RUNNING_TASK`].
const QUEUED_TASK: &str = "T-2";
/// The task that is done.
const DONE_TASK: &str = "T-3";

/// The `questions` row of the open question: attempt id and number, as
/// the question flow of Phase 5 names it.
const QUESTION: &str = "T-1/A1/Q1";
/// The text of the mail question of the worker.
const QUESTION_TEXT: &str = "QUESTION: Which limit does the reader use? I recommend 10.";

/// The folder of the tickets of the run, in the project.
const TICKETS: &str = "docs/tickets/zeta";

/// The tickets of the run: the task, its seq, its file name, its title.
const RUN_TASKS: [(&str, i64, &str, &str); 3] = [
    (RUNNING_TASK, 1, "01-read.md", "Read the limit"),
    (QUEUED_TASK, 2, "02-log.md", "Log each retry"),
    (DONE_TASK, 3, "03-docs.md", "Document the limit"),
];

/// A stand-in process: a `sleep` that a ledger row names. Killed at the
/// end of the test.
struct StandIn {
    child: Child,
    started: u64,
}

impl StandIn {
    fn start() -> StandIn {
        let child = Command::new("/bin/sleep").arg("600").spawn().unwrap();
        let started = procid::start_time(child.id()).expect("the start time of sleep");
        StandIn { child, started }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn alive(&self) -> bool {
        procid::alive(self.pid(), Some(self.started))
    }

    /// Stop it, and wait until it is gone.
    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for StandIn {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The run in flight on Phase 5 and fleetd of this branch after the
/// update. The fields drop in this order: each process stops before the
/// teardown of the harness checks that none is left.
struct Flight {
    fleetds: Fleetds,
    worker: StandIn,
    orchestrator: StandIn,
    old_fleetd: StandIn,
    h: Harness,
}

impl Flight {
    /// The Phase 5 run of the test `name`. fleetd of this branch does not
    /// run yet.
    fn on_phase_5(name: &str) -> Flight {
        let h = world(name);
        run_tickets(&h);
        let old_fleetd = StandIn::start();
        let orchestrator = StandIn::start();
        let worker = StandIn::start();
        phase_5_ledger(&h, &old_fleetd, &orchestrator, &worker);
        running_task_folder(&h);
        let fleetds = Fleetds::new(&h);
        Flight {
            fleetds,
            worker,
            orchestrator,
            old_fleetd,
            h,
        }
    }

    /// The update: stop the old fleetd, migrate the ledger to the schema of
    /// Phase 6 and start fleetd of this branch. Wait until its release duty
    /// wrote the approval of the idea of the run, then for 2 more passes.
    fn update(&mut self) {
        self.old_fleetd.stop();
        let mut conn = read(&self.h);
        schema::migrate_to(&mut conn, schema::PHASE_6_VERSION).unwrap();
        drop(conn);
        self.fleetds.start(&self.h, "fleetd");
        let approval = self
            .h
            .fleet_root()
            .join("ideas")
            .join(IDEA)
            .join("approval.json");
        wait_for_or(
            "the release to write the approval of the idea of the run",
            || approval.is_file().then_some(()),
            || self.diagnostics(),
        );
        self.wait_passes(2);
    }

    /// The `at` of the heartbeat of fleetd: it changes at the end of each
    /// pass.
    fn heartbeat_at(&self) -> Option<String> {
        let path = self.h.fleet_root().join("run").join("heartbeat.json");
        let hb: Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
        hb["at"].as_str().map(str::to_string)
    }

    /// Wait for the end of `n` passes of fleetd after this call.
    fn wait_passes(&self, n: usize) {
        for _ in 0..n {
            let before = self.heartbeat_at();
            wait_for_or(
                "1 more pass of fleetd",
                || {
                    let now = self.heartbeat_at();
                    (now.is_some() && now != before).then_some(())
                },
                || self.diagnostics(),
            );
        }
    }

    /// The rows of `sql`, each as 1 text column.
    fn rows(&self, sql: &str) -> Vec<String> {
        let conn = read(&self.h);
        let mut stmt = conn.prepare(sql).unwrap();
        let rows = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        rows
    }

    /// What a failed wait or assertion shows: the output of fleetd, the
    /// ideas, the features, the questions, the attempts, the tasks and
    /// their events.
    fn diagnostics(&self) -> String {
        let section = |name: &str, sql: &str| format!("{name}:\n{}", self.rows(sql).join("\n"));
        [
            format!(
                "fleetd.stderr:\n{}",
                self.fleetds.output("fleetd", "stderr")
            ),
            section(
                "ideas",
                "SELECT id || ' ' || feature_id || ' ' || state FROM ideas",
            ),
            section(
                "features",
                "SELECT id || ' ' || name || ' ' || state FROM features",
            ),
            section(
                "questions",
                "SELECT id || ' ' || feature_id || ' ' || owner || ' ' || state FROM questions",
            ),
            section(
                "attempts",
                "SELECT id || ' ' || IFNULL(task_id, '-') || ' ' || IFNULL(idea_id, '-') || ' ' \
                 || state || ' ' || IFNULL(outcome, '-') FROM attempts ORDER BY rowid",
            ),
            section(
                "tasks",
                "SELECT task_id || ' ' || status || ' ' || IFNULL(idea_id, '-') FROM tasks",
            ),
            section(
                "events",
                "SELECT IFNULL(task_id, IFNULL(entity_id, '-')) || ' ' || actor || ' ' || kind \
                 || ' ' || text FROM events ORDER BY event_id",
            ),
        ]
        .join("\n")
    }
}

/// A harness with its own ledger, fleet root, data folder and git
/// repository, no watchdog, a healthy quota, the workspace `w1` of the fake
/// herdr, and an empty local folder as the docs pages of the `docs_sync`
/// runs: no test uses the network. The fake agents stay live after their
/// start, so a sub-orchestrator does not end in a test.
fn world(name: &str) -> Harness {
    let mut h = Harness::new(name).with_git();
    let fleet = h.root.join("fleet");
    std::fs::create_dir_all(&fleet).unwrap();
    std::fs::write(fleet.join("fleet.toml"), FLEET_TOML).unwrap();
    h.set(
        "HORCH_LEDGER_DB",
        h.root.join("ledger.sqlite3").to_string_lossy(),
    );
    h.set("FLEET_ROOT", fleet.to_string_lossy());
    h.set("HORCH_DATA_DIR", h.root.join("data").to_string_lossy());
    h.set("HORCH_NO_WAKE", "1");
    h.set("HORCH_NO_WATCHDOG", "1");
    h.set(
        "HORCH_QUOTA_FILE",
        fixtures().join("quota/all-ok.json").to_string_lossy(),
    );
    h.set("HORCH_WORKSPACE_ID", WORKSPACE);
    h.set("HORCH_FAKE_SCENARIO", "exec,stay");
    let docs = h.root.join("docs-fixture");
    std::fs::create_dir_all(&docs).unwrap();
    h.set("HORCH_TEST_DOCS_FIXTURE", docs.to_string_lossy());
    let ws = h.fleet_workspace();
    assert_eq!(ws, WORKSPACE, "the run names the workspace w1");
    h
}

/// Run `git` in the project. It must pass.
fn git(h: &Harness, args: &[&str]) {
    let o = h.git_cmd(args).output().unwrap();
    assert!(o.status.success(), "git {args:?}: {o:?}");
}

/// The ticket of `title` in format C1.
fn ticket(title: &str) -> String {
    format!(
        "# {title}\n\n<!-- horch-ticket: 1 -->\n\
         **Persona:** primary `opus` · **Parallel:** yes\n\
         **Kind:** feature\n\
         **Check:** `true`\n\
         **Own:** `src/lib.rs`\n\n\
         ## Goal\n\n{title}.\n\n\
         ## Steps\n\n1. {title}.\n\n\
         ## Done when\n\nThe Check passes.\n"
    )
}

/// The `MAP.md` of the run and its tickets, committed in the project as the
/// planner of the old flow left them, the declared gates, and the run
/// branch.
fn run_tickets(h: &Harness) {
    let dir = h.project.join(TICKETS);
    std::fs::create_dir_all(&dir).unwrap();
    let mut map =
        format!("# Map: {SLUG}\n\n| # | Title | Blocked by | Wave |\n|---|---|---|---|\n");
    for (task, seq, file, title) in RUN_TASKS {
        std::fs::write(dir.join(file), ticket(title)).unwrap();
        let blocked_by = if task == QUEUED_TASK { "01" } else { "" };
        map.push_str(&format!(
            "| {seq:02} | [{title}]({file}) | {blocked_by} | {seq} |\n"
        ));
    }
    std::fs::write(dir.join("MAP.md"), map).unwrap();
    // The project declares its gates, so no hold of a missing fast gate
    // stops the run (Phase 4, Q-C).
    std::fs::write(
        h.project.join("fleet.toml"),
        "[gates]\nfast = \"true\"\nfull = \"true\"\n",
    )
    .unwrap();
    git(h, &["add", "fleet.toml", TICKETS]);
    git(
        h,
        &["commit", "--quiet", "--no-gpg-sign", "-m", "the run zeta"],
    );
    git(h, &["branch", RUN_BRANCH]);
}

/// A connection of the test to the ledger.
fn read(h: &Harness) -> Connection {
    Connection::open(h.ledger_db()).unwrap()
}

/// The ticket snapshot of `task`, as `add-map` stored it.
fn snapshot(task: &str) -> String {
    let (_, _, _, title) = RUN_TASKS.iter().find(|t| t.0 == task).unwrap();
    ticket(title)
}

/// The Phase 5 ledger of `h`: the run [`RUN`] with its feature, its
/// orchestrator, its 3 tasks, the live attempt of [`RUNNING_TASK`] and its
/// open question, and the `fleetd` row of `old`.
fn phase_5_ledger(h: &Harness, old: &StandIn, orchestrator: &StandIn, worker: &StandIn) {
    let mut conn = read(h);
    let now = util::now();
    let host = util::host_name();

    // The run exists before the step of Phase 4: its backfill writes the
    // feature row and the branch row of the run.
    schema::migrate_to(&mut conn, schema::PHASE_4_VERSION - 1).unwrap();
    conn.execute(
        "INSERT INTO projects (project_id, root, created_at) VALUES ('p', ?1, ?2)",
        (root_key(&h.project), &now),
    )
    .unwrap();
    conn.execute(
        "INSERT INTO runs (run_id, project_id, slug, base_branch, run_branch, workspace_id,
                status, started_at)
            VALUES (?1, 'p', ?2, 'main', ?3, ?4, 'active', ?5)",
        (RUN, SLUG, RUN_BRANCH, WORKSPACE, &now),
    )
    .unwrap();
    schema::migrate_to(&mut conn, schema::PHASE_5_VERSION).unwrap();

    conn.execute(
        "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
            VALUES (1, 1, ?1, ?2, ?3, ?4, ?3)",
        rusqlite::params![old.pid(), old.started, &now, CLOCK],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO workers (worker_id, run_id, kind, persona, role, host, workspace_id, pane_id,
                agent_pid, agent_started, last_checkin_at, status, spawned_at, started_at)
            VALUES (?1, ?2, 'orchestrator', 'orchestrator', 'orchestrator', ?3, ?4, ?5, ?6, ?7,
                ?8, 'working', ?8, ?8)",
        rusqlite::params![
            ORCHESTRATOR,
            RUN,
            &host,
            WORKSPACE,
            ORCHESTRATOR_PANE,
            orchestrator.pid(),
            orchestrator.started,
            &now
        ],
    )
    .unwrap();
    conn.execute(
        "UPDATE runs SET orchestrator_worker_id = ?1 WHERE run_id = ?2",
        (ORCHESTRATOR, RUN),
    )
    .unwrap();

    for (task, seq, file, title) in RUN_TASKS {
        let (status, attempts) = match task {
            RUNNING_TASK => (TaskStatus::Running, 1),
            QUEUED_TASK => (TaskStatus::Pending, 0),
            _ => (TaskStatus::Done, 1),
        };
        let text = snapshot(task);
        conn.execute(
            "INSERT INTO tasks (task_id, run_id, seq, title, task_file, primary_persona,
                    claimable_by, status, attempts, ticket_snapshot, ticket_sha, created_at,
                    updated_at)
                VALUES (?1, ?2, ?3, ?4, ?5, 'opus', 'opus', ?6, ?7, ?8, ?9, ?10, ?10)",
            rusqlite::params![
                task,
                RUN,
                seq,
                title,
                format!("{TICKETS}/{file}"),
                status.as_str(),
                attempts,
                &text,
                tasks::ticket_sha(&text),
                &now
            ],
        )
        .unwrap();
    }
    conn.execute(
        "INSERT INTO task_deps (task_id, blocked_by) VALUES (?1, ?2)",
        (QUEUED_TASK, RUNNING_TASK),
    )
    .unwrap();

    // The live push attempt of the running task.
    conn.execute(
        "INSERT INTO workers (worker_id, run_id, kind, persona, role, host, workspace_id,
                agent_pid, agent_started, last_checkin_at, task_id, attempt, status, spawned_at,
                started_at, claimed_at)
            VALUES (?1, ?2, 'worker', 'opus', ?1, ?3, ?4, ?5, ?6, ?7, ?8, 1, 'working', ?7, ?7, ?7)",
        rusqlite::params![
            WORKER,
            RUN,
            &host,
            WORKSPACE,
            worker.pid(),
            worker.started,
            &now,
            RUNNING_TASK
        ],
    )
    .unwrap();
    conn.execute(
        "UPDATE tasks SET current_worker_id = ?1 WHERE task_id = ?2",
        (WORKER, RUNNING_TASK),
    )
    .unwrap();
    conn.execute(
        "INSERT INTO attempts (id, task_id, worker_id, pid, pid_start, state, started_at,
                active_at, via)
            VALUES (?1, ?2, ?3, ?4, ?5, 'live', ?6, ?7, 'push')",
        rusqlite::params![
            RUNNING_ATTEMPT,
            RUNNING_TASK,
            WORKER,
            worker.pid(),
            worker.started,
            &now,
            CLOCK
        ],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO leases (task_id, attempt_id, expires_at, renewed_at, renew_basis)
            VALUES (?1, ?2, ?3, ?4, 'claim')",
        (RUNNING_TASK, RUNNING_ATTEMPT, LATER, &now),
    )
    .unwrap();

    open_question(h, &conn, &now);
}

/// The open question of the worker of [`RUNNING_TASK`], as Phase 5 left
/// it: the mail question, delivered to the orchestrator; the `questions`
/// row of the run, owned by the orchestrator; and its `question` event,
/// which holds the mail id.
fn open_question(h: &Harness, conn: &Connection, now: &str) {
    let mail = Mail::under(&h.state, WORKSPACE);
    let id = mail.enqueue(WORKER, QUESTION_TEXT).unwrap();
    mail.ack(&id).unwrap();
    assert!(
        mail.open_questions()
            .unwrap()
            .iter()
            .any(|m| m.id.as_str() == id.as_str()),
        "the mail question of the worker is open"
    );
    conn.execute(
        "INSERT INTO questions (id, task_id, feature_id, owner, state, due_at, asked_at)
            VALUES (?1, ?2, ?3, ?4, 'open', ?5, ?6)",
        rusqlite::params![QUESTION, RUNNING_TASK, RUN, ORCHESTRATOR, LATER, now],
    )
    .unwrap();
    let text = format!("question {QUESTION} asked");
    let data = json!({ "question_id": QUESTION, "mail_id": id.as_str(), "text": QUESTION_TEXT });
    conn.execute(
        "INSERT INTO events (at, run_id, task_id, actor, kind, text, entity, entity_id, cause,
                data_json)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'task', ?3, ?6, ?7)",
        rusqlite::params![
            now,
            RUN,
            RUNNING_TASK,
            Actor::Horch.as_str(),
            EventKind::Question.as_str(),
            &text,
            data.to_string()
        ],
    )
    .unwrap();
}

/// The task folder and the outbox of the live attempt, as the launch of
/// Phase 5 made them: `task.yaml`, `START.md` and an empty outbox, each
/// folder with mode 0700.
fn running_task_folder(h: &Harness) {
    use std::os::unix::fs::PermissionsExt;
    let dir = h.fleet_root().join("tasks").join(RUNNING_TASK);
    let attempt = attempt_dir(&h.fleet_root(), RUNNING_TASK, "A1");
    let outbox = attempt.join("outbox");
    std::fs::create_dir_all(&outbox).unwrap();
    for d in [
        h.fleet_root().join("tasks"),
        dir.clone(),
        dir.join("attempts"),
        attempt,
        outbox,
    ] {
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    std::fs::write(
        dir.join("task.yaml"),
        format!("task: {RUNNING_TASK}\nattempt: {RUNNING_ATTEMPT}\n"),
    )
    .unwrap();
    std::fs::write(dir.join("START.md"), snapshot(RUNNING_TASK)).unwrap();
}

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// The herdr calls that type into the pane `pane` or prompt its agent.
fn messages_to(h: &Harness, pane: &str) -> Vec<String> {
    h.calls_of("herdr")
        .into_iter()
        .filter_map(|c| {
            let argv: Vec<String> = c["argv"]
                .as_array()?
                .iter()
                .filter_map(|a| a.as_str().map(str::to_string))
                .collect();
            let message = matches!(
                argv.iter().map(String::as_str).collect::<Vec<_>>().as_slice(),
                ["agent", "prompt", p, ..]
                    | ["pane", "send-text", p, ..]
                    | ["pane", "send-keys", p, ..] if *p == pane
            );
            message.then(|| argv.join(" "))
        })
        .collect()
}

/// A run with an open question starts on Phase 5. After the update it has
/// 1 idea row, `in_progress`, and 1 feature row. Its question has 1 owner,
/// the sub-orchestrator of the feature, and fleetd starts that
/// sub-orchestrator. The old orchestrator gets no message, and the run's
/// `MAP.md` stays readable by `add-map --dry-run`.
#[test]
fn p6_release_run_with_an_open_question_gets_1_idea_1_feature_and_1_owner() {
    let mut f = Flight::on_phase_5("p6-release-owner");
    f.update();

    assert_eq!(
        f.rows("SELECT id || ' ' || feature_id || ' ' || state FROM ideas"),
        vec![format!("{IDEA} {FEATURE} in_progress")],
        "1 idea row, in_progress, of the feature of the run\n{}",
        f.diagnostics()
    );
    assert_eq!(
        f.rows(
            "SELECT id || ' ' || name || ' ' || IFNULL(branch, '-') || ' ' || state FROM features"
        ),
        vec![format!("{FEATURE} {SLUG} {RUN_BRANCH} open")],
        "1 feature row: the feature of the run\n{}",
        f.diagnostics()
    );

    let owner = format!("idea-lead:{FEATURE}");
    assert_eq!(
        f.rows("SELECT id || ' ' || feature_id || ' ' || owner FROM questions"),
        vec![format!("{QUESTION} {FEATURE} {owner}")],
        "the question of the run has 1 owner, the sub-orchestrator of the feature\n{}",
        f.diagnostics()
    );
    // The owner runs: fleetd starts 1 sub-orchestrator for the feature
    // (I13), and it takes the open question of the run.
    let sub_orchestrator = format!(
        "SELECT id FROM attempts WHERE task_id IS NULL AND idea_id = '{IDEA}' ORDER BY rowid"
    );
    wait_for_or(
        "fleetd to start the sub-orchestrator of the feature",
        || (!f.rows(&sub_orchestrator).is_empty()).then_some(()),
        || f.diagnostics(),
    );
    f.wait_passes(1);
    assert_eq!(
        f.rows(&sub_orchestrator).len(),
        1,
        "1 sub-orchestrator for the feature\n{}",
        f.diagnostics()
    );
    assert_eq!(
        f.rows("SELECT DISTINCT owner FROM questions"),
        vec![owner],
        "no other owner after the start of the sub-orchestrator\n{}",
        f.diagnostics()
    );

    // The old orchestrator gets no message after the update: no text is
    // typed into its pane, and its mail has no pending message.
    assert_eq!(
        messages_to(&f.h, ORCHESTRATOR_PANE),
        Vec::<String>::new(),
        "herdr typed into the pane of the old orchestrator\n{}",
        f.diagnostics()
    );
    let pending: Vec<String> = Mail::under(&f.h.state, WORKSPACE)
        .pending()
        .unwrap()
        .into_iter()
        .map(|m| m.line())
        .collect();
    assert_eq!(
        pending,
        Vec::<String>::new(),
        "the old orchestrator has new mail"
    );
    assert!(
        f.orchestrator.alive(),
        "the release does not stop the process of the old orchestrator"
    );

    // The MAP.md of the run in flight stays readable.
    let map = format!("{TICKETS}/MAP.md");
    assert!(f.h.project.join(&map).is_file());
    let o = f.h.run(&["ledger", "add-map", "--dry-run", &map]);
    assert!(
        o.status.success(),
        "add-map --dry-run of the MAP.md of the run: {}",
        text(&o)
    );
}

/// A task row: its id, status, idea, ticket snapshot and ticket sha.
type TaskRow = (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
);

/// No task of the run in flight is lost at the update: each task keeps its
/// row, its ticket snapshot and its state, or reaches `done`, and each
/// gets the idea of the run. The live attempt goes on.
#[test]
fn p6_release_loses_no_task() {
    let mut f = Flight::on_phase_5("p6-release-tasks");
    let before = f.rows("SELECT task_id || ' ' || status FROM tasks ORDER BY task_id");
    f.update();
    f.wait_passes(1);

    let conn = read(&f.h);
    let after: Vec<TaskRow> = conn
        .prepare(
            "SELECT task_id, status, idea_id, ticket_snapshot, ticket_sha FROM tasks
             ORDER BY task_id",
        )
        .unwrap()
        .query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    let ids: Vec<&str> = after.iter().map(|t| t.0.as_str()).collect();
    assert_eq!(
        ids,
        vec![RUNNING_TASK, QUEUED_TASK, DONE_TASK],
        "each task of the run keeps its row\n{}",
        f.diagnostics()
    );
    for (task, status, idea, snapshot_now, sha) in &after {
        let was = before
            .iter()
            .find_map(|row| row.strip_prefix(&format!("{task} ")))
            .unwrap();
        assert!(
            status == was || status == TaskStatus::Done.as_str(),
            "task {task} was {was} and is {status}: it keeps its state or reaches done\n{}",
            f.diagnostics()
        );
        assert_eq!(
            idea.as_deref(),
            Some(IDEA),
            "task {task} gets the idea of the run"
        );
        let want = snapshot(task);
        assert_eq!(
            snapshot_now.as_deref(),
            Some(want.as_str()),
            "task {task} keeps its ticket snapshot"
        );
        assert_eq!(
            sha.as_deref(),
            Some(tasks::ticket_sha(&want).as_str()),
            "task {task} keeps the sha of its ticket"
        );
    }

    // The live attempt goes on: fleetd does not stop its worker, and the
    // queued task still waits for it.
    assert!(f.worker.alive(), "fleetd stops no live worker");
    assert_eq!(
        f.rows(&format!(
            "SELECT state FROM attempts WHERE id = '{RUNNING_ATTEMPT}'"
        )),
        vec!["live".to_string()],
        "the live attempt of the run stays live\n{}",
        f.diagnostics()
    );
    assert_eq!(
        f.rows(&format!(
            "SELECT CAST(count(*) AS TEXT) FROM attempts WHERE task_id IN ('{QUEUED_TASK}', '{DONE_TASK}')"
        )),
        vec!["0".to_string()],
        "no attempt of a task that waits or is done\n{}",
        f.diagnostics()
    );
}
