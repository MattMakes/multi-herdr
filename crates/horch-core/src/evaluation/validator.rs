//! Mechanical validation of a frozen candidate (phase B3, CMP-09, SEC-07).
//!
//! The gates come from the operator's config through [`CommandValidator`]'s
//! constructor and from nowhere else: no method here takes a command from a
//! candidate's worktree or from a judge's output. Each gate runs as
//! `sh -c <command>` in the candidate's worktree, in its own process group,
//! with a timeout, a worktree-local `CARGO_TARGET_DIR` and `FORBIDDEN_ENV`
//! removed. Its output is stored redacted and capped.

use std::collections::BTreeSet;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};

use crate::fsx;
use crate::harness::launch::FORBIDDEN_ENV;
use crate::measure::digest::{sha256_bytes, Digest};
use crate::measure::redact::redact;
use crate::vcs::git::cut_utf8;
use crate::vcs::worktree::FrozenCandidate;

/// The most bytes of one gate's output that are stored.
pub(crate) const LOG_CAP: usize = 256 * 1024;

/// How often a running gate is polled for exit or timeout.
const POLL: Duration = Duration::from_millis(10);

/// One configured gate. From config only (SEC-07).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateSpec {
    pub name: String,
    pub command: String,
    pub timeout: Duration,
    /// A candidate is eligible only when every required gate passes. A gate
    /// that is not required still counts in the mechanical score.
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GateStatus {
    Passed,
    /// A non-zero exit; a signal `n` is reported as `128 + n`, as a shell does.
    Failed {
        code: i32,
    },
    TimedOut,
    /// The gate did not run to an exit code: `sh` did not start, or a fault
    /// was injected.
    Error {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GateResult {
    pub name: String,
    pub status: GateStatus,
    pub duration_ms: u64,
    pub log_ref: PathBuf,
    /// sha256 of the stored (redacted, capped) log bytes.
    pub log_digest: Digest,
    pub log_truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationReport {
    /// UUIDv7.
    pub validation_id: String,
    pub label: String,
    pub head_sha: String,
    pub gates: Vec<GateResult>,
    /// passed / total; 0.0 when there are no gates.
    pub mechanical_score: f64,
    /// Every required gate passed ([`GateSpec::required`]).
    pub eligible: bool,
}

pub trait Validator {
    fn validate(&self, candidate: &FrozenCandidate) -> anyhow::Result<ValidationReport>;
}

pub struct CommandValidator {
    pub gates: Vec<GateSpec>,
    /// Logs go to `<artifacts>/<label>/gate-<n>-<name>.log`.
    pub artifacts: PathBuf,
    /// Injected faults. `fail-gate:<name>` reports that gate as
    /// `Error { reason: "fault" }` without running it. A plain set until
    /// `runtime::fault::Faults` lands (phase A2).
    pub faults: BTreeSet<String>,
    /// Extra variables for every gate, set before `FORBIDDEN_ENV` is
    /// removed. Tests use it to prove the removal; nothing else needs it.
    pub env: Vec<(String, String)>,
}

impl CommandValidator {
    pub fn new(gates: Vec<GateSpec>, artifacts: PathBuf, faults: BTreeSet<String>) -> Self {
        Self {
            gates,
            artifacts,
            faults,
            env: Vec::new(),
        }
    }

    /// Add one variable to every gate's environment.
    pub fn with_env(mut self, key: &str, value: &str) -> Self {
        self.env.push((key.to_owned(), value.to_owned()));
        self
    }

    fn run_gate(
        &self,
        gate: &GateSpec,
        worktree: &Path,
        raw: &Path,
        log: &Path,
    ) -> anyhow::Result<GateResult> {
        let started = Instant::now();
        let status = if self.faults.contains(&format!("fail-gate:{}", gate.name)) {
            std::fs::write(raw, format!("horch: gate {} not run (fault)\n", gate.name))
                .with_context(|| format!("writing {}", raw.display()))?;
            GateStatus::Error {
                reason: "fault".to_owned(),
            }
        } else {
            self.spawn_and_wait(gate, worktree, raw)?
        };
        let duration_ms = started.elapsed().as_millis() as u64;
        let (log_digest, log_truncated) = store_log(raw, log)?;
        let _ = std::fs::remove_file(raw);
        Ok(GateResult {
            name: gate.name.clone(),
            status,
            duration_ms,
            log_ref: log.to_path_buf(),
            log_digest,
            log_truncated,
        })
    }

    fn spawn_and_wait(
        &self,
        gate: &GateSpec,
        worktree: &Path,
        raw: &Path,
    ) -> anyhow::Result<GateStatus> {
        let out = private_file(raw)?;
        let err = out.try_clone()?;
        match self.gate_command(gate, worktree, out, err).spawn() {
            Ok(mut child) => wait_gate(&mut child, gate.timeout),
            Err(e) => Ok(GateStatus::Error {
                reason: format!("cannot start sh: {e}"),
            }),
        }
    }

    /// The `sh -c` command of one gate, writing to `out` and `err`.
    fn gate_command(&self, gate: &GateSpec, worktree: &Path, out: File, err: File) -> Command {
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg(&gate.command)
            .current_dir(worktree)
            .stdin(Stdio::null())
            .stdout(out)
            .stderr(err);
        cmd.envs(self.env.iter().cloned());
        cmd.env("CARGO_TARGET_DIR", worktree.join("target"));
        for key in FORBIDDEN_ENV {
            cmd.env_remove(key);
        }
        // A gate that runs git works on its worktree, never on a repository
        // that horch's own environment names.
        horch_marketplace::git::scrub_repo_env(&mut cmd);
        // Its own process group, so a timeout kills everything the gate
        // started, not just `sh`.
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
        cmd
    }
}

/// Poll the gate leader until it exits or `timeout` passes.
fn wait_gate(child: &mut Child, timeout: Duration) -> anyhow::Result<GateStatus> {
    let deadline = Instant::now() + timeout;
    let status = loop {
        if leader_exited(child)? {
            // Stragglers the gate left in the background go too. The
            // leader is not reaped yet, so its pid, and with it the
            // group id, cannot be given to another program.
            crate::runtime::process::kill_group(child);
            break exit_status(child.wait()?);
        }
        if Instant::now() >= deadline {
            crate::runtime::process::kill_group(child);
            child.wait()?;
            break GateStatus::TimedOut;
        }
        std::thread::sleep(POLL);
    };
    Ok(status)
}

impl Validator for CommandValidator {
    fn validate(&self, candidate: &FrozenCandidate) -> anyhow::Result<ValidationReport> {
        let label = &candidate.label;
        if !plain_name(label) {
            bail!("candidate label '{label}' is not a plain name");
        }
        let dir = self.artifacts.join(label);
        fsx::ensure_private_dir(&dir)?;
        let mut gates = Vec::with_capacity(self.gates.len());
        for (i, gate) in self.gates.iter().enumerate() {
            let n = i + 1;
            let log = dir.join(format!("gate-{n}-{}.log", file_part(&gate.name)));
            let raw = dir.join(format!(".gate-{n}.raw"));
            gates.push(self.run_gate(gate, &candidate.worktree, &raw, &log)?);
        }
        let passed = gates
            .iter()
            .filter(|g| g.status == GateStatus::Passed)
            .count();
        let eligible = self
            .gates
            .iter()
            .zip(&gates)
            .all(|(spec, g)| !spec.required || g.status == GateStatus::Passed);
        let mechanical_score = if gates.is_empty() {
            0.0
        } else {
            passed as f64 / gates.len() as f64
        };
        Ok(ValidationReport {
            validation_id: crate::ids::mint_v7(crate::clock::now()).to_string(),
            label: label.clone(),
            head_sha: candidate.head_sha.clone(),
            eligible,
            gates,
            mechanical_score,
        })
    }
}

fn plain_name(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('-')
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

/// A gate name as a file name part: anything but `[A-Za-z0-9_-]` becomes `_`.
fn file_part(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Create or truncate `path` with mode 0600: the raw output can hold a
/// secret until it is redacted.
fn private_file(path: &Path) -> anyhow::Result<File> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, fsx::PRIVATE_FILE);
    opts.open(path)
        .with_context(|| format!("creating {}", path.display()))
}

/// Whether the gate leader has exited, without reaping it: `waitid` with
/// `WNOWAIT` leaves the zombie, so [`process::kill_group`](crate::runtime::process::kill_group) still names the gate's
/// own group.
#[cfg(unix)]
fn leader_exited(child: &mut Child) -> std::io::Result<bool> {
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    // SAFETY: `info` is a valid siginfo_t to fill; the pid is this
    // process's own unreaped child.
    let r = unsafe {
        libc::waitid(
            libc::P_PID,
            child.id() as libc::id_t,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        )
    };
    if r != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // With WNOHANG and no exit yet, the pid field stays 0.
    Ok(unsafe { info.si_pid() } != 0)
}

#[cfg(not(unix))]
fn leader_exited(child: &mut Child) -> std::io::Result<bool> {
    Ok(child.try_wait()?.is_some())
}

fn exit_status(exit: ExitStatus) -> GateStatus {
    if exit.success() {
        GateStatus::Passed
    } else {
        GateStatus::Failed {
            code: exit.code().or_else(|| signal_code(&exit)).unwrap_or(-1),
        }
    }
}

#[cfg(unix)]
fn signal_code(exit: &ExitStatus) -> Option<i32> {
    std::os::unix::process::ExitStatusExt::signal(exit).map(|sig| 128 + sig)
}

#[cfg(not(unix))]
fn signal_code(_exit: &ExitStatus) -> Option<i32> {
    None
}

/// Redact and cap the raw output in `raw`, write it to `log` (0600) and
/// return the digest of the stored bytes and whether anything was cut.
///
/// A raw log over [`LOG_CAP`] keeps its head and its tail, each cut at a line
/// boundary so no secret is split in two before redaction, with a marker line
/// between them. Only those two pieces are read.
fn store_log(raw: &Path, log: &Path) -> anyhow::Result<(Digest, bool)> {
    let mut file = File::open(raw).with_context(|| format!("opening {}", raw.display()))?;
    let len = file.metadata()?.len();
    let (text, truncated) = if len <= LOG_CAP as u64 {
        read_whole(&mut file)
    } else {
        read_head_tail(&mut file, len)
    }?;
    fsx::write_atomic(log, text.as_bytes(), fsx::PRIVATE_FILE)?;
    Ok((sha256_bytes(text.as_bytes()), truncated))
}

/// A raw log that fits the cap: redacted whole, and cut at the cap when
/// redaction made it longer.
fn read_whole(file: &mut File) -> anyhow::Result<(String, bool)> {
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let text = redact(&String::from_utf8_lossy(&bytes)).into_owned();
    if text.len() <= LOG_CAP {
        return Ok((text, false));
    }
    // Redaction can lengthen a short `key=v`; cut the end.
    let marker = "\n[horch: log cut at 256 KiB]\n";
    let kept = cut_utf8(&text, LOG_CAP - marker.len());
    Ok((format!("{kept}{marker}"), true))
}

/// A raw log over the cap: its head and tail with a marker between them.
fn read_head_tail(file: &mut File, len: u64) -> anyhow::Result<(String, bool)> {
    let half = (LOG_CAP - 128) / 2;
    let mut head = vec![0u8; half];
    file.read_exact(&mut head)?;
    let mut tail = vec![0u8; half];
    file.seek(SeekFrom::End(-(half as i64)))?;
    file.read_exact(&mut tail)?;
    trim_to_lines(&mut head, &mut tail);
    let omitted = len - head.len() as u64 - tail.len() as u64;
    let mut text = redact(&String::from_utf8_lossy(&head)).into_owned();
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(&format!(
        "[horch: {omitted} bytes omitted from the middle of this log]\n"
    ));
    text.push_str(&redact(&String::from_utf8_lossy(&tail)));
    Ok((cut_utf8(&text, LOG_CAP).to_owned(), true))
}

/// Drop the partial line at the end of `head` and at the start of `tail`.
fn trim_to_lines(head: &mut Vec<u8>, tail: &mut Vec<u8>) {
    if let Some(nl) = head.iter().rposition(|&c| c == b'\n') {
        head.truncate(nl + 1);
    }
    if let Some(nl) = tail.iter().position(|&c| c == b'\n') {
        tail.drain(..=nl);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored(raw_text: &[u8]) -> (String, bool) {
        let dir = tempfile::tempdir().unwrap();
        let raw = dir.path().join("raw");
        let log = dir.path().join("log");
        std::fs::write(&raw, raw_text).unwrap();
        let (digest, truncated) = store_log(&raw, &log).unwrap();
        let bytes = std::fs::read(&log).unwrap();
        assert_eq!(digest, sha256_bytes(&bytes));
        (String::from_utf8(bytes).unwrap(), truncated)
    }

    #[test]
    fn small_logs_are_redacted_whole() {
        let (text, truncated) = stored(b"ok\nsk-ant-api03-abcdefghijklmnop\n");
        assert!(!truncated);
        assert_eq!(text, "ok\n[REDACTED]\n");
    }

    #[test]
    fn large_logs_keep_head_and_tail_under_the_cap() {
        let mut raw = Vec::new();
        raw.extend_from_slice(b"FIRST LINE\n");
        while raw.len() < 3 * LOG_CAP {
            raw.extend_from_slice(b"filler filler filler filler filler\n");
        }
        raw.extend_from_slice(b"token=hunter2\nLAST LINE\n");
        let (text, truncated) = stored(&raw);
        assert!(truncated);
        assert!(text.len() <= LOG_CAP, "{}", text.len());
        assert!(text.starts_with("FIRST LINE\n"));
        assert!(text.ends_with("token=[REDACTED]\nLAST LINE\n"));
        assert!(text.contains("bytes omitted from the middle of this log]\n"));
        assert!(!text.contains("hunter2"));
    }

    #[test]
    fn gate_names_become_safe_file_parts() {
        assert_eq!(file_part("cargo test"), "cargo_test");
        assert_eq!(file_part("../x"), "___x");
        assert!(!plain_name("../x"));
        assert!(plain_name("c-1"));
    }

    /// An exited leader is seen but not reaped: its pid stays taken, so its
    /// group id cannot go to another program before the group kill.
    #[cfg(unix)]
    #[test]
    fn an_exited_leader_stays_unreaped_until_the_group_kill() {
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        while !leader_exited(&mut child).unwrap() {
            assert!(Instant::now() < until, "true did not exit");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(fsx::pid_alive(child.id()), "the zombie keeps the pid");
        assert!(child.wait().unwrap().success(), "the exit status is kept");
        assert!(!fsx::pid_alive(child.id()), "reaped");
    }

    #[test]
    fn a_small_log_that_redaction_lengthens_is_cut_at_the_cap() {
        // 8 bytes become 18, so the raw log fits and the stored one does not.
        let raw = "token=x\n".repeat(LOG_CAP / 8);
        assert!(raw.len() <= LOG_CAP);
        let (text, truncated) = stored(raw.as_bytes());
        assert!(truncated);
        assert_eq!(text.len(), LOG_CAP);
        assert!(text.ends_with("\n[horch: log cut at 256 KiB]\n"));
        assert!(text.starts_with("token=[REDACTED]\n"));
    }

    #[test]
    fn a_large_log_is_cut_on_line_boundaries_at_both_ends() {
        // Every line is 12 bytes, so a cut inside a line shows as a short one.
        let raw: String = (0..60_000).map(|i| format!("line {i:06}\n")).collect();
        let (text, truncated) = stored(raw.as_bytes());
        assert!(truncated);
        let lines: Vec<&str> = text.lines().collect();
        let marker = lines
            .iter()
            .position(|l| l.starts_with("[horch: "))
            .expect("a marker line");
        assert!(marker > 0 && marker < lines.len() - 1);
        for line in lines.iter().enumerate().filter(|(i, _)| *i != marker) {
            assert!(
                line.1.len() == 11 && line.1.starts_with("line "),
                "partial line {:?}",
                line.1
            );
        }
        assert_eq!(lines[0], "line 000000");
        assert_eq!(lines[lines.len() - 1], "line 059999");
        let kept = (lines.len() - 1) as u64 * 12;
        assert!(
            lines[marker].contains(&format!("{} bytes omitted", raw.len() as u64 - kept)),
            "{}",
            lines[marker]
        );
    }

    fn gate(command: &str, timeout_ms: u64) -> GateSpec {
        GateSpec {
            name: "g".into(),
            command: command.into(),
            timeout: Duration::from_millis(timeout_ms),
            required: true,
        }
    }

    fn run_gate_status(spec: &GateSpec, worktree: &Path) -> (GateStatus, String) {
        let dir = tempfile::tempdir().unwrap();
        let raw = dir.path().join("raw");
        let v = CommandValidator::new(vec![], dir.path().to_path_buf(), BTreeSet::new());
        let status = v.spawn_and_wait(spec, worktree, &raw).unwrap();
        (status, std::fs::read_to_string(&raw).unwrap_or_default())
    }

    #[cfg(unix)]
    #[test]
    fn a_gate_ends_as_passed_failed_signalled_or_timed_out() {
        let wt = tempfile::tempdir().unwrap();
        let (status, out) = run_gate_status(&gate("echo out; echo err >&2", 5000), wt.path());
        assert_eq!(status, GateStatus::Passed);
        assert_eq!(out, "out\nerr\n");
        let (status, _) = run_gate_status(&gate("exit 3", 5000), wt.path());
        assert_eq!(status, GateStatus::Failed { code: 3 });
        // The shell kills itself with SIGKILL: reported as 128 + 9.
        let (status, _) = run_gate_status(&gate("kill -9 $$", 5000), wt.path());
        assert_eq!(status, GateStatus::Failed { code: 137 });
        let started = Instant::now();
        let (status, _) = run_gate_status(&gate("sleep 30", 150), wt.path());
        assert_eq!(status, GateStatus::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn a_gate_in_a_missing_worktree_is_an_error_status_not_a_failure() {
        let (status, _) = run_gate_status(&gate("true", 5000), Path::new("/nonexistent/horch-wt"));
        match status {
            GateStatus::Error { reason } => {
                assert!(reason.starts_with("cannot start sh"), "{reason}")
            }
            other => panic!("{other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn exit_status_maps_success_codes_and_signals() {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(exit_status(ExitStatus::from_raw(0)), GateStatus::Passed);
        assert_eq!(
            exit_status(ExitStatus::from_raw(5 << 8)),
            GateStatus::Failed { code: 5 }
        );
        assert_eq!(
            exit_status(ExitStatus::from_raw(15)),
            GateStatus::Failed { code: 143 }
        );
    }
}
