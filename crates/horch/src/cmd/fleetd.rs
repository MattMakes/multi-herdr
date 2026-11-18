//! `horch fleetd` and `horch fleetd check`: the composition root of fleetd
//! (`docs/specs/fleetd/design.md`, "Architecture and roles", row fleetd;
//! `ddd/08-code/fleet-runtime/design.md`).
//!
//! `horch fleetd` runs the daemon: it takes the fleetd lock, takes the next
//! generation of the `fleetd` row, and runs 1 pass every `pass_period_s`.
//! A SIGTERM ends the loop between 2 passes, never inside a pass.
//!
//! `horch fleetd check` restarts a hung or older fleetd (PL-5, FM-12,
//! OR-6). The service manager runs it every 30 s on macOS and Windows, and
//! each `horch` command makes the same check before it runs
//! ([`check_before_command`]). The check:
//!
//! - no `fleetd` row: does nothing. The check never starts a fleetd that
//!   never ran; the service starts it.
//! - a row whose process is gone: starts fleetd.
//! - a heartbeat older than 30 s of awake time, or a fleetd whose binary
//!   file changed: restarts fleetd.
//! - else: does nothing.
//!
//! The mode of a restart (`docs/tickets/fleetd-p1/MAP.md`, Q8): the
//! service manager of the OS restarts fleetd only for the default ledger of
//! the OS user with no `FLEET_ROOT`. Else the mode is stand-in: the check
//! stops the old fleetd by the PID and start time of the row, waits until
//! the fleetd lock is free, and starts `horch fleetd` detached through the
//! launcher.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use clap::Subcommand;
use horch_core::exeid::exe_identity;
use horch_core::fleetd::fleet_runtime::adapters::clock_store::awake_now;
use horch_core::fleetd::fleet_runtime::adapters::config_file;
use horch_core::fleetd::fleet_runtime::adapters::effects::{
    exe_of, read_heartbeat, this_exe, write_heartbeat,
};
use horch_core::fleetd::fleet_runtime::adapters::fleetd_row::{self, lock_path, wait_until_free};
use horch_core::fleetd::fleet_runtime::adapters::old_watchdog;
use horch_core::fleetd::fleet_runtime::adapters::scans::FolderItem;
use horch_core::fleetd::fleet_runtime::adapters::watchdog_duties::{HerdrObserver, WatchdogDuties};
use horch_core::fleetd::fleet_runtime::application::holds;
use horch_core::fleetd::fleet_runtime::application::pass::{
    Outside, Pass, PassError, PoolFacts, Ports, Settings,
};
use horch_core::fleetd::fleet_runtime::domain::config::HANG_LIMIT_S;
use horch_core::fleetd::fleet_runtime::domain::health::{Finding, Hold};
use horch_core::fleetd::fleet_runtime::domain::heartbeat::{is_hung, is_older, Exe, Heartbeat};
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::task_execution::application::quota::Restart;
use horch_core::fleetd::task_execution::domain::task::{AttemptRow, InPane};
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::ports::{EnvRule, LaunchSpec, ProcessHost};
use horch_core::fleetd::worktree_custody::adapters::sqlite::{self as trees, WorktreeState};
use horch_core::fleetd::worktree_custody::application::reconcile::WorkDir;
use horch_core::messaging::delivery::{self, TypedLine};
use horch_core::procid;
use horch_core::runtime::{Paths, RuntimeContext};
use horch_core::taskledger::model::RunStatus;
use horch_core::taskledger::orchestrator;
use horch_core::taskledger::sql_words::list;
use horch_core::taskledger::store::{default_path, BUSY_TIMEOUT};
use horch_core::taskledger::watchdog::{Observation, Observer};
use horch_core::taskledger::workers::Worker;
use horch_core::taskledger::{watchdog, Store};
use horch_core::workspace::herdr::Herdr;
use horch_platform::lock::FleetdLock;
use horch_platform::service;
use rusqlite::Connection;

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

/// `horch fleetd` and `horch fleetd check`.
pub fn fleetd(ctx: &RuntimeContext, command: Option<FleetdCommand>) -> Result<ExitCode> {
    match command {
        None => daemon(ctx),
        Some(FleetdCommand::Check) => {
            let checked = run_check(ctx)?;
            output::println(&describe(&checked));
            Ok(ExitCode::SUCCESS)
        }
    }
}

