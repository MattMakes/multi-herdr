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
    /// Every gate passed.
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
        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg(&gate.command)
            .current_dir(worktree)
            .stdin(Stdio::null())
            .stdout(out)
            .stderr(err);
        for (k, v) in &self.env {
            cmd.env(k, v);
        }
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
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                return Ok(GateStatus::Error {
                    reason: format!("cannot start sh: {e}"),
                })
            }
        };
        let deadline = Instant::now() + gate.timeout;
        loop {
            if let Some(exit) = child.try_wait()? {
                // Stragglers the gate left in the background go too.
                kill_reaped_group(&mut child);
                return Ok(exit_status(exit));
            }
            if Instant::now() >= deadline {
                kill_group(&mut child);
                child.wait()?;
                return Ok(GateStatus::TimedOut);
            }
            std::thread::sleep(POLL);
        }
    }
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
        let mechanical_score = if gates.is_empty() {
            0.0
        } else {
            passed as f64 / gates.len() as f64
        };
        Ok(ValidationReport {
            validation_id: crate::ids::mint_v7(crate::clock::now()).to_string(),
            label: label.clone(),
            head_sha: candidate.head_sha.clone(),
            eligible: passed == gates.len(),
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

#[cfg(unix)]
fn kill_group(child: &mut Child) {
    // The group id is the child's pid (`process_group(0)`). The child is
    // not reaped yet, so its pid and group id are still its own. A group
    // that is already gone gives ESRCH, which is fine.
    unsafe {
        libc::killpg(child.id() as libc::pid_t, libc::SIGKILL);
    }
}

#[cfg(not(unix))]
fn kill_group(child: &mut Child) {
    let _ = child.kill();
}

/// Kill the group of a gate leader that `try_wait` has already reaped: the
/// stragglers it left in the background.
///
/// The leader's pid is free now, but POSIX gives no new process a pid while
/// a process group with that id exists. So while no process has the pid,
/// the group, if it has members, is still the gate's. A process that has
/// the pid is another program, and its group may be its own: no signal.
#[cfg(unix)]
fn kill_reaped_group(child: &mut Child) {
    if may_kill_reaped_group(child.id()) {
        kill_group(child);
    }
}

#[cfg(not(unix))]
fn kill_reaped_group(_child: &mut Child) {}

/// Whether the group of reaped leader `pid` is still the gate's.
#[cfg(unix)]
fn may_kill_reaped_group(pid: u32) -> bool {
    !fsx::pid_alive(pid)
}

fn exit_status(exit: ExitStatus) -> GateStatus {
    if exit.success() {
        return GateStatus::Passed;
    }
    if let Some(code) = exit.code() {
        return GateStatus::Failed { code };
    }
    #[cfg(unix)]
    if let Some(sig) = std::os::unix::process::ExitStatusExt::signal(&exit) {
        return GateStatus::Failed { code: 128 + sig };
    }
    GateStatus::Failed { code: -1 }
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
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let text = redact(&String::from_utf8_lossy(&bytes)).into_owned();
        if text.len() <= LOG_CAP {
            (text, false)
        } else {
            // Redaction can lengthen a short `key=v`; cut the end.
            let marker = "\n[horch: log cut at 256 KiB]\n";
            let kept = cut_utf8(&text, LOG_CAP - marker.len());
            (format!("{kept}{marker}"), true)
        }
    } else {
        let half = (LOG_CAP - 128) / 2;
        let mut head = vec![0u8; half];
        file.read_exact(&mut head)?;
        if let Some(nl) = head.iter().rposition(|&c| c == b'\n') {
            head.truncate(nl + 1);
        }
        let mut tail = vec![0u8; half];
        file.seek(SeekFrom::End(-(half as i64)))?;
        file.read_exact(&mut tail)?;
        if let Some(nl) = tail.iter().position(|&c| c == b'\n') {
            tail.drain(..=nl);
        }
        let omitted = len - head.len() as u64 - tail.len() as u64;
        let mut text = redact(&String::from_utf8_lossy(&head)).into_owned();
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&format!(
            "[horch: {omitted} bytes omitted from the middle of this log]\n"
        ));
        text.push_str(&redact(&String::from_utf8_lossy(&tail)));
        (cut_utf8(&text, LOG_CAP).to_owned(), true)
    };
    fsx::write_atomic(log, text.as_bytes(), fsx::PRIVATE_FILE)?;
    Ok((sha256_bytes(text.as_bytes()), truncated))
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

    /// After the reap, a process that has the leader's pid is another
    /// program (a reused pid): its group is not signalled. With no process
    /// at the pid, the group is still the gate's.
    #[cfg(unix)]
    #[test]
    fn a_reaped_leaders_pid_that_runs_again_is_not_the_gate() {
        let mut other = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        assert!(!may_kill_reaped_group(other.id()), "a running pid");
        other.kill().unwrap();
        other.wait().unwrap();
        assert!(may_kill_reaped_group(other.id()), "no process has the pid");
    }
}
