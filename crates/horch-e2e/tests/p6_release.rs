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

const CLOCK: i64 = 5000;
const LATER: i64 = CLOCK + 86_400;

const FLEET_TOML: &str = "pass_period_s = 1\n";

const RUN: &str = "r1";
const SLUG: &str = "zeta";
const RUN_BRANCH: &str = "fleet/zeta";
const WORKSPACE: &str = "w1";
const ORCHESTRATOR_PANE: &str = "w1:p1";
const ORCHESTRATOR: &str = "w-orchestrator";

const FEATURE: &str = "F-0001";
const IDEA: &str = "I-0001";

const RUNNING_TASK: &str = "T-1";
const RUNNING_ATTEMPT: &str = "T-1/A1";
const WORKER: &str = "w-running";
const QUEUED_TASK: &str = "T-2";
const DONE_TASK: &str = "T-3";

const QUESTION: &str = "T-1/A1/Q1";
const QUESTION_TEXT: &str = "QUESTION: Which limit does the reader use? I recommend 10.";

const TICKETS: &str = "docs/tickets/zeta";

const RUN_TASKS: [(&str, i64, &str, &str); 3] = [
    (RUNNING_TASK, 1, "01-read.md", "Read the limit"),
    (QUEUED_TASK, 2, "02-log.md", "Log each retry"),
    (DONE_TASK, 3, "03-docs.md", "Document the limit"),
];

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

struct Flight {
    fleetds: Fleetds,
    worker: StandIn,
    orchestrator: StandIn,
    old_fleetd: StandIn,
    h: Harness,
}

impl Flight {
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

    fn heartbeat_at(&self) -> Option<String> {
        let path = self.h.fleet_root().join("run").join("heartbeat.json");
        let hb: Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
        hb["at"].as_str().map(str::to_string)
    }

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

fn git(h: &Harness, args: &[&str]) {
    let o = h.git_cmd(args).output().unwrap();
    assert!(o.status.success(), "git {args:?}: {o:?}");
}

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

fn read(h: &Harness) -> Connection {
    Connection::open(h.ledger_db()).unwrap()
}

fn snapshot(task: &str) -> String {
    let (_, _, _, title) = RUN_TASKS.iter().find(|t| t.0 == task).unwrap();
    ticket(title)
}

fn phase_5_ledger(h: &Harness, old: &StandIn, orchestrator: &StandIn, worker: &StandIn) {
    let mut conn = read(h);
    let now = util::now();
    let host = util::host_name();

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
    let sub_orchestrator = format!(
        "SELECT id FROM attempts WHERE task_id IS NULL AND idea_id = '{IDEA}' ORDER BY rowid"
    );
    wait_for_or(
        "fleetd to start the sub-orchestrator of the feature",
        || (!f.rows(&sub_orchestrator).is_empty()).then_some(()),
        || f.diagnostics(),
    );
    f.wait_passes(1);
    let running = format!(
        "SELECT id FROM attempts WHERE task_id IS NULL AND idea_id = '{IDEA}' AND state <> 'ended'"
    );
    assert!(
        f.rows(&running).len() <= 1,
        "at most 1 sub-orchestrator of the feature at a time\n{}",
        f.diagnostics()
    );
    assert_eq!(
        f.rows("SELECT DISTINCT owner FROM questions"),
        vec![owner],
        "no other owner after the start of the sub-orchestrator\n{}",
        f.diagnostics()
    );

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

    let map = format!("{TICKETS}/MAP.md");
    assert!(f.h.project.join(&map).is_file());
    let o = f.h.run(&["ledger", "add-map", "--dry-run", &map]);
    assert!(
        o.status.success(),
        "add-map --dry-run of the MAP.md of the run: {}",
        text(&o)
    );
}

type TaskRow = (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
);

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