// ─── the check ──────────────────────────────────────────────────────────

/// What `horch fleetd check` found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Found {
    /// No `fleetd` row: no fleetd ran on this ledger.
    NoRow,
    /// The fleetd of the row runs, and its heartbeat is fresh.
    Fresh,
    /// No process has the PID and start time of the row.
    Dead,
    /// The fleetd of the row wrote no heartbeat for 30 s of awake time.
    Hung,
    /// The binary file of the fleetd of the row changed after its start.
    Older,
}

/// 1 fleetd process: its PID and start time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fleetd {
    pub pid: u32,
    pub pid_start: u64,
}

/// What `horch fleetd check` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    pub found: Found,
    /// The fleetd that the row named.
    pub old: Option<Fleetd>,
    /// The fleetd that the row names after a start. `None` when the check
    /// started nothing, or when no new fleetd wrote the row in time.
    pub new: Option<Fleetd>,
    pub mode: Mode,
}

impl Checked {
    /// Whether the check started or restarted fleetd.
    fn started(&self) -> bool {
        matches!(self.found, Found::Dead | Found::Hung | Found::Older)
    }
}

/// How a restart starts the new fleetd.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The service manager of the OS restarts the service.
    Service,
    /// The check stops the old fleetd and starts `horch fleetd` detached.
    StandIn,
}

/// The mode of the ledger of `paths`: [`Mode::Service`] only for the
/// default ledger of the OS user, whose home is `os_home`, with no other
/// fleet root (MAP.md, Q8).
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

/// The fleet of 1 ledger, as the check sees it.
pub(crate) struct Fleet<'a> {
    pub ledger: &'a Path,
    pub root: &'a Path,
    pub mode: Mode,
    /// Start the new fleetd from the binary `exe`: through the service
    /// manager, or detached through the launcher.
    pub start: &'a dyn Fn(&Path) -> Result<()>,
}

/// The time that a killed fleetd takes to free the lock, and that a new
/// fleetd takes to write the `fleetd` row.
const START_WAIT: Duration = Duration::from_secs(10);

/// The `fleetd` row of the ledger: `None` when the ledger file, the table
/// or the row is missing. The read never creates or migrates the ledger.
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

/// The heartbeat that the fleetd `f` wrote. A heartbeat of another
/// process, or one that does not read, is none.
fn own_heartbeat(root: &Path, f: Fleetd) -> Option<Heartbeat> {
    read_heartbeat(root)
        .ok()
        .flatten()
        .filter(|hb| hb.pid == f.pid && hb.pid_start == f.pid_start)
}

/// What the check finds for the live fleetd `f`.
fn judge_live(root: &Path, f: Fleetd) -> Result<(Found, Option<Heartbeat>)> {
    // fleetd writes its start heartbeat right after it writes the row, so
    // a live fleetd with no heartbeat of its own is in its start.
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

/// The binary file at the path of the binary of `hb`, as it is now.
fn binary_now(hb: &Heartbeat) -> Exe {
    exe_at(Path::new(&hb.exe.path))
}

/// The binary file at `path` as it is now. A file that cannot be read has
/// an empty identity.
fn exe_at(path: &Path) -> Exe {
    exe_of(exe_identity(path).unwrap_or_default())
}

/// Whether the binary file that this fleetd started from was replaced: a
/// file at its path is there, and it is another file. A missing file is no
/// new binary: an install can be between its 2 renames.
fn binary_changed(started: &Exe) -> bool {
    exe_identity(Path::new(&started.path)).is_some_and(|now| exe_of(now) != *started)
}

/// `horch fleetd check` on `fleet`.
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
    // The new fleetd runs the binary file of the old one, when it has one:
    // an install replaced that file. Else the binary of this command.
    let exe = hb
        .map(|hb| PathBuf::from(hb.exe.path))
        .filter(|p| p.is_file())
        .map_or_else(std::env::current_exe, Ok)
        .context("finding the horch binary")?;
    checked.new = restart(fleet, old, found, &exe)?;
    Ok(checked)
}

/// Restart the live fleetd of `fleet` on `installed` when it runs another
/// binary (OR-6, preservation 04 S10). `horch install` and
/// `horch self-update` call it after the new binary is in place. `None`
/// when it restarted nothing: no live fleetd, a fleetd that wrote no
/// heartbeat yet, or a fleetd of `installed`. It never starts a fleetd that
/// does not run.
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

