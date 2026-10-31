//! The agent CLI update step: `horch update`, and the step `horch fleet` and
//! `horch orchestration` run before their first pane starts (ARC-32 to
//! ARC-35).
//!
//! Each CLI has its own update command, and each command finds its own
//! install method (native, Homebrew, npm), so horch only runs it. The panes
//! themselves keep their auto-update switched off.
//!
//! Several fleets can start on 1 host, and each one runs this step (#205):
//! - 1 lock per CLI under `<default state root>/update/` lets 1 updater of
//!   a CLI run at a time on the host. The folder does not follow
//!   `HORCH_STATE_DIR`: a scratch fleet updates the same installed CLIs.
//!   A second horch that finds the lock held skips that CLI.
//! - The fleet step skips a CLI whose last update succeeded less than
//!   [`FRESH_FOR`] ago.
//! - An updater that passes its time limit is not killed: a package manager
//!   stopped between the removal of the old build and the link of the new
//!   one leaves no CLI. It is left to finish, and while it runs (at most
//!   `LOCK_STALE`), every later horch skips that CLI. An updater that
//!   ended unseen counts as the CLI's last check.
//!
//! So a CLI can change under a running fleet: a detached updater, or the
//! updater of a later fleet start, replaces it. It is never left half
//! installed. The files under `<default state root>/update/` are written without
//! following a link: a Codex pane can write the state root.
//!
//! Every updater waits out the release cooldown first ([`super::cooldown`]):
//! a CLI whose newest release is younger than it, or whose release dates
//! horch cannot read, is held back and keeps the build it has.
//!
//! herdr is not in the default list: `herdr update` can stop the herdr
//! server, and that closes every pane. [`run_herdr_update`] runs it only on
//! the operator's request, attached to the operator's terminal.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

use super::cooldown::{self, Packument, Verdict};
use super::HarnessKind;
use crate::fsx::{DirLock, FsxError, PRIVATE_FILE};
use crate::routing::quota_probe::probe_version;
use crate::runtime::{process, RuntimeContext};

/// Every agent CLI the update step updates, in the order of its results.
pub const ORDER: [HarnessKind; 6] = [
    HarnessKind::Claude,
    HarnessKind::Codex,
    HarnessKind::OpenCode,
    HarnessKind::Antigravity,
    HarnessKind::Pi,
    HarnessKind::Prime,
];

/// The longest failure reason, in characters.
const REASON_MAX: usize = 200;

/// How long a successful update of 1 CLI holds for the fleet step. The
/// horch self-update cache (`CACHE_TTL_HOURS` in `horch`) is 24 h too.
pub const FRESH_FOR: chrono::TimeDelta = chrono::TimeDelta::hours(24);

/// How old an update lock must be before a later horch breaks it while its
/// holder still runs, and how long a detached updater keeps its CLI busy.
/// A lock holder that has exited is broken at once.
const LOCK_STALE: Duration = Duration::from_secs(3600);

/// The longest horch waits for 1 updater: under `LOCK_STALE`, so a later
/// horch never breaks the lock of a horch that still waits.
const MAX_TIMEOUT: Duration = Duration::from_secs(1800);

// The lock of a horch that still waits must never be old enough to break.
const _: () = assert!(MAX_TIMEOUT.as_secs() < LOCK_STALE.as_secs());

/// How much of the end of an updater log horch reads for a failure reason.
const LOG_TAIL: u64 = 64 * 1024;

/// What the update step did for 1 harness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The binary is not on `PATH`. Nothing ran.
    NotInstalled,
    /// The updater exited 0. The first `--version` line before and after it.
    Updated {
        before: Option<String>,
        after: Option<String>,
    },
    /// The newest release is younger than the cooldown. Nothing ran.
    Cooldown {
        version: String,
        age: chrono::TimeDelta,
        days: u32,
    },
    /// horch cannot read when the CLI's releases were published, so the
    /// cooldown cannot pass. Nothing ran.
    NoReleaseDates { days: u32 },
    /// The updater failed, or the release dates could not be read: why.
    Failed { reason: String },
    /// Another horch holds this CLI's update lock, or an updater an earlier
    /// horch left running still runs. Nothing ran.
    Busy,
    /// The last update succeeded at `checked_at`, less than the fleet step's
    /// limit ago. Nothing ran.
    Fresh {
        checked_at: chrono::DateTime<chrono::Utc>,
    },
    /// The updater still ran at the time limit `after`. horch did not wait
    /// for it and did not kill it.
    Detached { after: Duration },
}

/// Read the npm registry document of a package: `Err` is why it could not.
pub type ReleaseLookup<'a> = dyn Fn(&str) -> Result<Packument, String> + Sync + 'a;

/// The update step's result for 1 harness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateResult {
    pub harness: HarnessKind,
    /// The program that ran, when it was found.
    pub bin: Option<PathBuf>,
    pub outcome: Outcome,
}

impl UpdateResult {
    /// The updater failed.
    pub fn failed(&self) -> bool {
        matches!(self.outcome, Outcome::Failed { .. })
    }

    /// The result prints as a `horch: NOTE:` line on stderr: the updater
    /// failed, or horch did not wait for it.
    pub fn is_note(&self) -> bool {
        matches!(
            self.outcome,
            Outcome::Failed { .. } | Outcome::Detached { .. }
        )
    }

    /// The CLI is not known to be updated: its updater failed, still runs,
    /// or another horch updates it. `horch update` exits 1 on it.
    pub fn unfinished(&self) -> bool {
        self.is_note() || self.outcome == Outcome::Busy
    }
}

