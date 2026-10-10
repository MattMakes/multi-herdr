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
    let Ok(out) = std::process::Command::new("/bin/ps")
        .args(["-axww", "-o", "pid=,pgid=,command="])
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
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
        .collect()
}

/// Send `signal` (`TERM`, `KILL`) to each process group.
///
/// No `--` before the groups: dash, `/bin/sh` on Debian and Ubuntu, rejects
/// it ("Illegal number"), and then nothing is signalled. After the signal
/// option every shell reads `-<pgid>` as a group.
#[cfg(unix)]
pub fn signal_groups(groups: &[u32], signal: &str) {
    if groups.is_empty() {
        return;
    }
    let targets: Vec<String> = groups.iter().map(|g| format!("-{g}")).collect();
    let _ = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(format!("kill -{signal} {} 2>/dev/null", targets.join(" ")))
        .status();
}
