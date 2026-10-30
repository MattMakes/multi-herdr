//! The agent CLI update step: `horch update`, and the step `horch fleet` and
//! `horch orchestration` run before their first pane starts (ARC-32 to
//! ARC-35).
//!
//! Each CLI has its own update command, and each command finds its own
//! install method (native, Homebrew, npm), so horch only runs it. The panes
//! themselves keep their auto-update switched off.
//!
//! Several fleets can start on 1 host, and each one runs this step (#205):
//! - 1 lock per CLI under `<state root>/update/` lets 1 updater of a CLI run
//!   at a time. A second horch that finds the lock held skips that CLI.
//! - The fleet step skips a CLI whose last update succeeded less than
//!   [`FRESH_FOR`] ago.
//! - An updater that passes its time limit is not killed: a package manager
//!   stopped between the removal of the old build and the link of the new
//!   one leaves no CLI. It is left to finish, and while it runs (at most
//!   [`LOCK_STALE`]), every later horch skips that CLI. An updater that
//!   ended unseen counts as the CLI's last check.
//!
//! So a CLI can change under a running fleet: a detached updater, or the
//! updater of a later fleet start, replaces it. It is never left half
//! installed. The files under `<state root>/update/` are written without
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

/// The longest horch waits for 1 updater: under [`LOCK_STALE`], so a later
/// horch never breaks the lock of a horch that still waits.
const MAX_TIMEOUT: Duration = Duration::from_secs(1800);

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
    let not_installed = UpdateResult {
        harness: kind,
        bin: None,
        outcome: Outcome::NotInstalled,
    };
    let (Some(args), Some(launched)) = (update_args(kind), kind.binary(&ctx.bins.harness)) else {
        return not_installed;
    };
    let Some(bin) = resolve(ctx, &update_bin(kind, &launched)) else {
        return not_installed;
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
        dir: ctx.paths.state_root.join("update"),
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

/// The files of 1 CLI's update step in `dir` (`<state root>/update/`): the
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
        let at = crate::clock::parse(&std::fs::read_to_string(self.file("checked")).ok()?)?;
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

    fn running(&self) -> Option<RunningMark> {
        let text = std::fs::read_to_string(self.file("running")).ok()?;
        let mut words = text.split_whitespace();
        let pid = words.next()?.parse().ok()?;
        let started = words.next()?.parse().ok();
        let at = crate::clock::parse(words.next()?)?;
        Some(RunningMark { pid, started, at })
    }

    /// Whether an updater a horch started still runs, for at most
    /// [`LOCK_STALE`]. A mark of an updater that ended, or ran too long,
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
        options.write(true).create_new(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, PRIVATE_FILE);
        options.open(path)
    }

    /// The last [`LOG_TAIL`] bytes of log `suffix`, as text.
    fn log_tail(&self, suffix: &str) -> String {
        use std::io::{Read, Seek, SeekFrom};
        let Ok(mut file) = std::fs::File::open(self.file(suffix)) else {
            return String::new();
        };
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        let _ = file.seek(SeekFrom::Start(len.saturating_sub(LOG_TAIL)));
        let mut bytes = Vec::new();
        let _ = file.read_to_end(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    }
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
    let doc = match releases(package) {
        Ok(doc) => doc,
        Err(e) => {
            return Some(Outcome::Failed {
                reason: cut(&format!("release date check: {e}")),
            })
        }
    };
    match cooldown::judge(&doc, now, days) {
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

/// The binary a name or path stands for, when it exists.
fn resolve(ctx: &RuntimeContext, bin: &Path) -> Option<PathBuf> {
    if bin.components().count() > 1 || bin.is_absolute() {
        return bin.is_file().then(|| bin.to_path_buf());
    }
    process::which(
        ctx.inherited.path.as_deref(),
        ctx.inherited.pathext.as_deref(),
        &bin.to_string_lossy(),
    )
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
    let out = match files.new_log("stdout.log") {
        Ok(file) => file,
        Err(e) => return Ran::Failed(cut(&format!("{name} log: {e}"))),
    };
    let err = match files.new_log("stderr.log") {
        Ok(file) => file,
        Err(e) => return Ran::Failed(cut(&format!("{name} log: {e}"))),
    };
    cmd.stdin(Stdio::null()).stdout(out).stderr(err);
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => return Ran::Failed(cut(&format!("{name} did not start: {e}"))),
    };
    let marked = files.set_running(child.id(), now);
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                return match marked {
                    Ok(()) => Ran::Detached,
                    Err(e) => Ran::Failed(cut(&format!(
                        "still running after {} s, not waited for, and unmarked: {e}",
                        timeout.as_secs()
                    ))),
                }
            }
            Err(e) => {
                kill_group(&mut child);
                let _ = child.wait();
                return Ran::Failed(cut(&format!("waiting for {name}: {e}")));
            }
        }
    };
    if status.success() {
        return Ran::Done;
    }
    Ran::Failed(
        reason_line(&files.log_tail("stderr.log"))
            .or_else(|| reason_line(&files.log_tail("stdout.log")))
            .unwrap_or_else(|| exited(&name, status)),
    )
}