/// The arguments of `kind`'s own update command. `None` for the agentless
/// smoke harness.
pub fn update_args(kind: HarnessKind) -> Option<&'static [&'static str]> {
    match kind {
        HarnessKind::Claude => Some(&["update"]),
        HarnessKind::Codex => Some(&["update"]),
        HarnessKind::OpenCode => Some(&["upgrade"]),
        HarnessKind::Antigravity => Some(&["update"]),
        HarnessKind::Pi => Some(&["update", "--self"]),
        HarnessKind::Prime => Some(&["update"]),
        HarnessKind::None => None,
    }
}

/// Update every harness in `harnesses` at the same time, each within
/// `ctx.settings.update_timeout` and only past the release cooldown, whose
/// dates `releases` reads. With `skip_fresh`, a harness whose last update
/// succeeded less than [`FRESH_FOR`] ago is skipped. The results come in
/// [`ORDER`], 1 per harness, whatever the order of `harnesses`.
pub fn update_all(
    ctx: &RuntimeContext,
    harnesses: &[HarnessKind],
    releases: &ReleaseLookup<'_>,
    skip_fresh: bool,
) -> Vec<UpdateResult> {
    let timeout = ctx.settings.update_timeout.min(MAX_TIMEOUT);
    let wanted: Vec<HarnessKind> = ORDER
        .iter()
        .copied()
        .filter(|k| harnesses.contains(k))
        .collect();
    std::thread::scope(|s| {
        let handles: Vec<_> = wanted
            .iter()
            .map(|&kind| {
                (
                    kind,
                    s.spawn(move || update_one(ctx, kind, timeout, releases, skip_fresh)),
                )
            })
            .collect();
        handles
            .into_iter()
            .map(|(kind, handle)| {
                handle.join().unwrap_or_else(|_| UpdateResult {
                    harness: kind,
                    bin: None,
                    outcome: Outcome::Failed {
                        reason: "the updater thread panicked".into(),
                    },
                })
            })
            .collect()
    })
}

fn update_one(
    ctx: &RuntimeContext,
    kind: HarnessKind,
    timeout: Duration,
    releases: &ReleaseLookup<'_>,
    skip_fresh: bool,
) -> UpdateResult {
    let Some((args, bin)) = updater(ctx, kind) else {
        return UpdateResult {
            harness: kind,
            bin: None,
            outcome: Outcome::NotInstalled,
        };
    };
    let job = Job {
        kind,
        bin: &bin,
        args,
        timeout,
    };
    let outcome = locked_update(ctx, &job, releases, skip_fresh);
    UpdateResult {
        harness: kind,
        bin: Some(bin),
        outcome,
    }
}

/// `kind`'s update arguments and the installed program that runs them.
fn updater(ctx: &RuntimeContext, kind: HarnessKind) -> Option<(&'static [&'static str], PathBuf)> {
    let args = update_args(kind)?;
    let launched = kind.binary(&ctx.bins.harness)?;
    Some((args, process::resolve(ctx, &update_bin(kind, &launched))?))
}

/// 1 CLI's updater, ready to run.
struct Job<'a> {
    kind: HarnessKind,
    bin: &'a Path,
    args: &'a [&'a str],
    timeout: Duration,
}

/// [`update_one`] for an installed CLI, under the CLI's update lock.
fn locked_update(
    ctx: &RuntimeContext,
    job: &Job<'_>,
    releases: &ReleaseLookup<'_>,
    skip_fresh: bool,
) -> Outcome {
    let files = UpdateFiles {
        // Host-wide, as the CLIs are: a fleet with its own state root
        // updates the same installs (#205).
        dir: ctx.paths.default_state_root.join("update"),
        name: job.kind.as_str(),
    };
    let _lock = match DirLock::acquire(&files.dir, files.name, LOCK_STALE, Duration::ZERO) {
        Ok(guard) => guard,
        Err(FsxError::LockTimeout { .. }) => return Outcome::Busy,
        Err(e) => {
            return Outcome::Failed {
                reason: cut(&format!("update lock: {e}")),
            }
        }
    };
    let now = crate::clock::now();
    if files.settle_running(now) {
        return Outcome::Busy;
    }
    if let Some(checked_at) = skip_fresh.then(|| files.checked_within(now)).flatten() {
        return Outcome::Fresh { checked_at };
    }
    let days = ctx.settings.update_cooldown_days;
    if days > 0 {
        if let Some(held) = hold(job.kind, now, days, releases) {
            return held;
        }
    }
    let before = probe_version(job.bin).version();
    let mut cmd = Command::new(job.bin);
    cmd.args(job.args);
    if days > 0 {
        cmd.envs(cooldown::child_env(now, days));
    }
    match run_updater(cmd, job.timeout, &files, now) {
        Ran::Done => {
            files.clear_running();
            files.set_checked(now);
            Outcome::Updated {
                before,
                after: probe_version(job.bin).version(),
            }
        }
        Ran::Detached => Outcome::Detached { after: job.timeout },
        Ran::Failed(reason) => {
            files.clear_running();
            Outcome::Failed { reason }
        }
    }
}

/// The files of 1 CLI's update step in `dir` (`<default state root>/update/`): the
/// lock `<name>.lock/`, the last check `<name>.checked`, the updater that
/// runs `<name>.running`, and its output `<name>.stdout.log` and
/// `<name>.stderr.log`.
struct UpdateFiles {
    dir: PathBuf,
    name: &'static str,
}

/// An updater as `<name>.running` records it: `<pid> <start time or -> <time>`.
struct RunningMark {
    pid: u32,
    started: Option<u64>,
    at: chrono::DateTime<chrono::Utc>,
}

