//! A bounded git runner: every git call that horch runs outside the
//! sandbox on worker-controlled input ends within a deadline, an output cap
//! and its process group (#T-10). T-517 (ticket 18) moves it into
//! `git_safe.rs` and shares it with every horch git call.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

/// The longest 1 git child runs before horch kills it.
pub const GIT_DEADLINE: Duration = Duration::from_secs(120);

/// The most bytes of 1 NUL-ended record of git output that horch reads: a
/// 3-byte status code and a path of `PATH_MAX` (4096) bytes.
pub const MAX_RECORD: usize = 4099;

/// The most time a killed git child gets to be reaped.
pub const REAP_GRACE: Duration = Duration::from_secs(1);

/// The most bytes of git's stderr kept for an error message.
pub const MAX_STDERR: u64 = 4096;

/// The NUL-ended stdout records of 1 git run, and how it ended.
#[derive(Debug)]
pub struct Records {
    pub status: ExitStatus,
    pub records: Vec<Vec<u8>>,
    /// False when git printed more than the cap: it was stopped there.
    pub complete: bool,
    /// The first [`MAX_STDERR`] bytes of stderr.
    pub stderr: Vec<u8>,
}

impl Records {
    /// stdout of a git call that prints at most 1 record, or its error.
    pub fn text(self, what: &str) -> Result<String> {
        if !self.complete || !self.status.success() {
            let stderr = String::from_utf8_lossy(&self.stderr);
            let why = if stderr.trim().is_empty() {
                self.status.to_string()
            } else {
                stderr.trim().to_string()
            };
            bail!("git {what} failed: {why}");
        }
        Ok(String::from_utf8_lossy(&self.records.concat())
            .trim()
            .to_string())
    }
}

/// Run `cmd` with `input` on its stdin, and read at most `max` NUL-ended
/// stdout records of at most [`MAX_RECORD`] bytes each. git is killed at
/// the first record past either bound, and at `limit`, which is an error.
/// So no worker-controlled input grows horch's memory or time past them.
pub fn run_until(mut cmd: Command, input: Vec<u8>, max: usize, limit: Duration) -> Result<Records> {
    let end = Instant::now() + limit;
    // git leads its own process group, so the kill also reaches a filter or
    // helper that holds a pipe.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let stdin = if input.is_empty() {
        Stdio::null()
    } else {
        Stdio::piped()
    };
    let mut child = cmd
        .stdin(stdin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("running git")?;
    // Names git's group after git is reaped (see `stop_group`).
    let started = crate::procid::start_time(child.id()).unwrap_or(0);
    // A writer thread, so a full stdout pipe never blocks the input. It
    // ends when the input is written or the last reader of the pipe ends.
    let writer = child
        .stdin
        .take()
        .map(|mut pipe| std::thread::spawn(move || pipe.write_all(&input)));
    let stdout = child.stdout.take().context("git has no stdout")?;
    let stderr = child.stderr.take().context("git has no stderr")?;
    let mut reader = Some(std::thread::spawn(move || {
        read_records(BufReader::new(stdout), max)
    }));
    let errors = std::thread::spawn(move || read_stderr(stderr));
    let mut read = None;
    // The last time a pipe reader may take: `end`, or 1 grace after a kill.
    let mut join_end = end;
    let status = loop {
        if let Some(status) = poll_exit(&mut child)? {
            break status;
        }
        if reader.as_ref().is_some_and(JoinHandle::is_finished) {
            read = reader.take().and_then(|h| h.join().ok());
        }
        let capped = read.as_ref().is_some_and(|r| !matches!(r, Ok((_, true))));
        let late = Instant::now() >= end;
        if capped || late {
            let status = kill_and_reap(&mut child)?;
            if !capped {
                bail!(
                    "git ran longer than {} s and was stopped",
                    limit.as_secs_f32()
                );
            }
            join_end = Instant::now() + REAP_GRACE;
            break status;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    // git is reaped. Stop what is left of its group every time: a helper
    // can hold only stdin, with stdout and stderr elsewhere, and no pipe
    // would show it. Then every pipe closes, and the joins end.
    #[cfg(unix)]
    stop_group(child.id(), started);
    #[cfg(not(unix))]
    let _ = started;
    let written = writer.map_or(Ok(Ok(())), |w| join_by(w, join_end));
    let read = match (read, reader) {
        (Some(read), _) => Ok(read),
        (None, Some(reader)) => join_by(reader, join_end),
        // The reader ended without a result: treat it as an open pipe.
        (None, None) => Err(()),
    };
    let errors = join_by(errors, join_end);
    let (Ok(read), Ok(stderr), Ok(_)) = (read, errors, written) else {
        bail!("a pipe of git was still open after git ended and its process group was stopped");
    };
    let (records, complete) = read?;
    Ok(Records {
        status,
        records,
        complete,
        stderr,
    })
}

/// git's exit status once it has exited. A failed wait stops git first, so
/// the error never leaves git running.
fn poll_exit(child: &mut Child) -> Result<Option<ExitStatus>> {
    child
        .try_wait()
        .inspect_err(|_| {
            let _ = kill_and_reap(child);
        })
        .context("waiting for git")
}

/// Stop what is left of git's process group after git is reaped: TERM, up
/// to [`REAP_GRACE`] for the group to empty, then KILL. Each signal is sent
/// only while the group is still git's: a member still holds the id, or
/// the id names a process with git's start time `started`
/// ([`crate::procid::is_same_group`]).
#[cfg(unix)]
fn stop_group(pgid: u32, started: u64) {
    let ours = || crate::procid::is_same_group(pgid, started);
    let Ok(group) = libc::pid_t::try_from(pgid) else {
        return;
    };
    if ours() {
        // SAFETY: `kill` only signals, and `ours` checked the group.
        unsafe { libc::kill(-group, libc::SIGTERM) };
    }
    let until = Instant::now() + REAP_GRACE;
    while ours() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(5));
    }
    if ours() {
        // SAFETY: as above.
        unsafe { libc::kill(-group, libc::SIGKILL) };
    }
}

