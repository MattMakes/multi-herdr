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
    parse_claude_usage, parse_codex_limits, HarnessHealth, Probed, QuotaFile, Window, POOL_CLAUDE,
    POOL_CODEX, POOL_LOCAL, POOL_ZEN,
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
    cmd.args(args);
    match run_with_deadline(cmd, timeout) {
        Err(e) => Err(format!("{name} could not start: {e}")),
        Ok(None) => Err(format!("{name} timed out after {}s", timeout.as_secs())),
        Ok(Some((status, out, _))) if status.success() => Ok(out),
        Ok(Some((status, out, err))) => Err(failure(&name, status, &err, &out)),
    }
}

/// Run `cmd` with no stdin and a deadline: its exit status, stdout and
/// stderr, or `None` when it did not exit in time (it is killed then).
///
/// Threads read both pipes while the child runs. A child that writes more
/// than the pipe buffer (64 KiB on macOS) blocks until someone reads, so
/// reading only after the exit would wait for the whole deadline (U-65).
fn run_with_deadline(
    mut cmd: Command,
    timeout: StdDuration,
) -> std::io::Result<Option<(std::process::ExitStatus, String, String)>> {
    crate::runtime::process::scrub_child_env(&mut cmd);
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let out = read_on_thread(child.stdout.take());
    let err = read_on_thread(child.stderr.take());
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let text = |r: std::thread::JoinHandle<String>| r.join().unwrap_or_default();
                return Ok(Some((status, text(out), text(err))));
            }
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(StdDuration::from_millis(20))
            }
            // The readers are not joined: a process the child started can
            // still hold the pipes open.
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Ok(None);
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

/// What `<bin> --version` showed about a harness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionProbe {
    /// It exited 0. The first line it printed.
    Version(String),
    /// It ran and failed: it exited non-zero or on a signal. Why, with the
    /// first error line it printed (`pi --version exited 1: Error
    /// [ERR_REQUIRE_ESM]: ...`).
    Broken(String),
    /// It did not start (not installed), timed out, or printed nothing.
    NoAnswer,
}

impl VersionProbe {
    pub fn version(self) -> Option<String> {
        match self {
            VersionProbe::Version(v) => Some(v),
            _ => None,
        }
    }

    pub fn broken(&self) -> Option<&str> {
        match self {
            VersionProbe::Broken(e) => Some(e),
            _ => None,
        }
    }
}

/// The first line a harness prints for `--version`. `None` when it is
/// broken or gives no answer; [`probe_version`] tells those apart.
pub fn harness_version(bin: &Path) -> Option<String> {
    probe_version(bin).version()
}

/// Run `<bin> --version` and judge the answer.
pub fn probe_version(bin: &Path) -> VersionProbe {
    probe_version_within(bin, VERSION_TIMEOUT)
}

/// [`probe_version`] with its own deadline: no answer within `timeout` is
/// [`VersionProbe::NoAnswer`].
fn probe_version_within(bin: &Path, timeout: StdDuration) -> VersionProbe {
    let mut cmd = Command::new(bin);
    cmd.arg("--version");
    let Ok(Some((status, out, err))) = run_with_deadline(cmd, timeout) else {
        return VersionProbe::NoAnswer;
    };
    if !status.success() {
        let name = bin
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        return VersionProbe::Broken(failure(&format!("{name} --version"), status, &err, &out));
    }
    // Some harnesses (Prime) print their version on stderr.
    [out, err]
        .iter()
        .find_map(|t| {
            t.lines()
                .map(str::trim)
                .find(|l| !l.is_empty())
                .map(str::to_string)
        })
        .map_or(VersionProbe::NoAnswer, VersionProbe::Version)
}

/// Read `pipe` to its end on a new thread. No pipe reads as empty text.
fn read_on_thread(
    pipe: Option<impl std::io::Read + Send + 'static>,
) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut p) = pipe {
            let _ = p.read_to_string(&mut text);
        }
        text
    })
}

