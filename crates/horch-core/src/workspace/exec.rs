//! Bounded child execution: timeout, deadline, output cap and process-group
//! containment shared by [`super::herdr`] and `hc`.

use std::io::Read;
use std::process::{Child, Output, Stdio};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use horch_platform::group::{Contained, NotResumed};
use horch_platform::start;

/// Whether `stderr` is herdr's `{"error":{"code":"timeout",...}}`.
pub(crate) fn is_timeout(stderr: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(stderr.trim())
        .is_ok_and(|v| v["error"]["code"].as_str() == Some("timeout"))
}

/// The most a bounded call reads from each of stdout and stderr.
const OUTPUT_CAP: u64 = 1024 * 1024;

/// The error for a call that reached its stop: the push deadline when
/// `at_deadline`, else `timeout`.
pub(crate) fn stop_error(at_deadline: bool, timeout: Duration) -> std::io::Error {
    let message = if at_deadline {
        "herdr did not answer inside the push budget".to_string()
    } else {
        format!("herdr did not answer within {} ms", timeout.as_millis())
    };
    limit_error(message, std::io::ErrorKind::TimedOut)
}

pub(crate) fn limit_error(message: String, kind: std::io::ErrorKind) -> std::io::Error {
    std::io::Error::new(kind, message)
}

/// A reader's channel and its thread.
type Reader = (mpsc::Receiver<std::io::Result<Vec<u8>>>, JoinHandle<()>);

/// The error for a stream that read past [`OUTPUT_CAP`], if one did.
pub(crate) fn over_cap(collected: &[Option<Vec<u8>>]) -> Option<std::io::Error> {
    collected
        .iter()
        .flatten()
        .any(|b| b.len() as u64 > OUTPUT_CAP)
        .then(|| {
            limit_error(
                format!("herdr printed more than {OUTPUT_CAP} bytes"),
                std::io::ErrorKind::InvalidData,
            )
        })
}

/// Read at most [`OUTPUT_CAP`] + 1 bytes of `pipe`, then send them. The
/// reader ends when the pipe closes or the cap is hit; a caller waits for it
/// only for [`READER_GRACE`].
fn read_capped(pipe: Option<impl Read + Send + 'static>) -> std::io::Result<Reader> {
    let (sender, receiver) = mpsc::channel();
    let handle = std::thread::Builder::new().spawn(move || {
        let mut bytes = Vec::new();
        let read = match pipe {
            Some(pipe) => pipe
                .take(OUTPUT_CAP + 1)
                .read_to_end(&mut bytes)
                .map(|_| ()),
            None => Ok(()),
        };
        let _ = sender.send(read.map(|()| bytes));
    })?;
    Ok((receiver, handle))
}

/// The most a finished call waits for its reader threads after the child is
/// killed. A reader still blocked then is left to end with its pipe: a
/// descendant that escaped containment can hold the pipe open.
const READER_GRACE: Duration = Duration::from_millis(200);

fn join_readers(handles: Vec<JoinHandle<()>>) {
    let until = Instant::now() + READER_GRACE;
    for handle in handles {
        while !handle.is_finished() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(5));
        }
        if handle.is_finished() {
            let _ = handle.join();
        }
    }
}

/// Kill and reap `child`. A failure of either is a warning, not an error:
/// the call has already failed and the caller goes on.
///
/// On macOS and Linux the child leads its own process group, so one signal
/// also ends every descendant that holds a pipe. The leader is still
/// unreaped here, so its pid cannot belong to another process: the group
/// signal reaches only this call's descendants.
///
/// On Windows this kills the child only. The job of [`kill_tree_and_reap`]
/// kills its descendants. When the job cannot be set up, the child is still
/// suspended and has started no descendant, so the child alone is enough.
fn kill_and_reap(child: &mut Child) -> std::io::Result<()> {
    if !horch_platform::group::kill_child_group(child) {
        eprintln!("horch: could not kill herdr");
    }
    reap_within(child, REAP_GRACE).inspect_err(|e| eprintln!("horch: could not reap herdr: {e}"))
}