/// Stop the fleetd `old`, which the check found as `found` (stand-in mode
/// only: the service manager stops it in service mode), start fleetd from
/// `exe`, and wait for the new fleetd in the row.
fn restart(fleet: &Fleet<'_>, old: Fleetd, found: Found, exe: &Path) -> Result<Option<Fleetd>> {
    if fleet.mode == Mode::StandIn {
        stop(fleet, old, found)?;
    }
    (fleet.start)(exe)?;
    Ok(wait_for_new(fleet.ledger, old))
}

/// Stop the old fleetd `old` by its PID and start time, then wait until
/// the fleetd lock is free. A gone fleetd gets no signal. A hung fleetd
/// gets the kill at once: it ends a polite stop only between 2 passes. An
/// older fleetd gets a polite stop, and the kill when `stop_grace_s` of
/// `fleet.toml` ends. Only that case reads `fleet.toml`.
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

/// [`wait_until_free`], with a missing run folder as a free lock.
fn lock_free(lock: &Path, deadline: Instant) -> Result<bool> {
    if lock.parent().is_some_and(|d| !d.is_dir()) {
        return Ok(true);
    }
    wait_until_free(lock, deadline).with_context(|| format!("waiting for {}", lock.display()))
}

/// The live fleetd that the row names in place of `old`, within
/// [`START_WAIT`].
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

/// The check of the fleet of `ctx`, with the start of this machine.
pub(crate) fn run_check(ctx: &RuntimeContext) -> Result<Checked> {
    with_fleet(ctx, check_with)
}

/// Run `f` on the fleet of `ctx`, with the start of this machine.
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

/// The mode of the ledger of `ctx` on this machine.
fn this_mode(ctx: &RuntimeContext) -> Mode {
    mode(&ctx.paths, service::os_user_home().ok().as_deref())
}

/// The service of fleetd on the binary `exe`, for the default ledger of the
/// OS user.
pub(crate) fn service_spec(exe: &Path) -> Result<service::Spec> {
    Ok(service::Spec {
        label: service::LABEL.into(),
        exe: exe.to_path_buf(),
        // The default ledger with no FLEET_ROOT needs no env, and a Windows
        // task refuses one.
        env: Vec::new(),
        folder: service::default_folder().context("finding the folder of the fleetd service")?,
    })
}

/// Start fleetd from `exe` in `mode`.
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
            // The new fleetd outlives this command, so it is not waited for.
            Launcher.launch(&spec).context("starting horch fleetd")?;
            Ok(())
        }
    }
}

/// The line that says what the check did.
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

/// The check of `horch fleetd check` before a `horch` command (PL-5). It
/// prints to stderr only when it starts fleetd or fails, so the output of
/// the command stays as it is. `HORCH_NO_WATCHDOG=1` turns it off.
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

// ─── the daemon ─────────────────────────────────────────────────────────

/// Write the heartbeat of this fleetd at its start, so that the hang limit
/// counts awake time from the start until the first full pass ends.
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

/// A SIGTERM came: the loop ends after the pass that runs.
static TERM: AtomicBool = AtomicBool::new(false);

/// Sleep `d`, or less when a SIGTERM comes.
fn sleep_unless_term(d: Duration) {
    let until = Instant::now() + d;
    while !TERM.load(Ordering::SeqCst) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(100).min(until - Instant::now()));
    }
}

/// The ledger connection of the pass, after [`Store::open`] created or
/// migrated the ledger.
pub(crate) fn ledger_conn(ledger: &Path) -> Result<Connection> {
    drop(Store::open(ledger)?);
    let conn = Connection::open(ledger)
        .with_context(|| format!("opening the task ledger {}", ledger.display()))?;
    conn.busy_timeout(BUSY_TIMEOUT)?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(conn)
}