/// Stop git and its process group, then reap git: TERM first, so git
/// removes its lock files, and KILL when git is not reaped within
/// [`REAP_GRACE`]. A git that is not reaped after the KILL either is an
/// error: no wait here blocks, and the whole stop takes at most 2 graces.
fn kill_and_reap(child: &mut Child) -> Result<ExitStatus> {
    #[cfg(unix)]
    {
        signal_group(child, libc::SIGTERM);
        if let Ok(status) = reap_within(child, REAP_GRACE) {
            // A helper that ignored the TERM still holds the group id, so
            // no new process can have it yet.
            signal_group(child, libc::SIGKILL);
            return Ok(status);
        }
    }
    #[cfg(unix)]
    let killed = signal_group(child, libc::SIGKILL);
    #[cfg(not(unix))]
    let killed = child.kill().is_ok();
    let kill_error = (!killed).then(std::io::Error::last_os_error);
    reap_within(child, REAP_GRACE).with_context(|| match &kill_error {
        Some(e) => format!("could not kill git: {e}"),
        None => "git was killed".to_string(),
    })
}

/// Send `signal` to the process group that `child` leads; true when sent.
/// Call it only while the group id cannot belong to another group: git is
/// unreaped, or git is reaped and a member of its group still lives (a
/// group id is not given out while the group has a member). Then the
/// signal reaches only this call's descendants; an empty group is ESRCH.
#[cfg(unix)]
fn signal_group(child: &Child, signal: libc::c_int) -> bool {
    // SAFETY: `kill` only signals, and the rule above keeps the group ours.
    unsafe { libc::kill(-(child.id() as libc::pid_t), signal) == 0 }
}

