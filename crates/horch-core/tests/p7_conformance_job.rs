#![cfg(unix)]

use std::cell::RefCell;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use horch_core::execution::lifecycle::ExecCli;
use horch_core::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use horch_core::fleetd::fleet_runtime::application::jobs::{
    self, result_data, CheckRunner, ConformanceSuite, JobCtx, JobKey, JobRunner, Queued, Runners,
    SandboxProfiles,
};
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::worker_profiles::adapters::conformance_runner::{
    reference_profile, ConformanceRunner,
};
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::conformance::{
    read_conformance_json, SuiteSubject,
};
use horch_core::fleetd::worker_profiles::application::ports::LaunchSpec;
use horch_core::fleetd::worker_profiles::domain::conformance::{
    Check, CheckResult, ConformanceReport, CHECKS, CONTEXT_LIMIT, FIRST_EVENT_LIMIT,
};

use horch_core::runtime::{MapEnv, RuntimeContext};
use horch_core::taskledger::schema;
use horch_core::taskledger::workers::Worker;
use rusqlite::{Connection, TransactionBehavior};

const VERSION: &str = "2.0.1 (Fake CLI)";

const HASH: &str = "ab12cd34";

struct FakeSuite {
    bin: PathBuf,
    fail: Vec<Check>,
    unsupported: Vec<Check>,
    ran: RefCell<Vec<SuiteSubject>>,
}

impl FakeSuite {
    fn new(dir: &Path) -> Self {
        let bin = dir.join("fakecli");
        std::fs::write(&bin, format!("#!/bin/sh\nprintf '%s\\n' '{VERSION}'\n")).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        FakeSuite {
            bin,
            fail: Vec::new(),
            unsupported: Vec::new(),
            ran: RefCell::new(Vec::new()),
        }
    }
}

impl ConformanceSuite for FakeSuite {
    fn version_cmd(&self, cli: &str) -> Result<String> {
        if cli != "fake" {
            bail!("no docs source {cli}");
        }
        Ok(format!("{} --version", self.bin.display()))
    }

    fn adapter_hash(&self, _cli: &str) -> Result<String> {
        Ok(HASH.into())
    }

    fn run(&self, subject: SuiteSubject) -> Result<ConformanceReport> {
        self.ran.borrow_mut().push(subject.clone());
        let checks = CHECKS
            .into_iter()
            .map(|c| {
                if self.fail.contains(&c) {
                    CheckResult::fail(c, "the probe saw no result")
                } else if self.unsupported.contains(&c) {
                    CheckResult::not_supported(c, "no sandbox on this OS")
                } else {
                    CheckResult::pass(c, "ok")
                }
            })
            .collect();
        Ok(ConformanceReport {
            cli: subject.cli,
            version: subject.version,
            adapter_hash: subject.adapter_hash,
            checks,
        })
    }
}

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
    fn root(&self) -> PathBuf {
        horch_platform::path::canonicalize(self.dir.path())
            .unwrap()
            .join("fleet")
    }

    fn row(&self, id: i64) -> JobRow {
        jobs_store::get(&self.conn, id).unwrap().unwrap()
    }

    fn run_conformance(&mut self, subject: &str, suite: &FakeSuite) -> JobRow {
        let ctx = JobCtx {
            guard: None,
            config: &self.config,
            needs_you: &NeedsYouList,
        };
        let input_key = format!("{subject} {HASH}");
        let key = JobKey {
            kind: Kind::Conformance,
            subject,
            input_key: &input_key,
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
            docs: None,
            conformance: Some(suite),
        };
        jobs::run_job_with(&ctx, &self.conn, id, pid, &runners)
            .unwrap()
            .expect("the job process runs its row");
        jobs::end_from_files(&ctx, &mut self.conn).unwrap();
        self.row(id)
    }
}