/// `horch fleetd`: take the lock and the generation, then 1 pass every
/// `pass_period_s` until a SIGTERM. When its binary file changed, it
/// starts the new binary after a pass and exits.
fn daemon(ctx: &RuntimeContext) -> Result<ExitCode> {
    // Windows has no signal handlers: there the loop runs until the
    // process ends.
    let _ = horch_platform::signal::on_termination_set(
        &TERM,
        &[horch_platform::signal::Termination::Terminate],
    );
    // The binary file that this fleetd runs, read before it writes the
    // fleetd row: an install after the row is then a new binary.
    let own_exe = this_exe();
    let horch = std::env::current_exe().context("finding the horch binary")?;
    let root = &ctx.paths.fleet_root;
    let mut store = Store::open(&ctx.paths.ledger_db)?;
    let Some(started) = fleetd_row::start(root, &mut store)? else {
        output::println(&format!(
            "Another fleetd holds {}: this fleetd exits.",
            lock_path(root).display()
        ));
        return Ok(ExitCode::SUCCESS);
    };
    write_start_heartbeat(root, started.generation, &own_exe)?;
    let settings = settings(ctx, started.generation, &horch)?;
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
    with_pass(ctx, settings, false, |pass, _| {
        run_loop(
            period,
            &mut || one_pass(pass, &mut conn),
            &|| self_restart.changed(),
            &mut || self_restart.restart(),
        )
    });
    Ok(ExitCode::SUCCESS)
}

/// The settings of the fleetd of `generation` that runs `horch`.
fn settings(ctx: &RuntimeContext, generation: Generation, horch: &Path) -> Result<Settings> {
    let root = &ctx.paths.fleet_root;
    Ok(Settings {
        fleet_root: root.clone(),
        generation,
        config: config_file::load(root)?,
        horch: horch.to_path_buf(),
        faults: ctx.settings.faults.clone(),
    })
}

/// Stop each old watchdog of the active runs and take its runs, before the
/// first pass ([`old_watchdog::take_over`]). Prints 1 line for each
/// recorded watchdog. An error, or a recorded watchdog that may still run,
/// is an error: fleetd runs no pass, so 1 control path acts on each run.
/// The next start of fleetd tries again.
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

/// Build the pass of fleetd on this machine with `settings`, and give it
/// with its run duties to `f`. `force` checks each working worker in each
/// pass, due or not. The daemon and `horch ledger watch --once` use it, so
/// both run the same pass.
fn with_pass<T>(
    ctx: &RuntimeContext,
    settings: Settings,
    force: bool,
    f: impl FnOnce(&mut Pass<'_>, &WatchdogDuties<'_>) -> T,
) -> T {
    let herdr = || Herdr::with_bin(&ctx.bins.harness.herdr);
    let outside = Live {
        ctx,
        herdr: herdr(),
        observer: HerdrObserver { herdr: herdr() },
    };
    let observer = HerdrObserver { herdr: herdr() };
    let workspaces = herdr();
    let open = |ws: &str| {
        workspaces
            .workspace_list()
            .ok()
            .map(|l| l.iter().any(|w| w.workspace_id == ws))
    };
    let lock_dir = orchestrator::lock_dir(&ctx.paths.ledger_db);
    let probe = orchestrator::Os {
        lock_dir: &lock_dir,
        herdr_open: &open,
    };
    let effects = LiveEffects { ctx };
    let duties = WatchdogDuties::new(&observer, &probe, &effects, ctx.paths.state_root.clone());
    let duties = if force { duties.forced() } else { duties };
    let ports = Ports {
        outside: &outside,
        duties: &duties,
        needs_you: &NeedsYouList,
        host: &Launcher,
    };
    let mut pass = Pass::new(settings, ports);
    f(&mut pass, &duties)
}

/// What `horch ledger watch --once` did.
pub(crate) enum Once {
    /// It ran 1 pass: the checks of the pass, with the run id of each.
    Ran(Vec<(String, watchdog::Checked)>),
    /// The fleetd of the row holds the fleetd lock: it does the checks.
    Held(Option<Fleetd>),
}

