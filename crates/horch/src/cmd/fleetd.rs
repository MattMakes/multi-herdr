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
use horch_core::fleetd::fleet_runtime::adapters::scans::FolderItem;
use horch_core::fleetd::fleet_runtime::adapters::watchdog_duties::{HerdrObserver, WatchdogDuties};
use horch_core::fleetd::fleet_runtime::application::holds;
use horch_core::fleetd::fleet_runtime::application::pass::{
    Outside, Pass, PassError, Ports, Settings,
};
use horch_core::fleetd::fleet_runtime::domain::config::HANG_LIMIT_S;
use horch_core::fleetd::fleet_runtime::domain::health::{Finding, Hold};
use horch_core::fleetd::fleet_runtime::domain::heartbeat::{is_hung, is_older, Heartbeat};
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::task_execution::domain::task::{AttemptRow, Recorded};
use horch_core::fleetd::worker_profiles::adapters::launcher::Launcher;
use horch_core::fleetd::worker_profiles::application::ports::{EnvRule, LaunchSpec, ProcessHost};
use horch_core::procid;
use horch_core::runtime::{Paths, RuntimeContext};
use horch_core::taskledger::orchestrator;
use horch_core::taskledger::sql_words::{list, WORKTREES_HELD};
use horch_core::taskledger::store::{default_path, BUSY_TIMEOUT};
use horch_core::taskledger::Store;
use horch_core::workspace::herdr::Herdr;
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
fn binary_now(hb: &Heartbeat) -> horch_core::fleetd::fleet_runtime::domain::heartbeat::Exe {
    exe_of(exe_identity(Path::new(&hb.exe.path)).unwrap_or_default())
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
    if fleet.mode == Mode::StandIn {
        stop(fleet, old, found)?;
    }
    // The new fleetd runs the binary file of the old one, when it has one:
    // an install replaced that file. Else the binary of this command.
    let exe = hb
        .map(|hb| PathBuf::from(hb.exe.path))
        .filter(|p| p.is_file())
        .map_or_else(std::env::current_exe, Ok)
        .context("finding the horch binary")?;
    (fleet.start)(&exe)?;
    checked.new = wait_for_new(fleet.ledger, old);
    Ok(checked)
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
fn run_check(ctx: &RuntimeContext) -> Result<Checked> {
    let paths = &ctx.paths;
    let os_home = service::os_user_home().ok();
    let mode = mode(paths, os_home.as_deref());
    let start = |exe: &Path| start_fleetd(ctx, mode, exe);
    check_with(&Fleet {
        ledger: &paths.ledger_db,
        root: &paths.fleet_root,
        mode,
        start: &start,
    })
}

/// Start fleetd from `exe` in `mode`.
fn start_fleetd(ctx: &RuntimeContext, mode: Mode, exe: &Path) -> Result<()> {
    match mode {
        Mode::Service => {
            let spec = service::Spec {
                label: service::LABEL.into(),
                exe: exe.to_path_buf(),
                // The default ledger with no FLEET_ROOT needs no env, and a
                // Windows task refuses one.
                env: Vec::new(),
                folder: service::default_folder()?,
            };
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
fn describe(c: &Checked) -> String {
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
    if std::env::var("HORCH_NO_WATCHDOG").is_ok_and(|v| v == "1") {
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
fn write_start_heartbeat(root: &Path, generation: Generation) -> Result<()> {
    let pid = std::process::id();
    let hb = Heartbeat {
        pid,
        pid_start: procid::start_time(pid).context("reading the start time of fleetd")?,
        generation,
        awake: awake_now().context("reading the awake clock")?,
        at: horch_core::taskledger::util::now(),
        exe: this_exe(),
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
/// `pass_period_s` until a SIGTERM.
fn daemon(ctx: &RuntimeContext) -> Result<ExitCode> {
    // Windows has no signal handlers: there the loop runs until the
    // process ends.
    let _ = horch_platform::signal::on_termination_set(
        &TERM,
        &[horch_platform::signal::Termination::Terminate],
    );
    let root = &ctx.paths.fleet_root;
    let mut store = Store::open(&ctx.paths.ledger_db)?;
    let Some(started) = fleetd_row::start(root, &mut store)? else {
        output::println(&format!(
            "Another fleetd holds {}: this fleetd exits.",
            lock_path(root).display()
        ));
        return Ok(ExitCode::SUCCESS);
    };
    drop(store);
    write_start_heartbeat(root, started.generation)?;
    let config = config_file::load(root)?;
    let period = Duration::from_secs(config.pass_period_s);
    let settings = Settings {
        fleet_root: root.clone(),
        generation: started.generation,
        config,
        horch: std::env::current_exe().context("finding the horch binary")?,
        faults: ctx.settings.faults.clone(),
    };
    let herdr = || Herdr::with_bin(&ctx.bins.harness.herdr);
    let outside = Live {
        ctx,
        herdr: herdr(),
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
    let ports = Ports {
        outside: &outside,
        duties: &duties,
        needs_you: &NeedsYouList,
        host: &Launcher,
    };
    let mut pass = Pass::new(settings, ports);
    let mut conn = ledger_conn(&ctx.paths.ledger_db)?;
    output::println(&format!(
        "fleetd PID {} runs generation {} on {}.",
        std::process::id(),
        started.generation.0,
        ctx.paths.ledger_db.display()
    ));
    while !TERM.load(Ordering::SeqCst) {
        let began = Instant::now();
        match pass.run(&mut conn) {
            Ok(report) => {
                for f in &report.failed {
                    eprintln!("horch fleetd: {f}");
                }
            }
            Err(PassError::GenerationLost(e)) => {
                eprintln!("horch fleetd: {e}: this fleetd exits");
                return Ok(ExitCode::SUCCESS);
            }
            Err(e @ PassError::Failed(_)) => eprintln!("horch fleetd: {e}"),
        }
        sleep_unless_term(period.saturating_sub(began.elapsed()));
    }
    Ok(ExitCode::SUCCESS)
}

/// What a pass reads and does outside the ledger on this machine.
struct Live<'a> {
    ctx: &'a RuntimeContext,
    herdr: Herdr,
}

impl Outside for Live<'_> {
    /// Each pane of each herdr workspace. herdr gives no process of a pane.
    /// A list that fails gives no snapshot, so no pane counts as gone.
    fn panes(&self) -> Option<BTreeMap<String, Option<Recorded>>> {
        let mut panes = BTreeMap::new();
        for ws in self.herdr.workspace_list().ok()? {
            for pane in self.herdr.pane_list(&ws.workspace_id).ok()? {
                panes.insert(pane.pane_id, None);
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

    /// The folder of each worktree that a task still holds.
    fn worktree_folders(&self, conn: &Connection) -> Result<Vec<FolderItem>> {
        let sql = format!(
            "SELECT worktree_id, path FROM worktrees WHERE status IN ({}) ORDER BY worktree_id",
            list(WORKTREES_HELD)
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], |r| {
            Ok(FolderItem {
                key: r.get(0)?,
                path: PathBuf::from(r.get::<_, String>(1)?),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Phase 1 puts no task attempt in `launching`: the only attempt there
    /// is the respawn of an orchestrator, and the run duties launch it.
    fn attempt_spec(&self, attempt: &AttemptRow) -> Result<LaunchSpec> {
        bail!(
            "attempt {} has no launch: Phase 1 launches no task attempt",
            attempt.id
        )
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::PathBuf;

    use horch_core::exeid::exe_identity;
    use horch_core::fleetd::fleet_runtime::adapters::clock_store::awake_now;
    use horch_core::fleetd::fleet_runtime::adapters::effects::{exe_of, write_heartbeat};
    use horch_core::fleetd::fleet_runtime::domain::clock::AwakeReading;
    use horch_core::fleetd::fleet_runtime::domain::heartbeat::Heartbeat;
    use horch_core::runtime::MapEnv;
    use horch_core::taskledger::Store;

    use super::*;

    /// A ledger and a fleet root in a temp folder.
    struct World {
        _dir: tempfile::TempDir,
        ledger: PathBuf,
        root: PathBuf,
        /// The binary of the fleetds of the test: a file that a test can
        /// change.
        exe: PathBuf,
        /// Each process that the test started, stopped at the end.
        started: RefCell<Vec<Fleetd>>,
    }

    impl World {
        fn new() -> World {
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
                _dir: dir,
                ledger,
                root,
                exe,
                started: RefCell::new(Vec::new()),
            }
        }

        /// A process that stands in for a fleetd: a `sleep` that is not a
        /// child of this test, so that its end is not a zombie.
        fn process(&self) -> Fleetd {
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
        fn record(&self, p: Fleetd) {
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
        fn heartbeat(&self, p: Fleetd, age_s: u64) {
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

        /// Run the check. Each start starts a new stand-in fleetd and
        /// records it in the row, as `horch fleetd` does. Gives the result
        /// and the binary of each start.
        fn check(&self) -> (Checked, Vec<PathBuf>) {
            let starts = RefCell::new(Vec::new());
            let start = |exe: &Path| -> Result<()> {
                starts.borrow_mut().push(exe.to_path_buf());
                self.record(self.process());
                Ok(())
            };
            let fleet = Fleet {
                ledger: &self.ledger,
                root: &self.root,
                mode: Mode::StandIn,
                start: &start,
            };
            let checked = check_with(&fleet).unwrap();
            (checked, starts.into_inner())
        }
    }

    impl Drop for World {
        fn drop(&mut self) {
            for p in self.started.borrow().iter() {
                horch_core::procid::stop_same(p.pid, p.pid_start, horch_core::procid::Stop::Kill);
            }
        }
    }

    fn alive(p: Fleetd) -> bool {
        horch_core::procid::alive(p.pid, Some(p.pid_start))
    }

    /// Wait up to 5 s for `p` to end.
    fn ended(p: Fleetd) -> bool {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while alive(p) {
            if std::time::Instant::now() > until {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        true
    }

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
        write_start_heartbeat(&w.root, Generation(3)).unwrap();
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
}
