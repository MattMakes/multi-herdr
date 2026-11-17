//! The concern service for Linux: a systemd user unit and the watchdog ping.

use std::ffi::OsString;
use std::io;
use std::os::linux::net::SocketAddrExt;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::{SocketAddr, UnixDatagram};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::service::{utf8, Spec, HANG_LIMIT_S};
use crate::NotSupported;

pub use crate::unix::service::{os_user_home, write_definition};

/// Whether a ping sent `READY=1`.
static READY_SENT: AtomicBool = AtomicBool::new(false);

pub fn definitions(spec: &Spec) -> io::Result<Vec<(PathBuf, String)>> {
    let exe = quote(&utf8(&spec.exe)?.replace('$', "$$"));
    // `StartLimitIntervalSec=0` turns off the start limit, so the restart is
    // always. `RestartSec=1` keeps a crash loop from a busy loop.
    let mut unit = format!(
        "[Unit]\n\
         Description=horch fleetd\n\
         StartLimitIntervalSec=0\n\
         \n\
         [Service]\n\
         Type=notify\n\
         ExecStart={exe} fleetd\n\
         Restart=always\n\
         RestartSec=1\n\
         WatchdogSec={HANG_LIMIT_S}\n"
    );
    for (name, value) in &spec.env {
        unit.push_str(&format!("Environment={}\n", quote(&format!("{name}={value}"))));
    }
    unit.push_str("\n[Install]\nWantedBy=default.target\n");
    Ok(vec![(spec.folder.join(unit_name(spec)), unit)])
}

pub fn install_commands(spec: &Spec) -> Vec<Vec<OsString>> {
    vec![
        args(&["--user", "daemon-reload"]),
        args(&["--user", "enable", "--now", &unit_name(spec)]),
    ]
}

pub fn restart_commands(spec: &Spec) -> Vec<Vec<OsString>> {
    vec![args(&["--user", "restart", &unit_name(spec)])]
}

pub fn watchdog_ping() -> Result<io::Result<()>, NotSupported> {
    Ok(ping())
}

fn ping() -> io::Result<()> {
    let Some(target) = std::env::var_os("NOTIFY_SOCKET") else {
        return Ok(());
    };
    let message: &[u8] = if READY_SENT.load(Ordering::Acquire) {
        b"WATCHDOG=1"
    } else {
        b"READY=1\nWATCHDOG=1"
    };
    let socket = UnixDatagram::unbound()?;
    // A name that starts with `@` is in the abstract namespace.
    match target.as_bytes().strip_prefix(b"@") {
        Some(name) => socket.send_to_addr(message, &SocketAddr::from_abstract_name(name)?)?,
        None => socket.send_to(message, Path::new(&target))?,
    };
    READY_SENT.store(true, Ordering::Release);
    Ok(())
}

fn unit_name(spec: &Spec) -> String {
    format!("{}.service", spec.label)
}

fn args(list: &[&str]) -> Vec<OsString> {
    list.iter().map(OsString::from).collect()
}

/// `text` as a quoted word of a unit file: `\`, `"` and line breaks are
/// escaped, and `%` is doubled, because systemd reads it as a specifier.
fn quote(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '%' => out.push_str("%%"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}