impl RunningMark {
    /// Whether its process group still runs. Without a start time, whether
    /// the pid exists.
    fn alive(&self) -> bool {
        #[cfg(unix)]
        if let Some(started) = self.started {
            return crate::procid::is_same_group(self.pid, started);
        }
        crate::procid::alive(self.pid, self.started)
    }
}

impl UpdateFiles {
    fn file(&self, suffix: &str) -> PathBuf {
        self.dir.join(format!("{}.{suffix}", self.name))
    }

    /// When the last check was, if that is less than [`FRESH_FOR`] before
    /// `now`. A time after `now` is not fresh.
    fn checked_within(
        &self,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Option<chrono::DateTime<chrono::Utc>> {
        let at = crate::clock::parse(&self.read_mark("checked")?)?;
        (at <= now && now - at < FRESH_FOR).then_some(at)
    }

    fn set_checked(&self, at: chrono::DateTime<chrono::Utc>) {
        let stamp = crate::clock::stamp(at);
        if let Err(e) =
            crate::fsx::write_atomic(&self.file("checked"), stamp.as_bytes(), PRIVATE_FILE)
        {
            eprintln!("horch: NOTE: update: {}: {e}", self.name);
        }
    }

    /// Mark file `suffix` as text: a regular file only, read without
    /// blocking and bounded, as a Codex pane can write the state root.
    fn read_mark(&self, suffix: &str) -> Option<String> {
        crate::fsx::read_regular_bounded(&self.file(suffix))
            .ok()
            .flatten()
    }

    fn running(&self) -> Option<RunningMark> {
        let text = self.read_mark("running")?;
        let mut words = text.split_whitespace();
        let pid = words.next()?.parse().ok()?;
        let started = words.next()?.parse().ok();
        let at = crate::clock::parse(words.next()?)?;
        Some(RunningMark { pid, started, at })
    }

    /// Whether an updater a horch started still runs, for at most
    /// `LOCK_STALE`. A mark of an updater that ended, or ran too long,
    /// is removed. The updater ran, so its start counts as the last check.
    fn settle_running(&self, now: chrono::DateTime<chrono::Utc>) -> bool {
        let Some(mark) = self.running() else {
            self.clear_running();
            return false;
        };
        let age = (now - mark.at).to_std().ok();
        let recent = age.is_some_and(|a| a < LOCK_STALE);
        if mark.alive() && recent {
            return true;
        }
        if mark.alive() {
            eprintln!(
                "horch: NOTE: update: {}: updater {} runs since {}; it no longer blocks updates. Its output is in {}",
                self.name,
                mark.pid,
                crate::clock::stamp(mark.at),
                self.file("stderr.log").display()
            );
        }
        self.clear_running();
        if mark.at <= now {
            self.set_checked(mark.at);
        }
        false
    }

    /// Record `pid`, the updater that runs from `now`.
    fn set_running(&self, pid: u32, now: chrono::DateTime<chrono::Utc>) -> crate::fsx::Result<()> {
        let started =
            crate::procid::start_time(pid).map_or_else(|| "-".to_string(), |t| t.to_string());
        let text = format!("{pid} {started} {}\n", crate::clock::stamp(now));
        crate::fsx::write_atomic(&self.file("running"), text.as_bytes(), PRIVATE_FILE)
    }

    fn clear_running(&self) {
        let _ = std::fs::remove_file(self.file("running"));
    }

    /// A new log file `suffix`: an old file or link at its path is removed
    /// first, and the new one is created, never opened through a link.
    fn new_log(&self, suffix: &str) -> std::io::Result<std::fs::File> {
        let path = self.file(suffix);
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
        let mut options = std::fs::OpenOptions::new();
        // Read too: horch reads the tail through a clone of this handle.
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, PRIVATE_FILE);
        options.open(path)
    }
}

/// The last [`LOG_TAIL`] bytes of an updater log, read through the handle
/// horch created it with, never by its path again.
fn log_tail(file: &mut std::fs::File) -> String {
    use std::io::{Read, Seek, SeekFrom};
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let _ = file.seek(SeekFrom::Start(len.saturating_sub(LOG_TAIL)));
    let mut bytes = Vec::new();
    let _ = file.take(LOG_TAIL).read_to_end(&mut bytes);
    String::from_utf8_lossy(&bytes).into_owned()
}

/// What keeps `kind`'s updater from running under a cooldown of `days` at
/// `now`, if anything does.
pub fn hold(
    kind: HarnessKind,
    now: chrono::DateTime<chrono::Utc>,
    days: u32,
    releases: &ReleaseLookup<'_>,
) -> Option<Outcome> {
    let Some(package) = cooldown::npm_package(kind) else {
        return Some(Outcome::NoReleaseDates { days });
    };
    match releases(package) {
        Err(e) => Some(Outcome::Failed {
            reason: cut(&format!("release date check: {e}")),
        }),
        Ok(doc) => judged(&doc, now, days),
    }
}

/// What the cooldown makes of the release dates in `doc`, if it holds the
/// updater back.
fn judged(doc: &Packument, now: chrono::DateTime<chrono::Utc>, days: u32) -> Option<Outcome> {
    match cooldown::judge(doc, now, days) {
        Verdict::Clear => None,
        Verdict::TooNew { version, age } => Some(Outcome::Cooldown { version, age, days }),
        Verdict::Unknown { reason } => Some(Outcome::Failed {
            reason: cut(&format!("release date check: {reason}")),
        }),
    }
}