/// Kill every process in `job`, then kill and reap `child`, as
/// [`kill_and_reap`] does. A failed kill of the job is a warning.
fn kill_tree_and_reap(job: &Contained, child: &mut Child) -> std::io::Result<()> {
    if !job.terminate() {
        eprintln!("horch: could not kill the herdr process tree");
    }
    kill_and_reap(child)
}

/// The most a failed call waits for its killed child to be reaped.
const REAP_GRACE: Duration = Duration::from_secs(1);

/// Reap `child` by polling, never by a blocking wait: a child that does not
/// exit within `grace` is an error, so the call still returns.
fn reap_within(child: &mut Child, grace: Duration) -> std::io::Result<()> {
    let until = Instant::now() + grace;
    loop {
        if child.try_wait()?.is_some() {
            return Ok(());
        }
        if Instant::now() >= until {
            return Err(limit_error(
                format!(
                    "herdr did not exit within {} ms of the kill",
                    grace.as_millis()
                ),
                std::io::ErrorKind::TimedOut,
            ));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// An error of [`Contained::start`], with herdr named where the platform
/// names only the child process.
fn name_herdr(error: std::io::Error) -> std::io::Error {
    if error
        .get_ref()
        .is_some_and(|inner| inner.is::<NotResumed>())
    {
        std::io::Error::other("could not resume herdr")
    } else {
        error
    }
}

/// Run `command` with one `timeout` for the exit and for both pipes. The
/// call stops at the earlier of `timeout` and the absolute `deadline`. On
/// that stop, or when either pipe passes the cap, the child is killed and the
/// call fails. On Windows the child starts suspended inside a job, and the
/// whole tree is killed. Every platform then waits at most [`READER_GRACE`]
/// for the readers.
pub(crate) fn output_within(
    mut command: start::Command,
    timeout: Duration,
    deadline: Option<Instant>,
) -> std::io::Result<Output> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let timeout_end = Instant::now().checked_add(timeout).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "herdr timeout is too large",
        )
    })?;
    let stop = deadline.map_or(timeout_end, |deadline| deadline.min(timeout_end));
    // On Windows the child starts suspended, so it cannot start a descendant
    // before the job holds it.
    Contained::prepare(&mut command);
    let mut child = command.spawn()?;
    let job = match Contained::start(&child) {
        Ok(job) => job,
        Err(error) => {
            let _ = kill_and_reap(&mut child);
            return Err(name_herdr(error));
        }
    };
    let ((stdout_rx, stdout_thread), (stderr_rx, stderr_thread)) = match (
        read_capped(child.stdout.take()),
        read_capped(child.stderr.take()),
    ) {
        (Ok(stdout), Ok(stderr)) => (stdout, stderr),
        (Err(error), _) | (_, Err(error)) => {
            let _ = kill_tree_and_reap(&job, &mut child);
            return Err(error);
        }
    };
    let readers = [stdout_rx, stderr_rx];
    let threads = vec![stdout_thread, stderr_thread];
    let mut collected: [Option<Vec<u8>>; 2] = [None, None];
    let mut exited = false;
    let failure = loop {
        let mut read_failure = None;
        for (slot, reader) in collected.iter_mut().zip(&readers) {
            match reader.try_recv() {
                Ok(Ok(bytes)) if slot.is_none() => *slot = Some(bytes),
                Ok(Err(error)) => read_failure = Some(error),
                _ => {}
            }
        }
        if read_failure.is_some() {
            break read_failure;
        }
        if let Some(error) = over_cap(&collected) {
            break Some(error);
        }
        if !exited {
            exited = match horch_platform::group::exited_without_reap(&mut child) {
                Ok(exited) => exited,
                Err(error) => {
                    let _ = kill_tree_and_reap(&job, &mut child);
                    join_readers(threads);
                    return Err(error);
                }
            };
        }
        if exited && collected.iter().all(Option::is_some) {
            break None;
        }
        if Instant::now() >= stop {
            break Some(stop_error(stop < timeout_end, timeout));
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    if let Some(error) = failure {
        let _ = kill_tree_and_reap(&job, &mut child);
        join_readers(threads);
        return Err(error);
    }
    join_readers(threads);
    let [stdout, stderr] = collected;
    Ok(Output {
        status: child.wait()?,
        stdout: stdout.unwrap_or_default(),
        stderr: stderr.unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::process::Command;

    use super::*;
    use crate::workspace::herdr::Herdr;

    /// A fake herdr that runs `body`, already run once. macOS scans a new
    /// executable at its first exec for seconds (up to 35 s under load),
    /// which would race every timed call. The warm run exits before `body`.
    #[cfg(unix)]
    fn script_in(body: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("herdr");
        let warm_guard = "if [ \"$1\" = warm ]; then\n  exit 0\nfi";
        let warm = crate::test_support::write_warmed(
            &script,
            &format!("#!/bin/sh\n{warm_guard}\n{body}\n"),
            &["warm"],
        );
        assert!(warm.status.success(), "warming {}", script.display());
        (dir, script)
    }

    #[test]
    fn a_child_that_does_not_resume_is_named_herdr() {
        let error = name_herdr(std::io::Error::other(NotResumed));
        assert_eq!(error.to_string(), "could not resume herdr");
        let other = name_herdr(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        assert_eq!(other.kind(), std::io::ErrorKind::PermissionDenied);
    }

    #[cfg(unix)]
    #[test]
    fn a_descendant_that_keeps_stdout_open_past_the_deadline_fails_the_call() {
        let (_dir, script) = script_in("sleep 4 &\nexit 0");
        let started = Instant::now();
        let herdr = Herdr::with_bin(&script).with_timeout(Duration::from_millis(1000));
        assert!(herdr.pane_list("w1").is_err());
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[cfg(unix)]
    #[test]
    fn a_descendant_holding_stdout_is_killed_with_the_call() {
        let (dir, script) = script_in("sleep 30 &\necho $! > \"$(dirname \"$0\")/pid\"\nexit 0");
        let herdr = Herdr::with_bin(&script).with_timeout(Duration::from_millis(1500));
        assert!(herdr.pane_list("w1").is_err());
        // A loaded machine can kill the script before it writes its pid:
        // then no process was left to leak.
        let Ok(pid) = std::fs::read_to_string(dir.path().join("pid")) else {
            return;
        };
        let gone = (0..40).any(|_| {
            let dead = !Command::new("kill")
                .args(["-0", pid.trim()])
                .output()
                .unwrap()
                .status
                .success();
            if !dead {
                std::thread::sleep(Duration::from_millis(50));
            }
            dead
        });
        assert!(gone, "descendant {pid} is still running");
    }

    /// A stream that never ends is read only to 1 byte past the cap, so the
    /// call can stop it: no timing is involved.
    #[test]
    fn a_reader_stops_1_byte_past_the_cap_of_an_endless_stream() {
        let (receiver, handle) = read_capped(Some(std::io::repeat(0))).unwrap();
        let bytes = receiver.recv().unwrap().unwrap();
        handle.join().unwrap();
        assert_eq!(bytes.len() as u64, OUTPUT_CAP + 1);
    }

    /// A herdr that prints past the cap on either stream fails the call with
    /// InvalidData. [`script_in`] warms the script, so the scan at its first
    /// exec does not race the call's timeout.
    #[cfg(unix)]
    #[test]
    fn a_herdr_that_prints_more_than_the_cap_fails_the_call() {
        for stream in ["", ">&2"] {
            let (_dir, script) = script_in(&format!("head -c 3000000 /dev/zero {stream}"));
            let error =
                output_within(start::command(&script), Duration::from_secs(5), None).unwrap_err();
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidData, "{stream}");
            assert!(error.to_string().contains("more than"), "{error}");
        }
    }

    /// Output past the cap on stdout or on stderr fails the call with
    /// InvalidData; output at the cap does not.
    #[test]
    fn output_past_the_cap_on_either_stream_is_invalid_data() {
        let at_cap = Some(vec![0; OUTPUT_CAP as usize]);
        let past = Some(vec![0; OUTPUT_CAP as usize + 1]);
        assert!(over_cap(&[at_cap.clone(), at_cap.clone()]).is_none());
        assert!(over_cap(&[None, at_cap.clone()]).is_none());
        for collected in [[past.clone(), None], [at_cap, past]] {
            let error = over_cap(&collected).unwrap();
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
            assert!(
                error.to_string().contains("more than 1048576 bytes"),
                "{error}"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_descendant_holding_stdout_does_not_outlive_the_timeout() {
        let mut command = start::command("cmd");
        command.args(["/c", "start /b ping -n 30 127.0.0.1 & exit 0"]);
        let started = Instant::now();
        let error = output_within(command, Duration::from_millis(1000), None).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        // The call joins its readers: it returns only once the tree is dead.
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[cfg(unix)]
    #[test]
    fn reaping_a_child_that_does_not_exit_returns_inside_the_grace() {
        let mut command = Command::new("sleep");
        command.arg("30");
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        let mut child = command.spawn().unwrap();
        let started = Instant::now();
        let error = reap_within(&mut child, Duration::from_millis(200)).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(5));
        kill_and_reap(&mut child).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_stalled_herdr_fails_with_timed_out_after_the_deadline() {
        let (_dir, script) = script_in("exec sleep 30");
        let started = Instant::now();
        let error =
            output_within(start::command(&script), Duration::from_millis(500), None).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert!(error.to_string().contains("did not answer"), "{error}");
        assert!(started.elapsed() >= Duration::from_millis(400));
    }

    #[cfg(unix)]
    #[test]
    fn a_timeout_too_large_for_an_instant_is_rejected_before_spawning() {
        let (_dir, script) = script_in("exit 0");
        let error = output_within(start::command(&script), Duration::MAX, None).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[cfg(unix)]
    #[test]
    fn a_herdr_that_outlasts_the_timeout_is_killed() {
        let (dir, script) = script_in("echo $$ > \"$(dirname \"$0\")/pid\"\nexec sleep 60");
        let pidfile = dir.path().join("pid");

        let started = Instant::now();
        let herdr = Herdr::with_bin(&script).with_timeout(Duration::from_millis(2000));
        assert!(herdr.pane_list("w1").is_err());

        assert!(started.elapsed() < Duration::from_secs(10));
        let Ok(pid) = std::fs::read_to_string(&pidfile) else {
            return;
        };
        let alive = Command::new("kill")
            .args(["-0", pid.trim()])
            .output()
            .unwrap();
        assert!(!alive.status.success(), "herdr {pid} is still running");
    }

    /// A herdr call that outlives the deadline is killed and fails at once;
    /// one that answers in time returns its output (#67).
    #[cfg(unix)]
    #[test]
    fn a_call_past_the_deadline_is_killed() {
        let (_stuck_dir, stuck_script) = script_in("sleep 10");
        let (_quick_dir, quick_script) = script_in("echo hi");
        let stuck =
            Herdr::with_bin(stuck_script).until(Some(Instant::now() + Duration::from_millis(300)));
        let started = Instant::now();
        let err = stuck.pane_get("w1:p1").unwrap_err().to_string();
        assert!(started.elapsed() < Duration::from_secs(3), "{err}");
        assert!(err.contains("push budget"), "{err}");
        let quick =
            Herdr::with_bin(quick_script).until(Some(Instant::now() + Duration::from_secs(10)));
        assert_eq!(quick.output(&["x"]).unwrap(), "hi\n");
    }

    /// With a timeout and a deadline, the earlier one stops the call.
    #[cfg(unix)]
    #[test]
    fn a_deadline_before_the_timeout_stops_the_call() {
        let (_dir, script) = script_in("sleep 10");
        let herdr = Herdr::with_bin(&script)
            .with_timeout(Duration::from_secs(10))
            .until(Some(Instant::now() + Duration::from_millis(300)));
        let started = Instant::now();
        let err = herdr.pane_get("w1:p1").unwrap_err().to_string();
        assert!(started.elapsed() < Duration::from_secs(3), "{err}");
        assert!(err.contains("push budget"), "{err}");
    }
}
