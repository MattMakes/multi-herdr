#![cfg(unix)]

use std::fs::{self, Permissions};
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};

use horch_core::fleetd::fleet_runtime::adapters::job_result_file::{
    self as files, FileRead, JobResult, Read,
};
use horch_core::fleetd::fleet_runtime::adapters::jobs_store::{self, JobRow};
use horch_core::fleetd::fleet_runtime::adapters::scans::{Liveness, ProcessFact};
use horch_core::fleetd::fleet_runtime::application::jobs::{
    self, outcome, CheckRunner, JobCtx, JobKey, JobRunner, Queued, SandboxProfiles,
};
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::jobs::Outcome;
use horch_core::fleetd::fleet_runtime::domain::jobs::{Kind, State};
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::ports::LaunchSpec;
use horch_core::taskledger::schema;
use horch_core::taskledger::workers::Worker;
use horch_platform::channel::User;
use rusqlite::{params, Connection, Transaction, TransactionBehavior};

struct World {
    dir: tempfile::TempDir,
    conn: Connection,
    config: FleetConfig,
    owner: User,
}

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let mut conn = Connection::open(dir.path().join("ledger.sqlite3")).unwrap();
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    World {
        dir,
        conn,
        config: FleetConfig::default(),
        owner: User::current().unwrap(),
    }
}

struct NoProfiles;

impl SandboxProfiles for NoProfiles {
    fn check_launch(&self, _: &Worker, _: &Path, _: &str) -> anyhow::Result<Option<LaunchSpec>> {
        Ok(None)
    }
}

impl World {
    fn root(&self) -> PathBuf {
        self.dir.path().join("fleet")
    }

    fn tx<T>(&mut self, f: impl FnOnce(&JobCtx<'_>, &Transaction<'_>) -> T) -> T {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        let ctx = JobCtx {
            guard: None,
            config: &self.config,
            needs_you: &NeedsYouList,
        };
        let out = f(&ctx, &tx);
        tx.commit().unwrap();
        out
    }

    fn queue(&mut self, kind: Kind, subject: &str, clock: i64) -> i64 {
        let key = JobKey {
            kind,
            subject,
            input_key: "in",
        };
        match self.tx(|ctx, tx| jobs::queue(ctx, tx, &key, Some("/repo"), FleetSeconds(clock))) {
            Ok(Queued::New(id)) => id,
            other => panic!("a new run is queued: {other:?}"),
        }
    }

    fn start_due(&mut self, runner: &mut JobRunner<'_>, clock: i64) {
        let spec = LaunchSpec {
            program: "true".into(),
            args: Vec::new(),
            cwd: None,
            env_rule: Default::default(),
            detach: false,
            output: None,
        };
        let due = jobs::due_to_start(&self.conn, FleetSeconds(clock)).unwrap();
        assert!(!due.is_empty(), "a run is due at {clock}");
        let ctx = JobCtx {
            guard: None,
            config: &self.config,
            needs_you: &NeedsYouList,
        };
        for job in due {
            assert!(runner
                .start(&ctx, &mut self.conn, &job, &spec, FleetSeconds(clock))
                .unwrap());
        }
    }

    fn run_job(&mut self, id: i64) -> Option<Outcome> {
        let me = self.row(id).pid.unwrap();
        let ctx = JobCtx {
            guard: None,
            config: &self.config,
            needs_you: &NeedsYouList,
        };
        let checks = CheckRunner {
            host: &Launcher,
            profiles: &NoProfiles,
            sweep: None,
        };
        jobs::run_job(&ctx, &self.conn, id, me, &checks).unwrap()
    }

    fn read(&self, max_bytes: u64) -> Vec<FileRead> {
        jobs::read_results(&self.conn, &self.root(), &self.owner, max_bytes).unwrap()
    }

    fn ingest(&mut self, reads: &[FileRead]) -> Vec<(i64, State)> {
        self.tx(|ctx, tx| jobs::ingest_results(ctx, tx, reads))
            .unwrap()
    }

