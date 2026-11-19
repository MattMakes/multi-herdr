//! The end-to-end loop of docs sync and conformance through the `horch`
//! binary (docs/tickets/fleetd-p7/32-p7-docs-sync-e2e.md;
//! `docs/specs/fleetd/design.md`, "Docs sync and adapter conformance",
//! FR-8, FR-9, SR-1, SR-8, FM-18, FM-19).
//!
//! Each test runs a test fleetd in the sealed environment of a
//! [`Harness`]. fleetd finds the CLIs of `docs-sources.toml` on its `PATH`,
//! queues a `docs_sync` run and a `conformance` run for each, and each run
//! is a `horch job run` process. The CLIs are the fake agents of this
//! crate, each started by a small script that gives its version and its
//! scenario. A new script is a new CLI binary.
//!
//! No test uses the network: `HORCH_TEST_DOCS_FIXTURE` makes the fetch
//! port of the docs read each page from a local folder, and the `also_git`
//! repository of Codex is a local repository in that folder. No test reads
//! the clock of the machine: a day passes through the fleet clock of the
//! ledger. No test waits for a time: each wait is for a row or a file.
#![cfg(unix)]

use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::{Duration, Instant};

use horch_core::fleetd::ideas::application::needs_you;
use horch_core::fleetd::ideas::domain::needs_you::Item;
use horch_core::fleetd::worker_profiles::adapters::conformance_runner::ConformanceRunner;
use horch_core::fleetd::worker_profiles::application::conformance::{
    read_conformance_json, run_check,
};
use horch_core::fleetd::worker_profiles::application::health::unhealthy_profiles;
use horch_core::fleetd::worker_profiles::domain::conformance::{
    Check, ConformanceReport, Verdict, CONTEXT_LIMIT, FIRST_EVENT_LIMIT,
};
use horch_core::harness::HarnessKind;
use horch_core::roster::{Roster, Teammate};
use horch_core::runtime::{MapEnv, RuntimeContext};
use horch_core::taskledger::model::{Actor, WorkerKind};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::{schema, Store};
use horch_e2e::harness::{fixtures, repo_root, wait_for_or, Harness};
use rusqlite::Connection;

/// The `fleet.toml` of each test: 1 pass each second.
const FLEET_TOML: &str = "pass_period_s = 1\n";

/// The persona of the task of a dispatch test. Its agent is `claude`.
const PERSONA: &str = "opus";

/// The hang guard of a wait for the end of a job run.
const JOB_GUARD: Duration = Duration::from_secs(900);

/// The seconds between 2 daily `docs_sync` runs of 1 CLI (MAP.md Q11).
const DAY_S: i64 = 86_400;

/// The docs sources with an exec adapter, and the fake of each. herdr is a
/// docs source only (MAP.md Q5).
const EXEC_CLIS: [(&str, &str); 3] = [
    ("claude", "fake-claude"),
    ("codex", "fake-codex"),
    ("agy", "fake-antigravity"),
];

/// The version text of the first binary of each exec CLI.
fn first_version(cli: &str) -> &'static str {
    match cli {
        "claude" => "2.1.284 (Claude Code)",
        // The lowest Codex on which the operator boundary is proven on
        // macOS.
        "codex" => "codex-cli 0.167.0",
        _ => "1.1.3",
    }
}

/// The version text of the new Claude Code binary of a drift.
const NEW_CLAUDE: &str = "2.2.0 (Claude Code)";

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

// ─── the docs fixture ───────────────────────────────────────────────────

/// 1 source of `docs-sources.toml`: the keys that the fixture needs.
#[derive(Default)]
struct Source {
    kind: String,
    url: String,
    md_suffix: String,
    include: Vec<String>,
    also_git: Option<String>,
}

/// The text of a string value `"..."` of 1 line of `docs-sources.toml`.
fn quoted(value: &str) -> String {
    value
        .trim()
        .trim_start_matches('"')
        .split('"')
        .next()
        .unwrap_or_default()
        .to_string()
}