/// `horch ledger watch --once`: 1 pass of fleetd in the foreground, when no
/// fleetd holds the fleetd lock (`docs/tickets/fleetd-p1/MAP.md`, Q11). It
/// starts as the daemon starts: it takes the lock and the next generation,
/// and takes over the old watchdogs. When a fleetd holds the lock, it runs
/// no pass and stops nothing.
pub(crate) fn watch_once(ctx: &RuntimeContext, force: bool) -> Result<Once> {
    let horch = std::env::current_exe().context("finding the horch binary")?;
    let mut store = Store::open(&ctx.paths.ledger_db)?;
    let Some(started) = fleetd_row::start(&ctx.paths.fleet_root, &mut store)? else {
        return Ok(Once::Held(read_row(&ctx.paths.ledger_db)?));
    };
    let settings = settings(ctx, started.generation, &horch)?;
    take_over(ctx, &mut store, &settings)?;
    drop(store);
    let mut conn = ledger_conn(&ctx.paths.ledger_db)?;
    with_pass(ctx, settings, force, |pass, duties| {
        let report = pass.run(&mut conn).map_err(|e| anyhow::anyhow!("{e}"))?;
        for f in &report.failed {
            eprintln!("horch ledger watch: {f}");
        }
        Ok(Once::Ran(duties.take_checked()))
    })
}

/// Run 1 pass, and print its failures.
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

/// The restart of this fleetd on its changed binary file (preservation 04
/// S10). It holds the fleetd lock until the restart.
struct SelfRestart<'a> {
    ctx: &'a RuntimeContext,
    /// The binary file that this fleetd started from.
    exe: Exe,
    /// The path that started this fleetd: the restart starts it again.
    horch: PathBuf,
    lock: RefCell<Option<FleetdLock>>,
}

impl SelfRestart<'_> {
    fn changed(&self) -> bool {
        binary_changed(&self.exe)
    }

    /// Free the lock, because a new fleetd exits at once while this one
    /// holds it, then start the new binary. This fleetd runs no pass after
    /// it.
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

/// What 1 pass of the loop gave.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Passed {
    Ran,
    /// Another fleetd took the generation: this fleetd exits.
    GenerationLost,
}

/// The loop of the daemon: 1 `pass` every `period`, until a SIGTERM or a
/// lost generation. When the binary file of fleetd `changed`, the loop
/// calls `restart` once after the pass, and so after its heartbeat, and
/// ends. A restart never stops a pass (preservation 04 S10, FM-34).
fn run_loop(
    period: Duration,
    pass: &mut dyn FnMut() -> Passed,
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
        sleep_unless_term(period.saturating_sub(began.elapsed()));
    }
}

/// What a pass reads and does outside the ledger on this machine.
struct Live<'a> {
    ctx: &'a RuntimeContext,
    herdr: Herdr,
    /// herdr and the transcript of a live attempt.
    observer: HerdrObserver,
}

impl Outside for Live<'_> {
    /// Each pane of each herdr workspace, with the process that herdr
    /// started in it (`shell_pid`) and its start time: the fact that proves
    /// a pane is the pane of a row (FM-22). A pane whose process-info fails,
    /// or whose process cannot be read, is [`InPane::Unknown`], so it is
    /// never closed. A pane whose pid no process has is [`InPane::Gone`]:
    /// only the record of a polite stop of that pid can prove the pane then.
    /// A list that fails gives no snapshot, so no pane counts as gone.
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

    /// The holds of the health guards. A failed probe holds nothing, and
    /// says so.
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

    /// The folder of each worktree row that is not `removed`, by its
    /// worktree id.
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

    /// The entries of `<project>/.worktrees/` of each repository of the
    /// fleet (MAP.md of fleetd-p3, Q4). A work directory that cannot be
    /// listed gives no entry in this pass.
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

    /// The project root of each run that is not `done` or `abandoned`,
    /// each once, in order (MAP.md of fleetd-p3, Q4).
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

    /// Phase 1 puts no task attempt in `launching`: the only attempt there
    /// is the respawn of an orchestrator, and the run duties launch it.
    fn attempt_spec(&self, attempt: &AttemptRow) -> Result<LaunchSpec> {
        bail!(
            "attempt {} has no launch: Phase 1 launches no task attempt",
            attempt.id
        )
    }

    fn observe(&self, w: &Worker) -> Option<Observation> {
        Some(self.observer.observe(w))
    }

    /// The roster and the pool readings of the state folder. The pass
    /// starts no probe: a reading that is missing or old stays as it is
    /// (`horch quota` and the spawn gate refresh it).
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

    fn restart_spec(&self, r: &Restart) -> Result<LaunchSpec> {
        super::recipes::restart_spec(self.ctx, r)
    }
}