/// The program that updates `kind`, given the one horch launches. `cpx` is a
/// proxy that launches `claude` and has no `update` command, so a `cpx`
/// launch updates `claude` from `PATH`.
pub fn update_bin(kind: HarnessKind, launched: &Path) -> PathBuf {
    let is_cpx = launched.file_stem().is_some_and(|s| s == "cpx");
    if kind == HarnessKind::Claude && is_cpx {
        PathBuf::from("claude")
    } else {
        launched.to_path_buf()
    }
}

/// How 1 updater run ended.
enum Ran {
    /// It exited 0.
    Done,
    /// It still ran at the time limit; its `.running` mark stays.
    Detached,
    /// It failed: why.
    Failed(String),
}

/// Run 1 updater with stdin null, no forbidden variable, in its own process
/// group, with its output in the log files of `files`. Files, not pipes: an
/// updater left running at `timeout` must not die of a closed pipe when
/// horch exits. It is not killed either (#205). Its `.running` mark is
/// written as soon as it starts, so a horch that dies while it waits
/// still leaves the mark.
fn run_updater(
    mut cmd: Command,
    timeout: Duration,
    files: &UpdateFiles,
    now: chrono::DateTime<chrono::Utc>,
) -> Ran {
    process::scrub_child_env(&mut cmd);
    let name = cmd.get_program().to_string_lossy().into_owned();
    match start_updater(cmd, files, now, &name) {
        Ok((mut child, mut tails)) => wait_or_kill(&mut child, &name, timeout, &mut tails),
        Err(reason) => Ran::Failed(reason),
    }
}

/// Start `cmd` with its output in the logs of `files` and its `.running`
/// mark written. Its child and horch's handles to the logs, or why not.
fn start_updater(
    mut cmd: Command,
    files: &UpdateFiles,
    now: chrono::DateTime<chrono::Utc>,
    name: &str,
) -> Result<(Child, LogPair), String> {
    let (logs, tails) = open_logs(files).map_err(|e| cut(&format!("{name} log: {e}")))?;
    cmd.stdin(Stdio::null()).stdout(logs.0).stderr(logs.1);
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let mut child = cmd
        .spawn()
        .map_err(|e| cut(&format!("{name} did not start: {e}")))?;
    // An unmarked updater must not outlive horch: a later horch would run a
    // second one beside it. It is stopped before it can change anything.
    if let Err(e) = files.set_running(child.id(), now) {
        stop_group(&mut child);
        return Err(cut(&format!("{name} could not be marked as running: {e}")));
    }
    Ok((child, tails))
}

/// The stdout and stderr log files of `files`, and horch's own handles to
/// them, which it reads the tails from.
fn open_logs(files: &UpdateFiles) -> std::io::Result<(LogPair, LogPair)> {
    // Both are created, whichever fails.
    let (out, err) = (files.new_log("stdout.log"), files.new_log("stderr.log"));
    let (out, err) = (out?, err?);
    let tails = (out.try_clone()?, err.try_clone()?);
    Ok(((out, err), tails))
}

/// A stdout log and a stderr log.
type LogPair = (std::fs::File, std::fs::File);

/// Wait for `child` until `timeout`. A child that still runs then is left
/// alone: `Ran::Detached`. A wait that fails kills the child's group. A
/// failed exit reads its reason from `tails`, the stdout and stderr logs.
fn wait_or_kill(child: &mut Child, name: &str, timeout: Duration, tails: &mut LogPair) -> Ran {
    let deadline = Instant::now() + timeout;
    match process::wait_until(child, deadline, Duration::from_millis(50)) {
        Ok(Some(status)) => finished(status, name, tails),
        Ok(None) => Ran::Detached,
        Err(e) => {
            stop_group(child);
            Ran::Failed(cut(&format!("waiting for {name}: {e}")))
        }
    }
}

/// How an updater that exited with `status` ran.
fn finished(status: ExitStatus, name: &str, tails: &mut LogPair) -> Ran {
    if status.success() {
        return Ran::Done;
    }
    Ran::Failed(
        reason_line(&log_tail(&mut tails.1))
            .or_else(|| reason_line(&log_tail(&mut tails.0)))
            .unwrap_or_else(|| exited(name, status)),
    )
}

/// Kill `child`'s process group and reap it.
fn stop_group(child: &mut Child) {
    crate::runtime::process::kill_group(child);
    let _ = child.wait();
}

/// `<name> exited <code>` for a failure that printed nothing.
fn exited(name: &str, status: ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("{name} exited {code}"),
        None => format!("{name} exited on a signal"),
    }
}

/// The failure reason in `text`: its last non-empty line, with ANSI escape
/// codes removed, cut to 200 characters. A carriage return starts a new
/// line, so a spinner's last frame is a line of its own.
pub fn reason_line(text: &str) -> Option<String> {
    let plain = strip_ansi(text);
    plain
        .split(['\n', '\r'])
        .map(str::trim)
        .rfind(|l| !l.is_empty())
        .map(cut)
}

fn cut(line: &str) -> String {
    match line.char_indices().nth(REASON_MAX) {
        Some((at, _)) => line[..at].to_string(),
        None => line.to_string(),
    }
}