/// Each source of `crates/agent-docs-sync/docs-sources.toml`. The file has
/// 1 key on each line, so a line split is enough here.
fn sources() -> Vec<Source> {
    let file = repo_root().join("crates/agent-docs-sync/docs-sources.toml");
    let mut out: Vec<Source> = Vec::new();
    for line in read(&file).lines() {
        let line = line.trim();
        if line.starts_with('[') {
            out.push(Source::default());
            continue;
        }
        let (Some(source), Some((key, value))) = (out.last_mut(), line.split_once('=')) else {
            continue;
        };
        match key.trim() {
            "kind" => source.kind = quoted(value),
            "url" => source.url = quoted(value),
            "md_suffix" => source.md_suffix = quoted(value),
            "also_git" => source.also_git = Some(quoted(value)),
            "include" => {
                let list = value.trim().trim_start_matches('[').trim_end_matches(']');
                source.include = list.split(',').map(quoted).collect();
            }
            _ => {}
        }
    }
    assert!(out.len() >= 4, "the docs sources of {}", file.display());
    out
}

/// The file of a URL in the fixture: its host and path, and `index.html`
/// for a path that ends with `/`.
fn fixture_path(fixture: &Path, url: &str) -> PathBuf {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let mut path = fixture.join(rest);
    if rest.ends_with('/') {
        path = path.join("index.html");
    }
    path
}

fn put(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// The pages of each docs source under `fixture`: the sitemap fixture of
/// agent-docs-sync for herdr, an `llms.txt` with each included page for the
/// others, and a local repository for each `also_git`.
fn docs_fixture(h: &Harness, fixture: &Path) {
    let herdr = repo_root().join("crates/agent-docs-sync/tests/fixtures/herdr");
    for source in sources() {
        if source.kind == "sitemap" {
            put(
                &fixture_path(fixture, &source.url),
                &read(&herdr.join("sitemap.xml")),
            );
            for (url, file) in [
                ("https://herdr.dev/docs/", "index.html"),
                ("https://herdr.dev/docs/install/", "install.html"),
            ] {
                put(&fixture_path(fixture, url), &read(&herdr.join(file)));
            }
        } else {
            let base = source.url.trim_end_matches("llms.txt");
            let mut index = String::from("# docs\n\n");
            for entry in &source.include {
                let url = format!("{base}{entry}{}", source.md_suffix);
                index.push_str(&format!("- [{entry}]({url}): {entry}.\n"));
                put(
                    &fixture_path(fixture, &url),
                    &format!("# {entry}\n\nA page.\n"),
                );
            }
            put(&fixture_path(fixture, &source.url), &index);
        }
        if let Some(url) = &source.also_git {
            let repo = fixture_path(fixture, url);
            put(&repo.join("docs/guide.md"), "# Guide\n");
            for args in [
                &["init", "-q", "-b", "main"][..],
                &["add", "."],
                &["commit", "-q", "--no-gpg-sign", "-m", "docs"],
            ] {
                let o = h.git_cmd(args).current_dir(&repo).output().unwrap();
                assert!(o.status.success(), "git {args:?}: {}", text(&o));
            }
        }
    }
}

// ─── the world of 1 test ────────────────────────────────────────────────

/// A test fleetd with the fake CLIs on its `PATH`, the docs fixture, 1 run
/// of the project (the project of a drift idea) and an empty workspace
/// `w1` whose panes run their commands.
struct World {
    h: Harness,
    run: Run,
}

/// The world of 1 test with the exec CLIs `clis` on the `PATH` of fleetd.
/// Each other exec CLI is not installed, so it gets no docs job: a test
/// that needs only Claude Code runs only its suite.
fn world(name: &str, clis: &[&str]) -> World {
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
    h.set("HORCH_WORKSPACE_ID", "w1");
    let docs = h.root.join("docs-fixture");
    docs_fixture(&h, &docs);
    h.set("HORCH_TEST_DOCS_FIXTURE", docs.to_string_lossy());
    // The docs sync of Codex clones its `also_git` repository with the
    // `git` of the `PATH`.
    let git = h.git_bin().expect("the git of with_git").to_path_buf();
    std::os::unix::fs::symlink(git, h.bin.join("git")).unwrap();
    let w = World {
        run: seed_run(&h),
        h,
    };
    for (cli, _) in EXEC_CLIS {
        if clis.contains(&cli) {
            w.install(cli, first_version(cli), "");
        } else {
            std::fs::remove_file(w.h.bin.join(cli)).unwrap();
        }
    }
    let mut create = std::process::Command::new(w.h.bin.join("herdr"));
    w.h.seal(&mut create);
    let o = create
        .args(["workspace", "create", "--label", name])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", text(&o));
    w.h.serve_fleetd();
    w
}

/// The run `r1` of the project, its idea in flight (FR-12), and the seat
/// of its orchestrator in the pane `w1:p1`, where fleetd opens the pane of
/// each attempt.
fn seed_run(h: &Harness) -> Run {
    let store = Store::open(&h.ledger_db()).unwrap();
    let conn = store.conn();
    let run = runs::create(
        conn,
        &h.project,
        &NewRun {
            slug: "r1".into(),
            base_branch: Some("main".into()),
            workspace_id: Some("w1".into()),
            ..Default::default()
        },
    )
    .unwrap();
    schema::run_idea(conn, &run.run_id, &horch_core::clock::now_stamp(), true)
        .unwrap()
        .expect("the idea of r1");
    let seat = workers::insert(
        conn,
        &NewWorker {
            run_id: run.run_id.clone(),
            kind: Some(WorkerKind::Orchestrator),
            persona: "orchestrator".into(),
            workspace_id: Some("w1".into()),
            pane_id: Some("w1:p1".into()),
            ..Default::default()
        },
    )
    .unwrap();
    runs::set_orchestrator(conn, &run.run_id, &seat).unwrap();
    run
}

/// 1 run of a job: id, subject, run number, state and repository.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Job {
    id: i64,
    subject: String,
    run: i64,
    state: String,
    repo: Option<String>,
}