    fn pass_no_archive(&mut self) -> (Vec<FileRead>, Vec<(i64, State)>) {
        let reads = self.read(self.config.job_result_max_bytes);
        let ended = self.ingest(&reads);
        (reads, ended)
    }

    fn row(&self, id: i64) -> JobRow {
        jobs_store::get(&self.conn, id).unwrap().unwrap()
    }

    fn events(&self, id: i64, kind: &str) -> Vec<String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT cause FROM events WHERE entity = 'job' AND entity_id = ?1 AND kind = ?2
                 ORDER BY event_id",
            )
            .unwrap();
        stmt.query_map(params![id.to_string(), kind], |r| {
            r.get::<_, Option<String>>(0)
        })
        .unwrap()
        .map(|c| c.unwrap().unwrap_or_default())
        .collect()
    }

    fn moves_to(&self, id: i64, state: State) -> i64 {
        self.conn
            .query_row(
                "SELECT count(*) FROM events WHERE entity = 'job' AND entity_id = ?1
                 AND to_state = ?2",
                params![id.to_string(), state.as_str()],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn ingested(&self, path: &Path) -> Option<(String, Option<String>)> {
        self.conn
            .query_row(
                "SELECT outcome, reason FROM ingested WHERE path = ?1",
                [path.display().to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok()
    }

    fn running(&mut self, runner: &mut JobRunner<'_>, kind: Kind) -> i64 {
        let id = self.queue(kind, "F-1", 0);
        self.start_due(runner, 0);
        assert_eq!(self.row(id).state, State::Running);
        id
    }
}

fn mode(path: &Path) -> u32 {
    fs::symlink_metadata(path).unwrap().permissions().mode() & 0o7777
}

fn passed(job_id: i64, run: i64, text: &str) -> JobResult {
    JobResult {
        job_id,
        run,
        passed: true,
        verdict: true,
        text: text.into(),
        data: None,
    }
}

fn names(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(folder)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn job_process_writes_its_result_file_and_no_row() {
    let mut w = world();
    let launcher = Launcher;
    let mut runner = JobRunner::new(&launcher);
    let id = w.running(&mut runner, Kind::DocsSync);
    let before = w.row(id);
    let changes = w.conn.total_changes();

    let o = w.run_job(id).expect("the job process runs its row");

    assert_eq!(w.conn.total_changes(), changes, "run_job wrote the ledger");
    assert_eq!(w.row(id), before);
    assert_eq!(w.row(id).result_json, None);
    assert!(w
        .events(id, "transition")
        .iter()
        .all(|c| !c.contains("result")));

    let path = files::path(&w.root(), id, 1);
    assert_eq!(path, w.root().join(format!("run/jobs/{id}-1.json")));
    assert_eq!(mode(&path), 0o600);
    assert_eq!(mode(&w.root().join("run/jobs")), 0o700);
    let file: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(file["job_id"], id);
    assert_eq!(file["run"], 1);
    assert_eq!(file["passed"], false);
    assert_eq!(file["verdict"], false);
    assert_eq!(file["text"], o.text.as_str());
    assert_eq!(o.text, "no executor before phase 7 for docs_sync");
    assert_eq!(
        names(&w.root().join("run/jobs")),
        vec![format!("{id}-1.json")]
    );
}

#[test]
fn pass_ends_the_job_from_its_file_once() {
    let mut w = world();
    let launcher = Launcher;
    let mut runner = JobRunner::new(&launcher);
    let id = w.running(&mut runner, Kind::DocsSync);
    w.run_job(id).unwrap();
    let path = files::path(&w.root(), id, 1);

    let reads = w.read(w.config.job_result_max_bytes);
    assert_eq!(reads.len(), 1);
    assert!(matches!(&reads[0].read, Read::Valid { result, .. } if result.job_id == id));
    assert_eq!(w.ingest(&reads), vec![(id, State::Failed)]);
    let row = w.row(id);
    assert_eq!(row.state, State::Failed);
    let o = outcome(&row).unwrap();
    assert!(!o.passed && !o.verdict);
    assert_eq!(o.text, "no executor before phase 7 for docs_sync");
    assert_eq!(
        w.events(id, "transition").last().map(String::as_str),
        Some("the job process wrote its result")
    );
    assert_eq!(w.ingested(&path), Some(("accepted".into(), None)));

    assert_eq!(w.ingest(&reads), vec![]);
    assert_eq!(w.moves_to(id, State::Failed), 1);

    jobs::archive_results(&w.root(), &reads).unwrap();
    assert!(!path.exists());
    assert!(w
        .root()
        .join(format!("run/jobs/archive/{id}-1.json"))
        .is_file());
    assert!(w.read(w.config.job_result_max_bytes).is_empty());
    assert_eq!(w.moves_to(id, State::Failed), 1);
}

#[test]
fn result_of_an_old_run_is_ignored_and_logged() {
    let mut w = world();
    let launcher = Launcher;
    let mut runner = JobRunner::new(&launcher);
    let old = w.running(&mut runner, Kind::Push);

    let gone = ProcessFact {
        key: old.to_string(),
        liveness: Liveness::Dead,
    };
    let seen = w.tx(|ctx, tx| jobs::observe(ctx, tx, &[gone], FleetSeconds(1)).unwrap());
    assert_eq!(seen.ended, vec![(old, State::Abandoned)]);
    let new = jobs::due_to_start(&w.conn, FleetSeconds(10_000)).unwrap()[0].id;
    w.start_due(&mut runner, 10_000);
    assert_eq!((w.row(new).run, w.row(new).state), (2, State::Running));

    files::write(&w.root(), &passed(old, 1, "late")).unwrap();
    files::write(&w.root(), &passed(new, 1, "wrong run")).unwrap();

    let (reads, ended) = w.pass_no_archive();
    assert_eq!(reads.len(), 2);
    assert_eq!(ended, vec![]);
    assert_eq!(w.row(old).state, State::Abandoned);
    assert_eq!(w.row(old).result_json, None);
    assert_eq!(w.row(new).state, State::Running);
    assert_eq!(w.row(new).result_json, None);
    let old_notes = w.events(old, "note");
    assert_eq!(old_notes.len(), 1, "{old_notes:?}");
    assert!(
        old_notes[0].contains("ignored the result file of run 1")
            && old_notes[0].contains("abandoned"),
        "{old_notes:?}"
    );
    let new_notes = w.events(new, "note");
    assert_eq!(new_notes.len(), 1, "{new_notes:?}");
    assert!(
        new_notes[0].contains("ignored the result file of run 1") && new_notes[0].contains("run 2"),
        "{new_notes:?}"
    );

    let again = w.read(w.config.job_result_max_bytes);
    assert!(again.iter().all(|r| matches!(r.read, Read::Seen)));
    assert_eq!(w.ingest(&again), vec![]);
    assert_eq!(w.events(old, "note").len(), 1);
    assert_eq!(w.events(new, "note").len(), 1);
}

#[test]
fn symlinked_result_file_is_rejected() {
    let mut w = world();
    let launcher = Launcher;
    let mut runner = JobRunner::new(&launcher);
    let id = w.running(&mut runner, Kind::Push);

    let outside = w.dir.path().join("elsewhere.json");
    fs::write(
        &outside,
        serde_json::to_vec(&passed(id, 1, "linked")).unwrap(),
    )
    .unwrap();
    fs::set_permissions(&outside, Permissions::from_mode(0o600)).unwrap();
    let folder = files::folder(&w.root());
    fs::create_dir_all(&folder).unwrap();
    fs::set_permissions(&folder, Permissions::from_mode(0o700)).unwrap();
    let link = files::path(&w.root(), id, 1);
    symlink(&outside, &link).unwrap();

    let (reads, ended) = w.pass_no_archive();
    assert_eq!(ended, vec![]);
    assert!(
        matches!(&reads[0].read, Read::Rejected { reason, .. } if reason == "the file is a link"),
        "{reads:?}"
    );
    assert_eq!(
        w.ingested(&link),
        Some(("rejected".into(), Some("the file is a link".into())))
    );
    assert_eq!(w.row(id).state, State::Running);
    assert_eq!(w.row(id).result_json, None);

    let again = w.read(w.config.job_result_max_bytes);
    assert!(matches!(again[0].read, Read::Seen));
    jobs::archive_results(&w.root(), &again).unwrap();
    assert!(fs::symlink_metadata(&link).is_err());
    assert!(outside.is_file());
}

#[test]
fn result_file_over_its_limit_is_rejected() {
    let mut w = world();
    let launcher = Launcher;
    let mut runner = JobRunner::new(&launcher);
    let id = w.running(&mut runner, Kind::Push);
    let path = files::write(&w.root(), &passed(id, 1, &"x".repeat(200))).unwrap();

    let reads = w.read(64);
    let ended = w.ingest(&reads);
    assert_eq!(ended, vec![]);
    let (outcome, reason) = w.ingested(&path).unwrap();
    assert_eq!(outcome, "rejected");
    let reason = reason.unwrap();
    assert!(reason.contains("over the limit of 64 bytes"), "{reason}");
    assert_eq!(w.row(id).state, State::Running);
    assert_eq!(w.row(id).result_json, None);
}

#[test]
fn crash_after_the_commit_only_archives_the_file() {
    let mut w = world();
    let launcher = Launcher;
    let mut runner = JobRunner::new(&launcher);
    let id = w.running(&mut runner, Kind::DocsSync);
    let path = files::write(&w.root(), &passed(id, 1, "pushed")).unwrap();

    let (_, ended) = w.pass_no_archive();
    assert_eq!(ended, vec![(id, State::Passed)]);
    assert!(path.is_file());

    w.conn = Connection::open(w.dir.path().join("ledger.sqlite3")).unwrap();
    let reads = w.read(w.config.job_result_max_bytes);
    assert_eq!(reads.len(), 1);
    assert!(matches!(reads[0].read, Read::Seen));
    assert_eq!(w.ingest(&reads), vec![]);
    jobs::archive_results(&w.root(), &reads).unwrap();
    assert!(!path.exists());
    assert_eq!(
        names(&files::folder(&w.root()).join("archive")),
        vec![format!("{id}-1.json")]
    );
    assert_eq!(w.moves_to(id, State::Passed), 1);
    assert_eq!(w.row(id).state, State::Passed);
    assert!(outcome(&w.row(id)).unwrap().passed);
    jobs::archive_results(&w.root(), &reads).unwrap();
}

fn run_job_as(w: &mut World, id: i64, me: u32) -> Option<Outcome> {
    let ctx = JobCtx {
        guard: None,
        config: &w.config,
        needs_you: &NeedsYouList,
    };
    let checks = CheckRunner {
        host: &Launcher,
        profiles: &NoProfiles,
        sweep: None,
    };
    jobs::run_job(&ctx, &w.conn, id, me, &checks).unwrap()
}

#[test]
fn unrecorded_or_mismatched_pid_process_cannot_write_job_result_file() {
    let mut w = world();
    let launcher = Launcher;
    let mut runner = JobRunner::new(&launcher);
    let id = w.running(&mut runner, Kind::DocsSync);
    let recorded_pid = w.row(id).pid.unwrap();
    let wrong_pid = recorded_pid.wrapping_add(1);
    let before = w.row(id);
    let changes = w.conn.total_changes();

    assert!(run_job_as(&mut w, id, wrong_pid).is_none());
    assert!(!files::path(&w.root(), id, 1).exists());
    assert!(!files::folder(&w.root()).exists() || names(&files::folder(&w.root())).is_empty());
    assert_eq!(w.row(id).state, State::Running);
    assert_eq!(w.row(id).result_json, None);
    assert_eq!(w.row(id), before);
    assert_eq!(w.conn.total_changes(), changes, "run_job wrote the ledger");

    assert!(run_job_as(&mut w, id, recorded_pid).is_some());
    assert!(files::path(&w.root(), id, 1).is_file());

    let ended = w.running(&mut runner, Kind::Push);
    let ended_pid = w.row(ended).pid.unwrap();
    let gone = ProcessFact {
        key: ended.to_string(),
        liveness: Liveness::Dead,
    };
    let seen = w.tx(|ctx, tx| jobs::observe(ctx, tx, &[gone], FleetSeconds(1)).unwrap());
    assert_eq!(seen.ended, vec![(ended, State::Abandoned)]);
    assert!(run_job_as(&mut w, ended, ended_pid).is_none());
    assert!(!files::path(&w.root(), ended, 1).exists());
    assert_eq!(w.row(ended).state, State::Abandoned);
}

#[test]
fn queued_job_has_no_pid_and_gets_no_result_file() {
    let mut w = world();
    let id = w.queue(Kind::DocsSync, "F-2", 0);
    assert_eq!(w.row(id).pid, None);
    assert!(run_job_as(&mut w, id, 4242).is_none());
    assert!(!files::path(&w.root(), id, 1).exists());
    assert_eq!(w.row(id).state, State::Queued);
    assert_eq!(w.row(id).result_json, None);
}

fn assert_one_ignored_note(w: &World, id: i64, run: i64, state: &str) {
    let notes = w.events(id, "note");
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(
        notes[0].contains(&format!("ignored the result file of run {run}"))
            && notes[0].contains(state),
        "{notes:?}"
    );
}

#[test]
fn result_file_for_queued_or_already_ended_job_is_ignored_and_logged_once() {
    let mut w = world();
    let launcher = Launcher;
    let mut runner = JobRunner::new(&launcher);

    let done = w.running(&mut runner, Kind::DocsSync);
    let first = files::write(&w.root(), &passed(done, 1, "real")).unwrap();
    let (reads, ended) = w.pass_no_archive();
    assert_eq!(ended, vec![(done, State::Passed)]);
    jobs::archive_results(&w.root(), &reads).unwrap();
    assert!(!first.exists());

    let queued = w.queue(Kind::Push, "F-1", 0);
    assert_eq!(w.row(queued).state, State::Queued);

    let forged_queued = files::write(&w.root(), &passed(queued, 1, "forged while queued")).unwrap();
    let forged_done = files::write(&w.root(), &passed(done, 2, "forged after pass")).unwrap();
    let again_first = files::write(&w.root(), &passed(done, 1, "forged again")).unwrap();
    assert_eq!(again_first, first);

    let (reads, ended) = w.pass_no_archive();
    assert_eq!(reads.len(), 3);
    assert_eq!(ended, vec![]);

    assert_eq!(w.row(queued).state, State::Queued);
    assert_eq!(w.row(queued).result_json, None);
    assert_eq!(w.moves_to(queued, State::Passed), 0);
    assert_eq!(w.moves_to(queued, State::Failed), 0);
    assert_one_ignored_note(&w, queued, 1, "queued");
    assert_eq!(w.ingested(&forged_queued), Some(("accepted".into(), None)));

    let row = w.row(done);
    assert_eq!(row.state, State::Passed);
    assert_eq!(outcome(&row).unwrap().text, "real");
    assert_eq!(w.moves_to(done, State::Passed), 1);
    assert_one_ignored_note(&w, done, 2, "passed");
    assert!(reads
        .iter()
        .any(|r| r.path == again_first && matches!(r.read, Read::Seen)));
    assert_eq!(w.ingested(&forged_done), Some(("accepted".into(), None)));

    let again = w.read(w.config.job_result_max_bytes);
    assert_eq!(again.len(), 3);
    assert!(again.iter().all(|r| matches!(r.read, Read::Seen)));
    assert_eq!(w.ingest(&again), vec![]);
    assert_eq!(w.events(queued, "note").len(), 1);
    assert_eq!(w.events(done, "note").len(), 1);
    assert_eq!(w.row(queued).state, State::Queued);
}