/// `<name> exited <code>` for a failure that printed nothing.
fn exited(name: &str, status: ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("{name} exited {code}"),
        None => format!("{name} exited on a signal"),
    }
}

#[cfg(unix)]
fn kill_group(child: &mut Child) {
    // The group id is the child's pid (`process_group(0)`). The child is not
    // reaped yet, so its pid still names its group. A group that is already
    // gone gives ESRCH, which is fine.
    unsafe {
        libc::killpg(child.id() as libc::pid_t, libc::SIGKILL);
    }
}

#[cfg(not(unix))]
fn kill_group(child: &mut Child) {
    let _ = child.kill();
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
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('[') => {
                // Parameters and intermediates, then 1 final byte in @..~.
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(c) = chars.next() {
                    if c == '\u{7}' {
                        break;
                    }
                    if c == '\u{1b}' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
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
            use std::os::unix::fs::PermissionsExt;
            let dir = tempfile::tempdir().unwrap();
            let bin = dir.path().join("codex");
            let runs = dir.path().join("runs");
            let script = format!(
                "#!/bin/sh\ncase \"$1\" in --version) echo 'codex 1.0';; \
                 *) echo $$ >> '{}'; sleep {sleep}; exit {exit};; esac\n",
                runs.display()
            );
            std::fs::write(&bin, script).unwrap();
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
            let env = crate::runtime::context::MapEnv::new(dir.path())
                .with(
                    "HORCH_STATE_DIR",
                    &dir.path().join("state").to_string_lossy(),
                )
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

        fn file(&self, suffix: &str) -> PathBuf {
            self.dir
                .path()
                .join("state/update")
                .join(format!("codex.{suffix}"))
        }

        /// Write the `.running` mark of `pid` with start time `started`,
        /// made at `at`.
        fn mark_running(&self, pid: u32, started: Option<u64>, at: chrono::DateTime<chrono::Utc>) {
            std::fs::create_dir_all(self.dir.path().join("state/update")).unwrap();
            let started = started.map_or_else(|| "-".to_string(), |t| t.to_string());
            let text = format!("{pid} {started} {}\n", crate::clock::stamp(at));
            std::fs::write(self.file("running"), text).unwrap();
        }
    }

    /// Kills the process group it names when dropped, so a failed assert
    /// leaves no sleeping updater behind.
    #[cfg(unix)]
    struct GroupKill(u32);

    #[cfg(unix)]
    impl Drop for GroupKill {
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
    fn sleeper() -> (std::process::Child, GroupKill) {
        let mut cmd = Command::new("sleep");
        cmd.arg("60");
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
        let child = cmd.spawn().unwrap();
        let guard = GroupKill(child.id());
        (child, guard)
    }

    /// #205: 2 fleets that start together run 1 codex updater, and the
    /// second one says it skipped codex.
    #[cfg(unix)]
    #[test]
    fn arc_33_concurrent_updates_run_the_updater_once_and_the_second_skips() {
        let fake = FakeUpdater::new(2, 0, 20_000);
        let barrier = std::sync::Barrier::new(2);
        let outcomes: Vec<Outcome> = std::thread::scope(|s| {
            let a = s.spawn(|| {
                barrier.wait();
                fake.update(false)
            });
            let b = s.spawn(|| {
                barrier.wait();
                fake.update(false)
            });
            vec![a.join().unwrap(), b.join().unwrap()]
        });
        assert_eq!(fake.runs().len(), 1, "{outcomes:?}");
        let updated = outcomes
            .iter()
            .filter(|o| matches!(o, Outcome::Updated { .. }))
            .count();
        let busy = outcomes.iter().filter(|o| **o == Outcome::Busy).count();
        assert_eq!((updated, busy), (1, 1), "{outcomes:?}");
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
        let started = Instant::now();
        let outcome = fake.update(false);
        assert!(started.elapsed() < Duration::from_secs(10), "horch waited");
        let deadline = Instant::now() + Duration::from_secs(10);
        while fake.runs().is_empty() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        let runs = fake.runs();
        assert_eq!(runs.len(), 1);
        let _kill = GroupKill(runs[0]);
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