/// Reap `child` by polling, never by a blocking wait: a child that does not
/// exit within `grace` is an error, so the call still returns.
fn reap_within(child: &mut Child, grace: Duration) -> Result<ExitStatus> {
    let until = Instant::now() + grace;
    loop {
        if let Some(status) = child.try_wait().context("waiting for git")? {
            return Ok(status);
        }
        if Instant::now() >= until {
            bail!("git did not exit within {} ms", grace.as_millis());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The result of `handle`, or `Err` when it is not done at `end`. A reader
/// that is not done is detached: the group stop closes its pipe, so it ends.
fn join_by<T>(handle: JoinHandle<T>, end: Instant) -> std::result::Result<T, ()> {
    while !handle.is_finished() {
        if Instant::now() >= end {
            return Err(());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    handle.join().map_err(drop)
}

/// The first [`MAX_STDERR`] bytes of `stderr`; the rest is read and dropped,
/// so git never blocks on a full pipe.
fn read_stderr(mut stderr: impl Read) -> Vec<u8> {
    let mut kept = Vec::new();
    let _ = stderr.by_ref().take(MAX_STDERR).read_to_end(&mut kept);
    let mut dropped = [0u8; 4096];
    while stderr.read(&mut dropped).is_ok_and(|n| n > 0) {}
    kept
}

/// At most `max` NUL-ended records from `reader`, and whether it ended
/// within them. A record longer than [`MAX_RECORD`] is an error.
pub fn read_records(mut reader: impl BufRead, max: usize) -> Result<(Vec<Vec<u8>>, bool)> {
    let mut records = Vec::new();
    loop {
        let mut record = Vec::new();
        reader
            .by_ref()
            .take(MAX_RECORD as u64 + 1)
            .read_until(0, &mut record)
            .context("reading git output")?;
        if record.is_empty() {
            return Ok((records, true));
        }
        if record.last() == Some(&0) {
            record.pop();
        } else if record.len() > MAX_RECORD {
            bail!("refused: git printed a record longer than {MAX_RECORD} bytes");
        }
        if records.len() == max {
            return Ok((records, false));
        }
        records.push(record);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn records_end_within_the_cap() {
        let (records, complete) = read_records(Cursor::new(b"a\0bb\0c".to_vec()), 3).unwrap();
        assert!(complete);
        assert_eq!(records, vec![b"a".to_vec(), b"bb".to_vec(), b"c".to_vec()]);
    }

    #[test]
    fn reading_stops_at_the_first_record_past_the_cap() {
        let input: Vec<u8> = (0..50)
            .flat_map(|n| format!("{n}\0").into_bytes())
            .collect();
        let mut reader = Cursor::new(input);
        let (records, complete) = read_records(&mut reader, 10).unwrap();
        assert!(!complete);
        assert_eq!(records.len(), 10);
        // Only 1 record past the cap was read: the rest stays unread.
        assert_eq!(
            reader.position(),
            b"0\x001\x002\x003\x004\x005\x006\x007\x008\x009\x0010\0".len() as u64
        );
    }

    #[test]
    fn a_record_longer_than_the_bound_is_refused_before_it_is_stored() {
        let mut long = vec![b'x'; MAX_RECORD + 1];
        long.push(0);
        let mut reader = Cursor::new(long);
        let e = read_records(&mut reader, 10).unwrap_err();
        assert!(e.to_string().contains("longer than 4099 bytes"), "{e}");
        assert_eq!(reader.position(), MAX_RECORD as u64 + 1);
        let mut exact = vec![b'x'; MAX_RECORD];
        exact.push(0);
        let (records, complete) = read_records(Cursor::new(exact), 10).unwrap();
        assert!(complete && records[0].len() == MAX_RECORD);
    }

    #[cfg(unix)]
    fn sh(script: &str) -> Command {
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", script]);
        cmd
    }

    #[cfg(unix)]
    #[test]
    fn a_child_past_the_deadline_is_killed_and_is_an_error() {
        let started = Instant::now();
        let e = run_until(
            sh("exec sleep 30"),
            Vec::new(),
            1,
            Duration::from_millis(300),
        )
        .unwrap_err();
        assert!(e.to_string().contains("was stopped"), "{e}");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_child_past_the_record_cap_is_killed_at_once() {
        let started = Instant::now();
        let out = run_until(
            sh("exec yes x | tr '\\n' '\\0'"),
            Vec::new(),
            10,
            Duration::from_secs(30),
        )
        .unwrap();
        assert!(!out.complete);
        assert_eq!(out.records.len(), 10);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_child_within_the_bounds_keeps_its_output_and_stderr() {
        let out = run_until(
            sh("printf 'a\\0b'; echo oops >&2; exit 3"),
            Vec::new(),
            5,
            GIT_DEADLINE,
        )
        .unwrap();
        assert!(out.complete);
        assert_eq!(out.records, vec![b"a".to_vec(), b"b".to_vec()]);
        assert_eq!(out.status.code(), Some(3));
        let e = out.text("x").unwrap_err();
        assert!(e.to_string().contains("oops"), "{e}");
    }

    #[cfg(unix)]
    #[test]
    fn reaping_a_child_that_does_not_exit_returns_inside_the_grace() {
        let mut cmd = sh("exec sleep 30");
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
        let mut child = cmd.spawn().unwrap();
        let started = Instant::now();
        let e = reap_within(&mut child, Duration::from_millis(200)).unwrap_err();
        assert!(e.to_string().contains("did not exit within 200 ms"), "{e}");
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(kill_and_reap(&mut child).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn a_child_that_ignores_term_is_killed_inside_2_graces() {
        let mut cmd = sh("trap '' TERM; while :; do sleep 1; done");
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
        let mut child = cmd.spawn().unwrap();
        // Let the shell set its trap before the TERM.
        std::thread::sleep(Duration::from_millis(200));
        let started = Instant::now();
        let status = kill_and_reap(&mut child).unwrap();
        assert!(!status.success());
        assert!(
            started.elapsed() >= REAP_GRACE && started.elapsed() < 3 * REAP_GRACE,
            "{:?}",
            started.elapsed()
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_helper_that_ignores_term_ends_after_git_is_reaped() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("helper.pid");
        let script = format!(
            "sh -c \"trap '' TERM; while :; do sleep 1; done\" & echo $! > '{}'; sleep 0.2; exec sleep 30",
            pid_file.display()
        );
        let started = Instant::now();
        run_until(sh(&script), Vec::new(), 1, Duration::from_millis(500)).unwrap_err();
        let pid: libc::pid_t = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        // SAFETY: signal 0 only checks that the pid exists.
        while unsafe { libc::kill(pid, 0) } == 0 {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "helper {pid} survived"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_helper_that_holds_a_pipe_after_git_exits_is_stopped() {
        // A helper that holds stdout, 1 that holds only stderr, and 1 that
        // holds only stdin (an explicit `<&0`: sh gives a background job
        // /dev/null otherwise).
        for redirect in ["", ">/dev/null", "<&0 >/dev/null 2>&1"] {
            let dir = tempfile::tempdir().unwrap();
            let pid_file = dir.path().join("helper.pid");
            let script = format!(
                "sleep 30 {redirect} & echo $! > '{}'; exit 0",
                pid_file.display()
            );
            let limit = Duration::from_millis(300);
            let started = Instant::now();
            // Input, so stdin is a pipe the helper can hold.
            let _ = run_until(sh(&script), b"x".to_vec(), 5, limit);
            assert!(
                started.elapsed() < limit + 2 * REAP_GRACE + Duration::from_millis(500),
                "{redirect}: {:?}",
                started.elapsed()
            );
            let pid: libc::pid_t = std::fs::read_to_string(&pid_file)
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            // Dead at the return; reparented, it is reaped soon after.
            let returned = Instant::now();
            // SAFETY: signal 0 only checks that the pid exists.
            while unsafe { libc::kill(pid, 0) } == 0 {
                assert!(
                    returned.elapsed() < Duration::from_secs(1),
                    "{redirect}: helper {pid} survived"
                );
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_deadline_kill_also_ends_a_helper_that_git_started() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("helper.pid");
        let script = format!(
            "sleep 30 & echo $! > '{}'; exec sleep 30",
            pid_file.display()
        );
        let started = Instant::now();
        run_until(sh(&script), Vec::new(), 1, Duration::from_millis(300)).unwrap_err();
        let pid: libc::pid_t = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        // The helper is reparented and reaped once it is dead.
        // SAFETY: signal 0 only checks that the pid exists.
        while unsafe { libc::kill(pid, 0) } == 0 {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "helper {pid} survived"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
