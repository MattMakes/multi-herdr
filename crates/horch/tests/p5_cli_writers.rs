//! The last direct writers of the CLI send requests (T-885; RR-5, FM-24):
//! `horch rescue drop`, `horch ledger status` (the workspace adoption),
//! `horch ledger import --dry-run`, `horch note` of a worker with no live
//! attempt, `horch ledger unfinished` and the refusal of the triage
//! decisions that no fleetd job does yet.
//!
//! Each test runs the development `horch` with the seam
//! `HORCH_TEST_ASSERT_READ_ONLY=1`: a writable open of the ledger in that
//! process panics. The test process plays fleetd: it writes the `fleetd`
//! row with its own PID and start time, makes the operator token, and
//! serves the channel in-process with the handlers of fleetd. All folders
//! are temporary.
//!
//! The tests are unix only: the socket path must be short.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use horch_core::fleetd::fleet_runtime::adapters::channel_server::ChannelServer;
use horch_core::fleetd::fleet_runtime::adapters::token_file;
use horch_core::fleetd::fleet_runtime::application::handlers_cli;
use horch_core::fleetd::fleet_runtime::application::handlers_custody::{self, Custody};
use horch_core::fleetd::fleet_runtime::application::handlers_run;
use horch_core::fleetd::fleet_runtime::application::requests::Handlers;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::fleetd::integration::application::commands::GitRefTips;
use horch_core::fleetd::worktree_custody::adapters::git::CliWorktreeGit;
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::store::ASSERT_READ_ONLY_ENV;
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::{util, Store};
use rusqlite::{params, Connection};

const PATH: &str = "/usr/bin:/bin";
const GIT: &str = "/usr/bin/git";

struct Out {
    code: i32,
    out: String,
    err: String,
}

/// A fleet in temporary folders: a git project with 1 commit on `main`, a
/// ledger with the run `r1`, and the channel served by this process as
/// fleetd.
struct World {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    run_id: String,
    server: Option<ChannelServer>,
}

