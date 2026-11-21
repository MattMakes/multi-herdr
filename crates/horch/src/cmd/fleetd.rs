use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use clap::Subcommand;
use horch_core::exeid::exe_identity;
use horch_core::fleetd::fleet_runtime::adapters::channel_server::ChannelServer;
use horch_core::fleetd::fleet_runtime::adapters::clock_store::awake_now;
use horch_core::fleetd::fleet_runtime::adapters::config_file;
use horch_core::fleetd::fleet_runtime::adapters::effects::{
    exe_of, read_heartbeat, this_exe, write_heartbeat, WATCH_LIMIT,
};
use horch_core::fleetd::fleet_runtime::adapters::fleetd_row::{self, lock_path, wait_until_free};
use horch_core::fleetd::fleet_runtime::adapters::old_watchdog;
use horch_core::fleetd::fleet_runtime::adapters::peer_process::{
    fleet_process, recorded_processes, Fact, PlatformFacts,
};
use horch_core::fleetd::fleet_runtime::adapters::scans::{
    outbox_dir, requests_dir, CliPath, FolderItem,
};
use horch_core::fleetd::fleet_runtime::adapters::watchdog_duties::{
    idea_lead_launch, HerdrObserver, WatchdogDuties,
};
use horch_core::fleetd::fleet_runtime::application::handlers_cli;
use horch_core::fleetd::fleet_runtime::application::handlers_custody::{self, Custody};
use horch_core::fleetd::fleet_runtime::application::handlers_idea::{self, IdeaHandlers};
use horch_core::fleetd::fleet_runtime::application::handlers_task::{self, TaskHandlers};
use horch_core::fleetd::fleet_runtime::application::handlers_worker::{self, WorkerDeps};
use horch_core::fleetd::fleet_runtime::application::pass::{
    IdeaPorts, Outside, Pass, PassError, PoolFacts, Ports, Primary, Settings, TaskWork,
    DROP_WATCH_EVENTS, IDEAS_DIR,
};
use horch_core::fleetd::fleet_runtime::application::requests::Handlers;
use horch_core::fleetd::fleet_runtime::application::{handlers_run, holds};
use horch_core::fleetd::fleet_runtime::domain::config::HANG_LIMIT_S;
use horch_core::fleetd::fleet_runtime::domain::health::{Finding, Hold};
use horch_core::fleetd::fleet_runtime::domain::heartbeat::{is_hung, is_older, Exe, Heartbeat};
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::fleetd::ideas::adapters::needs_you_folder::Mirrored;
use horch_core::fleetd::ideas::adapters::notify::{self, Notice};
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::ideas::domain::plan_validator::{DeclaredGates, RosterProfile};
use horch_core::fleetd::integration::adapters::git::ProjectGitGateway;
use horch_core::fleetd::integration::adapters::project_file;
use horch_core::fleetd::integration::adapters::sqlite as feature_rows;
use horch_core::fleetd::integration::application::commands::GitRefTips;
use horch_core::fleetd::integration::application::ports::IntegrationGitGateway;
use horch_core::fleetd::task_execution::application::dispatch::CheckSandboxes;
use horch_core::fleetd::task_execution::application::launch::{Launches, PushLaunch};
use horch_core::fleetd::task_execution::domain::task::{AttemptRow, InPane};
use horch_core::fleetd::worker_profiles::adapters::check_sandbox::WorkerSandboxes;
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::ports::{EnvRule, LaunchSpec, ProcessHost};
use horch_core::fleetd::worktree_custody::adapters::git::{CliWorktreeGit, FLEETD_GIT_LIMIT};
use horch_core::fleetd::worktree_custody::adapters::rescue_git::rescue_git_version;
use horch_core::fleetd::worktree_custody::adapters::sqlite::{self as trees, WorktreeState};
use horch_core::fleetd::worktree_custody::application::reconcile::WorkDir;
use horch_core::messaging::delivery::{self, TypedLine};
use horch_core::procid;
use horch_core::runtime::{process, Paths, RuntimeContext};
use horch_core::taskledger::finish::CodexSandbox;
use horch_core::taskledger::model::RunStatus;
use horch_core::taskledger::orchestrator;
use horch_core::taskledger::runs;
use horch_core::taskledger::sql_words::list;
use horch_core::taskledger::store::{canonical, default_path, BUSY_TIMEOUT};
use horch_core::taskledger::watchdog::{Observation, Observer};
use horch_core::taskledger::workers::Worker;
use horch_core::taskledger::{watchdog, LedgerWriter, Store};
use horch_core::workspace::PaneHost;
use horch_platform::channel::User;
use horch_platform::lock::FleetdLock;
use horch_platform::service;
use horch_platform::watch::Watcher;
use rusqlite::Connection;
use sha2::{Digest, Sha256};

use super::taskledger_watch::LiveEffects;
use crate::output;

/// `horch fleetd` with no subcommand runs the daemon.
#[derive(Subcommand)]
pub enum FleetdCommand {
    /// Restart fleetd when its process is gone, its heartbeat is older than
    /// 30 s of awake time, or its binary changed. Does nothing when no
    /// fleetd ran on this ledger.
    Check,
}

pub(crate) const HC_REFUSED: &str = "fleetd runs its workers only on herdr: \
     the hc pane host (herdr-core and the Herdr desktop app) has no fleetd adapter. \
     Run this command with --backend herdr, or unset HORCH_BACKEND";

pub(crate) fn refuse_hc(ctx: &RuntimeContext) -> Result<()> {
    if ctx.herdr.backend == horch_core::runtime::Backend::Hc {
        bail!(HC_REFUSED);
    }
    Ok(())
}