/// `<name> exited <code>: <first error line>` for a failed command.
fn failure(name: &str, status: std::process::ExitStatus, err: &str, out: &str) -> String {
    let code = status
        .code()
        .map(|c| c.to_string())
        .unwrap_or_else(|| "on a signal".into());
    match first_error_line(err).or_else(|| first_error_line(out)) {
        Some(line) => format!("{name} exited {code}: {line}"),
        None => format!("{name} exited {code}"),
    }
}

/// The line of `text` that names the error: the first whose first word is
/// an error kind (`Error`, `TypeError:`, `error:`), else the first that is
/// not empty. A Node crash starts with a source location
/// (`node:internal/modules/cjs/loader:1669`); the `Error [...]` line under
/// it says what went wrong. Display only: nothing branches on it. Cut to
/// 200 chars.
fn first_error_line(text: &str) -> Option<String> {
    let lines = || text.lines().map(str::trim).filter(|l| !l.is_empty());
    let line = lines()
        .find(|l| {
            let word = l.split([' ', ':', '[']).next().unwrap_or_default();
            word.to_ascii_lowercase().ends_with("error")
        })
        .or_else(|| lines().next())?;
    const MAX: usize = 200;
    Some(match line.char_indices().nth(MAX) {
        Some((at, _)) => format!("{}...", &line[..at]),
        None => line.to_string(),
    })
}

/// How long `harness_version` waits for `<bin> --version`.
/// Generous: a loaded machine can take seconds just to start a binary, and
/// a missed version fails the dataset preflight.
const VERSION_TIMEOUT: StdDuration = StdDuration::from_secs(15);