/// A ledger, a fleet root and stand-in fleetds in a temp folder: the
/// fixture of the tests of the fleetd restart here, in `install.rs` and in
/// `selfupdate.rs`.
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

    /// A ledger and a fleet root in a temp folder.
    pub(crate) struct World {
        pub(crate) dir: tempfile::TempDir,
        pub(crate) ledger: PathBuf,
        pub(crate) root: PathBuf,
        /// The binary of the fleetds of the test: a file that a test can
        /// change.
        pub(crate) exe: PathBuf,
        /// Each process that the test started, stopped at the end.
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
            // The binary of a heartbeat is a canonical path.
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

        /// A process that stands in for a fleetd: a `sleep` that is not a
        /// child of this test, so that its end is not a zombie.
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

        /// Write `p` to the `fleetd` row.
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

        /// The heartbeat of `p`, `age_s` seconds of awake time old, with the
        /// binary of the world as it is now.
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

        /// Run the check in stand-in mode. Gives the result and the binary
        /// of each start.
        pub(crate) fn check(&self) -> (Checked, Vec<PathBuf>) {
            self.run(Mode::StandIn, |fleet| check_with(fleet).unwrap())
        }

        /// Run `f` on the fleet of the world in `mode`. Each start starts a
        /// new stand-in fleetd and records it in the row, as `horch fleetd`
        /// does. Gives the result of `f` and the binary of each start.
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

        /// The fleetd that the `fleetd` row names.
        pub(crate) fn row(&self) -> Option<Fleetd> {
            super::read_row(&self.ledger).unwrap()
        }

        /// A new binary file in a folder of its own: `<folder>/horch`.
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

    /// Wait up to 5 s for `p` to end.
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
    fn check_with_no_row_starts_nothing() {
        let w = World::new();
        let (checked, starts) = w.check();
        assert_eq!(checked.found, Found::NoRow);
        assert!(starts.is_empty(), "{starts:?}");
        // A ledger that does not exist is not created.
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
        // A row with the PID and another start time names a process that
        // ended: the check starts a fleetd and stops nothing.
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
        // The row with the PID and its start time, and a heartbeat 31 s old.
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
        // An install replaces the binary of the running fleetd.
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

    /// fleetd writes a heartbeat at its start, so the hang limit counts
    /// awake time from the start until the first full pass.
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

    /// The check of a ledger with no fleetd row reads no `fleet.toml`: a
    /// file that does not parse does not fail it.
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
        // The default ledger of the OS user, with no FLEET_ROOT.
        let p = paths(&[("HORCH_LEDGER_DB", &default)]);
        assert_eq!(mode(&p, Some(&os_home)), Mode::Service);
        // `$HOME` is not the home of the OS user: its default ledger is not
        // the default ledger of the OS user.
        assert_eq!(mode(&paths(&[]), Some(&os_home)), Mode::StandIn);
        // Another ledger.
        let other = home.path().join("ledger.sqlite3");
        let p = paths(&[("HORCH_LEDGER_DB", &other.to_string_lossy())]);
        assert_eq!(mode(&p, Some(&os_home)), Mode::StandIn);
        // The default ledger with another FLEET_ROOT.
        let root = home.path().join("fleet");
        let p = paths(&[
            ("HORCH_LEDGER_DB", &default),
            ("FLEET_ROOT", &root.to_string_lossy()),
        ]);
        assert_eq!(mode(&p, Some(&os_home)), Mode::StandIn);
        // No home of the OS user.
        let p = paths(&[("HORCH_LEDGER_DB", &default)]);
        assert_eq!(mode(&p, None), Mode::StandIn);
    }

    /// Preservation 04 S10: a fleetd whose binary file changed starts the
    /// new binary once, after the heartbeat of its pass, and ends its loop.
    /// It never stops a pass.
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
            // An install replaces the binary inside the 2nd pass.
            if passes.get() == 2 {
                std::fs::write(&exe, "22").unwrap();
            }
            events.borrow_mut().push("heartbeat");
            Passed::Ran
        };
        let changed = || binary_changed(&started);
        let mut restart = || events.borrow_mut().push("restart");
        run_loop(Duration::ZERO, &mut pass, &changed, &mut restart);
        assert_eq!(
            *events.borrow(),
            ["pass", "heartbeat", "pass", "heartbeat", "restart"]
        );
        // A binary file that is gone (an install between its 2 renames) is
        // not a new binary.
        std::fs::remove_file(&exe).unwrap();
        assert!(!binary_changed(&started), "a missing binary");
    }
}