impl World {
    fn new() -> World {
        // A short folder: `sun_path` holds about 104 bytes on macOS.
        let tmp = tempfile::Builder::new()
            .prefix("cw")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir_in("/tmp")
            .unwrap();
        let root = tmp.path().canonicalize().unwrap();
        for dir in ["home", "fleet", "proj", "state"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        let mut w = World {
            _tmp: tmp,
            root,
            run_id: String::new(),
            server: None,
        };
        let proj = w.proj();
        git(&proj, &["init", "-q", "-b", "main"]);
        std::fs::write(proj.join("a.txt"), "a\n").unwrap();
        git(&proj, &["add", "a.txt"]);
        git(&proj, &["commit", "-q", "-m", "first"]);
        let mut store = Store::open(&w.ledger()).unwrap();
        let run = store
            .immediate(|tx| {
                let new_run = NewRun {
                    slug: "r1".into(),
                    base_branch: Some("main".into()),
                    ..Default::default()
                };
                runs::create(tx, &proj, &new_run)
            })
            .unwrap();
        w.run_id = run.run_id;
        let pid = std::process::id();
        let start = horch_platform::process::start_time(pid).expect("the start time of the test");
        store
            .conn()
            .execute(
                "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
                 VALUES (1, 1, ?1, ?2, ?3, 0, ?3)",
                params![pid, i64::try_from(start).unwrap(), util::now()],
            )
            .unwrap();
        token_file::ensure(&w.fleet()).unwrap();
        w.server = Some(w.serve());
        w
    }

    /// The channel with the handlers of fleetd that these commands use.
    fn serve(&self) -> ChannelServer {
        let mut handlers = Handlers::new();
        handlers_run::register(&mut handlers);
        handlers_cli::register(&mut handlers);
        let refs = CliWorktreeGit::new(GIT, self.fleet()).reading(&[self.proj()]);
        handlers_custody::register(
            &mut handlers,
            Arc::new(Custody {
                guard: Generation(1),
                config: FleetConfig::default(),
                refs: Box::new(GitRefTips { git: refs }),
            }),
        );
        let conn = Connection::open(self.ledger()).unwrap();
        conn.busy_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        ChannelServer::start(&self.fleet(), conn, Generation(1), handlers).unwrap()
    }

    fn ledger(&self) -> PathBuf {
        self.root.join("ledger.sqlite3")
    }

    fn fleet(&self) -> PathBuf {
        self.root.join("fleet")
    }

    fn proj(&self) -> PathBuf {
        self.root.join("proj")
    }

    /// `horch` with `args` and the environment `extra`, under the seam.
    fn horch(&self, args: &[&str], extra: &[(&str, &str)]) -> Out {
        let root = &self.root;
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_horch"));
        cmd.args(args)
            .current_dir(self.proj())
            .env_clear()
            .env("PATH", PATH)
            .env("HOME", root.join("home"))
            .env("XDG_DATA_HOME", root.join("data"))
            .env("HORCH_DATA_DIR", root.join("data"))
            .env("HORCH_STATE_DIR", root.join("state"))
            .env("HORCH_LEDGER_DB", self.ledger())
            .env("HORCH_PROJECT_DIR", self.proj())
            .env("FLEET_ROOT", self.fleet())
            .env("HORCH_NO_WATCHDOG", "1")
            .env("HORCH_NO_WAKE", "1")
            .env(ASSERT_READ_ONLY_ENV, "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1");
        for (k, v) in extra {
            cmd.env(k, v);
        }
        let out = cmd.output().unwrap();
        Out {
            code: out.status.code().unwrap_or(-1),
            out: String::from_utf8_lossy(&out.stdout).into_owned(),
            err: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }

    /// `horch` with `args`, which must exit 0 and must not panic.
    fn ok(&self, args: &[&str], extra: &[(&str, &str)]) -> Out {
        let o = self.horch(args, extra);
        assert_eq!(
            o.code, 0,
            "{args:?}\nstdout:\n{}\nstderr:\n{}",
            o.out, o.err
        );
        o
    }

    fn conn(&self) -> Connection {
        Connection::open(self.ledger()).unwrap()
    }

    /// The kind and state of each `requests` row, in the order of receipt.
    fn requests(&self) -> Vec<(String, String)> {
        let conn = self.conn();
        let mut stmt = conn
            .prepare("SELECT kind, state FROM requests ORDER BY received_at, rowid")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    fn count(&self, sql: &str) -> i64 {
        self.conn().query_row(sql, [], |r| r.get(0)).unwrap()
    }

    fn text(&self, sql: &str) -> Option<String> {
        self.conn().query_row(sql, [], |r| r.get(0)).unwrap()
    }
}

impl Drop for World {
    fn drop(&mut self) {
        if let Some(server) = self.server.take() {
            let _ = server.stop();
        }
    }
}

fn applied(kinds: &[&str]) -> Vec<(String, String)> {
    kinds
        .iter()
        .map(|k| (k.to_string(), "applied".to_string()))
        .collect()
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new(GIT)
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn rescue_drop_sends_rescue_drop_then_deletes_the_ref() {
    let w = World::new();
    let proj = w.proj();
    git(&proj, &["checkout", "-q", "-b", "side"]);
    std::fs::write(proj.join("b.txt"), "b\n").unwrap();
    git(&proj, &["add", "b.txt"]);
    git(&proj, &["commit", "-q", "-m", "b"]);
    let tip = git(&proj, &["rev-parse", "HEAD"]);
    let name = "refs/horch/rescue/branch/side/1";
    git(&proj, &["update-ref", name, &tip]);
    git(&proj, &["checkout", "-q", "main"]);
    git(&proj, &["branch", "-q", "-D", "side"]);

    let o = w.ok(&["rescue", "drop", name], &[]);
    assert!(
        o.out.contains(&format!("dropped {name} at {tip}")),
        "{}",
        o.out
    );
    assert_eq!(w.requests(), applied(&["rescue_drop"]));
    assert_eq!(
        w.count(&format!(
            "SELECT count(*) FROM events WHERE entity_id = '{name}' AND kind = 'note'"
        )),
        1
    );
    let gone = Command::new(GIT)
        .args(["rev-parse", "--verify", "-q", name])
        .current_dir(&proj)
        .output()
        .unwrap();
    assert!(!gone.status.success(), "the ref is deleted");
}

#[test]
fn status_in_an_orchestrator_pane_sends_run_adopt() {
    let w = World::new();
    let pane = [("HORCH_WORKSPACE_ID", "w1"), ("HERDR_PANE_ID", "p1")];
    let o = w.horch(&["ledger", "status"], &pane);
    assert!(!o.err.contains("panicked"), "{}", o.err);
    assert!(
        o.err
            .contains("NOTE: run r1 had no workspace. It now has workspace w1"),
        "{}",
        o.err
    );
    assert_eq!(w.requests(), applied(&["run_adopt"]));
    assert_eq!(
        w.text(&format!(
            "SELECT workspace_id FROM runs WHERE run_id = '{}'",
            w.run_id
        ))
        .as_deref(),
        Some("w1")
    );
    // The run has its workspace now: a second status sends nothing.
    w.horch(&["ledger", "status"], &pane);
    assert_eq!(w.requests(), applied(&["run_adopt"]));
}

#[test]
fn import_dry_run_sends_ledger_import_and_keeps_no_row() {
    let w = World::new();
    let runs = w.count("SELECT count(*) FROM runs");
    let o = w.ok(&["ledger", "import", "--dry-run"], &[]);
    assert!(o.out.starts_with("Dry run, nothing kept."), "{}", o.out);
    assert_eq!(w.requests(), applied(&["ledger_import"]));
    assert_eq!(w.count("SELECT count(*) FROM runs"), runs);
}

#[test]
fn note_of_a_worker_with_no_attempt_sends_worker_progress() {
    let w = World::new();
    let id = workers::insert(
        &w.conn(),
        &NewWorker {
            run_id: w.run_id.clone(),
            persona: "opus".into(),
            ..Default::default()
        },
    )
    .unwrap();
    w.ok(&["note", "read the ticket"], &[("HORCH_WORKER_ID", &id)]);
    assert_eq!(w.requests(), applied(&["worker_progress"]));
    assert_eq!(
        w.text(&format!(
            "SELECT text FROM events WHERE worker_id = '{id}' AND kind = 'note'"
        ))
        .as_deref(),
        Some("read the ticket")
    );
}

/// The fix of T-880: `horch ledger unfinished` wrote the findings of native
/// runs, and failed with "attempt to write a readonly database". It only
/// reads now, and the pass of fleetd writes the findings.
#[test]
fn unfinished_reads_only_and_lists_what_the_pass_wrote() {
    let w = World::new();
    let c = w.conn();
    c.execute_batch(
        "INSERT INTO projects (project_id, root, created_at) VALUES ('p9', '/p9', 'now');
         INSERT INTO runs (run_id, project_id, slug, finish_steps, status, origin, started_at)
             VALUES ('r9', 'p9', 'r9', '[]', 'done', 'native', 'now');",
    )
    .unwrap();
    let idea = horch_core::taskledger::schema::run_idea(&c, "r9", "now", true).unwrap();
    c.execute(
        "INSERT INTO tasks (task_id, run_id, seq, title, task_file, primary_persona, claimable_by, status, origin, created_at, updated_at, idea_id)
         VALUES ('t9', 'r9', 1, 'one', 'a.md', 'dev', 'dev', 'running', 'native', 'now', 'now', ?1)",
        [idea],
    )
    .unwrap();
    c.execute(
        "INSERT INTO workers (worker_id, run_id, persona, host, status, origin, task_id)
         VALUES ('w9', 'r9', 'dev', 'h', 'incomplete', 'native', 't9')",
        [],
    )
    .unwrap();
    let all = ["ledger", "unfinished", "--project", "all"];
    let o = w.ok(&all, &[]);
    assert_eq!(o.out, "No unfinished work.\n");
    // The pass of fleetd writes the finding; the command lists it.
    horch_core::taskledger::import::sync_native(&c).unwrap();
    let o = w.ok(&all, &[]);
    assert!(o.out.contains("reaped_never_finished"), "{}", o.out);
    assert!(w.requests().is_empty());
}

#[test]
fn triage_archive_is_refused_with_a_later_task() {
    let w = World::new();
    let proj = w.proj();
    let tree = proj.join(".worktrees").join("feat");
    git(
        &proj,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feat",
            tree.to_str().unwrap(),
        ],
    );
    let reply = w.root.join("reply.md");
    std::fs::write(
        &reply,
        "| ID | Worktree | Suggested | Decision | Direction |\n|----|----|----|----|----|\n| W1 | feat | | DELETE | |\n",
    )
    .unwrap();
    let o = w.horch(
        &["worktree", "triage", "apply", reply.to_str().unwrap()],
        &[],
    );
    assert_eq!(o.code, 1, "{}{}", o.out, o.err);
    assert!(
        o.err
            .contains("the decisions DELETE in this version, and nothing ran")
            && o.err
                .contains("They return in a later task as a fleetd job"),
        "{}",
        o.err
    );
    assert!(w.requests().is_empty());
    assert!(tree.exists());
    // The listing reads the ledger read-only.
    let o = w.ok(&["worktree", "triage", "--json"], &[]);
    assert!(o.out.contains("feat"), "{}", o.out);
}
