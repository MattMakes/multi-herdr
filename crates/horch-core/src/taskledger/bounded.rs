//! A bounded git runner: every git call that horch runs outside the
//! sandbox on worker-controlled input ends within a deadline, an output cap
//! and its process group (#T-10). T-517 (ticket 18) moves it into
//! `git_safe.rs` and shares it with every horch git call.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStderr, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
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

/// How 1 bounded run ended.
pub struct Bounded<O, E> {
    pub status: ExitStatus,
    /// What the stdout reader returned.
    pub out: O,
    /// What the stderr reader returned.
    pub err: E,
    /// True when the deadline passed and the process group was stopped.
    pub timed_out: bool,
}

/// Run `cmd` with `input` on its stdin, as the leader of its own process
/// group. `read_out` and `read_err` read its pipes on their own threads;
/// when `read_out` returns before the child exits and `stop_early` says so,
/// the group is stopped. At `limit` the group is stopped too. On every
/// return, what is left of the group is stopped, so nothing it started
/// outlives the call.
pub fn run_bounded<O, E>(
    cmd: Command,
    input: Vec<u8>,
    limit: Duration,
    read_out: impl FnOnce(ChildStdout) -> O + Send + 'static,
    read_err: impl FnOnce(ChildStderr) -> E + Send + 'static,
    stop_early: impl Fn(Option<&O>) -> bool,
) -> Result<Bounded<O, E>>
where
    O: Send + 'static,
    E: Send + 'static,
{
    let end = Instant::now() + limit;
    let mut run = start(cmd, input, read_out, read_err)?;
    let (status, read, timed_out, join_end) = monitor(&mut run, end, &stop_early)?;
    let (out, err) = collect(run, read, join_end)?;
    Ok(Bounded {
        status,
        out,
        err,
        timed_out,
    })
}

/// Run `cmd` with `input` on its stdin, and read at most `max` NUL-ended
/// stdout records of at most [`MAX_RECORD`] bytes each. git is stopped at
/// the first record past either bound, and at `limit`, which is an error.
/// So no worker-controlled input grows horch's memory or time past them.
pub fn run_until(cmd: Command, input: Vec<u8>, max: usize, limit: Duration) -> Result<Records> {
    run_records(cmd, input, 0, max, limit)
}

/// [`run_until`] with records ended by `sep`.
pub fn run_records(
    cmd: Command,
    input: Vec<u8>,
    sep: u8,
    max: usize,
    limit: Duration,
) -> Result<Records> {
    let run = run_bounded(
        cmd,
        input,
        limit,
        move |o| read_delimited(BufReader::new(o), sep, max),
        read_stderr,
        |r: Option<&Result<(Vec<Vec<u8>>, bool)>>| r.is_some_and(|r| !matches!(r, Ok((_, true)))),
    )?;
    if run.timed_out {
        bail!(
            "git ran longer than {} s and was stopped",
            limit.as_secs_f32()
        );
    }
    let (records, complete) = run.out?;
    Ok(Records {
        status: run.status,
        records,
        complete,
        stderr: run.err,
    })
}

