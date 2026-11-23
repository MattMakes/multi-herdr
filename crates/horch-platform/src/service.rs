use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use crate::os::service as os;
use crate::NotSupported;

pub(crate) const HANG_LIMIT_S: u32 = 30;

pub const LABEL: &str = "horch-fleetd";

pub const LABEL_MAX: usize = 64;

pub fn ledger_label(digest: &[u8]) -> String {
    let hex: String = digest.iter().take(4).map(|b| format!("{b:02x}")).collect();
    format!("{LABEL}-{hex}")
}

pub fn validate_label(label: &str) -> io::Result<()> {
    let starts_ok = label
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric());
    let chars_ok = label
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if !starts_ok || !chars_ok || label.len() > LABEL_MAX || label.contains("..") {
        return Err(invalid(format!(
            "the service label {label:?} is not valid: use 1 to {LABEL_MAX} ASCII letters, \
             digits, '-', '_' or '.', and start with a letter or a digit"
        )));
    }
    Ok(())
}

pub const MANAGER: &str = os::MANAGER;

pub fn default_folder() -> io::Result<PathBuf> {
    Ok(os::default_folder(&os_user_home()?))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    pub label: String,
    pub exe: PathBuf,
    pub env: Vec<(String, String)>,
    pub folder: PathBuf,
}

pub fn definitions(spec: &Spec) -> io::Result<Vec<(PathBuf, String)>> {
    validate_label(&spec.label)?;
    for (name, value) in &spec.env {
        if name.is_empty() || name.contains(['=', '\0']) || value.contains('\0') {
            return Err(invalid(format!("the env entry {name:?} is not valid")));
        }
    }
    os::definitions(spec)
}

pub fn ensure(spec: &Spec, manager: &Path) -> io::Result<()> {
    let defs = definitions(spec)?;
    std::fs::create_dir_all(&spec.folder)?;
    for (path, text) in &defs {
        os::write_definition(path, text)?;
    }
    for step in os::ensure_steps(spec) {
        run(manager, &step.args, step.nothing_to_do)?;
    }
    Ok(())
}

pub(crate) struct Step {
    pub(crate) args: Vec<OsString>,
    pub(crate) nothing_to_do: &'static [i32],
}

pub fn restart(spec: &Spec, manager: &Path) -> io::Result<()> {
    for args in os::restart_commands(spec) {
        run(manager, &args, &[])?;
    }
    Ok(())
}

pub fn watchdog_ping() -> Result<io::Result<()>, NotSupported> {
    os::watchdog_ping()
}

pub fn os_user_home() -> io::Result<PathBuf> {
    os::os_user_home()
}

fn run(manager: &Path, args: &[OsString], nothing_to_do: &[i32]) -> io::Result<()> {
    let command = || {
        let mut text = manager.display().to_string();
        for arg in args {
            text.push(' ');
            text.push_str(&arg.to_string_lossy());
        }
        text
    };
    let out = crate::start::command(manager)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| io::Error::new(e.kind(), format!("cannot run {}: {e}", command())))?;
    if out.status.success()
        || out
            .status
            .code()
            .is_some_and(|c| nothing_to_do.contains(&c))
    {
        return Ok(());
    }
    Err(io::Error::other(format!(
        "{} failed with {}: {}",
        command(),
        out.status,
        String::from_utf8_lossy(&out.stderr).trim()
    )))
}

pub(crate) fn utf8(path: &Path) -> io::Result<&str> {
    path.to_str()
        .ok_or_else(|| invalid(format!("the path {} is not UTF-8", path.display())))
}

pub(crate) fn invalid(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

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
