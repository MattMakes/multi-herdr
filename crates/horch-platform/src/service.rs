//! The concern service: service install and restart, and hang detection
//! (PL-2, PL-3, PL-5, FM-12).
//!
//! - The service definitions of fleetd: [`Spec`] and [`definitions`].
//! - Install and restart through the service manager: [`ensure`] and
//!   [`restart`].
//! - The systemd watchdog: [`watchdog_ping`].
//! - The home of the OS user: [`os_user_home`].
//! - The service of the default ledger: its label [`LABEL`], the service
//!   manager command of this OS [`MANAGER`], and the folder of its
//!   definition files [`default_folder`].
//!
//! Each OS restarts a hung fleetd within 30 s of awake time. Linux: the
//! systemd unit has `Restart=always` and `WatchdogSec=30`, and fleetd calls
//! [`watchdog_ping`] on each pass. macOS: a launchd agent with `KeepAlive`,
//! and a 2nd agent that runs `horch fleetd check` every 30 s. Windows: a
//! task that runs `horch fleetd` at logon, and a task that runs
//! `horch fleetd check` every 30 s.
//!
//! [`ensure`] and [`restart`] take the service manager command as an input
//! (`launchctl`, `systemctl` or `schtasks`), so a test passes a stand-in.
//! Core uses the service manager only for the default ledger of the OS user
//! and with no `FLEET_ROOT` (`docs/tickets/fleetd-p1/MAP.md`, Q8). The
//! platform does not decide that.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::os::service as os;
use crate::NotSupported;

/// The hang limit of fleetd and the period of `horch fleetd check`, in
/// seconds (PL-5).
pub(crate) const HANG_LIMIT_S: u32 = 30;

/// The label of the fleetd service of the default ledger of the OS user.
pub const LABEL: &str = "horch-fleetd";

/// The service manager command of this OS, for [`ensure`] and
/// [`restart`]: `launchctl` on macOS, `systemctl` on Linux and `schtasks`
/// on Windows.
pub const MANAGER: &str = os::MANAGER;

/// The folder of the definition files in the home of the OS user
/// ([`os_user_home`]): `~/Library/LaunchAgents` on macOS,
/// `~/.config/systemd/user` on Linux and `~\AppData\Local\horch\service` on
/// Windows.
pub fn default_folder() -> io::Result<PathBuf> {
    Ok(os::default_folder(&os_user_home()?))
}

/// What the service definitions of fleetd need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    /// The name of the service: the launchd label, the stem of the systemd
    /// unit, or the name of the Windows task. The check job or task of
    /// macOS and Windows is `<label>.check`.
    pub label: String,
    /// The `horch` executable.
    pub exe: PathBuf,
    /// The env of fleetd, for example `HORCH_LEDGER_DB` and `FLEET_ROOT`. A
    /// Windows task carries no env: [`definitions`] refuses a non-empty env
    /// there.
    pub env: Vec<(String, String)>,
    /// The folder of the definition files, for example
    /// `~/Library/LaunchAgents` or `~/.config/systemd/user`.
    pub folder: PathBuf,
}

/// The definition files of `spec`, each as its path in `spec.folder` and its
/// text:
///
/// - Linux: `<label>.service`, a systemd user unit with `Type=notify`,
///   `ExecStart=<exe> fleetd`, `Restart=always` and `WatchdogSec=30`.
/// - macOS: `<label>.plist`, a launchd agent that runs `<exe> fleetd` with
///   `KeepAlive` and `RunAtLoad`, and `<label>.check.plist`, which runs
///   `<exe> fleetd check` with `StartInterval` 30.
/// - Windows: `<label>.xml`, a task that runs `<exe> fleetd` at logon, and
///   `<label>.check.xml`, a task that runs `<exe> fleetd check` every 30 s.
///
/// An `InvalidInput` error: `exe` is not UTF-8, an env name is empty or has
/// `=` or NUL, an env value has NUL, or the OS is Windows and the env is not
/// empty.
pub fn definitions(spec: &Spec) -> io::Result<Vec<(PathBuf, String)>> {
    for (name, value) in &spec.env {
        if name.is_empty() || name.contains(['=', '\0']) || value.contains('\0') {
            return Err(invalid(format!("the env entry {name:?} is not valid")));
        }
    }
    os::definitions(spec)
}

