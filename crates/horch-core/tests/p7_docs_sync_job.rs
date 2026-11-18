//! The `docs_sync` job (fleetd-p7 ticket 22): steps 1 and 2 of 1 sync as a
//! job with no repository. The version command starts through the launcher
//! (SR-7), and its text names the version folder under
//! `<fleet root>/docs/<cli>/`.
//!
//! The docs runner is a fake of the port: no test uses the network. The
//! version command is a script in a temp folder, not a real agent CLI. The
//! job process is `true`, started through the launcher, and the executor
//! runs in the test process with the recorded pid, as `horch job run <id>`
//! runs it.
#![cfg(unix)]

use std::cell::RefCell;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use horch_core::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use horch_core::fleetd::fleet_runtime::application::jobs::{
    self, result_data, CheckRunner, JobCtx, JobKey, JobRunner, Queued, Runners, SandboxProfiles,
};
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::docs_sync::{
    DocsSyncRunner, FailedPage, Synced,
};
use horch_core::fleetd::worker_profiles::application::ports::LaunchSpec;
use horch_core::taskledger::schema;
use horch_core::taskledger::workers::Worker;
use horch_platform::start::FORBIDDEN_ENV;
use rusqlite::{Connection, TransactionBehavior};

/// The text that the fake version command prints.
const VERSION: &str = "1.2.3 (Fake CLI)";

/// A fake docs runner: the version command is a script that prints
/// [`VERSION`] and writes its environment to `env.txt`; a sync writes 1 page
/// and CHANGES.md, or reports the pages of `fail` as failed.
struct FakeDocs {
    bin: PathBuf,
    fail: Vec<&'static str>,
    synced: RefCell<Vec<(String, String, PathBuf)>>,
}

impl FakeDocs {
    fn new(dir: &Path) -> Self {
        let bin = dir.join("fakecli");
        let env_file = dir.join("env.txt");
        std::fs::write(
            &bin,
            format!(
                "#!/bin/sh\nenv > '{}'\nprintf '%s\\n' '{VERSION}' ''\n",
                env_file.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        FakeDocs {
            bin,
            fail: Vec::new(),
            synced: RefCell::new(Vec::new()),
        }
    }

    fn env_file(&self) -> PathBuf {
        self.bin.with_file_name("env.txt")
    }
}

impl DocsSyncRunner for FakeDocs {
    fn version_cmd(&self, cli: &str) -> Result<String> {
        if cli != "fake" {
            bail!("no docs source {cli}");
        }
        Ok(format!("{} --version", self.bin.display()))
    }

    fn sync(&self, cli: &str, version: &str, out: &Path) -> Result<Synced> {
        self.synced
            .borrow_mut()
            .push((cli.to_string(), version.to_string(), out.to_path_buf()));
        let dir = out.join(cli).join(version);
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("page.md"), "# Page\n")?;
        std::fs::write(out.join(cli).join("CHANGES.md"), "first sync\n")?;
        Ok(Synced {
            dir,
            pages: 1,
            failed: self
                .fail
                .iter()
                .map(|f| FailedPage {
                    file: f.to_string(),
                    why: "404".into(),
                })
                .collect(),
        })
    }
}

/// No worker of these tests has a sandbox profile.
struct NoProfiles;

impl SandboxProfiles for NoProfiles {
    fn check_launch(&self, _: &Worker, _: &Path, _: &str) -> Result<Option<LaunchSpec>> {
        Ok(None)
    }
}

struct World {
    dir: tempfile::TempDir,
    conn: Connection,
    config: FleetConfig,
}

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let mut conn = Connection::open(dir.path().join("ledger.sqlite3")).unwrap();
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    World {
        dir,
        conn,
        config: FleetConfig::default(),
    }
}

impl World {
    /// The fleet root of the ledger: the folder `fleet` beside it, as the
    /// ledger names its folder (symlinks resolved).
    fn root(&self) -> PathBuf {
        self.dir.path().canonicalize().unwrap().join("fleet")
    }

    fn row(&self, id: i64) -> JobRow {
        jobs_store::get(&self.conn, id).unwrap().unwrap()
    }

