//! The processes on this machine, for the harness teardown and for
//! fake-herdr's `pane close`. Both signal a process group only while it is
//! still the one they started: a pid that has ended can belong to another
//! program, and a kill of a reused group id can end another test run, even
//! a whole gate.

/// One row of `ps`.
#[derive(Debug, Clone)]
pub struct Process {
    pub pid: u32,
    pub pgid: u32,
    /// The start time as `ps` prints `lstart`: to the second.
    pub started: String,
    pub command: String,
}

/// Every process on the machine, from `ps`. Empty when `ps` cannot run.
#[cfg(unix)]
pub fn processes() -> Vec<Process> {
    let Ok(out) = std::process::Command::new("/bin/ps")
        .args(["-axww", "-o", "pid=,pgid=,lstart=,command="])
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
            // `lstart` is 5 words: `Sat Oct  3 21:41:47 2026`.
            let started: Vec<&str> = parts.by_ref().take(5).collect();
            if started.len() < 5 {
                return None;
            }
            Some(Process {
                pid,
                pgid,
                started: started.join(" "),
                command: parts.collect::<Vec<_>>().join(" "),
            })
        })
        .collect()
}

/// The start time of `pid`, when it runs.
#[cfg(unix)]
pub fn started(pid: u32) -> Option<String> {
    processes()
        .into_iter()
        .find(|p| p.pid == pid)
        .map(|p| p.started)
}

/// Whether process group `pgid` is still the one whose leader started at
/// `started`. Its leader runs with that start time; or the leader has
/// ended and the group still has members, which keeps the id from being
/// given out again.
#[cfg(unix)]
pub fn is_same_group(all: &[Process], pgid: u32, started: &str) -> bool {
    match all.iter().find(|p| p.pid == pgid) {
        Some(leader) => leader.pgid == pgid && leader.started == started,
        None => all.iter().any(|p| p.pgid == pgid),
    }
}

/// Send `signal` (`TERM`, `KILL`) to each process group.
#[cfg(unix)]
pub fn signal_groups(groups: &[u32], signal: &str) {
    if groups.is_empty() {
        return;
    }
    let targets: Vec<String> = groups.iter().map(|g| format!("-{g}")).collect();
    let _ = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(format!(
            "kill -{signal} -- {} 2>/dev/null",
            targets.join(" ")
        ))
        .status();
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn p(pid: u32, pgid: u32, started: &str) -> Process {
        Process {
            pid,
            pgid,
            started: started.into(),
            command: "x".into(),
        }
    }

    const T0: &str = "Sat Oct  3 20:28:53 2026";
    const T1: &str = "Sat Oct  3 21:02:10 2026";

    /// A recorded group is killed only while it is the same group.
    #[test]
    fn a_reused_group_id_is_not_the_same_group() {
        // The leader that `pane run` recorded still runs.
        assert!(is_same_group(&[p(40, 40, T0), p(41, 40, T0)], 40, T0));
        // The leader ended; its child keeps the group, so the id is ours.
        assert!(is_same_group(&[p(41, 40, T0)], 40, T0));
        // The pid now belongs to a later group leader: another program.
        assert!(!is_same_group(&[p(40, 40, T1)], 40, T0));
        // The pid now belongs to a process of another group.
        assert!(!is_same_group(&[p(40, 7, T1), p(8, 40, T1)], 40, T0));
        // Nothing of it runs.
        assert!(!is_same_group(&[p(5, 5, T0)], 40, T0));
    }
}
