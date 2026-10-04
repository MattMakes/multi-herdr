//! The quota probes (design section 11.2): the only routing code that starts
//! a process. Each harness reports its own limits through its own client, so
//! horch never holds a credential (D2). [`crate::routing::snapshot::obtain`]
//! is the only routing path that calls them.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration as StdDuration, Instant};

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::clock;
use crate::routing::quota::{
    parse_claude_usage, parse_codex_limits, Probed, QuotaFile, Window, POOL_CLAUDE, POOL_CODEX,
    POOL_LOCAL, POOL_ZEN,
};

/// A child process whose stdout is read line by line with a deadline.
struct Child {
    child: std::process::Child,
    lines: mpsc::Receiver<String>,
}

impl Child {
    fn spawn(mut cmd: Command) -> std::io::Result<Child> {
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = cmd.spawn()?;
        let stdout = child.stdout.take().expect("piped stdout");
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Ok(Child { child, lines: rx })
    }

    fn send(&mut self, line: &str) -> std::io::Result<()> {
        let stdin = self.child.stdin.as_mut().expect("piped stdin");
        stdin.write_all(line.as_bytes())?;
        stdin.write_all(b"\n")?;
        stdin.flush()
    }

    /// The next stdout line that `pick` accepts, before `deadline`.
    fn wait_for(&self, deadline: Instant, mut pick: impl FnMut(&Value) -> bool) -> Option<Value> {
        loop {
            let left = deadline.checked_duration_since(Instant::now())?;
            let line = self.lines.recv_timeout(left).ok()?;
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                if pick(&v) {
                    return Some(v);
                }
            }
        }
    }
}