/// What 1 capped run printed.
#[derive(Debug)]
pub struct Capped {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Run `cmd` with no input for at most `limit`, and keep at most `cap`
/// bytes of each pipe. More output on a pipe stops the group; it and the
/// deadline are errors.
pub fn run_capped(cmd: Command, limit: Duration, cap: usize) -> Result<Capped> {
    let program = cmd.get_program().to_string_lossy().into_owned();
    // stderr over the cap stops the run as soon as stdout over it does.
    let err_over = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = Arc::clone(&err_over);
    let run = run_bounded(
        cmd,
        Vec::new(),
        limit,
        move |o| read_capped(o, cap),
        move |e| {
            let read = read_capped(e, cap);
            flag.store(read.1, std::sync::atomic::Ordering::SeqCst);
            read
        },
        |r: Option<&(Vec<u8>, bool)>| {
            r.is_some_and(|r| r.1) || err_over.load(std::sync::atomic::Ordering::SeqCst)
        },
    )?;
    if run.timed_out {
        bail!(
            "{program} ran longer than {} s and was stopped",
            limit.as_secs_f32()
        );
    }
    if run.out.1 || run.err.1 {
        bail!("{program} printed more than {cap} bytes on a pipe and was stopped");
    }
    Ok(Capped {
        status: run.status,
        stdout: run.out.0,
        stderr: run.err.0,
    })
}

/// At most `cap` bytes of `reader`, and true when it had more. The read
/// stops at the first byte past the cap: the runner then stops the child.
fn read_capped(reader: impl Read, cap: usize) -> (Vec<u8>, bool) {
    let mut kept = Vec::new();
    let _ = reader.take(cap as u64 + 1).read_to_end(&mut kept);
    let over = kept.len() > cap;
    kept.truncate(cap);
    (kept, over)
}

/// Run `cmd` with no input for at most `limit`, with stdout and stderr
/// appended to `log` as they come, at most `cap` bytes in all. Past the
/// cap, the log gets 1 line `# log truncated` and the rest is read and
/// dropped: memory and the log stay bounded. Returns the status, and true
/// when the deadline stopped the group.
pub fn run_logged(
    cmd: Command,
    log: &Path,
    limit: Duration,
    cap: u64,
) -> Result<(ExitStatus, bool)> {
    let file = std::fs::OpenOptions::new()
        .append(true)
        .open(log)
        .with_context(|| format!("opening {}", log.display()))?;
    let sink = Arc::new(Mutex::new(LogSink {
        file,
        written: 0,
        cap,
        truncated: false,
    }));
    let (out, err) = (Arc::clone(&sink), Arc::clone(&sink));
    let run = run_bounded(
        cmd,
        Vec::new(),
        limit,
        move |o| stream_to(o, &out),
        move |e| stream_to(e, &err),
        |_: Option<&()>| false,
    )?;
    Ok((run.status, run.timed_out))
}

/// A log file and how much of its cap is used.
struct LogSink {
    file: std::fs::File,
    written: u64,
    cap: u64,
    truncated: bool,
}

/// Copy `reader` to the sink until it ends, within the sink's cap.
fn stream_to(mut reader: impl Read, sink: &Mutex<LogSink>) {
    let mut buf = [0u8; 8192];
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => return,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return,
        };
        let Ok(mut s) = sink.lock() else { continue };
        let room = s.cap.saturating_sub(s.written).min(n as u64) as usize;
        if room > 0 {
            let _ = s.file.write_all(&buf[..room]);
        }
        if room < n && !s.truncated {
            s.truncated = true;
            let _ = s.file.write_all(b"\n# log truncated\n");
        }
        s.written = s.written.saturating_add(n as u64);
    }
}

/// A child, its group's guard, and the threads on its pipes.
struct Running<O, E> {
    child: Child,
    group: GroupGuard,
    writer: Option<JoinHandle<std::io::Result<()>>>,
    out: Option<JoinHandle<O>>,
    err: JoinHandle<E>,
}