impl Job {
    fn ended(&self) -> bool {
        matches!(self.state.as_str(), "passed" | "failed" | "abandoned")
    }
}

impl World {
    /// The script `<bin>/<cli>` that starts the fake of `cli` with the
    /// version text `version` and the conformance probes, and with the
    /// extra scenario keys `extra`. A new script is a new binary id.
    fn install(&self, cli: &str, version: &str, extra: &str) {
        let fake = EXEC_CLIS
            .iter()
            .find(|(c, _)| *c == cli)
            .map(|(_, f)| *f)
            .unwrap();
        let built = horch_e2e::bin_dir().join(fake);
        let scenario = format!("conformance,version:{version}{extra}");
        // Codex answers `codex features list` with the `network_proxy`
        // feature, which denies the connect to the command channel.
        let features = if cli == "codex" {
            "case \"$1\" in features) echo 'network_proxy  experimental  true'; exit 0 ;; esac\n"
        } else {
            ""
        };
        let script = format!(
            "#!/bin/sh\n{features}HORCH_FAKE_SCENARIO='{scenario}' exec '{}' \"$@\"\n",
            built.display()
        );
        let path = self.h.bin.join(cli);
        let _ = std::fs::remove_file(&path);
        // A new file, then a rename: fleetd never reads half a script.
        let tmp = self.h.root.join(format!("{cli}.new"));
        std::fs::write(&tmp, script).unwrap();
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::rename(&tmp, &path).unwrap();
    }

    fn ledger(&self) -> Connection {
        let conn = Connection::open(self.h.ledger_db()).unwrap();
        conn.busy_timeout(Duration::from_secs(10)).unwrap();
        conn
    }

