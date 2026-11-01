//! The processes on this machine, for the harness teardown and for
//! fake-herdr's `pane close`. Both signal a process group only while it is
//! still the one they started: a pid that has ended can belong to another
//! program, and a kill of a reused group id can end another test run, even
//! a whole gate. Whether a group is still the same is
//! [`horch_core::procid::is_same_group`].

/// One row of `ps`.
#[derive(Debug, Clone)]
pub struct Process {
    pub pid: u32,
    pub pgid: u32,
    pub command: String,
}

/// Every process on the machine, from `ps`. Empty when `ps` cannot run.
#[cfg(unix)]
pub fn processes() -> Vec<Process> {
    try_processes().unwrap_or_default()
}

/// Every process on the machine, from `ps`, or why `ps` did not run: a
/// fork can fail on a loaded machine, and an empty list then hides every
/// process.
#[cfg(unix)]
pub fn try_processes() -> std::io::Result<Vec<Process>> {
    let out = std::process::Command::new("/bin/ps")
        .args(["-axww", "-o", "pid=,pgid=,command="])
        .output()?;
    if !out.status.success() {
        return Err(std::io::Error::other(format!("ps failed: {}", out.status)));
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let pid = parts.next()?.parse().ok()?;
            let pgid = parts.next()?.parse().ok()?;
            Some(Process {
                pid,
                pgid,
                command: parts.collect::<Vec<_>>().join(" "),
            })
        })
        .collect())
}

/// The command line and environment of each process of this user, by pid,
/// as 1 line each. A detached process that a harness started (the mail
/// courier) names the harness only in its environment. Empty when they
/// cannot be read.
#[cfg(target_os = "macos")]
pub fn commands_with_environment() -> std::collections::HashMap<u32, String> {
    let Ok(out) = std::process::Command::new("/bin/ps")
        .args(["-xwwE", "-o", "pid=,command="])
        .output()
    else {
        return Default::default();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let line = line.trim_start();
            let (pid, rest) = line.split_once(' ')?;
            Some((pid.parse().ok()?, rest.to_string()))
        })
        .collect()
}

/// [`commands_with_environment`] from `/proc`.
#[cfg(target_os = "linux")]
pub fn commands_with_environment() -> std::collections::HashMap<u32, String> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Default::default();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let pid: u32 = entry.file_name().to_str()?.parse().ok()?;
            let mut text = Vec::new();
            for part in ["cmdline", "environ"] {
                text.extend(std::fs::read(entry.path().join(part)).ok()?);
                text.push(0);
            }
            let text = String::from_utf8_lossy(&text).replace('\0', " ");
            Some((pid, text))
        })
        .collect()
}

/// [`commands_with_environment`] where neither `ps -E` nor `/proc` exists.
#[cfg(all(unix, not(any(target_os = "macos", target_os = "linux"))))]
pub fn commands_with_environment() -> std::collections::HashMap<u32, String> {
    Default::default()
}

/// Send `signal` (`TERM`, `KILL`) to each process group, with the `kill`
/// syscall: a `kill` program would need a fork, which can fail on a loaded
/// machine, and then nothing is signalled.
#[cfg(unix)]
pub fn signal_groups(groups: &[u32], signal: &str) {
    let signal = match signal {
        "KILL" => libc::SIGKILL,
        _ => libc::SIGTERM,
    };
    for group in groups {
        let Ok(group) = libc::pid_t::try_from(*group) else {
            continue;
        };
        if group > 1 {
            // SAFETY: plain syscall; a group that has ended answers ESRCH.
            unsafe { libc::kill(-group, signal) };
        }
    }
}