    /// Queue a `docs_sync` run of `subject` with no repository, start its
    /// job process through the launcher, run the executor with `docs`, and
    /// end the run from its result file.
    fn run_docs_sync(&mut self, subject: &str, docs: &FakeDocs) -> JobRow {
        let ctx = JobCtx {
            guard: None,
            config: &self.config,
            needs_you: &NeedsYouList,
        };
        let key = JobKey {
            kind: Kind::DocsSync,
            subject,
            input_key: "id-1",
        };
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let queued = jobs::queue(&ctx, &tx, &key, None, FleetSeconds(0)).unwrap();
        tx.commit().unwrap();
        let Queued::New(id) = queued else {
            panic!("a new run is queued: {queued:?}");
        };
        let spec = LaunchSpec {
            program: "true".into(),
            args: Vec::new(),
            cwd: None,
            env_rule: Default::default(),
            detach: false,
            output: None,
        };
        let job = self.row(id);
        JobRunner::new(&Launcher)
            .start(&ctx, &mut self.conn, &job, &spec, FleetSeconds(0))
            .unwrap();
        let pid = self.row(id).pid.expect("a running job has a pid");
        let checks = CheckRunner {
            host: &Launcher,
            profiles: &NoProfiles,
            sweep: None,
        };
        let runners = Runners {
            checks: &checks,
            docs: Some(docs),
            conformance: None,
        };
        jobs::run_job_with(&ctx, &mut self.conn, id, pid, &runners)
            .unwrap()
            .expect("the job process runs its row");
        jobs::end_from_files(&ctx, &mut self.conn).unwrap();
        self.row(id)
    }
}

#[test]
fn docs_sync_job_with_no_repository_is_a_valid_row() {
    let mut w = world();
    let docs = FakeDocs::new(w.dir.path());

    let row = w.run_docs_sync("fake 9f2c", &docs);

    assert_eq!(row.kind, Kind::DocsSync);
    assert_eq!(row.repo, None);
    assert_eq!(row.subject, "fake 9f2c");
    assert_eq!(row.state, State::Passed, "{:?}", row.result_json);
    let data = result_data(&row).expect("the run records its facts");
    assert_eq!(data["cli"], "fake");
    assert!(data["failed"].as_array().unwrap().is_empty());
    // The pages go into the fleet root, not into a repository.
    let synced = docs.synced.borrow();
    assert_eq!(synced.len(), 1);
    assert_eq!(synced[0].2, w.root().join("docs"));
    assert!(w.root().join("docs/fake/CHANGES.md").is_file());
}

#[test]
fn docs_sync_job_records_the_version_text() {
    let mut w = world();
    let docs = FakeDocs::new(w.dir.path());

    let row = w.run_docs_sync("fake 9f2c", &docs);

    let data = result_data(&row).unwrap();
    assert_eq!(data["version"], VERSION);
    assert_eq!(docs.synced.borrow()[0].1, VERSION);
    let page = w.root().join("docs/fake").join(VERSION).join("page.md");
    assert!(page.is_file(), "no page at {}", page.display());
    assert_eq!(
        data["dir"],
        w.root()
            .join("docs/fake")
            .join(VERSION)
            .to_string_lossy()
            .as_ref()
    );
    let text = jobs::outcome(&row).unwrap().text;
    assert!(text.contains(VERSION), "{text}");
}

#[test]
fn docs_sync_job_with_a_failed_page_fails_and_names_it() {
    let mut w = world();
    let mut docs = FakeDocs::new(w.dir.path());
    docs.fail = vec!["hooks.md", "mcp.md"];

    let row = w.run_docs_sync("fake 9f2c", &docs);

    assert_eq!(row.state, State::Failed);
    let data = result_data(&row).unwrap();
    let failed: Vec<&str> = data["failed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["file"].as_str().unwrap())
        .collect();
    assert_eq!(failed, ["hooks.md", "mcp.md"]);
    let text = jobs::outcome(&row).unwrap().text;
    assert!(
        text.contains("hooks.md") && text.contains("mcp.md"),
        "{text}"
    );
}

#[test]
fn docs_sync_job_of_an_unknown_cli_fails_with_a_result() {
    let mut w = world();
    let docs = FakeDocs::new(w.dir.path());

    let row = w.run_docs_sync("nope 9f2c", &docs);

    assert_eq!(row.state, State::Failed);
    let text = jobs::outcome(&row).unwrap().text;
    assert!(text.contains("no docs source nope"), "{text}");
    assert!(docs.synced.borrow().is_empty());
}

#[test]
fn version_command_has_no_forbidden_name() {
    let mut w = world();
    let docs = FakeDocs::new(w.dir.path());
    for name in FORBIDDEN_ENV {
        std::env::set_var(name, "forbidden-in-test");
    }

    let row = w.run_docs_sync("fake 9f2c", &docs);

    assert_eq!(row.state, State::Passed, "{:?}", row.result_json);
    let env = std::fs::read_to_string(docs.env_file()).expect("the version command ran");
    for name in FORBIDDEN_ENV {
        assert!(
            !env.lines().any(|l| l.starts_with(&format!("{name}="))),
            "{name} reached the version command"
        );
    }
}