impl Drop for Child {
    fn drop(&mut self) {
        drop(self.child.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Ask Claude Code for its plan limits (QUO-01). One stdin line, a
/// `get_usage` control request; no user message; no `ANTHROPIC_API_KEY`.
/// The scratch dir goes under `temp_root`.
pub(crate) fn probe_claude(
    bin: &Path,
    timeout: StdDuration,
    temp_root: &Path,
) -> Result<Probed, String> {
    let dir = temp_root.join(format!("horch-quota-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let mut cmd = Command::new(bin);
    cmd.args([
        "-p",
        "--model",
        "haiku",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
    ])
    .current_dir(&dir);
    for key in crate::harness::launch::FORBIDDEN_ENV {
        cmd.env_remove(key);
    }
    let result = (|| {
        let mut child =
            Child::spawn(cmd).map_err(|e| format!("starting {}: {e}", bin.display()))?;
        let id = "horch-quota-1";
        let request = serde_json::json!({
            "type": "control_request",
            "request_id": id,
            "request": {"subtype": "get_usage", "skip_behaviors": true}
        });
        child
            .send(&request.to_string())
            .map_err(|e| format!("writing to {}: {e}", bin.display()))?;
        let reply = child
            .wait_for(Instant::now() + timeout, |v| {
                v.get("type").and_then(Value::as_str) == Some("control_response")
                    && v.pointer("/response/request_id").and_then(Value::as_str) == Some(id)
            })
            .ok_or_else(|| format!("no get_usage reply within {} ms", timeout.as_millis()))?;
        parse_claude_usage(&reply)
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// Ask Codex for its plan limits (QUO-02): `initialize`, `initialized`,
/// `account/rateLimits/read`, and nothing else.
pub(crate) fn probe_codex(bin: &Path, timeout: StdDuration) -> Result<Probed, String> {
    let mut cmd = Command::new(bin);
    cmd.arg("app-server");
    for key in crate::harness::launch::FORBIDDEN_ENV {
        cmd.env_remove(key);
    }
    let mut child = Child::spawn(cmd).map_err(|e| format!("starting {}: {e}", bin.display()))?;
    let deadline = Instant::now() + timeout;
    let send = |child: &mut Child, v: Value| {
        child
            .send(&v.to_string())
            .map_err(|e| format!("writing to {} app-server: {e}", bin.display()))
    };
    send(
        &mut child,
        serde_json::json!({"id": 1, "method": "initialize", "params": {
            "clientInfo": {"name": "horch", "version": env!("CARGO_PKG_VERSION")}}}),
    )?;
    child
        .wait_for(deadline, |v| v.get("id") == Some(&Value::from(1)))
        .ok_or("no initialize reply")?;
    send(&mut child, serde_json::json!({"method": "initialized"}))?;
    send(
        &mut child,
        serde_json::json!({"id": 2, "method": "account/rateLimits/read", "params": null}),
    )?;
    let reply = child
        .wait_for(deadline, |v| v.get("id") == Some(&Value::from(2)))
        .ok_or_else(|| {
            format!(
                "no account/rateLimits/read reply within {} ms",
                timeout.as_millis()
            )
        })?;
    parse_codex_limits(&reply)
}

/// Run a short command with a deadline; its stdout, or why it failed.
fn run_short(bin: &Path, args: &[&str], timeout: StdDuration) -> Result<String, String> {
    let name = format!(
        "{} {}",
        bin.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        args.join(" ")
    );
    let mut cmd = Command::new(bin);
    for key in crate::harness::launch::FORBIDDEN_ENV {
        cmd.env_remove(key);
    }
    let mut child = cmd
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{name} could not start: {e}"))?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut out = String::new();
                if let Some(mut s) = child.stdout.take() {
                    let _ = std::io::Read::read_to_string(&mut s, &mut out);
                }
                return if status.success() {
                    Ok(out)
                } else {
                    Err(format!(
                        "{name} exited {}",
                        status
                            .code()
                            .map(|c| c.to_string())
                            .unwrap_or_else(|| "on a signal".into())
                    ))
                };
            }
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(StdDuration::from_millis(20))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{name} timed out after {}s", timeout.as_secs()));
            }
        }
    }
}

/// The local pool's health (QUO-06): `pi --version`, then `ollama list`.
pub(crate) fn probe_local(pi: &Path, ollama: &Path) -> Result<Vec<String>, String> {
    let t = StdDuration::from_secs(5);
    run_short(pi, &["--version"], t)?;
    let list = run_short(ollama, &["list"], t)?;
    Ok(list
        .lines()
        .skip(1)
        .filter_map(|l| l.split_whitespace().next())
        .map(str::to_owned)
        .collect())
}

/// The first line a harness prints for `--version`.
pub fn harness_version(bin: &Path) -> Option<String> {
    let mut cmd = Command::new(bin);
    for key in crate::harness::launch::FORBIDDEN_ENV {
        cmd.env_remove(key);
    }
    let mut child = cmd
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    // Generous: a loaded machine can take seconds just to start a binary,
    // and a missed version fails the dataset preflight.
    let deadline = Instant::now() + VERSION_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(StdDuration::from_millis(20))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    if !status.success() {
        return None;
    }
    let read = |pipe: Option<&mut dyn std::io::Read>| {
        let mut text = String::new();
        if let Some(p) = pipe {
            let _ = p.read_to_string(&mut text);
        }
        text
    };
    let out = read(child.stdout.as_mut().map(|p| p as &mut dyn std::io::Read));
    let err = read(child.stderr.as_mut().map(|p| p as &mut dyn std::io::Read));
    // Some harnesses (Prime) print their version on stderr.
    [out, err].iter().find_map(|t| {
        t.lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .map(str::to_string)
    })
}

/// How long `harness_version` waits for `<bin> --version`.
const VERSION_TIMEOUT: StdDuration = StdDuration::from_secs(15);

/// The newest `rate_limits` snapshot in any rollout under `sessions` (the
/// Codex fallback, QUO-03), with the time it was written.
pub(crate) fn newest_rollout_snapshot(sessions: &Path) -> Option<(String, Vec<Window>)> {
    let mut files = Vec::new();
    collect_rollouts(sessions, &mut files);
    files.sort_by_key(|(t, _)| std::cmp::Reverse(*t));
    for (_, path) in files.into_iter().take(20) {
        let text = std::fs::read_to_string(&path).ok()?;
        let found = text.lines().rev().find_map(|line| {
            let v: Value = serde_json::from_str(line).ok()?;
            let rl = v
                .pointer("/payload/rate_limits")
                .filter(|r| r.is_object())?;
            let windows = crate::telemetry::readers::codex_rollout_windows(rl);
            let at = v
                .get("timestamp")
                .and_then(Value::as_str)
                .and_then(clock::parse)?;
            (!windows.is_empty()).then(|| (clock::stamp(at), windows))
        });
        if found.is_some() {
            return found;
        }
    }
    None
}

fn collect_rollouts(dir: &Path, out: &mut Vec<(std::time::SystemTime, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_rollouts(&p, out);
        } else if p
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("rollout-"))
        {
            if let Ok(t) = e.metadata().and_then(|m| m.modified()) {
                out.push((t, p));
            }
        }
    }
}

/// Where probes come from. Tests pass fakes through the usual `HORCH_*_BIN`,
/// which the binary reads into `RuntimeContext::bins`.
#[derive(Debug, Clone)]
pub struct ProbeBins {
    pub claude: PathBuf,
    pub codex: PathBuf,
    pub pi: PathBuf,
    pub ollama: PathBuf,
    pub codex_sessions: PathBuf,
}

impl ProbeBins {
    /// The probe programs from resolved [`HarnessBins`], and the codex
    /// sessions directory the fallback signal reads.
    ///
    /// [`HarnessBins`]: crate::runtime::HarnessBins
    pub fn new(bins: &crate::runtime::HarnessBins, codex_sessions: PathBuf) -> Self {
        ProbeBins {
            claude: bins.claude.clone(),
            codex: bins.codex.clone(),
            pi: bins.pi.clone(),
            ollama: bins.ollama.clone(),
            codex_sessions,
        }
    }

    /// The context's programs and codex sessions directory.
    pub fn from_context(ctx: &crate::runtime::RuntimeContext) -> Self {
        Self::new(
            &ctx.bins.harness,
            crate::usage::Locations::from_context(ctx).codex_sessions,
        )
    }
}

/// 10 s (section 11.2). `HORCH_PROBE_TIMEOUT_MS` shortens it for tests.
pub(crate) const PROBE_TIMEOUT: StdDuration = StdDuration::from_secs(10);

/// Probe every pool and fold the results into `file` (QUO-01..03, 06).
/// A failed probe keeps the last windows, records the error, and falls back
/// to the newest on-disk signal. `timeout` is `None` for [`PROBE_TIMEOUT`];
/// the claude probe's scratch dir goes under `temp_root`.
pub(crate) fn probe_all(
    file: &mut QuotaFile,
    bins: &ProbeBins,
    now: DateTime<Utc>,
    timeout: Option<StdDuration>,
    temp_root: &Path,
) {
    let stamp = clock::stamp(now);
    let timeout = timeout.unwrap_or(PROBE_TIMEOUT);

    let claude = file.pools.entry(POOL_CLAUDE.into()).or_default();
    claude.probed_at = Some(stamp.clone());
    claude.harness_version = harness_version(&bins.claude).or(claude.harness_version.take());
    match probe_claude(&bins.claude, timeout, temp_root) {
        Ok(p) => {
            claude.windows = p.windows;
            claude.observed_at = Some(stamp.clone());
            claude.source = Some("get_usage".into());
            claude.error = None;
        }
        Err(e) => claude.error = Some(e),
    }

    let codex = file.pools.entry(POOL_CODEX.into()).or_default();
    codex.probed_at = Some(stamp.clone());
    codex.harness_version = harness_version(&bins.codex).or(codex.harness_version.take());
    match probe_codex(&bins.codex, timeout) {
        Ok(p) => {
            codex.windows = p.windows;
            codex.ordinary_usage_allowed = p.ordinary_usage_allowed;
            codex.observed_at = Some(stamp.clone());
            codex.source = Some("app-server".into());
            codex.error = None;
        }
        Err(e) => {
            codex.error = Some(e);
            if let Some((at, windows)) = newest_rollout_snapshot(&bins.codex_sessions) {
                if codex
                    .observed_at
                    .as_deref()
                    .is_none_or(|prev| at.as_str() > prev)
                {
                    codex.windows = windows;
                    codex.observed_at = Some(at);
                    codex.source = Some("rollout".into());
                }
            }
        }
    }

    let zen = file.pools.entry(POOL_ZEN.into()).or_default();
    zen.observed_at = Some(stamp.clone());
    if zen.cooling_until.is_none() {
        zen.no_signal = true;
        zen.source = Some("none".into());
    }

    let local = file.pools.entry(POOL_LOCAL.into()).or_default();
    local.probed_at = Some(stamp.clone());
    local.observed_at = Some(stamp);
    local.source = Some("health".into());
    match probe_local(&bins.pi, &bins.ollama) {
        Ok(models) => {
            local.models = Some(models);
            local.error = None;
            local.state = "ok".into();
        }
        Err(e) => {
            local.error = Some(e);
            local.state = "broken".into();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(rel: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/telemetry")
            .join(rel)
    }

    #[test]
    fn quo_03_rollout_snapshot_is_the_codex_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        let from = fixture("codex/sessions");
        copy(&from, &tmp.path().join("sessions"));
        let (at, windows) = newest_rollout_snapshot(&tmp.path().join("sessions")).unwrap();
        assert!(!at.is_empty());
        assert!(windows.iter().any(|w| w.name == "7d"));
    }

    fn copy(from: &Path, to: &Path) {
        if from.is_dir() {
            std::fs::create_dir_all(to).unwrap();
            for e in std::fs::read_dir(from).unwrap().flatten() {
                copy(&e.path(), &to.join(e.file_name()));
            }
        } else {
            std::fs::copy(from, to).unwrap();
        }
    }
}