/// Start `cmd` in its own process group, with a thread on each pipe. The
/// writer thread keeps a full stdout pipe from blocking the input.
fn start<O, E>(
    mut cmd: Command,
    input: Vec<u8>,
    read_out: impl FnOnce(ChildStdout) -> O + Send + 'static,
    read_err: impl FnOnce(ChildStderr) -> E + Send + 'static,
) -> Result<Running<O, E>>
where
    O: Send + 'static,
    E: Send + 'static,
{
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let stdin = if input.is_empty() {
        Stdio::null()
    } else {
        Stdio::piped()
    };
    let program = cmd.get_program().to_string_lossy().into_owned();
    cmd.stdin(stdin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // On Windows the child starts suspended inside a job that kills its
    // whole tree when the guard drops.
    #[cfg(windows)]
    let (mut child, job) = crate::runtime::job::spawn_contained(&mut cmd)
        .with_context(|| format!("running {program}"))?;
    #[cfg(not(windows))]
    let mut child = cmd.spawn().with_context(|| format!("running {program}"))?;
    let group = GroupGuard::of(&child);
    #[cfg(windows)]
    let group = GroupGuard {
        job: Some(job),
        ..group
    };
    let writer = child
        .stdin
        .take()
        .map(|mut pipe| std::thread::spawn(move || pipe.write_all(&input)));
    let stdout = child.stdout.take().context("the child has no stdout")?;
    let stderr = child.stderr.take().context("the child has no stderr")?;
    Ok(Running {
        out: Some(std::thread::spawn(move || read_out(stdout))),
        err: std::thread::spawn(move || read_err(stderr)),
        child,
        group,
        writer,
    })
}

/// Wait for the child until `end`, and stop it early when its finished
/// stdout result says so. Returns its status, the stdout result when it was
/// read here, whether the deadline passed, and the last time a pipe thread
/// may take: `end`, or 1 grace after a stop.
fn monitor<O, E>(
    run: &mut Running<O, E>,
    end: Instant,
    stop_early: &dyn Fn(Option<&O>) -> bool,
) -> Result<(ExitStatus, Option<O>, bool, Instant)> {
    let mut read = None;
    let mut next_scan = Instant::now();
    loop {
        if Instant::now() >= next_scan {
            run.group.track();
            next_scan = Instant::now() + TRACK_INTERVAL;
        }
        if let Some(status) = poll_exit(&mut run.child)? {
            return Ok((status, read, false, end));
        }
        if run.out.as_ref().is_some_and(JoinHandle::is_finished) {
            read = run.out.take().and_then(|h| h.join().ok());
        }
        let early = stop_early(read.as_ref());
        let late = Instant::now() >= end;
        if early || late {
            let status = kill_and_reap(&mut run.child)?;
            return Ok((status, read, late && !early, Instant::now() + REAP_GRACE));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Stop what is left of the group, then join every pipe thread by
/// `join_end`. A helper can hold only stdin, with stdout and stderr
/// elsewhere, and no pipe would show it: the stop runs every time.
fn collect<O, E>(run: Running<O, E>, read: Option<O>, join_end: Instant) -> Result<(O, E)> {
    let Running {
        child,
        group,
        writer,
        out,
        err,
    } = run;
    drop(group);
    let written = writer.map_or(Ok(Ok(())), |w| join_by(w, join_end));
    let out = match (read, out) {
        (Some(read), _) => Ok(read),
        (None, Some(reader)) => join_by(reader, join_end),
        // The reader ended without a result: treat it as an open pipe.
        (None, None) => Err(()),
    };
    let err = join_by(err, join_end);
    drop(child);
    let (Ok(out), Ok(err), Ok(_)) = (out, err, written) else {
        bail!("a pipe was still open after the child ended and its process group was stopped");
    };
    Ok((out, err))
}

/// How often the runner reads the process table for descendants of the
/// child. A helper that leaves the group and whose parent exits within 1
/// interval is not seen.
const TRACK_INTERVAL: Duration = Duration::from_millis(100);

/// Stops what is left of a child's process tree when dropped, on every
/// return. Unix: the child's process group (TERM, a grace, then KILL, see
/// [`stop_group`]), and each descendant seen outside it (a helper that
/// called `setsid` or `setpgid`), while its start time still matches.
/// Windows: the job that holds the tree closes and kills it.
struct GroupGuard {
    pgid: u32,
    started: u64,
    /// Descendants seen by [`GroupGuard::track`], with their start times.
    tracked: Vec<(u32, u64)>,
    #[cfg(windows)]
    job: Option<crate::runtime::job::Job>,
}

impl GroupGuard {
    fn of(child: &Child) -> GroupGuard {
        GroupGuard {
            pgid: child.id(),
            // Names the group after the leader is reaped (see `stop_group`).
            started: crate::procid::start_time(child.id()).unwrap_or(0),
            tracked: Vec::new(),
            #[cfg(windows)]
            job: None,
        }
    }

    /// Add every process now in the tree to the tracked set.
    fn track(&mut self) {
        #[cfg(unix)]
        {
            let known: Vec<u32> = self.tracked.iter().map(|t| t.0).collect();
            for pid in new_descendants(self.pgid, &known, &process_table()) {
                if let Some(started) = crate::procid::start_time(pid) {
                    self.tracked.push((pid, started));
                }
            }
        }
    }
}

impl Drop for GroupGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            self.track();
            stop_group(self.pgid, self.started);
            stop_tracked(&self.tracked);
        }
    }
}

/// `(pid, ppid, pgid)` of every process, from `ps`; empty when it fails.
/// A fixed path: never a `ps` found on a `PATH` a worker can change.
#[cfg(unix)]
fn process_table() -> Vec<(u32, u32, u32)> {
    let ps = ["/bin/ps", "/usr/bin/ps"]
        .into_iter()
        .find(|p| Path::new(p).is_file());
    let Some(Ok(out)) = ps.map(|ps| {
        Command::new(ps)
            .args(["-A", "-o", "pid=", "-o", "ppid=", "-o", "pgid="])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
    }) else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let mut f = line.split_whitespace().map(str::parse::<u32>);
            Some((f.next()?.ok()?, f.next()?.ok()?, f.next()?.ok()?))
        })
        .collect()
}

