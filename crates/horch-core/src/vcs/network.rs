//! The 1 way horch runs a git command that talks to a remote (`fetch`,
//! `ls-remote`, `push`, `pull`). A stalled connection must not hang a
//! finish step: on 2026-11-08 2 finish runs hung for more than 1 h 50 min in
//! a `fetch` and an `ls-remote` over ssh.
//!
//! [`output`] gives the child ssh and http stall limits and kills the
//! child's process group by its PID when the wall-clock bound passes. It
//! never retries.
//!
//! The limits go in as `-c` config, not as environment variables, because
//! core code never reads the process environment (`arc_05`). Git ranks the
//! operator's `GIT_SSH_COMMAND`, `GIT_HTTP_LOW_SPEED_LIMIT` and
//! `GIT_HTTP_LOW_SPEED_TIME` above `-c`, so those still win. A key the
//! operator's git config sets (for example `core.sshCommand`) is not passed.

use std::fmt;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use horch_marketplace::git::{scrub_repo_env, GitOutput, GitRunner};

use crate::runtime::process;

/// The ssh command for a git child when the operator has none.
pub const SSH_COMMAND: &str =
    "ssh -o ConnectTimeout=30 -o ServerAliveInterval=15 -o ServerAliveCountMax=4";

/// The stall limits, as git config keys. The http pair ends a transfer that
/// stays below 1000 bytes/s for 60 s.
const STALL_CONFIG: [(&str, &str); 3] = [
    ("core.sshCommand", SSH_COMMAND),
    ("http.lowSpeedLimit", "1000"),
    ("http.lowSpeedTime", "60"),
];

/// The wall-clock bound of a push.
pub const PUSH_BOUND: Duration = Duration::from_secs(10 * 60);
/// The wall-clock bound of every other network command.
pub const OTHER_BOUND: Duration = Duration::from_secs(3 * 60);

/// The same environment and config as [`GitRunner::output`] sets.
const FIXED_ENV: [(&str, &str); 3] = [
    ("GIT_TERMINAL_PROMPT", "0"),
    ("LC_ALL", "C"),
    ("GIT_CONFIG_NOSYSTEM", "1"),
];
const FORBIDDEN_ENV: [&str; 1] = ["ANTHROPIC_API_KEY"];
const FIXED_CONFIG: &str = "core.hooksPath=/dev/null";

/// A git network command ran past its bound and horch killed it.
#[derive(Debug)]
pub struct Timeout {
    pub command: String,
    pub bound: Duration,
    pub pid: u32,
}

impl fmt::Display for Timeout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ran past its bound of {} and horch killed it (pid {}). \
             Check the network, then run the command again",
            self.command,
            show(self.bound),
            self.pid
        )
    }
}

impl std::error::Error for Timeout {}

fn show(d: Duration) -> String {
    let ms = d.as_millis();
    if ms.is_multiple_of(60_000) {
        format!("{} min", ms / 60_000)
    } else if ms.is_multiple_of(1000) {
        format!("{} s", ms / 1000)
    } else {
        format!("{} s", ms as f64 / 1000.0)
    }
}

/// A debug build lets a test shorten every bound (0: no override).
#[cfg(debug_assertions)]
static TEST_BOUND_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Set the bound of every network command in a debug build, for a test.
#[cfg(debug_assertions)]
pub fn set_test_bound(bound: Duration) {
    TEST_BOUND_MS.store(
        bound.as_millis() as u64,
        std::sync::atomic::Ordering::SeqCst,
    );
}

/// The bound of `git <args>`.
pub fn bound(args: &[&str]) -> Duration {
    #[cfg(debug_assertions)]
    match TEST_BOUND_MS.load(std::sync::atomic::Ordering::SeqCst) {
        0 => {}
        ms => return Duration::from_millis(ms),
    }
    if args.first() == Some(&"push") {
        PUSH_BOUND
    } else {
        OTHER_BOUND
    }
}

/// The `-c` values to pass, given the keys the operator's config sets
/// (`git config --get-regexp` prints them in lower case).
fn stall_config(configured: &str) -> Vec<String> {
    let set: Vec<String> = configured
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .map(str::to_ascii_lowercase)
        .collect();
    STALL_CONFIG
        .iter()
        .filter(|(k, _)| !set.contains(&k.to_ascii_lowercase()))
        .map(|(k, v)| format!("{k}={v}"))
        .collect()
}

/// Run `git <args>` in `dir` with the stall limits and the wall-clock bound.
/// A non-zero exit is an `Ok` output; a passed bound is a [`Timeout`] error.
pub fn output(bin: &Path, dir: &Path, args: &[&str]) -> anyhow::Result<GitOutput> {
    let configured = GitRunner::new(bin)
        .output(
            dir,
            &[
                "config",
                "--get-regexp",
                r"^(core\.sshcommand|http\.lowspeedlimit|http\.lowspeedtime)$",
            ],
        )
        .map(|o| o.stdout_text())
        .unwrap_or_default();
    let mut cmd = Command::new(bin);
    cmd.current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in FIXED_ENV {
        cmd.env(k, v);
    }
    for k in FORBIDDEN_ENV {
        cmd.env_remove(k);
    }
    scrub_repo_env(&mut cmd);
    // Its own group, so a kill also ends the ssh child that holds the pipes.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    cmd.arg("-C")
        .arg(std::path::absolute(dir)?)
        .arg("-c")
        .arg(FIXED_CONFIG);
    for c in stall_config(&configured) {
        cmd.arg("-c").arg(c);
    }
    cmd.args(args);
    let mut child = cmd
        .spawn()
        .map_err(|e| anyhow::anyhow!("cannot run {}: {e}", bin.display()))?;
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_end(&mut buf);
            }
            buf
        })
    };
    let stdout = drain(child.stdout.take().map(|p| Box::new(p) as _));
    let stderr = drain(child.stderr.take().map(|p| Box::new(p) as _));
    let limit = bound(args);
    let deadline = Instant::now() + limit;
    let status = process::wait_until(&mut child, deadline, Duration::from_millis(50))?;
    let Some(status) = status else {
        let pid = child.id();
        process::kill_group(&mut child);
        let _ = child.wait();
        return Err(Timeout {
            command: format!("git {}", args.join(" ")),
            bound: limit,
            pid,
        }
        .into());
    };
    Ok(GitOutput {
        status: status.code().unwrap_or(-1),
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_operator_config_wins() {
        assert_eq!(
            stall_config(""),
            vec![
                format!("core.sshCommand={SSH_COMMAND}"),
                "http.lowSpeedLimit=1000".to_string(),
                "http.lowSpeedTime=60".to_string(),
            ]
        );
        assert_eq!(
            stall_config("core.sshcommand ssh -i key\nhttp.lowspeedtime 5"),
            vec!["http.lowSpeedLimit=1000".to_string()]
        );
    }

    #[test]
    fn a_push_has_the_long_bound() {
        assert_eq!(bound(&["push", "origin", "x"]), PUSH_BOUND);
        assert_eq!(bound(&["fetch", "origin", "x"]), OTHER_BOUND);
        assert_eq!(bound(&["ls-remote", "origin"]), OTHER_BOUND);
    }

    #[test]
    fn bounds_read_plainly() {
        assert_eq!(show(PUSH_BOUND), "10 min");
        assert_eq!(show(OTHER_BOUND), "3 min");
        assert_eq!(show(Duration::from_millis(1500)), "1.5 s");
    }
}