pub fn fleetd(ctx: &RuntimeContext, command: Option<FleetdCommand>) -> Result<ExitCode> {
    refuse_hc(ctx)?;
    match command {
        None => daemon(ctx),
        Some(FleetdCommand::Check) => {
            let checked = run_check(ctx)?;
            output::println(&describe(&checked));
            Ok(ExitCode::SUCCESS)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Found {
    NoRow,
    Fresh,
    Dead,
    Hung,
    Older,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fleetd {
    pub pid: u32,
    pub pid_start: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    pub found: Found,
    pub old: Option<Fleetd>,
    pub new: Option<Fleetd>,
    pub mode: Mode,
}

impl Checked {
    fn started(&self) -> bool {
        matches!(self.found, Found::Dead | Found::Hung | Found::Older)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Service,
    StandIn,
}

pub fn mode(paths: &Paths, os_home: Option<&Path>) -> Mode {
    let Some(home) = os_home else {
        return Mode::StandIn;
    };
    let ledger = default_path(home);
    let own_root = ledger.parent().map(|d| d.join("fleet"));
    if paths.ledger_db == ledger && own_root.as_deref() == Some(paths.fleet_root.as_path()) {
        Mode::Service
    } else {
        Mode::StandIn
    }
}

pub(crate) struct Fleet<'a> {
    pub ledger: &'a Path,
    pub root: &'a Path,
    pub mode: Mode,
    pub start: &'a dyn Fn(&Path) -> Result<()>,
}

const START_WAIT: Duration = Duration::from_secs(10);

fn read_row(ledger: &Path) -> Result<Option<Fleetd>> {
    if !ledger.is_file() {
        return Ok(None);
    }
    let store = Store::open_read_only(ledger)?;
    let has_table: bool = store.conn().query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'fleetd')",
        [],
        |r| r.get(0),
    )?;
    if !has_table {
        return Ok(None);
    }
    let row = horch_core::taskledger::store::optional(store.conn().query_row(
        "SELECT pid, pid_start FROM fleetd WHERE id = 1",
        [],
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
    ))?;
    let Some((pid, pid_start)) = row else {
        return Ok(None);
    };
    Ok(Some(Fleetd {
        pid: u32::try_from(pid).context("the PID of the fleetd row")?,
        pid_start: u64::try_from(pid_start).context("the start time of the fleetd row")?,
    }))
}

fn own_heartbeat(root: &Path, f: Fleetd) -> Option<Heartbeat> {
    read_heartbeat(root)
        .ok()
        .flatten()
        .filter(|hb| hb.pid == f.pid && hb.pid_start == f.pid_start)
}

fn judge_live(root: &Path, f: Fleetd) -> Result<(Found, Option<Heartbeat>)> {
    let Some(hb) = own_heartbeat(root, f) else {
        return Ok((Found::Fresh, None));
    };
    let now = awake_now().context("reading the awake clock")?;
    let found = if is_hung(&hb, &now, HANG_LIMIT_S) {
        Found::Hung
    } else if is_older(&hb, &binary_now(&hb)) {
        Found::Older
    } else {
        Found::Fresh
    };
    Ok((found, Some(hb)))
}

fn binary_now(hb: &Heartbeat) -> Exe {
    exe_at(Path::new(&hb.exe.path))
}

fn exe_at(path: &Path) -> Exe {
    exe_of(exe_identity(path).unwrap_or_default())
}

fn binary_changed(started: &Exe) -> bool {
    exe_identity(Path::new(&started.path)).is_some_and(|now| exe_of(now) != *started)
}

pub(crate) fn check_with(fleet: &Fleet<'_>) -> Result<Checked> {
    let Some(old) = read_row(fleet.ledger)? else {
        return Ok(Checked {
            found: Found::NoRow,
            old: None,
            new: None,
            mode: fleet.mode,
        });
    };
    let (found, hb) = if procid::alive(old.pid, Some(old.pid_start)) {
        judge_live(fleet.root, old)?
    } else {
        (Found::Dead, None)
    };
    let mut checked = Checked {
        found,
        old: Some(old),
        new: None,
        mode: fleet.mode,
    };
    if !checked.started() {
        return Ok(checked);
    }
    let exe = hb
        .map(|hb| PathBuf::from(hb.exe.path))
        .filter(|p| p.is_file())
        .map_or_else(std::env::current_exe, Ok)
        .context("finding the horch binary")?;
    checked.new = restart(fleet, old, found, &exe)?;
    Ok(checked)
}

pub(crate) fn restart_older(fleet: &Fleet<'_>, installed: &Path) -> Result<Option<Checked>> {
    let Some(old) = read_row(fleet.ledger)? else {
        return Ok(None);
    };
    if !procid::alive(old.pid, Some(old.pid_start)) {
        return Ok(None);
    }
    let Some(hb) = own_heartbeat(fleet.root, old) else {
        return Ok(None);
    };
    if !is_older(&hb, &exe_at(installed)) {
        return Ok(None);
    }
    let new = restart(fleet, old, Found::Older, installed)?;
    Ok(Some(Checked {
        found: Found::Older,
        old: Some(old),
        new,
        mode: fleet.mode,
    }))
}

fn restart(fleet: &Fleet<'_>, old: Fleetd, found: Found, exe: &Path) -> Result<Option<Fleetd>> {
    if fleet.mode == Mode::StandIn {
        refuse_fleet_caller(fleet.ledger)?;
        stop(fleet, old, found)?;
    }
    (fleet.start)(exe)?;
    Ok(wait_for_new(fleet.ledger, old))
}

fn refuse_fleet_caller(ledger: &Path) -> Result<()> {
    let reason = match own_fact(ledger) {
        Fact::No => return Ok(()),
        Fact::Yes(attempts) => format!(
            "this process is a fleet process of attempt {}",
            attempts.join(", ")
        ),
        Fact::Unknown => {
            "horch cannot prove that this process runs outside a fleet attempt, because a process fact is unknown".into()
        }
    };
    bail!(
        "a fleet process stops and starts no fleetd: {reason}. Run `horch fleetd check` yourself from a terminal outside the fleet"
    )
}

fn own_fact(ledger: &Path) -> Fact {
    if !ledger.is_file() {
        return Fact::No;
    }
    let Ok(store) = Store::open_read_only(ledger) else {
        return Fact::Unknown;
    };
    match recorded_processes(store.conn()) {
        Ok(Some((recorded, _))) => fleet_process(std::process::id(), &recorded, &PlatformFacts),
        Ok(None) | Err(_) => Fact::Unknown,
    }
}

fn stop(fleet: &Fleet<'_>, old: Fleetd, found: Found) -> Result<()> {
    let lock = lock_path(fleet.root);
    if found == Found::Dead {
        if !lock_free(&lock, Instant::now() + START_WAIT)? {
            bail!(
                "{} is held, but fleetd PID {} is gone",
                lock.display(),
                old.pid
            );
        }
        return Ok(());
    }
    if found == Found::Older {
        procid::stop_same(old.pid, old.pid_start, procid::Stop::Term);
        let grace = Duration::from_secs(config_file::load(fleet.root)?.stop_grace_s);
        if lock_free(&lock, Instant::now() + grace)? {
            return Ok(());
        }
    }
    procid::stop_same(old.pid, old.pid_start, procid::Stop::Kill);
    if !lock_free(&lock, Instant::now() + START_WAIT)? {
        bail!(
            "fleetd PID {} was stopped, but {} is still held",
            old.pid,
            lock.display()
        );
    }
    Ok(())
}

fn lock_free(lock: &Path, deadline: Instant) -> Result<bool> {
    if lock.parent().is_some_and(|d| !d.is_dir()) {
        return Ok(true);
    }
    wait_until_free(lock, deadline).with_context(|| format!("waiting for {}", lock.display()))
}

fn wait_for_new(ledger: &Path, old: Fleetd) -> Option<Fleetd> {
    let until = Instant::now() + START_WAIT;
    loop {
        if let Ok(Some(row)) = read_row(ledger) {
            if row != old && procid::alive(row.pid, Some(row.pid_start)) {
                return Some(row);
            }
        }
        if Instant::now() >= until {
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub(crate) fn run_check(ctx: &RuntimeContext) -> Result<Checked> {
    with_fleet(ctx, check_with)
}

pub(crate) fn with_fleet<T>(ctx: &RuntimeContext, f: impl FnOnce(&Fleet<'_>) -> T) -> T {
    let paths = &ctx.paths;
    let mode = this_mode(ctx);
    let start = |exe: &Path| start_fleetd(ctx, mode, exe);
    f(&Fleet {
        ledger: &paths.ledger_db,
        root: &paths.fleet_root,
        mode,
        start: &start,
    })
}

fn this_mode(ctx: &RuntimeContext) -> Mode {
    mode(&ctx.paths, service::os_user_home().ok().as_deref())
}

pub(crate) fn service_spec(exe: &Path) -> Result<service::Spec> {
    let home = service::os_user_home().context("finding the home of the OS user")?;
    service_spec_for(exe, &default_path(&home), Some(&home))
}

pub(crate) fn service_spec_for(
    exe: &Path,
    ledger: &Path,
    os_home: Option<&Path>,
) -> Result<service::Spec> {
    let label = service_label(ledger, os_home);
    let env = if label == service::LABEL {
        Vec::new()
    } else {
        vec![(
            "HORCH_LEDGER_DB".into(),
            ledger.to_string_lossy().into_owned(),
        )]
    };
    Ok(service::Spec {
        label,
        exe: exe.to_path_buf(),
        env,
        folder: service::default_folder().context("finding the folder of the fleetd service")?,
    })
}

pub(crate) fn service_label(ledger: &Path, os_home: Option<&Path>) -> String {
    let real = canonical(ledger);
    if os_home.is_some_and(|home| canonical(&default_path(home)) == real) {
        return service::LABEL.into();
    }
    service::ledger_label(&Sha256::digest(real.as_os_str().as_encoded_bytes()))
}

fn start_fleetd(ctx: &RuntimeContext, mode: Mode, exe: &Path) -> Result<()> {
    match mode {
        Mode::Service => {
            let spec = service_spec(exe)?;
            service::restart(&spec, Path::new(service::MANAGER))
                .with_context(|| format!("restarting the service {}", service::LABEL))
        }
        Mode::StandIn => {
            let paths = &ctx.paths;
            let var = |k: &str, v: &Path| (k.into(), v.as_os_str().to_os_string());
            let spec = LaunchSpec {
                program: exe.to_path_buf(),
                args: vec!["fleetd".into()],
                cwd: Some(paths.fleet_root.clone()).filter(|r| r.is_dir()),
                env_rule: EnvRule {
                    vars: vec![
                        var("HORCH_LEDGER_DB", &paths.ledger_db),
                        var("HORCH_STATE_DIR", &paths.state_root),
                        var("HORCH_DATA_DIR", &paths.data_root),
                        var("FLEET_ROOT", &paths.fleet_root),
                    ],
                },
                detach: true,
                output: Some(paths.fleet_root.join("run").join("fleetd.log")),
            };
            Launcher.launch(&spec).context("starting horch fleetd")?;
            Ok(())
        }
    }
}

pub(crate) fn describe(c: &Checked) -> String {
    let old = c.old.map_or(0, |f| f.pid);
    let new = match (c.new, c.mode) {
        (Some(f), _) => format!("fleetd PID {} runs now", f.pid),
        (None, Mode::Service) => format!("the service {} was restarted", service::LABEL),
        (None, Mode::StandIn) => {
            "a new fleetd was started, but the fleetd row does not name it yet".into()
        }
    };
    match c.found {
        Found::NoRow => "No fleetd ran on this ledger: the check starts none.".into(),
        Found::Fresh => format!("fleetd PID {old} runs, and its heartbeat is fresh."),
        Found::Dead => format!("fleetd PID {old} is gone. {new}."),
        Found::Hung => format!(
            "fleetd PID {old} wrote no heartbeat for {HANG_LIMIT_S} s of awake time. It was stopped, and {new}."
        ),
        Found::Older => format!(
            "fleetd PID {old} runs an older binary. It was stopped, and {new}."
        ),
    }
}

pub fn check_before_command(ctx: &RuntimeContext) {
    if ctx.settings.no_watchdog {
        return;
    }
    match run_check(ctx) {
        Ok(c) if c.started() => eprintln!("horch: {}", describe(&c)),
        Ok(_) => {}
        Err(e) => eprintln!("horch: the fleetd check failed: {e:#}"),
    }
}

fn write_start_heartbeat(root: &Path, generation: Generation, exe: &Exe) -> Result<()> {
    let pid = std::process::id();
    let hb = Heartbeat {
        pid,
        pid_start: procid::start_time(pid).context("reading the start time of fleetd")?,
        generation,
        awake: awake_now().context("reading the awake clock")?,
        at: horch_core::taskledger::util::now(),
        exe: exe.clone(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };
    write_heartbeat(root, &hb)
}

static TERM: AtomicBool = AtomicBool::new(false);

fn sleep_unless_term(d: Duration, woke: &mut dyn FnMut() -> bool) {
    let until = Instant::now() + d;
    while !TERM.load(Ordering::SeqCst) && Instant::now() < until && !woke() {
        std::thread::sleep(Duration::from_millis(100).min(until - Instant::now()));
    }
}

struct Wake {
    root: PathBuf,
    ledger: Option<Connection>,
    watcher: Option<Watcher>,
    watched: BTreeSet<PathBuf>,
    armed_version: Option<i64>,
}

impl Wake {
    fn new(root: &Path, ledger: &Path) -> Wake {
        Wake {
            root: root.to_path_buf(),
            ledger: ledger_conn(ledger).ok(),
            watcher: Watcher::new().ok(),
            watched: BTreeSet::new(),
            armed_version: None,
        }
    }

    fn arm(&mut self) {
        let dirs = self.folders();
        if let Some(w) = self.watcher.as_mut() {
            let wanted: BTreeSet<&PathBuf> = dirs.iter().collect();
            self.watched.retain(|d| {
                let keep = wanted.contains(d);
                if !keep {
                    w.remove(d);
                }
                keep
            });
            for d in &dirs {
                if self.watched.len() >= WATCH_LIMIT {
                    break;
                }
                if !self.watched.contains(d) && w.add(d).is_ok() {
                    self.watched.insert(d.clone());
                }
            }
            let _events_of_the_pass = w.changed();
        }
        self.armed_version = self.data_version();
    }

    fn woke(&mut self) -> bool {
        let files = self
            .watcher
            .as_mut()
            .is_some_and(|w| !w.changed().is_empty());
        files || (self.armed_version.is_some() && self.data_version() != self.armed_version)
    }

    fn data_version(&self) -> Option<i64> {
        let conn = self.ledger.as_ref()?;
        conn.query_row("PRAGMA data_version", [], |r| r.get(0)).ok()
    }

    fn folders(&self) -> Vec<PathBuf> {
        let mut dirs = vec![requests_dir(&self.root), self.root.join("run").join("jobs")];
        let Some(conn) = self.ledger.as_ref() else {
            return dirs;
        };
        let attempts = conn
            .prepare(
                "SELECT task_id, id FROM attempts
                 WHERE state <> 'ended' AND task_id IS NOT NULL ORDER BY id LIMIT ?1",
            )
            .and_then(|mut stmt| {
                stmt.query_map([WATCH_LIMIT / 2], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap_or_default();
        for (task, attempt) in attempts {
            let outbox = outbox_dir(&self.root, &task, &attempt);
            if let Some(dir) = outbox.parent() {
                dirs.push(dir.to_path_buf());
            }
            dirs.push(outbox);
        }
        dirs
    }
}

pub(crate) fn ledger_conn(ledger: &Path) -> Result<Connection> {
    drop(Store::open(ledger)?);
    let conn = Connection::open(ledger)
        .with_context(|| format!("opening the task ledger {}", ledger.display()))?;
    conn.busy_timeout(BUSY_TIMEOUT)?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(conn)
}

fn daemon(ctx: &RuntimeContext) -> Result<ExitCode> {
    let _ = horch_platform::signal::on_termination_set(
        &TERM,
        &[horch_platform::signal::Termination::Terminate],
    );
    let own_exe = this_exe();
    let horch = std::env::current_exe().context("finding the horch binary")?;
    if let Fact::Yes(attempts) = own_fact(&ctx.paths.ledger_db) {
        bail!(
            "horch fleetd: refused inside a fleet attempt: this process is a fleet process of attempt {}. Only an operator process or the service manager starts fleetd",
            attempts.join(", ")
        );
    }
    if let Err(problem) = rescue_git_version(
        &ctx.bins.harness.git,
        ctx.inherited.path.as_deref(),
        ctx.inherited.pathext.as_deref(),
    ) {
        bail!("horch fleetd: refused: {problem}");
    }
    let root = &ctx.paths.fleet_root;
    let writer = LedgerWriter::fleetd();
    let mut store = writer.open(&ctx.paths.ledger_db)?;
    let Some(started) = fleetd_row::start(root, &mut store)? else {
        output::println(&format!(
            "Another fleetd holds {}: this fleetd exits.",
            lock_path(root).display()
        ));
        return Ok(ExitCode::SUCCESS);
    };
    write_start_heartbeat(root, started.generation, &own_exe)?;
    let settings = settings(ctx, started.generation)?;
    take_over(ctx, &mut store, &settings)?;
    drop(store);
    let period = Duration::from_secs(settings.config.pass_period_s);
    let mut conn = ledger_conn(&ctx.paths.ledger_db)?;
    output::println(&format!(
        "fleetd PID {} runs generation {} on {}.",
        std::process::id(),
        started.generation.0,
        ctx.paths.ledger_db.display()
    ));
    let self_restart = SelfRestart {
        ctx,
        exe: own_exe,
        horch,
        lock: RefCell::new(Some(started.lock)),
    };
    let handlers = request_handlers(ctx, &settings);
    let wake = (!settings.faults.has(DROP_WATCH_EVENTS))
        .then(|| RefCell::new(Wake::new(root, &ctx.paths.ledger_db)));
    with_pass(ctx, &writer, settings, false, |pass, _| {
        let Some(_channel) = start_channel(ctx, pass, &mut conn, handlers) else {
            return;
        };
        run_loop(
            period,
            &mut || {
                let passed = one_pass(pass, &mut conn);
                if let Some(w) = &wake {
                    w.borrow_mut().arm();
                }
                passed
            },
            &mut || wake.as_ref().is_some_and(|w| w.borrow_mut().woke()),
            &|| self_restart.changed(),
            &mut || self_restart.restart(),
        )
    });
    Ok(ExitCode::SUCCESS)
}

fn start_channel(
    ctx: &RuntimeContext,
    pass: &mut Pass<'_>,
    conn: &mut Connection,
    handlers: Handlers,
) -> Option<Option<ChannelServer>> {
    match pass.start(conn) {
        Ok(failed) => {
            for f in &failed {
                eprintln!("horch fleetd: {f}");
            }
        }
        Err(PassError::GenerationLost(e)) => {
            eprintln!("horch fleetd: {e}: this fleetd exits");
            return None;
        }
        Err(e @ PassError::Failed(_)) => eprintln!("horch fleetd: the start: {e}"),
    }
    let server = ledger_conn(&ctx.paths.ledger_db).and_then(|c| pass.serve(c, handlers));
    match server {
        Ok(server) => Some(Some(server)),
        Err(e) => {
            eprintln!("horch fleetd: the command channel did not start: {e:#}");
            Some(None)
        }
    }
}

fn request_handlers(ctx: &RuntimeContext, settings: &Settings) -> Handlers {
    let config = Arc::new(settings.config.clone());
    let guard = settings.generation;
    let mut handlers = Handlers::new().with_faults(settings.faults.clone());
    handlers_run::register(&mut handlers);
    handlers_task::register(
        &mut handlers,
        TaskHandlers {
            config: Arc::clone(&config),
            evidence_root: super::taskledger::tools(ctx).evidence_root,
        },
    );
    let worker = WorkerDeps { guard };
    handlers_worker::register(&mut handlers, &worker);
    handlers_cli::register(&mut handlers);
    let git = CliWorktreeGit::new(&ctx.bins.harness.git, &settings.fleet_root)
        .with_time_limit(FLEETD_GIT_LIMIT);
    handlers_custody::register(
        &mut handlers,
        Arc::new(Custody {
            guard,
            config: settings.config.clone(),
            refs: Box::new(GitRefTips { git: git.clone() }),
        }),
    );
    register_idea_handlers(ctx, settings, &mut handlers, git);
    handlers
}

fn register_idea_handlers(
    ctx: &RuntimeContext,
    settings: &Settings,
    handlers: &mut Handlers,
    git: CliWorktreeGit,
) {
    let owner = match User::current() {
        Ok(owner) => owner,
        Err(e) => {
            eprintln!("horch fleetd: no idea requests: reading the user of fleetd: {e}");
            return;
        }
    };
    let roster_ctx = ctx.clone();
    let sandboxes = check_sandboxes(ctx);
    handlers_idea::register(
        handlers,
        IdeaHandlers {
            guard: settings.generation,
            config: Arc::new(settings.config.clone()),
            ideas: settings.fleet_root.join(IDEAS_DIR),
            owner,
            roster: Arc::new(move || roster_profiles(&roster_ctx, &sandboxes)),
            git: Arc::new(ProjectGitGateway::new(git)),
        },
    );
}

fn roster_profiles(ctx: &RuntimeContext, sandboxes: &WorkerSandboxes) -> Vec<RosterProfile> {
    let Ok(roster) = super::load_roster_unwarned(ctx, None) else {
        return Vec::new();
    };
    roster
        .names()
        .into_iter()
        .filter_map(|name| roster.get(name))
        .map(|t| RosterProfile {
            name: t.name.clone(),
            provider: t.agent.as_str().to_string(),
            model: t.model.clone().unwrap_or_default(),
            effort: t.effort.clone(),
            fallback: t.fallbacks.first().cloned(),
            sandbox_here: sandboxes.supported(&t.name),
        })
        .collect()
}

struct LiveIdeas<'a> {
    ctx: &'a RuntimeContext,
    sandboxes: &'a WorkerSandboxes,
    git: ProjectGitGateway,
    herdr: PaneHost,
}

impl LiveIdeas<'_> {
    fn projects(&self, conn: &Connection, branchless: bool) -> Result<Vec<(PathBuf, String)>> {
        let mut stmt = conn.prepare(
            "SELECT id, primary_branch FROM features
             WHERE state IN ('open', 'ready') AND (?1 = 0 OR branch IS NULL)",
        )?;
        let features = stmt
            .query_map([branchless], |r| Ok((r.get::<_, String>(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<Vec<(String, String)>>>()
            .context("reading the open features")?;
        let mut projects = std::collections::BTreeSet::new();
        for (id, branch) in features {
            if let Some(run) = runs::feature_run(conn, &id)? {
                projects.insert((PathBuf::from(run.project_root), branch));
            }
        }
        Ok(projects.into_iter().collect())
    }

    fn tip(&self, conn: &Connection, repo: &Path, branch: &str) -> Result<Option<String>> {
        if let Some(row) = feature_rows::remote_tip(conn, &repo.to_string_lossy(), branch)? {
            if row.stale_since.is_none() {
                return Ok(Some(row.tip));
            }
        }
        self.git.tip(repo, &format!("refs/heads/{branch}"))
    }
}

impl IdeaPorts for LiveIdeas<'_> {
    fn roster(&self) -> Vec<RosterProfile> {
        roster_profiles(self.ctx, self.sandboxes)
    }

    fn gates(&self, conn: &Connection) -> Result<DeclaredGates> {
        let Some((repo, branch)) = self.projects(conn, false)?.into_iter().next() else {
            return Ok(DeclaredGates::default());
        };
        let Some(tip) = self.tip(conn, &repo, &branch)? else {
            return Ok(DeclaredGates::default());
        };
        let Some(text) = self.git.fleet_toml(&repo, &tip)? else {
            return Ok(DeclaredGates::default());
        };
        let project =
            project_file::parse(&text).with_context(|| format!("{} at {tip}", repo.display()))?;
        Ok(DeclaredGates {
            fast: project.fast_gate().map(str::to_string),
            full: project.full_gate().map(str::to_string),
        })
    }

    fn git(&self) -> &dyn IntegrationGitGateway {
        &self.git
    }

    fn primaries(&self, conn: &Connection) -> Result<Vec<Primary>> {
        let mut primaries = Vec::new();
        for (repo, branch) in self.projects(conn, true)? {
            if let Some(tip) = self.tip(conn, &repo, &branch)? {
                primaries.push(Primary { repo, branch, tip });
            }
        }
        Ok(primaries)
    }

    fn now(&self) -> chrono::DateTime<chrono::FixedOffset> {
        horch_core::clock::now()
            .with_timezone(&chrono::Local)
            .fixed_offset()
    }

    fn notify(&self, mirrored: &Mirrored, seq: u64) -> Result<()> {
        match notify::notify(&self.herdr, mirrored, seq) {
            Notice::Quiet | Notice::Sent => Ok(()),
            Notice::Failed(line) => bail!("the needs-you report to herdr: {line}"),
        }
    }
}

fn check_sandboxes(ctx: &RuntimeContext) -> WorkerSandboxes {
    let codex = CodexSandbox {
        bin: ctx.bins.harness.codex.clone(),
        state_root: ctx.paths.state_root.clone(),
        personas: super::taskledger::codex_personas(ctx),
    };
    WorkerSandboxes::on_path(
        codex,
        ctx.paths.fleet_root.clone(),
        ctx.inherited.path.clone(),
    )
}

struct PushSpawns<'a> {
    ctx: &'a RuntimeContext,
    herdr: PaneHost,
}

impl Launches for PushSpawns<'_> {
    fn spec(&self, l: &PushLaunch) -> Result<LaunchSpec> {
        spawn_spec(self.ctx, &self.herdr, l.clone())
    }

    fn resuming_harness(&self, persona: &str) -> Option<String> {
        let roster = super::load_roster_unwarned(self.ctx, None).ok()?;
        let agent = roster.get(persona)?.agent;
        agent
            .capabilities()
            .resumes
            .then(|| agent.as_str().to_string())
    }
}

fn spawn_spec(ctx: &RuntimeContext, herdr: &PaneHost, mut l: PushLaunch) -> Result<LaunchSpec> {
    let (pane, workspace) = launch_pane(herdr, &l)?;
    l.from_pane = Some(pane);
    l.workspace_id = Some(workspace);
    super::recipes::push_launch_spec(ctx, &l)
}

fn launch_pane(herdr: &PaneHost, l: &PushLaunch) -> Result<(String, String)> {
    if let Some(pane) = &l.from_pane {
        let workspace = match &l.workspace_id {
            Some(w) => w.clone(),
            None => herdr.pane_get(pane)?.workspace_id.with_context(|| {
                format!(
                    "herdr names no workspace of pane {pane} of attempt {}",
                    l.attempt
                )
            })?,
        };
        return Ok((pane.clone(), workspace));
    }
    let label = Path::new(&l.project_root)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let no_pane = |why: &str| {
        anyhow::anyhow!(
            "the launch of attempt {} has no pane: the ledger records no pane for it, \
             and {why}. Start the fleet with horch fleet in {}",
            l.attempt,
            l.project_root
        )
    };
    let workspace = herdr
        .workspace_list()?
        .into_iter()
        .find(|w| w.label.as_deref() == Some(label.as_str()))
        .ok_or_else(|| no_pane(&format!("no herdr workspace has the label {label}")))?;
    let pane = herdr
        .pane_list(&workspace.workspace_id)?
        .into_iter()
        .next()
        .ok_or_else(|| no_pane(&format!("herdr workspace {label} has no pane")))?;
    Ok((pane.pane_id, workspace.workspace_id))
}

fn idea_lead_spec(
    ctx: &RuntimeContext,
    herdr: &PaneHost,
    a: &AttemptRow,
) -> Result<Option<LaunchSpec>> {
    let Some(store) = horch_core::taskledger::open_existing_read_only(ctx)? else {
        return Ok(None);
    };
    let Some(l) = idea_lead_launch(store.conn(), &ctx.paths.fleet_root, &a.id)? else {
        return Ok(None);
    };
    let push = PushLaunch {
        attempt: l.attempt,
        task_id: String::new(),
        worker_id: l.worker_id,
        persona: l.persona,
        run_slug: l.run_slug,
        project_root: l.project_root,
        from_pane: l.pane,
        workspace_id: l.workspace_id,
    };
    spawn_spec(ctx, herdr, push).map(Some)
}

fn settings(ctx: &RuntimeContext, generation: Generation) -> Result<Settings> {
    let root = &ctx.paths.fleet_root;
    Ok(Settings {
        fleet_root: root.clone(),
        generation,
        config: config_file::load(root)?,
        horch: ctx.bins.horch_exe.clone(),
        faults: ctx.settings.faults.clone(),
    })
}

fn take_over(ctx: &RuntimeContext, store: &mut Store, settings: &Settings) -> Result<()> {
    let locks = orchestrator::lock_dir(&ctx.paths.ledger_db);
    let grace = Duration::from_secs(settings.config.stop_grace_s);
    let taken =
        old_watchdog::take_over(store, settings.generation, &locks, grace, &old_watchdog::Os)
            .context("taking over the old watchdogs")?;
    for t in &taken {
        eprintln!("horch fleetd: {}", t.describe());
    }
    if !old_watchdog::all_gone(&taken) {
        bail!("an old watchdog may still run: fleetd runs no pass, and its next start tries again");
    }
    Ok(())
}

const HERDR_CALL_LIMIT: Duration = Duration::from_secs(5);

fn with_pass<T>(
    ctx: &RuntimeContext,
    writer: &LedgerWriter,
    settings: Settings,
    force: bool,
    f: impl FnOnce(&mut Pass<'_>, &WatchdogDuties<'_>) -> T,
) -> T {
    let herdr = || {
        PaneHost::for_backend(ctx, horch_core::runtime::Backend::Herdr)
            .with_timeout(HERDR_CALL_LIMIT)
    };
    let outside = Live {
        ctx,
        herdr: herdr(),
        observer: HerdrObserver { herdr: herdr() },
    };
    let observer = HerdrObserver { herdr: herdr() };
    let effects = LiveEffects { ctx, writer };
    let duties = WatchdogDuties::new(&observer, &effects);
    let duties = if force { duties.forced() } else { duties };
    let ports = Ports {
        outside: &outside,
        duties: &duties,
        needs_you: &NeedsYouList,
        host: &Launcher,
    };
    let requests = request_handlers(ctx, &settings);
    let tools = super::taskledger::tools(ctx);
    let sandboxes = check_sandboxes(ctx);
    let ideas = LiveIdeas {
        ctx,
        sandboxes: &sandboxes,
        git: ProjectGitGateway::new(
            CliWorktreeGit::new(&ctx.bins.harness.git, &settings.fleet_root)
                .with_time_limit(FLEETD_GIT_LIMIT),
        ),
        herdr: herdr(),
    };
    let spawns = PushSpawns {
        ctx,
        herdr: herdr(),
    };
    let work = TaskWork {
        tools: &tools,
        sandboxes: &sandboxes,
        launches: &spawns,
    };
    let mut pass = Pass::new(settings, ports)
        .with_requests(requests)
        .with_task_work(work)
        .with_idea_work(&ideas);
    f(&mut pass, &duties)
}

pub(crate) enum Once {
    Ran(Vec<(String, watchdog::Checked)>),
    Held(Option<Fleetd>),
}

pub(crate) fn watch_once(ctx: &RuntimeContext, force: bool) -> Result<Once> {
    let writer = LedgerWriter::fleetd();
    let mut store = writer.open(&ctx.paths.ledger_db)?;
    let Some(started) = fleetd_row::start(&ctx.paths.fleet_root, &mut store)? else {
        return Ok(Once::Held(read_row(&ctx.paths.ledger_db)?));
    };
    let settings = settings(ctx, started.generation)?;
    take_over(ctx, &mut store, &settings)?;
    drop(store);
    let mut conn = ledger_conn(&ctx.paths.ledger_db)?;
    with_pass(ctx, &writer, settings, force, |pass, duties| {
        for f in pass.start(&mut conn).map_err(|e| anyhow::anyhow!("{e}"))? {
            eprintln!("horch ledger watch: {f}");
        }
        let report = pass.run(&mut conn).map_err(|e| anyhow::anyhow!("{e}"))?;
        for f in &report.failed {
            eprintln!("horch ledger watch: {f}");
        }
        Ok(Once::Ran(duties.take_checked()))
    })
}

fn one_pass(pass: &mut Pass<'_>, conn: &mut Connection) -> Passed {
    match pass.run(conn) {
        Ok(report) => {
            for f in &report.failed {
                eprintln!("horch fleetd: {f}");
            }
            Passed::Ran
        }
        Err(PassError::GenerationLost(e)) => {
            eprintln!("horch fleetd: {e}: this fleetd exits");
            Passed::GenerationLost
        }
        Err(e @ PassError::Failed(_)) => {
            eprintln!("horch fleetd: {e}");
            Passed::Ran
        }
    }
}

struct SelfRestart<'a> {
    ctx: &'a RuntimeContext,
    exe: Exe,
    horch: PathBuf,
    lock: RefCell<Option<FleetdLock>>,
}

impl SelfRestart<'_> {
    fn changed(&self) -> bool {
        binary_changed(&self.exe)
    }

    fn restart(&self) {
        drop(self.lock.borrow_mut().take());
        let ctx = self.ctx;
        match start_fleetd(ctx, this_mode(ctx), &self.horch) {
            Ok(()) => output::println(&format!(
                "The binary {} changed: fleetd PID {} started it, and exits.",
                self.horch.display(),
                std::process::id()
            )),
            Err(e) => eprintln!(
                "horch fleetd: the binary {} changed, but the new fleetd did not start: {e:#}. `horch fleetd check` starts it.",
                self.horch.display()
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Passed {
    Ran,
    GenerationLost,
}

fn run_loop(
    period: Duration,
    pass: &mut dyn FnMut() -> Passed,
    woke: &mut dyn FnMut() -> bool,
    changed: &dyn Fn() -> bool,
    restart: &mut dyn FnMut(),
) {
    while !TERM.load(Ordering::SeqCst) {
        let began = Instant::now();
        if pass() == Passed::GenerationLost {
            return;
        }
        if !TERM.load(Ordering::SeqCst) && changed() {
            restart();
            return;
        }
        sleep_unless_term(period.saturating_sub(began.elapsed()), woke);
    }
}

struct Live<'a> {
    ctx: &'a RuntimeContext,
    herdr: PaneHost,
    observer: HerdrObserver,
}

impl Outside for Live<'_> {
    fn state_root(&self) -> Option<&Path> {
        Some(&self.ctx.paths.state_root)
    }

    fn panes(&self) -> Option<BTreeMap<String, InPane>> {
        let mut panes = BTreeMap::new();
        for ws in self.herdr.workspace_list().ok()? {
            for pane in self.herdr.pane_list(&ws.workspace_id).ok()? {
                let in_pane = match self.herdr.pane_shell_pid(&pane.pane_id) {
                    Err(_) => InPane::Unknown,
                    Ok(pid) => match procid::start_time(pid) {
                        Some(pid_start) => InPane::Runs { pid, pid_start },
                        None if !procid::alive(pid, None) => InPane::Gone(pid),
                        None => InPane::Unknown,
                    },
                };
                panes.insert(pane.pane_id, in_pane);
            }
        }
        Some(panes)
    }

    fn close_pane(&self, pane: &str) -> Result<()> {
        self.herdr.pane_close(pane)
    }

    fn holds(&self) -> Vec<Hold> {
        match holds::holds(self.ctx) {
            Ok(findings) => findings
                .into_iter()
                .filter_map(|f| match f {
                    Finding::Hold(h) => Some(h),
                    Finding::Unknown { .. } => None,
                })
                .collect(),
            Err(e) => {
                eprintln!("horch fleetd: the health guards: {e:#}");
                Vec::new()
            }
        }
    }

    fn worktree_folders(&self, conn: &Connection) -> Result<Vec<FolderItem>> {
        let kept: Vec<WorktreeState> = WorktreeState::ALL
            .into_iter()
            .filter(|s| *s != WorktreeState::Removed)
            .collect();
        Ok(trees::in_states(conn, &kept)?
            .into_iter()
            .map(|r| FolderItem {
                key: r.worktree_id,
                path: PathBuf::from(r.path),
            })
            .collect())
    }

    fn work_dirs(&self, conn: &Connection) -> Result<Vec<WorkDir>> {
        let mut out = Vec::new();
        for repo in self.repositories(conn)? {
            let work_dir = Path::new(&repo).join(".worktrees");
            let entries = match std::fs::read_dir(&work_dir) {
                Ok(entries) => entries,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => {
                    eprintln!("horch fleetd: listing {}: {e}", work_dir.display());
                    continue;
                }
            };
            let folders = entries
                .flatten()
                .map(|e| e.path().display().to_string())
                .collect();
            out.push(WorkDir { repo, folders });
        }
        Ok(out)
    }

    fn repositories(&self, conn: &Connection) -> Result<Vec<String>> {
        let sql = format!(
            "SELECT DISTINCT p.root FROM runs r JOIN projects p ON p.project_id = r.project_id
             WHERE r.status NOT IN ({}) ORDER BY p.root",
            list(&[RunStatus::Done, RunStatus::Abandoned])
        );
        let mut stmt = conn.prepare(&sql)?;
        let roots = stmt
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>();
        roots.context("reading the repositories of the fleet")
    }

    fn attempt_spec(&self, attempt: &AttemptRow) -> Result<LaunchSpec> {
        if let Some(spec) = idea_lead_spec(self.ctx, &self.herdr, attempt)? {
            return Ok(spec);
        }
        bail!(
            "attempt {} has no launch: Phase 1 launches no task attempt",
            attempt.id
        )
    }

    fn observe(&self, w: &Worker) -> Option<Observation> {
        Some(self.observer.observe(w))
    }

    fn pools(&self) -> Option<PoolFacts> {
        let ctx = self.ctx;
        let root = &ctx.paths.state_root;
        let roster = super::load_roster_unwarned(ctx, None).ok()?;
        let policy = super::quotacmd::load_policy(ctx, root).ok()?;
        let env = super::quotacmd::quota_env(ctx);
        let now = horch_core::clock::now();
        let quota = horch_core::routing::snapshot::obtain(root, now, &policy, false, &env).ok()?;
        Some(PoolFacts { roster, quota })
    }

    fn doorbell(&self, pane: &str) -> Result<()> {
        delivery::send_line(&self.herdr, pane, TypedLine::Doorbell)
    }

    fn cli_binaries(&self) -> Vec<CliPath> {
        let Ok(sources) = agent_docs_sync::sources() else {
            return Vec::new();
        };
        let path = self.ctx.inherited.path.as_deref();
        let pathext = self.ctx.inherited.pathext.as_deref();
        sources
            .into_iter()
            .filter_map(|s| {
                let bin = s.version_cmd.split_whitespace().next()?;
                let path = process::which(path, pathext, bin)?;
                Some(CliPath { cli: s.tool, path })
            })
            .collect()
    }
}

#[cfg(test)]
pub(crate) mod test_fleet {
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};

    use anyhow::Result;
    use horch_core::exeid::exe_identity;
    use horch_core::fleetd::fleet_runtime::adapters::clock_store::awake_now;
    use horch_core::fleetd::fleet_runtime::adapters::effects::{exe_of, write_heartbeat};
    use horch_core::fleetd::fleet_runtime::domain::clock::AwakeReading;
    use horch_core::fleetd::fleet_runtime::domain::heartbeat::Heartbeat;
    use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
    use horch_core::taskledger::Store;

    use super::{check_with, Checked, Fleet, Fleetd, Mode};

    pub(crate) struct World {
        pub(crate) dir: tempfile::TempDir,
        pub(crate) ledger: PathBuf,
        pub(crate) root: PathBuf,
        pub(crate) exe: PathBuf,
        pub(crate) started: RefCell<Vec<Fleetd>>,
    }

    impl World {
        pub(crate) fn new() -> World {
            let dir = tempfile::tempdir().unwrap();
            let ledger = dir.path().join("ledger.sqlite3");
            let root = dir.path().join("fleet");
            std::fs::create_dir_all(root.join("run")).unwrap();
            let exe = dir.path().join("horch");
            std::fs::write(&exe, "1").unwrap();
            let exe = exe.canonicalize().unwrap();
            drop(Store::open(&ledger).unwrap());
            World {
                dir,
                ledger,
                root,
                exe,
                started: RefCell::new(Vec::new()),
            }
        }

        pub(crate) fn process(&self) -> Fleetd {
            let out = std::process::Command::new("/bin/sh")
                .args(["-c", "sleep 120 >/dev/null 2>&1 & echo $!"])
                .output()
                .unwrap();
            let pid: u32 = String::from_utf8_lossy(&out.stdout).trim().parse().unwrap();
            let pid_start = horch_core::procid::start_time(pid).unwrap();
            let p = Fleetd { pid, pid_start };
            self.started.borrow_mut().push(p);
            p
        }

        pub(crate) fn record(&self, p: Fleetd) {
            let store = Store::open(&self.ledger).unwrap();
            store
                .conn()
                .execute(
                    "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
                     VALUES (1, 1, ?1, ?2, 'now', 0, 'now')
                     ON CONFLICT (id) DO UPDATE SET generation = generation + 1,
                       pid = excluded.pid, pid_start = excluded.pid_start",
                    rusqlite::params![p.pid, i64::try_from(p.pid_start).unwrap()],
                )
                .unwrap();
        }

        pub(crate) fn heartbeat(&self, p: Fleetd, age_s: u64) {
            let now = awake_now().unwrap();
            let hb = Heartbeat {
                pid: p.pid,
                pid_start: p.pid_start,
                generation: Generation(1),
                awake: AwakeReading {
                    boot_id: now.boot_id,
                    awake_ms: now.awake_ms - age_s * 1000,
                },
                at: "now".into(),
                exe: exe_of(exe_identity(&self.exe).unwrap()),
                version: "0".into(),
            };
            write_heartbeat(&self.root, &hb).unwrap();
        }

        pub(crate) fn check(&self) -> (Checked, Vec<PathBuf>) {
            self.run(Mode::StandIn, |fleet| check_with(fleet).unwrap())
        }

        pub(crate) fn run<T>(
            &self,
            mode: Mode,
            f: impl FnOnce(&Fleet<'_>) -> T,
        ) -> (T, Vec<PathBuf>) {
            let starts = RefCell::new(Vec::new());
            let start = |exe: &Path| -> Result<()> {
                starts.borrow_mut().push(exe.to_path_buf());
                self.record(self.process());
                Ok(())
            };
            let fleet = Fleet {
                ledger: &self.ledger,
                root: &self.root,
                mode,
                start: &start,
            };
            let out = f(&fleet);
            (out, starts.into_inner())
        }

        pub(crate) fn row(&self) -> Option<Fleetd> {
            super::read_row(&self.ledger).unwrap()
        }

        pub(crate) fn new_binary(&self, folder: &str) -> PathBuf {
            let dir = self.dir.path().join(folder);
            std::fs::create_dir_all(&dir).unwrap();
            let exe = dir.join(format!("horch{}", std::env::consts::EXE_SUFFIX));
            std::fs::write(&exe, "new").unwrap();
            exe.canonicalize().unwrap()
        }
    }

    impl Drop for World {
        fn drop(&mut self) {
            for p in self.started.borrow().iter() {
                horch_core::procid::stop_same(p.pid, p.pid_start, horch_core::procid::Stop::Kill);
            }
        }
    }

    pub(crate) fn alive(p: Fleetd) -> bool {
        horch_core::procid::alive(p.pid, Some(p.pid_start))
    }

    pub(crate) fn ended(p: Fleetd) -> bool {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while alive(p) {
            if std::time::Instant::now() > until {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use horch_core::runtime::MapEnv;

    use super::test_fleet::{alive, ended, World};
    use super::*;

    #[test]
    fn the_hc_backend_is_refused_with_its_fix_and_herdr_is_taken() {
        let tmp = tempfile::tempdir().unwrap();
        let on = |backend: &str| {
            let env = MapEnv::new(tmp.path())
                .with("HOME", &tmp.path().to_string_lossy())
                .with("HORCH_BACKEND", backend);
            RuntimeContext::from_env(&env).unwrap()
        };
        let error = refuse_hc(&on("hc")).unwrap_err().to_string();
        assert_eq!(error, HC_REFUSED);
        assert!(error.contains("--backend herdr"), "{error}");
        assert!(refuse_hc(&on("herdr")).is_ok());
        let fleet = super::super::recipes::fleet_with(&on("hc"), None, &Default::default());
        assert_eq!(fleet.unwrap_err().to_string(), HC_REFUSED);
    }

    #[test]
    fn check_with_no_row_starts_nothing() {
        let w = World::new();
        let (checked, starts) = w.check();
        assert_eq!(checked.found, Found::NoRow);
        assert!(starts.is_empty(), "{starts:?}");
        std::fs::remove_file(&w.ledger).unwrap();
        let (checked, starts) = w.check();
        assert_eq!(checked.found, Found::NoRow);
        assert!(starts.is_empty(), "{starts:?}");
        assert!(!w.ledger.exists(), "the check created the ledger");
    }

    #[cfg(unix)]
    #[test]
    fn check_restarts_a_hung_fleetd_by_pid_and_start_time() {
        let w = World::new();
        let hung = w.process();
        let other = Fleetd {
            pid_start: hung.pid_start + 1,
            ..hung
        };
        w.record(other);
        let (checked, starts) = w.check();
        assert_eq!(checked.found, Found::Dead);
        assert_eq!(starts.len(), 1, "{starts:?}");
        assert!(
            alive(hung),
            "the check stopped a PID with another start time"
        );
        w.record(hung);
        w.heartbeat(hung, 31);
        let (checked, starts) = w.check();
        assert_eq!(checked.found, Found::Hung);
        assert_eq!(checked.old, Some(hung));
        assert!(ended(hung), "the hung fleetd still runs");
        assert_eq!(starts, std::slice::from_ref(&w.exe));
        let new = checked.new.expect("the row names the new fleetd");
        assert_ne!(new, hung);
        assert!(alive(new));
    }

    #[cfg(unix)]
    #[test]
    fn check_restarts_an_older_fleetd() {
        let w = World::new();
        let old = w.process();
        w.record(old);
        w.heartbeat(old, 1);
        std::fs::write(&w.exe, "22").unwrap();
        let (checked, starts) = w.check();
        assert_eq!(checked.found, Found::Older);
        assert!(ended(old), "the older fleetd still runs");
        assert_eq!(
            starts,
            std::slice::from_ref(&w.exe),
            "the new fleetd runs the new binary"
        );
        assert!(checked.new.is_some_and(alive));
    }

    fn record_this_process_as_an_attempt(w: &World) {
        use horch_core::taskledger::workers::{self, NewWorker};
        let pid = std::process::id();
        let start = horch_core::procid::start_time(pid).unwrap();
        let store = Store::open(&w.ledger).unwrap();
        let conn = store.conn();
        let run = runs::create(conn, w.dir.path(), &runs::solo_new_run("w1")).unwrap();
        let worker = workers::insert(
            conn,
            &NewWorker {
                run_id: run.run_id,
                persona: "dev".into(),
                ..Default::default()
            },
        )
        .unwrap();
        conn.execute(
            "INSERT INTO attempts (id, worker_id, pid, pid_start, state, started_at)
             VALUES ('A-1', ?1, ?2, ?3, 'live', '2026-11-18T12:00:00Z')",
            rusqlite::params![worker, pid, i64::try_from(start).unwrap()],
        )
        .unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_stand_in_start_from_a_fleet_process_gives_fleetd_no_fault_and_no_caller_env() {
        let w = World::new();
        record_this_process_as_an_attempt(&w);
        let hung = w.process();
        w.record(hung);
        w.heartbeat(hung, 31);
        let dead = Fleetd {
            pid_start: hung.pid_start + 1,
            ..hung
        };
        for (old, found) in [(hung, "hung"), (dead, "dead")] {
            w.record(old);
            let (checked, starts) = w.run(Mode::StandIn, check_with);
            let err = format!("{:#}", checked.expect_err(found));
            assert!(
                err.contains("fleet process of attempt A-1"),
                "{found}: {err}"
            );
            assert!(
                starts.is_empty(),
                "{found}: a fleet process started {starts:?}"
            );
            assert_eq!(w.row(), Some(old), "{found}: the fleetd row changed");
            assert!(alive(hung), "{found}: a fleet process stopped the fleetd");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_fleet_process_runs_no_fleetd_daemon() {
        let w = World::new();
        record_this_process_as_an_attempt(&w);
        let env = MapEnv::new(w.root.parent().unwrap())
            .with("HOME", &w.root.to_string_lossy())
            .with("HORCH_LEDGER_DB", &w.ledger.to_string_lossy())
            .with("FLEET_ROOT", &w.root.to_string_lossy())
            .with("HORCH_FAULT", "hang-pass");
        let ctx = RuntimeContext::from_env(&env).unwrap();
        let err = format!(
            "{:#}",
            daemon(&ctx).expect_err("a fleet process ran fleetd")
        );
        assert!(err.contains("fleet process of attempt A-1"), "{err}");
        assert_eq!(w.row(), None, "the refused fleetd wrote the fleetd row");
    }

    #[cfg(unix)]
    #[test]
    fn an_old_git_refuses_the_fleetd_start() {
        let w = World::new();
        let bin = w.dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let out = crate::test_support::write_warmed(
            &bin.join("git"),
            "#!/bin/sh\n[ \"$1\" = warm ] && exit 0\necho 'git version 2.39.5'\n",
            &["warm"],
        );
        assert!(out.status.success());
        let env = MapEnv::new(w.root.parent().unwrap())
            .with("HOME", &w.root.to_string_lossy())
            .with("HORCH_LEDGER_DB", &w.ledger.to_string_lossy())
            .with("FLEET_ROOT", &w.root.to_string_lossy())
            .with("PATH", &bin.to_string_lossy());
        let ctx = RuntimeContext::from_env(&env).unwrap();
        let err = format!(
            "{:#}",
            daemon(&ctx).expect_err("fleetd started on git 2.39.5")
        );
        assert!(err.contains("horch fleetd: refused"), "{err}");
        assert!(err.contains("2.39.5"), "{err}");
        assert!(err.contains("needs git 2.41 or newer"), "{err}");
        assert_eq!(w.row(), None, "the refused fleetd wrote the fleetd row");
        if w.ledger.is_file() {
            let store = Store::open_read_only(&w.ledger).unwrap();
            let jobs: i64 = store
                .conn()
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'jobs'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            if jobs == 1 {
                let n: i64 = store
                    .conn()
                    .query_row("SELECT COUNT(*) FROM jobs", [], |r| r.get(0))
                    .unwrap();
                assert_eq!(n, 0, "the refused fleetd queued a job");
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn check_leaves_a_fresh_fleetd() {
        let w = World::new();
        let fresh = w.process();
        w.record(fresh);
        w.heartbeat(fresh, 1);
        let (checked, starts) = w.check();
        assert_eq!(checked.found, Found::Fresh);
        assert!(starts.is_empty(), "{starts:?}");
        assert!(alive(fresh));
        assert_eq!(checked.new, None);
    }

    #[test]
    fn a_started_fleetd_has_a_heartbeat_before_its_first_pass() {
        let w = World::new();
        write_start_heartbeat(&w.root, Generation(3), &this_exe()).unwrap();
        let me = Fleetd {
            pid: std::process::id(),
            pid_start: horch_core::procid::start_time(std::process::id()).unwrap(),
        };
        let hb = own_heartbeat(&w.root, me).expect("the start heartbeat");
        assert_eq!(hb.generation, Generation(3));
        assert_eq!(judge_live(&w.root, me).unwrap().0, Found::Fresh);
    }

    #[test]
    fn check_with_no_row_reads_no_fleet_toml() {
        let w = World::new();
        std::fs::write(w.root.join("fleet.toml"), "pass_period_s = \"x\"").unwrap();
        let env = MapEnv::new(w.root.parent().unwrap())
            .with("HOME", &w.root.to_string_lossy())
            .with("HORCH_LEDGER_DB", &w.ledger.to_string_lossy())
            .with("FLEET_ROOT", &w.root.to_string_lossy());
        let ctx = RuntimeContext::from_env(&env).unwrap();
        assert_eq!(run_check(&ctx).unwrap().found, Found::NoRow);
    }

    #[test]
    fn service_mode_is_stand_in_for_a_ledger_outside_the_os_user_home() {
        let home = tempfile::tempdir().unwrap();
        let paths = |vars: &[(&str, &str)]| {
            let mut env = MapEnv::new(home.path()).with("HOME", &home.path().to_string_lossy());
            for (k, v) in vars {
                env = env.with(k, v);
            }
            Paths::from_env(&env)
        };
        let os_home = home.path().join("os-user");
        let default = os_home.join(".horch").join("ledger.sqlite3");
        let default = default.to_string_lossy();
        let p = paths(&[("HORCH_LEDGER_DB", &default)]);
        assert_eq!(mode(&p, Some(&os_home)), Mode::Service);
        assert_eq!(mode(&paths(&[]), Some(&os_home)), Mode::StandIn);
        let other = home.path().join("ledger.sqlite3");
        let p = paths(&[("HORCH_LEDGER_DB", &other.to_string_lossy())]);
        assert_eq!(mode(&p, Some(&os_home)), Mode::StandIn);
        let root = home.path().join("fleet");
        let p = paths(&[
            ("HORCH_LEDGER_DB", &default),
            ("FLEET_ROOT", &root.to_string_lossy()),
        ]);
        assert_eq!(mode(&p, Some(&os_home)), Mode::StandIn);
        let p = paths(&[("HORCH_LEDGER_DB", &default)]);
        assert_eq!(mode(&p, None), Mode::StandIn);
    }

    #[test]
    fn the_service_label_is_horch_fleetd_only_for_the_default_ledger() {
        let dir = tempfile::tempdir().unwrap();
        let os_home = dir.path().join("os-user");
        std::fs::create_dir_all(&os_home).unwrap();
        let default = default_path(&os_home);
        assert_eq!(service_label(&default, Some(&os_home)), service::LABEL);

        let a = dir.path().join("a").join("ledger.sqlite3");
        let b = dir.path().join("b").join("ledger.sqlite3");
        let label_a = service_label(&a, Some(&os_home));
        let label_b = service_label(&b, Some(&os_home));
        for label in [&label_a, &label_b] {
            assert!(label.starts_with("horch-fleetd-"), "{label}");
            assert_eq!(label.len(), "horch-fleetd-".len() + 8, "{label}");
            service::validate_label(label).unwrap();
        }
        assert_ne!(label_a, label_b);
        assert_eq!(service_label(&a, Some(&os_home)), label_a, "stable");
        assert_ne!(service_label(&default, None), service::LABEL);
    }

    #[cfg(unix)]
    #[test]
    fn the_service_label_uses_the_canonical_ledger_path() {
        let dir = tempfile::tempdir().unwrap();
        let os_home = dir.path().join("os-user");
        std::fs::create_dir_all(&os_home).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&os_home, &link).unwrap();
        assert_eq!(
            service_label(&default_path(&link), Some(&os_home)),
            service::LABEL
        );
        let other = dir.path().join("other").join("ledger.sqlite3");
        let via_dots = dir
            .path()
            .join("other")
            .join("..")
            .join("other")
            .join("ledger.sqlite3");
        std::fs::create_dir_all(dir.path().join("other")).unwrap();
        assert_eq!(
            service_label(&via_dots, Some(&os_home)),
            service_label(&other, Some(&os_home))
        );
    }

    #[test]
    fn the_service_spec_of_a_non_default_ledger_names_its_ledger() {
        let dir = tempfile::tempdir().unwrap();
        let os_home = dir.path().join("os-user");
        let exe = dir.path().join("horch");
        let spec = service_spec_for(&exe, &default_path(&os_home), Some(&os_home)).unwrap();
        assert_eq!(spec.label, service::LABEL);
        assert!(spec.env.is_empty(), "{:?}", spec.env);
        let other = dir.path().join("ledger.sqlite3");
        let spec = service_spec_for(&exe, &other, Some(&os_home)).unwrap();
        assert_eq!(spec.label, service_label(&other, Some(&os_home)));
        assert_eq!(
            spec.env,
            vec![(
                "HORCH_LEDGER_DB".to_string(),
                other.to_string_lossy().into_owned()
            )]
        );
    }

    #[test]
    fn rr4_a_wake_runs_the_next_pass_before_the_period() {
        let passes = std::cell::Cell::new(0);
        let mut pass = || {
            passes.set(passes.get() + 1);
            Passed::Ran
        };
        let mut woke = || passes.get() == 1;
        let changed = || passes.get() == 2;
        let began = Instant::now();
        run_loop(
            Duration::from_secs(60),
            &mut pass,
            &mut woke,
            &changed,
            &mut || {},
        );
        assert_eq!(passes.get(), 2, "the wake ran a 2nd pass");
        assert!(
            began.elapsed() < Duration::from_secs(5),
            "the 2nd pass waited {:?} of the period of 60 s",
            began.elapsed()
        );
    }

    #[test]
    fn pres_04_s10_changed_binary_restarts_between_passes() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("horch");
        std::fs::write(&exe, "1").unwrap();
        let started = exe_at(&exe);
        assert!(!binary_changed(&started), "an unchanged binary");
        let events = std::cell::RefCell::new(Vec::new());
        let passes = std::cell::Cell::new(0);
        let mut pass = || {
            passes.set(passes.get() + 1);
            events.borrow_mut().push("pass");
            if passes.get() == 2 {
                std::fs::write(&exe, "22").unwrap();
            }
            events.borrow_mut().push("heartbeat");
            Passed::Ran
        };
        let changed = || binary_changed(&started);
        let mut restart = || events.borrow_mut().push("restart");
        run_loop(
            Duration::ZERO,
            &mut pass,
            &mut || false,
            &changed,
            &mut restart,
        );
        assert_eq!(
            *events.borrow(),
            ["pass", "heartbeat", "pass", "heartbeat", "restart"]
        );
        std::fs::remove_file(&exe).unwrap();
        assert!(!binary_changed(&started), "a missing binary");
    }

    #[test]
    fn a_job_host_of_the_pass_runs_the_sibling_horch_and_not_this_binary() {
        let dir = tempfile::tempdir().unwrap();
        let deps = dir.path().join("deps");
        std::fs::create_dir_all(&deps).unwrap();
        let test_binary = deps.join("horch-0123456789abcdef");
        let sibling = deps.join(format!("horch{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&test_binary, "").unwrap();
        std::fs::write(&sibling, "").unwrap();
        let env = MapEnv::new(dir.path())
            .with(
                "HORCH_STATE_DIR",
                dir.path().join("state").to_str().unwrap(),
            )
            .with("HORCH_DATA_DIR", dir.path().join("data").to_str().unwrap())
            .with_exe(&test_binary);
        let ctx = RuntimeContext::from_env(&env).unwrap();
        let settings = settings(&ctx, Generation(1)).unwrap();
        let spec = horch_core::fleetd::fleet_runtime::application::jobs::job_spec(
            &settings.horch,
            1,
            None,
        );
        assert_eq!(spec.program, sibling);
        assert_ne!(spec.program, test_binary);
        assert_ne!(spec.program, std::env::current_exe().unwrap());
    }
}
