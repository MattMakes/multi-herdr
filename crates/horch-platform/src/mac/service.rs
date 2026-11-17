//! The concern service for macOS: 2 launchd agents. The 1st keeps
//! `horch fleetd` alive. The 2nd runs `horch fleetd check` every 30 s, the
//! hang detection of macOS (PL-5).

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use crate::service::{utf8, xml_escape, Spec, Step, HANG_LIMIT_S};
use crate::NotSupported;

pub use crate::unix::service::{os_user_home, write_definition};

pub const MANAGER: &str = "launchctl";

/// The exit statuses of a `launchctl bootout` of a label that is not
/// loaded: 3 (`ESRCH`, "No such process") and 113 ("Could not find
/// specified service").
const NOT_LOADED: &[i32] = &[3, 113];

pub fn default_folder(home: &Path) -> PathBuf {
    home.join("Library").join("LaunchAgents")
}

pub fn definitions(spec: &Spec) -> io::Result<Vec<(PathBuf, String)>> {
    let exe = utf8(&spec.exe)?;
    let check = check_label(spec);
    Ok(vec![
        (
            plist_path(spec, &spec.label),
            plist(
                spec,
                &spec.label,
                &[exe, "fleetd"],
                "    <key>KeepAlive</key>\n    <true/>\n    <key>RunAtLoad</key>\n    <true/>\n",
            ),
        ),
        (
            plist_path(spec, &check),
            plist(
                spec,
                &check,
                &[exe, "fleetd", "check"],
                &format!("    <key>StartInterval</key>\n    <integer>{HANG_LIMIT_S}</integer>\n"),
            ),
        ),
    ])
}

/// Unloads the check agent, then fleetd, then loads fleetd, then the check
/// agent. A `bootout` of a label that is not loaded exits with
/// [`NOT_LOADED`].
pub fn ensure_steps(spec: &Spec) -> Vec<Step> {
    let labels = [spec.label.clone(), check_label(spec)];
    let bootout = labels.iter().rev().map(|label| Step {
        args: vec!["bootout".into(), format!("{}/{label}", domain()).into()],
        nothing_to_do: NOT_LOADED,
    });
    let bootstrap = labels.iter().map(|label| Step {
        args: vec![
            "bootstrap".into(),
            domain().into(),
            plist_path(spec, label).into_os_string(),
        ],
        nothing_to_do: &[],
    });
    bootout.chain(bootstrap).collect()
}

pub fn restart_commands(spec: &Spec) -> Vec<Vec<OsString>> {
    vec![vec![
        "kickstart".into(),
        "-k".into(),
        format!("{}/{}", domain(), spec.label).into(),
    ]]
}

pub fn watchdog_ping() -> Result<io::Result<()>, NotSupported> {
    Err(NotSupported {
        concern: "service",
        os: "macos",
    })
}

fn check_label(spec: &Spec) -> String {
    format!("{}.check", spec.label)
}

fn plist_path(spec: &Spec, label: &str) -> PathBuf {
    spec.folder.join(format!("{label}.plist"))
}

/// The launchd domain of the GUI session of this user.
fn domain() -> String {
    // SAFETY: getuid has no preconditions and cannot fail.
    format!("gui/{}", unsafe { libc::getuid() })
}

/// A launchd agent plist with `label`, `program` and the env of `spec`, and
/// the keys `extra`.
fn plist(spec: &Spec, label: &str, program: &[&str], extra: &str) -> String {
    let mut text = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
         <dict>\n",
    );
    text.push_str(&format!(
        "    <key>Label</key>\n    <string>{}</string>\n",
        xml_escape(label)
    ));
    text.push_str("    <key>ProgramArguments</key>\n    <array>\n");
    for arg in program {
        text.push_str(&format!("        <string>{}</string>\n", xml_escape(arg)));
    }
    text.push_str("    </array>\n");
    if !spec.env.is_empty() {
        text.push_str("    <key>EnvironmentVariables</key>\n    <dict>\n");
        for (name, value) in &spec.env {
            text.push_str(&format!(
                "        <key>{}</key>\n        <string>{}</string>\n",
                xml_escape(name),
                xml_escape(value)
            ));
        }
        text.push_str("    </dict>\n");
    }
    text.push_str(extra);
    text.push_str("</dict>\n</plist>\n");
    text
}