/// `text` without ANSI escape sequences: CSI (`ESC [ ... final`), OSC
/// (`ESC ] ... BEL` or `ESC ] ... ESC \`) and 2-character escapes.
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            match chars.next() {
                Some('[') => skip_csi(&mut chars),
                Some(']') => skip_osc(&mut chars),
                _ => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Skip the rest of a CSI: parameters and intermediates, then 1 final byte
/// in `@..~`.
fn skip_csi(chars: &mut impl Iterator<Item = char>) {
    for c in chars {
        if ('@'..='~').contains(&c) {
            break;
        }
    }
}

/// Skip the rest of an OSC, up to `BEL` or `ESC \`.
fn skip_osc(chars: &mut std::iter::Peekable<impl Iterator<Item = char>>) {
    while let Some(c) = chars.next() {
        if c == '\u{7}' || (c == '\u{1b}' && chars.next_if_eq(&'\\').is_some()) {
            break;
        }
    }
}

/// Run `<herdr> update` attached to the operator's terminal, with no time
/// limit: herdr can ask the operator whether to stop its server. The exit
/// status is herdr's.
pub fn run_herdr_update(ctx: &RuntimeContext) -> Result<ExitStatus> {
    let bin = &ctx.bins.harness.herdr;
    let mut cmd = Command::new(bin);
    cmd.arg("update");
    process::scrub_child_env(&mut cmd);
    cmd.stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("running {} update", bin.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arc_33_strip_ansi_removes_csi_osc_and_short_escapes() {
        assert_eq!(strip_ansi("\u{1b}[31mred\u{1b}[0m"), "red");
        assert_eq!(strip_ansi("\u{1b}[2K\u{1b}[1Gdone"), "done");
        assert_eq!(strip_ansi("\u{1b}[?25lhidden\u{1b}[?25h"), "hidden");
        assert_eq!(strip_ansi("\u{1b}]0;title\u{7}text"), "text");
        assert_eq!(strip_ansi("\u{1b}]8;;url\u{1b}\\link"), "link");
        assert_eq!(strip_ansi("\u{1b}7saved\u{1b}8"), "saved");
        assert_eq!(strip_ansi("plain ü text"), "plain ü text");
    }

    #[test]
    fn strip_ansi_drops_an_unterminated_escape_to_the_end() {
        assert_eq!(strip_ansi("a\u{1b}"), "a");
        assert_eq!(strip_ansi("a\u{1b}[31"), "a");
        assert_eq!(strip_ansi("a\u{1b}]0;title"), "a");
        // A lone ESC inside an OSC is not a terminator; ESC \ is.
        assert_eq!(strip_ansi("\u{1b}]0;ti\u{1b}tle\u{7}b"), "b");
        assert_eq!(strip_ansi("\u{1b}]0;t\u{1b}\\b\u{1b}[1;2Hc"), "bc");
    }

    #[test]
    fn arc_33_the_reason_is_the_last_plain_line_cut_to_200_characters() {
        let spinner = "\u{1b}[?25l⠋ Upgrading\r\u{1b}[2K⠙ Upgrading\r\u{1b}[2K\
                       \u{1b}[31mError: network down\u{1b}[0m\n\n  \n";
        assert_eq!(reason_line(spinner).as_deref(), Some("Error: network down"));
        assert_eq!(reason_line("first\nlast\n").as_deref(), Some("last"));
        assert_eq!(reason_line(" \n\u{1b}[0m\n"), None);
        assert_eq!(reason_line(""), None);

        let long = "é".repeat(250);
        let reason = reason_line(&long).unwrap();
        assert_eq!(reason.chars().count(), 200);
    }

    fn packument(latest: &str, published: &str) -> Packument {
        serde_json::from_value(serde_json::json!({
            "dist-tags": {"latest": latest},
            "time": {latest: published},
        }))
        .unwrap()
    }

    #[test]
    fn arc_36_hold_keeps_back_a_young_or_undated_release() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-10-27T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let old = |_: &str| -> Result<Packument, String> {
            Ok(packument("2.0.0", "2026-10-01T00:00:00Z"))
        };
        let young = |_: &str| -> Result<Packument, String> {
            Ok(packument("2.1.0", "2026-10-25T00:00:00Z"))
        };
        let down = |_: &str| -> Result<Packument, String> { Err("network down".into()) };
        let asked = std::sync::Mutex::new(Vec::new());
        let record = |p: &str| -> Result<Packument, String> {
            asked.lock().unwrap().push(p.to_string());
            Ok(packument("2.0.0", "2026-10-01T00:00:00Z"))
        };

        assert_eq!(hold(HarnessKind::Claude, now, 7, &old), None);
        assert_eq!(
            hold(HarnessKind::Codex, now, 7, &young),
            Some(Outcome::Cooldown {
                version: "2.1.0".into(),
                age: chrono::TimeDelta::days(2),
                days: 7,
            })
        );
        assert_eq!(
            hold(HarnessKind::Pi, now, 7, &down),
            Some(Outcome::Failed {
                reason: "release date check: network down".into()
            })
        );
        // No npm package: no lookup, and no update.
        assert_eq!(
            hold(HarnessKind::Antigravity, now, 7, &record),
            Some(Outcome::NoReleaseDates { days: 7 })
        );
        assert_eq!(
            hold(HarnessKind::Prime, now, 7, &record),
            Some(Outcome::NoReleaseDates { days: 7 })
        );
        assert!(asked.lock().unwrap().is_empty());
        assert_eq!(hold(HarnessKind::OpenCode, now, 7, &record), None);
        assert_eq!(*asked.lock().unwrap(), ["opencode-ai"]);
    }

    /// A codex updater script in a temp dir: `--version` prints 1 line, and
    /// `update` appends its pid to `runs`, sleeps `sleep` seconds and exits
    /// `exit`.
    #[cfg(unix)]
    struct FakeUpdater {
        dir: tempfile::TempDir,
        ctx: RuntimeContext,
    }

    #[cfg(unix)]
    impl FakeUpdater {
        fn new(sleep: u32, exit: u8, timeout_ms: u64) -> Self {
            Self::with_wait(&format!("sleep {sleep}"), exit, timeout_ms)
        }

        /// An updater that runs until the test creates `release`: no clock
        /// decides how long it holds the update lock.
        fn gated(timeout_ms: u64) -> Self {
            Self::with_wait(
                "while [ ! -e \"$(dirname \"$0\")/release\" ]; do sleep 0.05; done",
                0,
                timeout_ms,
            )
        }

        fn with_wait(wait: &str, exit: u8, timeout_ms: u64) -> Self {
            use std::os::unix::fs::PermissionsExt;
            let dir = tempfile::tempdir().unwrap();
            let bin = dir.path().join("codex");
            let runs = dir.path().join("runs");
            let finished = dir.path().join("finished");
            let script = format!(
                "#!/bin/sh\ncase \"$1\" in --version) echo 'codex 1.0';; \
                 *) echo $$ >> '{}'; {wait}; echo $$ >> '{}'; exit {exit};; esac\n",
                runs.display(),
                finished.display()
            );
            std::fs::write(&bin, script).unwrap();
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
            // macOS scans a new executable at its first exec. Warm it here,
            // without a limit, so the scan never counts against a timeout.
            let warm = Command::new(&bin).arg("--version").output().unwrap();
            assert!(warm.status.success());
            let env = crate::runtime::context::MapEnv::new(dir.path())
                .with(
                    "HORCH_STATE_DIR",
                    &dir.path().join("state").to_string_lossy(),
                )
                .with("HOME", &dir.path().join("home").to_string_lossy())
                .with("HORCH_CODEX_BIN", &bin.to_string_lossy())
                .with("HORCH_UPDATE_COOLDOWN_DAYS", "0")
                .with("HORCH_UPDATE_TIMEOUT_MS", &timeout_ms.to_string());
            let ctx = RuntimeContext::from_env(&env).unwrap();
            FakeUpdater { dir, ctx }
        }

        /// The pid of each updater run, in order.
        fn runs(&self) -> Vec<u32> {
            std::fs::read_to_string(self.dir.path().join("runs"))
                .unwrap_or_default()
                .lines()
                .map(|l| l.trim().parse().unwrap())
                .collect()
        }

        fn update(&self, skip_fresh: bool) -> Outcome {
            let none = |_: &str| -> Result<Packument, String> { Err("unused".into()) };
            let mut results = update_all(&self.ctx, &[HarnessKind::Codex], &none, skip_fresh);
            assert_eq!(results.len(), 1);
            results.remove(0).outcome
        }

        /// The pids of live processes whose command line names this
        /// updater's script.
        fn live_updaters(&self) -> Vec<u32> {
            let out = Command::new("pgrep")
                .arg("-f")
                .arg(self.dir.path().join("codex"))
                .output()
                .unwrap();
            String::from_utf8_lossy(&out.stdout)
                .lines()
                .filter_map(|l| l.trim().parse().ok())
                .collect()
        }

        fn file(&self, suffix: &str) -> PathBuf {
            self.dir
                .path()
                .join("home/.local/state/horch/update")
                .join(format!("codex.{suffix}"))
        }

        /// Write the `.running` mark of `pid` with start time `started`,
        /// made at `at`.
        fn mark_running(&self, pid: u32, started: Option<u64>, at: chrono::DateTime<chrono::Utc>) {
            std::fs::create_dir_all(self.dir.path().join("home/.local/state/horch/update"))
                .unwrap();
            let started = started.map_or_else(|| "-".to_string(), |t| t.to_string());
            let text = format!("{pid} {started} {}\n", crate::clock::stamp(at));
            std::fs::write(self.file("running"), text).unwrap();
        }
    }

    /// Kills the process group it names when dropped, so a failed assert
    /// leaves no sleeping updater behind.
    #[cfg(unix)]
    struct StopGroupOnDrop(u32);

    #[cfg(unix)]
    impl Drop for StopGroupOnDrop {
        fn drop(&mut self) {
            // SAFETY: plain syscall on a group this test started; a group
            // that has ended answers ESRCH, which is fine here.
            unsafe {
                libc::killpg(self.0 as libc::pid_t, libc::SIGKILL);
            }
        }
    }

    /// A `sleep 60` in its own process group: a live updater stand-in.
    #[cfg(unix)]
    fn sleeper() -> (std::process::Child, StopGroupOnDrop) {
        let mut cmd = Command::new("sleep");
        cmd.arg("60");
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
        let child = cmd.spawn().unwrap();
        let guard = StopGroupOnDrop(child.id());
        (child, guard)
    }

    /// #205: 2 fleets that start together run 1 codex updater, and the
    /// second one says it skipped codex.
    #[cfg(unix)]
    #[test]
    fn arc_33_concurrent_updates_run_the_updater_once_and_the_second_skips() {
        // The updater holds the lock until the second horch has answered, so
        // a slow thread under load cannot start after the first one is done.
        let fake = FakeUpdater::gated(20_000);
        let (tx, rx) = std::sync::mpsc::channel();
        let outcomes: Vec<Outcome> = std::thread::scope(|s| {
            for _ in 0..2 {
                let tx = tx.clone();
                let fake = &fake;
                s.spawn(move || tx.send(fake.update(false)).unwrap());
            }
            let first = rx.recv().unwrap();
            std::fs::write(fake.dir.path().join("release"), "").unwrap();
            vec![first, rx.recv().unwrap()]
        });
        assert_eq!(fake.runs().len(), 1, "{outcomes:?}");
        assert_eq!(outcomes[0], Outcome::Busy, "{outcomes:?}");
        assert!(
            matches!(outcomes[1], Outcome::Updated { .. }),
            "{outcomes:?}"
        );
        assert!(
            !fake.file("running").exists(),
            "a finished run left its mark"
        );
    }

    /// #205: the fleet step does not run a CLI again within 24 h of its
    /// last successful update; `horch update` (no skip), an older success,
    /// a stamp in the future and an unreadable stamp do run it.
    #[cfg(unix)]
    #[test]
    fn arc_33_a_cli_updated_less_than_24_h_ago_is_not_run_again() {
        let fake = FakeUpdater::new(0, 0, 20_000);
        assert!(matches!(fake.update(true), Outcome::Updated { .. }));
        assert_eq!(fake.runs().len(), 1);
        assert!(fake.file("checked").is_file());

        let skipped = fake.update(true);
        assert!(matches!(skipped, Outcome::Fresh { .. }), "{skipped:?}");
        assert_eq!(fake.runs().len(), 1, "a fresh CLI ran again");

        // The operator's own `horch update` does not skip.
        assert!(matches!(fake.update(false), Outcome::Updated { .. }));
        assert_eq!(fake.runs().len(), 2);

        let now = chrono::Utc::now();
        for (stamp, why) in [
            (
                crate::clock::stamp(now - chrono::TimeDelta::hours(25)),
                "25 h ago",
            ),
            (
                crate::clock::stamp(now + chrono::TimeDelta::hours(1)),
                "in the future",
            ),
            ("garbage".to_string(), "unreadable"),
        ] {
            let runs = fake.runs().len();
            std::fs::write(fake.file("checked"), stamp).unwrap();
            assert!(
                matches!(fake.update(true), Outcome::Updated { .. }),
                "a stamp {why} skipped the CLI"
            );
            assert_eq!(fake.runs().len(), runs + 1, "{why}");
        }
    }

    /// #205: a failed updater records no check, so the next fleet runs it
    /// again.
    #[cfg(unix)]
    #[test]
    fn arc_33_a_failed_updater_writes_no_check_and_runs_again() {
        let fake = FakeUpdater::new(0, 1, 20_000);
        assert_eq!(
            fake.update(true),
            Outcome::Failed {
                reason: format!("{} exited 1", fake.dir.path().join("codex").display())
            }
        );
        assert!(matches!(fake.update(true), Outcome::Failed { .. }));
        assert_eq!(fake.runs().len(), 2);
        assert!(!fake.file("checked").exists());
        assert!(!fake.file("running").exists());
    }

    /// #205: an updater still running at the time limit is left to finish:
    /// it is alive after `update_all` returns, the result says horch did
    /// not wait, and the next update skips the CLI while it runs.
    #[cfg(unix)]
    #[test]
    fn arc_33_an_updater_past_the_timeout_is_not_killed_and_not_waited_for() {
        let fake = FakeUpdater::new(30, 0, 500);
        // The updater sleeps 30 s, so a live updater after `update` returns
        // shows that horch did not wait for it.
        let outcome = fake.update(false);
        let deadline = Instant::now() + Duration::from_secs(10);
        while fake.runs().is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        let runs = fake.runs();
        assert_eq!(runs.len(), 1);
        let _kill = StopGroupOnDrop(runs[0]);
        assert_eq!(
            outcome,
            Outcome::Detached {
                after: Duration::from_millis(500)
            }
        );
        assert!(
            crate::procid::alive(runs[0], None),
            "the updater was killed"
        );

        assert_eq!(fake.update(false), Outcome::Busy);
        assert_eq!(
            fake.runs().len(),
            1,
            "a second updater ran beside the first"
        );
    }

    /// #205: a `.running` mark keeps the CLI busy only while its updater
    /// runs, and for at most `LOCK_STALE`. A mark of an updater that ended
    /// counts as the CLI's last check.
    #[cfg(unix)]
    #[test]
    fn arc_33_a_running_mark_blocks_only_a_live_recent_updater() {
        let fake = FakeUpdater::new(0, 0, 20_000);
        let now = chrono::Utc::now();
        let (live, _kill) = sleeper();
        let started = crate::procid::start_time(live.id());
        assert!(started.is_some(), "this host reads start times");

        fake.mark_running(live.id(), started, now);
        assert_eq!(fake.update(false), Outcome::Busy);
        assert!(fake.runs().is_empty());

        // The same pid with another start time is another process.
        fake.mark_running(live.id(), started.map(|t| t + 1), now);
        assert!(matches!(fake.update(false), Outcome::Updated { .. }));
        assert_eq!(fake.runs().len(), 1);

        // A live updater older than LOCK_STALE no longer blocks.
        fake.mark_running(live.id(), started, now - chrono::TimeDelta::hours(2));
        assert!(matches!(fake.update(false), Outcome::Updated { .. }));
        assert_eq!(fake.runs().len(), 2);

        // An updater that ended unseen: the fleet step counts its start as
        // the last check and removes the mark.
        let mut ended = Command::new("true").spawn().unwrap();
        ended.wait().unwrap();
        let at = now - chrono::TimeDelta::hours(1);
        fake.mark_running(ended.id(), None, at);
        let outcome = fake.update(true);
        assert_eq!(
            outcome,
            Outcome::Fresh {
                checked_at: crate::clock::parse(&crate::clock::stamp(at)).unwrap()
            }
        );
        assert!(!fake.file("running").exists());
        assert_eq!(fake.runs().len(), 2);
    }

    /// Which outcomes print as a NOTE and which make `horch update` exit 1.
    #[test]
    fn arc_35_notes_and_unfinished_outcomes() {
        let cases = [
            (Outcome::NotInstalled, false, false),
            (
                Outcome::Updated {
                    before: None,
                    after: None,
                },
                false,
                false,
            ),
            (
                Outcome::Cooldown {
                    version: "1".into(),
                    age: chrono::TimeDelta::hours(1),
                    days: 7,
                },
                false,
                false,
            ),
            (Outcome::NoReleaseDates { days: 7 }, false, false),
            (
                Outcome::Fresh {
                    checked_at: chrono::Utc::now(),
                },
                false,
                false,
            ),
            (Outcome::Busy, false, true),
            (
                Outcome::Detached {
                    after: Duration::from_secs(1),
                },
                true,
                true,
            ),
            (Outcome::Failed { reason: "x".into() }, true, true),
        ];
        for (outcome, note, unfinished) in cases {
            let result = UpdateResult {
                harness: HarnessKind::Codex,
                bin: None,
                outcome: outcome.clone(),
            };
            assert_eq!(result.is_note(), note, "{outcome:?}");
            assert_eq!(result.unfinished(), unfinished, "{outcome:?}");
        }
    }

    /// #205: an updater whose `.running` mark cannot be written is stopped
    /// at once, so no unmarked updater outlives horch.
    #[cfg(unix)]
    #[test]
    fn arc_33_an_updater_that_cannot_be_marked_is_stopped() {
        let fake = FakeUpdater::new(30, 0, 20_000);
        // A directory where the mark file belongs: the write fails.
        std::fs::create_dir_all(fake.file("running")).unwrap();
        let outcome = fake.update(false);
        let Outcome::Failed { reason } = &outcome else {
            panic!("{outcome:?}");
        };
        assert!(
            reason.contains("could not be marked as running"),
            "{reason}"
        );
        // The updater can be stopped before it writes `runs`, so look for
        // the process itself: its command line names the unique temp dir.
        let deadline = Instant::now() + Duration::from_secs(1);
        let mut left = fake.live_updaters();
        while !left.is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
            left = fake.live_updaters();
        }
        let _stop: Vec<_> = left.iter().map(|&pid| StopGroupOnDrop(pid)).collect();
        assert!(left.is_empty(), "updater still runs: {left:?}");
        // Stopped, not waited for: the updater never reached its end.
        assert!(
            !fake.dir.path().join("finished").exists(),
            "horch waited for the updater to end"
        );
    }

    /// #205: the update files are host-wide. A fleet with its own
    /// `HORCH_STATE_DIR` sees the 24 h mark that another state root wrote.
    #[cfg(unix)]
    #[test]
    fn arc_33_a_scratch_state_root_shares_the_update_files() {
        let fake = FakeUpdater::new(0, 0, 20_000);
        assert!(matches!(fake.update(true), Outcome::Updated { .. }));
        let env = crate::runtime::context::MapEnv::new(fake.dir.path())
            .with(
                "HORCH_STATE_DIR",
                &fake.dir.path().join("scratch").to_string_lossy(),
            )
            .with("HOME", &fake.dir.path().join("home").to_string_lossy())
            .with(
                "HORCH_CODEX_BIN",
                &fake.dir.path().join("codex").to_string_lossy(),
            )
            .with("HORCH_UPDATE_COOLDOWN_DAYS", "0");
        let scratch = RuntimeContext::from_env(&env).unwrap();
        let none = |_: &str| -> Result<Packument, String> { Err("unused".into()) };
        let results = update_all(&scratch, &[HarnessKind::Codex], &none, true);
        assert!(
            matches!(results[0].outcome, Outcome::Fresh { .. }),
            "{:?}",
            results[0].outcome
        );
        assert_eq!(fake.runs().len(), 1, "the scratch fleet ran the updater");
        assert!(!fake.dir.path().join("scratch/update").exists());
    }

    /// #205: a FIFO planted as a mark file neither hangs the update step nor
    /// counts as a mark.
    #[cfg(unix)]
    #[test]
    fn arc_33_a_fifo_mark_is_ignored() {
        let fake = FakeUpdater::new(0, 0, 20_000);
        std::fs::create_dir_all(fake.dir.path().join("home/.local/state/horch/update")).unwrap();
        for suffix in ["running", "checked"] {
            let made = Command::new("mkfifo")
                .arg(fake.file(suffix))
                .status()
                .unwrap();
            assert!(made.success());
        }
        // A FIFO that blocked the step would hang this call.
        assert!(matches!(fake.update(true), Outcome::Updated { .. }));
        assert_eq!(fake.runs().len(), 1);
    }

    #[test]
    fn arc_32_update_bin_maps_cpx_to_claude() {
        let claude = HarnessKind::Claude;
        assert_eq!(
            update_bin(claude, Path::new("cpx")),
            PathBuf::from("claude")
        );
        assert_eq!(
            update_bin(claude, Path::new("/opt/proxy/bin/cpx")),
            PathBuf::from("claude")
        );
        assert_eq!(
            update_bin(claude, Path::new("/opt/bin/claude")),
            PathBuf::from("/opt/bin/claude")
        );
        // Only claude has the proxy.
        assert_eq!(
            update_bin(HarnessKind::Codex, Path::new("cpx")),
            PathBuf::from("cpx")
        );
    }

    #[test]
    fn arc_32_every_agent_cli_has_its_update_command() {
        let args: Vec<_> = ORDER.iter().map(|k| update_args(*k).unwrap()).collect();
        assert_eq!(
            args,
            [
                &["update"][..],
                &["update"],
                &["upgrade"],
                &["update"],
                &["update", "--self"],
                &["update"],
            ]
        );
        assert_eq!(update_args(HarnessKind::None), None);
    }
}