/// Writes the [`definitions`] of `spec`, then makes `manager` run each
/// label with its current definition. It is safe to run again: after a
/// first run, a 2nd run, a run that failed in the middle, or a changed
/// definition. Each run restarts fleetd:
///
/// - Linux: `systemctl --user daemon-reload`, then
///   `systemctl --user enable <label>.service`, then
///   `systemctl --user restart <label>.service`.
/// - macOS: `launchctl bootout gui/<uid>/<label>.check`, then
///   `launchctl bootout gui/<uid>/<label>`, then
///   `launchctl bootstrap gui/<uid> <plist>` for `<label>` and then for
///   `<label>.check`. launchd refuses a `bootstrap` of a loaded label, so
///   each label is unloaded first. A failed `bootout` is not a failure: the
///   label was not loaded. The check agent goes out first and in last, so it
///   never acts on a fleetd that is between `bootout` and `bootstrap`.
/// - Windows: `schtasks /Create /XML <file> /TN <name> /F` for each task,
///   which replaces a task that exists, then `schtasks /Run /TN <label>`.
///
/// It stops at the first manager command that fails, and its error names
/// the command, the exit status and the error output, and each failed
/// `bootout` before it.
pub fn ensure(spec: &Spec, manager: &Path) -> io::Result<()> {
    let defs = definitions(spec)?;
    std::fs::create_dir_all(&spec.folder)?;
    for (path, text) in &defs {
        os::write_definition(path, text)?;
    }
    let mut tolerated = Vec::new();
    for step in os::ensure_steps(spec) {
        match run(manager, &step.args) {
            Ok(()) => {}
            Err(e) if step.may_fail => tolerated.push(e.to_string()),
            Err(e) if tolerated.is_empty() => return Err(e),
            Err(e) => {
                return Err(io::Error::new(
                    e.kind(),
                    format!("{e} (before it: {})", tolerated.join("; ")),
                ))
            }
        }
    }
    Ok(())
}

/// 1 manager command of [`ensure`]. A failure of a command that `may_fail`
/// does not stop [`ensure`].
pub(crate) struct Step {
    pub(crate) args: Vec<OsString>,
    pub(crate) may_fail: bool,
}

/// Restarts the service of `spec` with 1 `manager` command:
///
/// - Linux: `systemctl --user restart <label>.service`.
/// - macOS: `launchctl kickstart -k gui/<uid>/<label>`.
/// - Windows: `schtasks /Run /TN <label>`. The policy `StopExisting` of the
///   task stops the old instance first.
pub fn restart(spec: &Spec, manager: &Path) -> io::Result<()> {
    for args in os::restart_commands(spec) {
        run(manager, &args)?;
    }
    Ok(())
}

/// Tells the systemd watchdog that fleetd is alive: a datagram `WATCHDOG=1`
/// to `$NOTIFY_SOCKET`. The first ping that is sent also carries `READY=1`,
/// which the unit of `Type=notify` waits for. Without `NOTIFY_SOCKET`,
/// fleetd does not run under systemd, and the ping does nothing, as
/// `sd_notify` does.
///
/// macOS and Windows have no such watchdog and give `NotSupported`: there
/// the check job or task runs `horch fleetd check` (PL-3).
pub fn watchdog_ping() -> Result<io::Result<()>, NotSupported> {
    os::watchdog_ping()
}

/// The home folder of the OS user, from the user database of the OS: the
/// passwd database on unix, the profile folder on Windows. It never reads
/// `HOME` or `USERPROFILE`.
pub fn os_user_home() -> io::Result<PathBuf> {
    os::os_user_home()
}

/// Runs `manager` with `args` and waits. A start failure or an exit status
/// other than 0 is an error that names the command.
fn run(manager: &Path, args: &[OsString]) -> io::Result<()> {
    let command = || {
        let mut text = manager.display().to_string();
        for arg in args {
            text.push(' ');
            text.push_str(&arg.to_string_lossy());
        }
        text
    };
    let out = Command::new(manager)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| io::Error::new(e.kind(), format!("cannot run {}: {e}", command())))?;
    if out.status.success() {
        return Ok(());
    }
    Err(io::Error::other(format!(
        "{} failed with {}: {}",
        command(),
        out.status,
        String::from_utf8_lossy(&out.stderr).trim()
    )))
}

/// The text of `path`, which a definition holds. A path that is not UTF-8
/// is an `InvalidInput` error.
pub(crate) fn utf8(path: &Path) -> io::Result<&str> {
    path.to_str()
        .ok_or_else(|| invalid(format!("the path {} is not UTF-8", path.display())))
}

pub(crate) fn invalid(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

/// `text` as XML character data or as an attribute value.
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub(crate) fn xml_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}