/// The pids in `table` that `known` lacks and whose parent chain reaches
/// `leader` or a `known` pid, or whose group is `leader`'s.
fn new_descendants(leader: u32, known: &[u32], table: &[(u32, u32, u32)]) -> Vec<u32> {
    let mut found: std::collections::HashSet<u32> = known.iter().copied().collect();
    found.insert(leader);
    let mut added = Vec::new();
    loop {
        let before = added.len();
        for &(pid, ppid, pgid) in table {
            if !found.contains(&pid) && (found.contains(&ppid) || pgid == leader) {
                found.insert(pid);
                added.push(pid);
            }
        }
        if added.len() == before {
            return added;
        }
    }
}

/// TERM each tracked process that still has its start time, wait up to
/// [`REAP_GRACE`] for them to end, then KILL the rest. A pid with another
/// start time was given out again: it is left alone.
#[cfg(unix)]
fn stop_tracked(tracked: &[(u32, u64)]) {
    use crate::procid::{is_same, stop_same, Stop};
    let live = |t: &&(u32, u64)| is_same(t.0, t.1);
    for t in tracked.iter().filter(live) {
        stop_same(t.0, t.1, Stop::Term);
    }
    let until = Instant::now() + REAP_GRACE;
    while tracked.iter().any(|t| is_same(t.0, t.1)) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(5));
    }
    for t in tracked.iter().filter(live) {
        stop_same(t.0, t.1, Stop::Kill);
    }
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
pub fn read_records(reader: impl BufRead, max: usize) -> Result<(Vec<Vec<u8>>, bool)> {
    read_delimited(reader, 0, max)
}