#[test]
fn conformance_job_with_no_repository_is_a_valid_row() {
    let mut w = world();
    let suite = FakeSuite::new(w.dir.path());

    let row = w.run_conformance("fake 9f2c", &suite);

    assert_eq!(row.kind, Kind::Conformance);
    assert_eq!(row.repo, None);
    assert_eq!(row.subject, "fake 9f2c");
    assert_eq!(row.state, State::Passed, "{:?}", row.result_json);
    let data = result_data(&row).expect("the run records its facts");
    assert_eq!(data["cli"], "fake");
    assert_eq!(data["version"], VERSION);
    assert_eq!(data["adapter_hash"], HASH);
    assert!(data["failed"].as_array().unwrap().is_empty());
    let ran = suite.ran.borrow();
    assert_eq!(
        *ran,
        [SuiteSubject {
            cli: "fake".into(),
            version: VERSION.into(),
            adapter_hash: HASH.into(),
        }]
    );
}

#[test]
fn conformance_job_writes_conformance_json() {
    let mut w = world();
    let mut suite = FakeSuite::new(w.dir.path());
    suite.unsupported = vec![Check::OperatorBoundary];

    let row = w.run_conformance("fake 9f2c", &suite);

    assert_eq!(row.state, State::Passed, "{:?}", row.result_json);
    let path = w
        .root()
        .join("docs/fake")
        .join(VERSION)
        .join("conformance.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("no conformance.json at {}: {e}", path.display()));
    let report = read_conformance_json(&text).unwrap();
    assert_eq!(report.cli, "fake");
    assert_eq!(report.version, VERSION);
    assert_eq!(report.adapter_hash, HASH);
    assert_eq!(report.checks.len(), CHECKS.len());
    let data = result_data(&row).unwrap();
    assert_eq!(data["path"], path.to_string_lossy().as_ref());
    assert!(!w.dir.path().join("docs").exists());
}

#[test]
fn failed_check_fails_the_job_with_its_name() {
    let mut w = world();
    let mut suite = FakeSuite::new(w.dir.path());
    suite.fail = vec![Check::SharedWrite, Check::StopGate];

    let row = w.run_conformance("fake 9f2c", &suite);

    assert_eq!(row.state, State::Failed, "{:?}", row.result_json);
    let data = result_data(&row).unwrap();
    let failed: Vec<&str> = data["failed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_str().unwrap())
        .collect();
    assert_eq!(failed, [Check::SharedWrite.name(), Check::StopGate.name()]);
    let text = jobs::outcome(&row).unwrap().text;
    for check in [Check::SharedWrite, Check::StopGate] {
        assert!(text.contains(check.name()), "{text}");
    }
    assert!(text.contains(VERSION), "{text}");
    let path = w
        .root()
        .join("docs/fake")
        .join(VERSION)
        .join("conformance.json");
    assert!(path.is_file(), "no conformance.json at {}", path.display());
}

#[test]
fn conformance_job_of_an_unknown_cli_fails_with_a_result() {
    let mut w = world();
    let suite = FakeSuite::new(w.dir.path());

    let row = w.run_conformance("nope 9f2c", &suite);

    assert_eq!(row.state, State::Failed);
    let text = jobs::outcome(&row).unwrap().text;
    assert!(text.contains("no docs source nope"), "{text}");
    assert!(suite.ran.borrow().is_empty());
}

fn stub_sandbox_tools(bin: &Path) {
    std::fs::create_dir_all(bin).unwrap();
    for tool in ["bwrap", "socat"] {
        let path = bin.join(tool);
        std::fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
        horch_platform::fs::make_executable(&path).unwrap();
    }
}

fn render_ctx(tmp: &Path) -> RuntimeContext {
    for d in ["home", "state", "data", "ledger/fleet", "scratch", "bin"] {
        std::fs::create_dir_all(tmp.join(d)).unwrap();
    }
    let s = |p: PathBuf| p.to_string_lossy().into_owned();
    let mut env = MapEnv::new(tmp.join("home"))
        .with("HOME", &s(tmp.join("home")))
        .with("PATH", &s(tmp.join("bin")))
        .with("HORCH_STATE_DIR", &s(tmp.join("state")))
        .with("HORCH_DATA_DIR", &s(tmp.join("data")))
        .with("HORCH_LEDGER_DB", &s(tmp.join("ledger/ledger.sqlite3")))
        .with("FLEET_ROOT", &s(tmp.join("ledger/fleet")))
        .with(
            "HORCH_CLAUDE_MANAGED_SETTINGS",
            &s(tmp.join("home/no-managed-settings.json")),
        );
    for (key, name) in [
        ("HORCH_CLAUDE_BIN", "claude"),
        ("HORCH_CODEX_BIN", "codex"),
        ("HORCH_ANTIGRAVITY_BIN", "agy"),
    ] {
        let bin = tmp.join("bin").join(name);
        std::fs::write(&bin, "#!/bin/sh\nexit 1\n").unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        env = env.with(key, &s(bin));
    }
    stub_sandbox_tools(&tmp.join("bin"));
    RuntimeContext::from_env(&env).unwrap()
}

fn hash_of(
    ctx: &RuntimeContext,
    teammate: &horch_core::roster::Teammate,
    scratch: &Path,
) -> String {
    let runner = ConformanceRunner {
        ctx,
        teammate,
        fleet_window: Some((CONTEXT_LIMIT, "context-windows.md".into())),
        env_rule: Vec::new(),
        scratch: scratch.to_path_buf(),
        first_event_limit: FIRST_EVENT_LIMIT,
    };
    runner.adapter_hash().unwrap()
}

fn codex_with_operator_skills(tmp: &Path, names: &[&str]) -> horch_core::roster::Teammate {
    for name in names {
        let dir = tmp.join("home/.agents/skills").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: Operator skill {name}.\n---\nbody\n"),
        )
        .unwrap();
    }
    let mut t = reference_profile(ExecCli::Codex);
    t.operator_skills = Some(horch_core::roster::OperatorSkills {
        dir: "~/.agents/skills".into(),
        names: names.iter().map(|n| n.to_string()).collect(),
    });
    t
}

#[test]
fn codex_adapter_hash_hides_the_skills_bundle_folder() {
    let tmp = tempfile::tempdir_in("/tmp").unwrap();
    let ctx = render_ctx(tmp.path());
    let scratch = tmp.path().join("scratch");
    let profile = codex_with_operator_skills(tmp.path(), &["probe-a"]);
    let first = hash_of(&ctx, &profile, &scratch);
    assert_eq!(hash_of(&ctx, &profile, &scratch), first);
}

#[test]
fn codex_adapter_hash_changes_when_the_briefing_changes() {
    let tmp = tempfile::tempdir_in("/tmp").unwrap();
    let ctx = render_ctx(tmp.path());
    let scratch = tmp.path().join("scratch");
    let one = codex_with_operator_skills(tmp.path(), &["probe-a"]);
    let two = codex_with_operator_skills(tmp.path(), &["probe-a", "probe-b"]);
    assert_ne!(hash_of(&ctx, &one, &scratch), hash_of(&ctx, &two, &scratch));
}

#[test]
fn adapter_hash_changes_when_the_rendered_launch_changes() {
    let tmp = tempfile::tempdir_in("/tmp").unwrap();
    let ctx = render_ctx(tmp.path());
    let scratch = tmp.path().join("scratch");

    for cli in [ExecCli::Claude, ExecCli::Codex, ExecCli::Antigravity] {
        let profile = reference_profile(cli);
        assert_eq!(ExecCli::of(&profile), Some(cli), "{cli:?}");
        let first = hash_of(&ctx, &profile, &scratch);
        assert_eq!(first.len(), 64, "a sha256 in hex: {first}");
        assert_eq!(hash_of(&ctx, &profile, &scratch), first, "{cli:?}");

        let mut changed = profile.clone();
        changed.model = Some("another-model".into());
        assert_ne!(hash_of(&ctx, &changed, &scratch), first, "{cli:?}");
    }
    let left: Vec<_> = std::fs::read_dir(&scratch)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert!(left.is_empty(), "throwaway folders are left: {left:?}");
}