    /// Each run of `kind` of the CLI `cli`, oldest first.
    fn jobs(&self, kind: &str, cli: &str) -> Vec<Job> {
        let conn = self.ledger();
        let mut stmt = conn
            .prepare(
                "SELECT id, subject, run, state, repo FROM jobs
                 WHERE kind = ?1 AND subject LIKE ?2 ORDER BY id",
            )
            .unwrap();
        stmt.query_map([kind, &format!("{cli} %")], |r| {
            Ok(Job {
                id: r.get(0)?,
                subject: r.get(1)?,
                run: r.get(2)?,
                state: r.get(3)?,
                repo: r.get(4)?,
            })
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
    }

    /// What a failed wait shows: the jobs, the output of fleetd and the
    /// launch log.
    fn diagnostics(&self) -> String {
        let conn = self.ledger();
        let mut stmt = conn
            .prepare(
                "SELECT id || ' ' || kind || ' ' || subject || ' run ' || run || ' '
                 || state || ' ' || COALESCE(result_json, '') FROM jobs ORDER BY id",
            )
            .unwrap();
        let jobs: Vec<String> = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        let mut failed = Vec::new();
        let docs = self.h.fleet_root().join("docs");
        for (cli, _) in EXEC_CLIS {
            for dir in std::fs::read_dir(docs.join(cli)).into_iter().flatten() {
                let path = dir.unwrap().path().join("conformance.json");
                let Ok(report) = read_conformance_json(&read(&path)) else {
                    continue;
                };
                failed.extend(
                    report
                        .checks
                        .iter()
                        .filter(|r| r.verdict == Verdict::Fail)
                        .map(|r| format!("{cli} {}: {r:?}", report.version)),
                );
            }
        }
        format!(
            "jobs:\n{}\nfailed checks:\n{}\nfleetd stderr:\n{}\nlaunch log:\n{}",
            jobs.join("\n"),
            failed.join("\n"),
            read(&self.h.root.join("fleetd.stderr")),
            read(&self.h.fleet_root().join("run/launch.log")),
        )
    }

    /// Wait until `f` gives a value. A failed wait shows the diagnostics.
    fn wait<T>(&self, what: &str, f: impl FnMut() -> Option<T>) -> T {
        wait_for_or(what, f, || self.diagnostics())
    }

    /// Wait until a run of `kind` of `cli` after the run `after` (a job id)
    /// ends, and give it.
    /// A run ends by itself: its process ends, or fleetd ends it at its
    /// time limit. [`JOB_GUARD`] only stops a test that hangs, so the speed
    /// of the host does not decide the test.
    fn ended_after(&self, kind: &str, cli: &str, after: i64) -> Job {
        let until = Instant::now() + JOB_GUARD;
        while Instant::now() < until {
            let ended = self
                .jobs(kind, cli)
                .into_iter()
                .find(|j| j.id > after && j.ended());
            if let Some(job) = ended {
                return job;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        panic!(
            "no ended {kind} run of {cli} after job {after} in {} s\n{}",
            JOB_GUARD.as_secs(),
            self.diagnostics()
        );
    }

    /// Wait until the first `docs_sync` run and the first `conformance` run
    /// of each exec CLI end, and give the conformance runs. Only the docs
    /// sync must pass here: the verdict of a conformance run is the subject
    /// of the tests that read it.
    fn first_round(&self) -> Vec<Job> {
        EXEC_CLIS
            .iter()
            .filter(|(cli, _)| self.h.bin.join(cli).exists())
            .map(|(cli, _)| {
                let docs = self.ended_after("docs_sync", cli, 0);
                assert_eq!(docs.state, "passed", "{docs:?}\n{}", self.diagnostics());
                self.ended_after("conformance", cli, docs.id)
            })
            .collect()
    }

    /// The report of the newest `conformance.json` of the CLI `cli` whose
    /// version is `version`.
    fn report(&self, cli: &str, version: &str) -> ConformanceReport {
        let docs = self.h.fleet_root().join("docs").join(cli);
        let found = std::fs::read_dir(&docs)
            .unwrap_or_else(|e| panic!("{}: {e}", docs.display()))
            .filter_map(|e| {
                let path = e.ok()?.path().join("conformance.json");
                let report = read_conformance_json(&read(&path)).ok()?;
                (report.version == version).then_some(report)
            })
            .next();
        found.unwrap_or_else(|| {
            panic!(
                "no conformance.json of {cli} {version} in {}\n{}",
                docs.display(),
                self.diagnostics()
            )
        })
    }

    /// A drift: a new Claude Code binary whose run ends with no
    /// `outbox/result.json`, after a first round that passed. Gives the
    /// failed `conformance` run of the new binary.
    fn drift(&self) -> Job {
        let first = self.first_round();
        let last = first.iter().map(|j| j.id).max().unwrap();
        self.install("claude", NEW_CLAUDE, ",exit_without_result");
        let docs = self.ended_after("docs_sync", "claude", last);
        let run = self.ended_after("conformance", "claude", docs.id);
        assert_eq!(run.state, "failed", "{run:?}\n{}", self.diagnostics());
        self.wait("the health marks of the failed run", || {
            (!self.unhealthy().is_empty()).then_some(())
        });
        run
    }

    /// Each unhealthy profile.
    fn unhealthy(&self) -> BTreeSet<String> {
        unhealthy_profiles(&self.ledger())
            .unwrap()
            .into_keys()
            .collect()
    }

    /// The open needs-you items of a failed conformance run whose text
    /// starts with `title`.
    fn drift_items(&self, title: &str) -> Vec<Item> {
        needs_you::open_items(&self.ledger())
            .unwrap()
            .into_iter()
            .filter(|i| i.kind == "adapter-conformance-failed" && i.text.starts_with(title))
            .collect()
    }

    /// 1 queued task of the run for [`PERSONA`]. Gives its id.
    fn add_task(&self) -> String {
        let ticket = "# T0\n";
        let dir = self.h.project.join("tickets");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("t0.md"), ticket).unwrap();
        let mut store = Store::open(&self.h.ledger_db()).unwrap();
        store
            .immediate(|tx| {
                let mut new = NewTask::work("t0", "tickets/t0.md", PERSONA);
                new.ticket_snapshot = Some(ticket.to_string());
                tasks::add(tx, &self.run, &new, Actor::Orchestrator)
            })
            .unwrap()
            .task_id
    }

    /// The status of the task `task`, and its count of attempts.
    fn task(&self, task: &str) -> (String, i64) {
        self.ledger()
            .query_row(
                "SELECT status, (SELECT COUNT(*) FROM attempts WHERE task_id = ?1)
                 FROM tasks WHERE task_id = ?1",
                [task],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
    }

    /// The output of `horch ledger status`.
    fn status(&self) -> String {
        let o = self.h.run(&["ledger", "status"]);
        String::from_utf8_lossy(&o.stdout).into_owned()
    }
}

/// Each profile of the built-in roster whose agent is `agent`.
fn profiles_of(agent: HarnessKind) -> BTreeSet<String> {
    let roster = Roster::builtin().unwrap();
    roster
        .names()
        .into_iter()
        .filter(|n| roster.get(n).is_some_and(|t| t.agent == agent))
        .map(str::to_owned)
        .collect()
}

// ─── the tests ──────────────────────────────────────────────────────────

/// Gate test: a `docs_sync` run and a `conformance` run have no repository,
/// and each is a valid row that ends.
#[test]
fn docs_sync_and_conformance_rows_have_no_repository() {
    let w = world("p7-norepo", &["claude", "codex", "agy"]);
    let runs = w.first_round();
    for (cli, _) in EXEC_CLIS {
        let docs = w.jobs("docs_sync", cli);
        assert!(!docs.is_empty(), "{cli}");
        for job in docs.iter().chain(w.jobs("conformance", cli).iter()) {
            assert_eq!(job.repo, None, "{job:?}");
            assert!(job.subject.starts_with(&format!("{cli} ")), "{job:?}");
        }
    }
    assert_eq!(runs.len(), EXEC_CLIS.len());
    // herdr is a docs source with no exec adapter: a docs sync, and no
    // conformance run (MAP.md Q5).
    let herdr = w.ended_after("docs_sync", "herdr", 0);
    assert_eq!(herdr.repo, None, "{herdr:?}");
    assert!(w.jobs("conformance", "herdr").is_empty());
}

/// Gate test: a day after the last `docs_sync` run of a CLI started, a new
/// run starts.
#[test]
fn next_day_starts_a_new_docs_sync_run() {
    let w = world("p7-daily", &["claude"]);
    let first = w.first_round();
    let before = w.jobs("docs_sync", "claude");
    assert_eq!(before.len(), 1, "{before:?}");
    let last = first.iter().map(|j| j.id).max().unwrap();

    w.h.advance_fleet_clock(DAY_S + 1);
    let next = w.ended_after("docs_sync", "claude", last);
    assert_eq!(next.state, "passed", "{next:?}\n{}", w.diagnostics());
    // The same binary: the new run is for the same subject.
    assert_eq!(next.subject, before[0].subject);
    assert!(
        (next.run, next.id) != (before[0].run, before[0].id),
        "{next:?}"
    );
}

/// FR-8, FM-19: a new CLI binary whose run fails a check marks each
/// profile of that CLI unhealthy and adds exactly 1 drift idea, which you
/// see in `needs-you/`.
#[test]
fn new_fake_cli_version_fails_a_check_marks_its_profiles_unhealthy_and_adds_1_idea() {
    let w = world("p7-drift", &["claude"]);
    let run = w.drift();

    let report = w.report("claude", NEW_CLAUDE);
    let failed: Vec<Check> = report.failed();
    assert!(failed.contains(&Check::SharedWrite), "{report:?}");

    // Each profile of the agent `claude` is unhealthy. A failed run of
    // another CLI marks only the profiles of its own agent.
    let claude = profiles_of(HarnessKind::Claude);
    assert!(claude.contains(PERSONA), "{claude:?}");
    let unhealthy = w.unhealthy();
    assert!(
        claude.is_subset(&unhealthy),
        "not unhealthy: {:?}\n{}",
        claude.difference(&unhealthy).collect::<Vec<_>>(),
        w.diagnostics()
    );

    // Exactly 1 drift idea for the new version, with its needs-you item.
    let title = format!("Update the claude adapter for {NEW_CLAUDE}");
    let drift = w.wait("the needs-you item of the drift idea", || {
        let ours = w.drift_items(&title);
        (!ours.is_empty()).then_some(ours)
    });
    assert_eq!(drift.len(), 1, "{drift:?}");
    let idea = &drift[0].entity_id;
    assert_eq!(drift[0].entity, "idea", "{drift:?}");

    // The idea gives CHANGES.md of the CLI, then the failed checks.
    let idea_md = w.wait("the idea.md of the drift idea", || {
        [w.h.fleet_root(), w.h.project.clone()]
            .iter()
            .map(|root| root.join("ideas").join(idea).join("idea.md"))
            .find(|p| p.is_file())
    });
    let body = read(&idea_md);
    let changes = w.h.fleet_root().join("docs/claude/CHANGES.md");
    let at = |what: &str| {
        body.find(what)
            .unwrap_or_else(|| panic!("no {what:?} in the idea: {body}"))
    };
    assert!(at(&title) < at(&changes.display().to_string()), "{body}");
    assert!(
        at(&changes.display().to_string()) < at(Check::SharedWrite.name()),
        "{body}"
    );

    // You see it in the needs-you list of `horch ideas`. fleetd writes no
    // `needs-you/` folder today: no production code calls
    // `needs_you_folder::mirror` (reported in the done summary of T-602).
    let o = w.h.run(&["ideas", "--json"]);
    assert!(o.status.success(), "{}", text(&o));
    let board: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    let listed: Vec<&serde_json::Value> = board["needs_you"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|l| l["kind"] == "adapter-conformance-failed")
        .filter(|l| l["text"].as_str().is_some_and(|t| t.starts_with(&title)))
        .collect();
    assert_eq!(listed.len(), 1, "{}", text(&o));
    let listed_text = listed[0]["text"].as_str().unwrap();
    assert!(listed_text.contains(idea.as_str()), "{listed_text}");
    // The pass noted the run, so it adds no second idea for it.
    assert_eq!(w.drift_items(&title).len(), 1, "conformance run {run:?}");
}

/// FR-8 (Q15): fleetd dispatches no task to an unhealthy profile, and
/// `horch ledger status` names the hold.
#[test]
fn unhealthy_profile_gets_no_dispatch_and_status_names_the_hold() {
    let w = world("p7-hold", &["claude"]);
    w.drift();
    let task = w.add_task();
    let hold = format!("hold on dispatch: unhealthy profile {PERSONA}");
    w.wait("the status to name the hold", || {
        w.status().contains(&hold).then_some(())
    });
    let (status, attempts) = w.task(&task);
    assert_eq!(
        (status.as_str(), attempts),
        ("ready", 0),
        "{}",
        w.diagnostics()
    );
}

/// FR-8: a passed conformance run of a new binary marks the profiles
/// healthy again, and dispatch resumes.
#[test]
fn passed_conformance_marks_the_profiles_healthy_and_dispatch_resumes() {
    horch_e2e::expected::check(
        file!(),
        "passed_conformance_marks_the_profiles_healthy_and_dispatch_resumes",
        recovery,
    );
}

fn recovery() {
    let w = world("p7-recover", &["claude"]);
    let failed = w.drift();
    let task = w.add_task();
    let hold = format!("hold on dispatch: unhealthy profile {PERSONA}");
    w.wait("the hold", || w.status().contains(&hold).then_some(()));

    w.install("claude", "2.2.1 (Claude Code)", "");
    let docs = w.ended_after("docs_sync", "claude", failed.id);
    let run = w.ended_after("conformance", "claude", docs.id);
    assert!(
        run.state == "passed",
        "the conformance run of the new binary did not pass: {run:?}\n{}",
        w.diagnostics()
    );
    w.wait("the healthy marks", || {
        let claude = profiles_of(HarnessKind::Claude);
        w.unhealthy().is_disjoint(&claude).then_some(())
    });
    w.wait("the dispatch of the task", || {
        let (status, attempts) = w.task(&task);
        (status != "ready" && attempts > 0).then_some(())
    });
    assert!(!w.status().contains(&hold), "{}", w.status());
}

/// Gate test (SR-8): the check "Operator boundary" passes for each
/// adapter. The 3 exec adapters run it in the `conformance` job of the
/// test fleetd. OpenCode, pi and Prime have no conformance run (MAP.md
/// Q5), so the test runs only this check for them against the fleet root
/// of the test fleetd: its token file, its channel and its request folder.
/// An adapter with no sandbox on this OS gives `not supported` with its
/// reason, never `pass` (ticket 24).
#[test]
fn operator_boundary_passes_for_each_adapter() {
    horch_e2e::expected::check(
        file!(),
        "operator_boundary_passes_for_each_adapter",
        operator_boundary,
    );
}

/// The body of [`operator_boundary_passes_for_each_adapter`]. It checks
/// each adapter, then fails 1 time with each adapter whose check failed.
fn operator_boundary() {
    let w = world("p7-boundary", &["claude", "codex", "agy"]);
    w.first_round();
    let mut failed = Vec::new();
    let mut verdicts = Vec::new();
    for (cli, _) in EXEC_CLIS {
        let report = w.report(cli, first_version(cli));
        let r = report
            .checks
            .iter()
            .find(|r| r.check == Check::OperatorBoundary)
            .unwrap_or_else(|| panic!("{cli}: {report:?}"))
            .clone();
        verdicts.push((cli.to_string(), r));
    }

    let fleet = w.h.fleet_root();
    assert!(fleet.join("run/operator.token").is_file());
    let s = |p: PathBuf| p.to_string_lossy().into_owned();
    let env = MapEnv::new(w.h.home.clone())
        .with("HOME", &s(w.h.home.clone()))
        .with("PATH", &s(w.h.bin.clone()))
        .with("HORCH_STATE_DIR", &s(w.h.state.clone()))
        .with("HORCH_DATA_DIR", &s(w.h.root.join("data")))
        .with("HORCH_LEDGER_DB", &s(w.h.ledger_db()))
        .with("FLEET_ROOT", &s(fleet));
    let ctx = RuntimeContext::from_env(&env).unwrap();
    for agent in [HarnessKind::OpenCode, HarnessKind::Pi, HarnessKind::Prime] {
        let teammate = Teammate {
            name: format!("conformance-{}", agent.as_str()),
            agent,
            model: Some("test-model".into()),
            ..Teammate::default()
        };
        let mut runner = ConformanceRunner {
            ctx: &ctx,
            teammate: &teammate,
            fleet_window: Some((CONTEXT_LIMIT, "context-windows.md".into())),
            env_rule: Vec::new(),
            scratch: w.h.root.join("scratch"),
            first_event_limit: FIRST_EVENT_LIMIT,
        };
        let r = run_check(&mut runner, Check::OperatorBoundary);
        verdicts.push((agent.as_str().to_string(), r));
    }

    for (adapter, r) in &verdicts {
        assert!(!r.evidence.is_empty(), "{adapter}: {r:?}");
        let ok = match r.verdict {
            Verdict::Pass => r.evidence.starts_with("refused: "),
            // An adapter with no sandbox on this OS (ticket 24). Claude
            // Code and Codex have one on each OS of the fleet.
            Verdict::NotSupported => adapter != "claude" && adapter != "codex",
            Verdict::Fail => false,
        };
        if !ok {
            failed.push(format!("{adapter}: {r:?}"));
        }
    }
    assert!(
        failed.is_empty(),
        "the check \"Operator boundary\" failed for {} adapters: {}",
        failed.len(),
        failed.join("; ")
    );
}

/// FR-9: the check "Context limit" of each exec CLI reads back 256000.
#[test]
fn context_limit_check_reads_back_256000() {
    let w = world("p7-256k", &["claude", "codex", "agy"]);
    w.first_round();
    for (cli, _) in EXEC_CLIS {
        let report = w.report(cli, first_version(cli));
        let r = report
            .checks
            .iter()
            .find(|r| r.check == Check::ContextLimit)
            .unwrap_or_else(|| panic!("{cli}: {report:?}"));
        assert_eq!(r.verdict, Verdict::Pass, "{cli}: {r:?}");
        assert!(r.evidence.contains("256000"), "{cli}: {r:?}");
    }
}