/// The newest `rate_limits` snapshot in any rollout under `sessions` (the
/// Codex fallback, QUO-03), with the time it was written.
pub(crate) fn newest_rollout_snapshot(sessions: &Path) -> Option<(String, Vec<Window>)> {
    let mut files = Vec::new();
    collect_rollouts(sessions, &mut files);
    files.sort_by_key(|(t, _)| std::cmp::Reverse(*t));
    for (_, path) in files.into_iter().take(20) {
        // An unreadable file, or a cut multi-byte tail, skips that file only.
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
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
    /// Every harness binary by harness name, for the `--version` health
    /// check.
    pub harnesses: Vec<(&'static str, PathBuf)>,
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
            harnesses: crate::harness::HarnessKind::ALL
                .iter()
                .filter_map(|k| Some((k.as_str(), k.binary(bins)?)))
                .collect(),
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

    probe_harnesses(file, &bins.harnesses, &stamp, VERSION_TIMEOUT);
    let version_of = |name: &str| file.harnesses.get(name).and_then(|h| h.version.clone());
    let (claude_version, codex_version) = (version_of("claude"), version_of("codex"));

    let claude = file.pools.entry(POOL_CLAUDE.into()).or_default();
    claude.probed_at = Some(stamp.clone());
    claude.harness_version = claude_version.or(claude.harness_version.take());
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
    codex.harness_version = codex_version.or(codex.harness_version.take());
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

    // The google pool (agy) has no probe (U-61): agy 1.2.17 writes no local
    // quota or usage signal and has no usage subcommand, so the pool stays
    // `unknown`. Seen live: docs/live-checks/harnesses.md.

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

/// Run every harness's `--version` in parallel and record the health of
/// each in `file.harnesses`. A failure marks the harness broken; a later
/// success clears it. No answer (not installed, timed out) is not a
/// failure: it keeps the last version and clears the error.
fn probe_harnesses(
    file: &mut QuotaFile,
    bins: &[(&'static str, PathBuf)],
    stamp: &str,
    timeout: StdDuration,
) {
    let probes: Vec<(&'static str, VersionProbe)> = std::thread::scope(|s| {
        let handles: Vec<_> = bins
            .iter()
            .map(|(name, bin)| (*name, s.spawn(move || probe_version_within(bin, timeout))))
            .collect();
        handles
            .into_iter()
            .map(|(name, h)| (name, h.join().unwrap_or(VersionProbe::NoAnswer)))
            .collect()
    });
    for (name, probe) in probes {
        if probe == VersionProbe::NoAnswer && !file.harnesses.contains_key(name) {
            continue;
        }
        let h: &mut HarnessHealth = file.harnesses.entry(name.to_string()).or_default();
        h.probed_at = Some(stamp.to_string());
        h.error = probe.broken().map(str::to_owned);
        if let VersionProbe::Version(v) = probe {
            h.version = Some(v);
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

    /// Writes a rollout whose mtime is `age_secs` in the past.
    fn rollout(dir: &Path, name: &str, body: &[u8], age_secs: u64) {
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, body).unwrap();
        let at = std::time::SystemTime::now() - StdDuration::from_secs(age_secs);
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(at)
            .unwrap();
    }

    /// Codex 0.160: `primary` is the weekly window, `secondary` is null, and
    /// there is no 5-hour window. A newer rollout from a `codex exec` probe
    /// holds `rate_limits: null`, and the newest one is cut mid-character.
    /// The snapshot comes from the next newest rollout that has a window.
    #[test]
    fn quo_03_rollout_snapshot_skips_null_rate_limits_on_codex_0_160() {
        let tmp = tempfile::tempdir().unwrap();
        let day = tmp.path().join("sessions/2026/10/06");
        let weekly = concat!(
            r#"{"timestamp":"2026-10-07T03:51:41.160Z","type":"event_msg","payload":{"type":"token_count","info":null,"#,
            r#""rate_limits":{"limit_id":"codex","limit_name":null,"primary":{"used_percent":43.0,"window_minutes":10080,"resets_at":1791595230},"#,
            r#""secondary":null,"credits":{"has_credits":false,"unlimited":false,"balance":"0"}}}}"#,
            "\n",
            r#"{"timestamp":"2026-10-07T03:51:42.000Z","type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":null}}"#,
            "\n"
        );
        let null_only = r#"{"timestamp":"2026-10-07T03:52:00.000Z","type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":null}}"#;
        rollout(&day, "rollout-a-weekly.jsonl", weekly.as_bytes(), 300);
        rollout(&day, "rollout-b-probe.jsonl", null_only.as_bytes(), 200);
        let mut cut = null_only.as_bytes().to_vec();
        cut.extend_from_slice(&[b'\n', 0xE2, 0x82]);
        rollout(&day, "rollout-c-cut.jsonl", &cut, 100);
        let (at, windows) = newest_rollout_snapshot(&tmp.path().join("sessions")).unwrap();
        assert_eq!(at, "2026-10-07T03:51:41Z");
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].name, "7d");
        assert!((windows[0].used - 0.43).abs() < 1e-9);
    }

    /// Only null snapshots: no answer, not an error.
    #[test]
    fn quo_03_rollout_snapshot_is_none_when_every_rollout_is_null() {
        let tmp = tempfile::tempdir().unwrap();
        let null_only = r#"{"timestamp":"2026-10-07T03:52:00.000Z","type":"event_msg","payload":{"type":"token_count","rate_limits":null}}"#;
        rollout(
            &tmp.path().join("sessions"),
            "rollout-x.jsonl",
            null_only.as_bytes(),
            10,
        );
        assert!(newest_rollout_snapshot(&tmp.path().join("sessions")).is_none());
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

    /// A Node crash starts with a source location; the `Error [...]` line
    /// names the fault.
    #[test]
    fn first_error_line_skips_the_node_source_location() {
        let crash = "node:internal/modules/cjs/loader:1669\n      const err = new ERR_REQUIRE_ESM(filename,\n\nError [ERR_REQUIRE_ESM]: require() of ES Module x.js not supported.\n";
        assert_eq!(
            first_error_line(crash).as_deref(),
            Some("Error [ERR_REQUIRE_ESM]: require() of ES Module x.js not supported.")
        );
        assert_eq!(
            first_error_line("\n  plain failure\n").as_deref(),
            Some("plain failure")
        );
        assert_eq!(first_error_line(" \n"), None);
        assert_eq!(
            first_error_line("at errorHandler (x.js:1)\nTypeError: x is undefined").as_deref(),
            Some("TypeError: x is undefined")
        );
        let long = format!("Error: {}", "x".repeat(300));
        assert_eq!(first_error_line(&long).unwrap().chars().count(), 203);
    }

    /// A fake harness that crashes is broken with its error line; one that is
    /// missing gives no answer; once it works again the error clears.
    ///
    /// The fake always exits, so the probes wait for it. With the 15 s
    /// [`VERSION_TIMEOUT`], a fake starved by a loaded test run gives
    /// `NoAnswer`, and the test fails.
    #[cfg(unix)]
    #[test]
    fn a_crashing_harness_is_broken_until_it_recovers() {
        const UNTIL_IT_EXITS: StdDuration = StdDuration::from_secs(600);
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let fixed = tmp.path().join("fixed");
        let bin = tmp.path().join("pi");
        std::fs::write(
            &bin,
            format!(
                "#!/bin/sh\nif [ -e {} ]; then echo 0.70.0; exit 0; fi\n\
                 echo 'node:internal/modules/cjs/loader:1669' >&2\n\
                 echo 'Error [ERR_REQUIRE_ESM]: require() of ES Module' >&2\nexit 1\n",
                fixed.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();

        let broken = "pi --version exited 1: Error [ERR_REQUIRE_ESM]: require() of ES Module";
        assert_eq!(
            probe_version_within(&bin, UNTIL_IT_EXITS),
            VersionProbe::Broken(broken.into())
        );
        // `None` whether it is broken or (on a starved run) gives no answer.
        assert_eq!(harness_version(&bin), None);
        let missing = tmp.path().join("absent");
        assert_eq!(probe_version(&missing), VersionProbe::NoAnswer);

        let bins = vec![("pi", bin.clone()), ("antigravity", missing)];
        let mut file = QuotaFile::default();
        probe_harnesses(&mut file, &bins, "2026-10-04T10:00:00Z", UNTIL_IT_EXITS);
        assert_eq!(file.harnesses["pi"].error.as_deref(), Some(broken));
        assert!(
            !file.harnesses.contains_key("antigravity"),
            "missing is not broken"
        );

        std::fs::write(&fixed, "").unwrap();
        probe_harnesses(&mut file, &bins, "2026-10-04T10:05:00Z", UNTIL_IT_EXITS);
        let pi = &file.harnesses["pi"];
        assert_eq!(pi.error, None);
        assert_eq!(pi.version.as_deref(), Some("0.70.0"));
        assert_eq!(pi.probed_at.as_deref(), Some("2026-10-04T10:05:00Z"));
    }

    /// U-65: a harness that prints more than the pipe buffer (64 KiB on
    /// macOS) before it exits. The probe reads its pipes while it runs, so
    /// the harness exits and the probe returns its output, not a timeout.
    #[cfg(unix)]
    #[test]
    fn a_harness_that_prints_200_kib_is_read_before_the_timeout() {
        const SIZE: usize = 200 * 1024;
        let timeout = StdDuration::from_secs(20);
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("chatty");
        std::fs::write(
            &bin,
            format!(
                "#!/bin/sh\necho 1.2.3\nhead -c {SIZE} /dev/zero | tr '\\0' x\n\
                 head -c {SIZE} /dev/zero | tr '\\0' y >&2\nexit 0\n"
            ),
        )
        .unwrap();
        crate::runtime::process::make_executable(&bin).unwrap();

        let started = Instant::now();
        let out = run_short(&bin, &[], timeout).unwrap();
        assert_eq!(out.len(), "1.2.3\n".len() + SIZE);
        assert_eq!(
            probe_version_within(&bin, timeout),
            VersionProbe::Version("1.2.3".into())
        );
        assert!(started.elapsed() < timeout, "{:?}", started.elapsed());
    }
}