/// [`read_records`] with records ended by `sep`.
pub fn read_delimited(
    mut reader: impl BufRead,
    sep: u8,
    max: usize,
) -> Result<(Vec<Vec<u8>>, bool)> {
    let mut records = Vec::new();
    loop {
        let mut record = Vec::new();
        reader
            .by_ref()
            .take(MAX_RECORD as u64 + 1)
            .read_until(sep, &mut record)
            .context("reading git output")?;
        if record.is_empty() {
            return Ok((records, true));
        }
        if record.last() == Some(&sep) {
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

    /// Wait up to 1 s for `pid` to be gone, or fail the test.
    #[cfg(unix)]
    fn assert_gone(pid: libc::pid_t, what: &str) {
        let returned = Instant::now();
        // SAFETY: signal 0 only checks that the pid exists.
        while unsafe { libc::kill(pid, 0) } == 0 {
            assert!(
                returned.elapsed() < Duration::from_secs(1),
                "{what}: {pid} survived"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_helper_that_leaves_the_group_is_stopped_when_the_run_ends() {
        // A helper in a new session (setsid), and one that also ignores TERM.
        for (what, term) in [
            ("setsid", ""),
            ("setsid ignoring TERM", "$SIG{TERM}='IGNORE';"),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let pid_file = dir.path().join("helper.pid");
            let script = format!(
                "perl -e 'use POSIX; POSIX::setsid(); {term} sleep 30' </dev/null >/dev/null 2>&1 & \
                 echo $! > '{}'; sleep 0.5; exit 0",
                pid_file.display()
            );
            let started = Instant::now();
            let _ = run_until(sh(&script), Vec::new(), 5, Duration::from_secs(10));
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "{what}: {:?}",
                started.elapsed()
            );
            let pid: libc::pid_t = std::fs::read_to_string(&pid_file)
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            assert_gone(pid, what);
        }
    }

    #[test]
    fn descendants_are_found_by_parent_chain_or_group() {
        // 10 leads; 11 is its child; 12 left the group but is 11's child;
        // 13 is in 10's group with another parent; 20 is unrelated.
        let table = [
            (10, 1, 10),
            (11, 10, 10),
            (12, 11, 12),
            (13, 1, 10),
            (20, 1, 20),
        ];
        let mut found = new_descendants(10, &[], &table);
        found.sort();
        assert_eq!(found, vec![11, 12, 13]);
        // A known pid whose parent died still leads its own children.
        assert_eq!(new_descendants(10, &[12], &[(30, 12, 30)]), vec![30]);
    }

    #[test]
    fn a_capped_run_that_floods_stderr_stops_at_the_cap() {
        let started = Instant::now();
        let e = run_capped(
            sh("while :; do head -c 65536 /dev/zero >&2; done"),
            Duration::from_secs(20),
            1024,
        )
        .unwrap_err();
        assert!(e.to_string().contains("more than 1024 bytes"), "{e}");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn a_logged_run_past_its_deadline_is_stopped_inside_it() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("check.log");
        std::fs::write(&log, "").unwrap();
        let limit = Duration::from_millis(300);
        let started = Instant::now();
        let (_, timed_out) = run_logged(sh("echo begun; sleep 30"), &log, limit, 1 << 20).unwrap();
        assert!(timed_out);
        assert!(started.elapsed() < limit + 2 * REAP_GRACE + Duration::from_millis(500));
        assert!(std::fs::read_to_string(&log).unwrap().contains("begun"));
    }

    #[test]
    fn a_logged_run_past_its_cap_keeps_the_log_at_the_cap_and_ends() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("check.log");
        std::fs::write(&log, "").unwrap();
        // 4 MiB on stdout and on stderr against a 64 KiB cap.
        let script = "head -c 4194304 /dev/zero; head -c 4194304 /dev/zero >&2; exit 3";
        let (status, timed_out) =
            run_logged(sh(script), &log, Duration::from_secs(20), 64 << 10).unwrap();
        assert!(
            !timed_out && status.code() == Some(3),
            "the run ended by itself"
        );
        let text = std::fs::read(&log).unwrap();
        let line = b"\n# log truncated\n";
        assert!(text.len() <= (64 << 10) + line.len(), "{}", text.len());
        assert!(text.ends_with(line));
    }

    #[test]
    fn a_capped_run_fails_past_its_deadline_or_its_cap() {
        let limit = Duration::from_millis(300);
        let started = Instant::now();
        let e = run_capped(sh("sleep 30"), limit, 1024).unwrap_err();
        assert!(e.to_string().contains("ran longer than"), "{e}");
        assert!(started.elapsed() < limit + 2 * REAP_GRACE + Duration::from_millis(500));
        let e = run_capped(
            sh("head -c 100000 /dev/zero"),
            Duration::from_secs(20),
            1024,
        )
        .unwrap_err();
        assert!(e.to_string().contains("more than 1024 bytes"), "{e}");
        let e = run_capped(
            sh("head -c 100000 /dev/zero >&2"),
            Duration::from_secs(20),
            1024,
        )
        .unwrap_err();
        assert!(e.to_string().contains("more than 1024 bytes"), "{e}");
        let ok = run_capped(sh("echo hi; echo oh >&2"), Duration::from_secs(20), 1024).unwrap();
        assert_eq!((ok.stdout, ok.stderr), (b"hi\n".to_vec(), b"oh\n".to_vec()));
    }

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
