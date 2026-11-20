//! Typed wrapper around `hc` (`herdr-core`) and `herdr-app`.
//!
//! Verified against `hc 0.1.21` and `herdr-app 0.1.16` (see
//! `docs/live-checks/hc-backend.md`).

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;

use crate::runtime::{Backend, RuntimeContext};
use crate::workspace::exec::output_within;
use crate::workspace::host::Unsupported;
use crate::workspace::model::Pane;

/// An `hc` command that exited non-zero (or reported a finished terminal on a
/// live-only lookup), decoded from `hc`'s `{"error":{"kind":...,"message":...}}`
/// stderr envelope so callers branch on `kind` rather than message text
/// (ARC-22).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HcFailed {
    pub command: String,
    pub kind: String,
    pub message: String,
}

impl HcFailed {
    /// True when the target terminal no longer exists as a live pane.
    pub fn is_gone(&self) -> bool {
        matches!(self.kind.as_str(), "not_found" | "finished")
    }

    fn from_stderr(command: String, stderr: &[u8]) -> Self {
        #[derive(Deserialize)]
        struct ErrEnvelope {
            error: ErrBody,
        }
        #[derive(Deserialize)]
        struct ErrBody {
            kind: Option<String>,
            message: Option<String>,
        }
        let raw = String::from_utf8_lossy(stderr).trim().to_string();
        if let Ok(parsed) = serde_json::from_str::<ErrEnvelope>(&raw) {
            return Self {
                command,
                kind: parsed.error.kind.unwrap_or_else(|| "unknown".into()),
                message: parsed.error.message.unwrap_or(raw),
            };
        }
        Self {
            command,
            kind: "unknown".into(),
            message: raw,
        }
    }
}

impl fmt::Display for HcFailed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "hc {} failed ({}): {}",
            self.command, self.kind, self.message
        )
    }
}

impl std::error::Error for HcFailed {}

#[derive(Debug, Deserialize)]
struct LsResponse {
    terminals: Vec<LsRow>,
}

#[derive(Debug, Deserialize)]
struct LsRow {
    address: String,
}

#[derive(Debug, Deserialize)]
struct TerminalStatus {
    app: Option<String>,
    state: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TerminalRecord {
    address: String,
    state: String,
    #[serde(default)]
    labels: BTreeMap<String, String>,
    #[serde(default)]
    status: Option<TerminalStatus>,
    #[serde(default)]
    status_ms: Option<i64>,
}

impl TerminalRecord {
    fn into_pane(self, command: &str) -> Result<Pane, HcFailed> {
        if self.state == "finished" {
            return Err(HcFailed {
                command: command.to_string(),
                kind: "finished".into(),
                message: format!("terminal {} has finished", self.address),
            });
        }
        Ok(Pane {
            pane_id: self.address,
            workspace_id: self.labels.get("herdr.space").cloned(),
            tab_id: self.labels.get("herdr.tab").cloned(),
            agent_session: None,
            agent: self.status.as_ref().and_then(|s| s.app.clone()),
            agent_status: self.status.and_then(|s| s.state),
            state_change_seq: self.status_ms,
        })
    }
}

#[derive(Debug, Deserialize)]
struct WaitResponse {
    reason: String,
}

fn missing_hc(err: std::io::Error) -> anyhow::Error {
    if err.kind() == std::io::ErrorKind::NotFound {
        anyhow!("hc CLI not found on PATH; open the Herdr app or install herdr-core")
    } else {
        anyhow!(err)
    }
}

/// Handle to `hc` and `herdr-app`, with socket paths resolved from
/// [`RuntimeContext`].
#[derive(Debug, Clone)]
pub struct HcApp {
    hc: PathBuf,
    app: PathBuf,
    core_socket: PathBuf,
    app_home: Option<PathBuf>,
    timeout: Option<Duration>,
    deadline: Option<Instant>,
}

impl HcApp {
    /// Build an [`HcApp`] handle from [`RuntimeContext`] without reading the
    /// ambient environment (ARC-05).
    pub fn from_context(ctx: &RuntimeContext) -> Self {
        let core_socket = ctx.inherited.hc_terminal_socket.clone().unwrap_or_else(|| {
            ctx.paths
                .default_state_root
                .parent()
                .unwrap_or(ctx.paths.home.as_path())
                .join("herdr-core")
                .join("default")
                .join("default.sock")
        });
        Self {
            hc: ctx.bins.harness.hc.clone(),
            app: ctx.bins.harness.herdr_app.clone(),
            core_socket,
            app_home: ctx.inherited.herdr_app_home.clone(),
            timeout: None,
            deadline: None,
        }
    }

    /// Construct directly with explicit binary and core-socket paths.
    pub fn new(
        hc: impl Into<PathBuf>,
        app: impl Into<PathBuf>,
        core_socket: impl Into<PathBuf>,
    ) -> Self {
        Self {
            hc: hc.into(),
            app: app.into(),
            core_socket: core_socket.into(),
            app_home: None,
            timeout: None,
            deadline: None,
        }
    }

    pub fn app_bin(&self) -> &Path {
        &self.app
    }

    pub fn app_home(&self) -> Option<&Path> {
        self.app_home.as_deref()
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn until(mut self, deadline: Option<Instant>) -> Self {
        self.deadline = deadline;
        self
    }

    /// Fix a single deadline across a multi-child operation (`pane_list`).
    fn scoped(&self) -> Self {
        let deadline = match (self.timeout, self.deadline) {
            (Some(t), Some(d)) => Some(Instant::now().checked_add(t).map_or(d, |te| d.min(te))),
            (Some(t), None) => Instant::now().checked_add(t),
            (None, d) => d,
        };
        Self {
            timeout: None,
            deadline,
            ..self.clone()
        }
    }

    fn hc_command<S: AsRef<OsStr>>(&self, args: &[S]) -> Command {
        let mut command = Command::new(&self.hc);
        command.env("HC_TERMINAL_SOCKET", &self.core_socket);
        command.args(args);
        command
    }

    fn run_hc<S: AsRef<OsStr>>(&self, args: &[S]) -> std::io::Result<Output> {
        let mut command = self.hc_command(args);
        match (self.timeout, self.deadline) {
            (None, None) => command.output(),
            (Some(timeout), deadline) => output_within(command, timeout, deadline),
            (None, Some(deadline)) => output_within(
                command,
                deadline.saturating_duration_since(Instant::now()),
                Some(deadline),
            ),
        }
    }

    fn hc_output<S: AsRef<OsStr>>(&self, args: &[S]) -> Result<String> {
        let out = self.run_hc(args).map_err(missing_hc)?;
        if !out.status.success() {
            let rendered = args
                .iter()
                .map(|a| a.as_ref().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join(" ");
            return Err(HcFailed::from_stderr(rendered, &out.stderr).into());
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn hc_json<T: for<'de> Deserialize<'de>, S: AsRef<OsStr>>(&self, args: &[S]) -> Result<T> {
        let stdout = self.hc_output(args)?;
        serde_json::from_str(stdout.trim())
            .with_context(|| format!("could not parse hc response: {}", stdout.trim()))
    }

    /// True when the `herdr-core` socket exists and `hc server status` exits 0.
    /// Checking the socket first guarantees this probe never starts a core.
    pub fn server_reachable(&self) -> bool {
        if !self.core_socket.exists() {
            return false;
        }
        self.hc_command(&["server", "status"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    pub fn pane_get(&self, address: &str) -> Result<Pane> {
        let rec: TerminalRecord = self.hc_json(&["get", "--json", address])?;
        rec.into_pane(&format!("get --json {address}"))
            .map_err(Into::into)
    }

    pub fn pane_exists(&self, address: &str) -> bool {
        self.pane_get(address).is_ok()
    }

    pub fn pane_list(&self, workspace_id: &str) -> Result<Vec<Pane>> {
        let scoped = self.scoped();
        let label = format!("herdr.space={workspace_id}");
        let listed: LsResponse = scoped.hc_json(&["ls", "--json", "--label", &label])?;
        let mut panes = Vec::with_capacity(listed.terminals.len());
        for row in listed.terminals {
            match scoped.pane_get(&row.address) {
                Ok(pane) => panes.push(pane),
                Err(e) if e.downcast_ref::<HcFailed>().is_some_and(HcFailed::is_gone) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(panes)
    }

    pub fn pane_read(&self, address: &str, source: &str) -> Result<String> {
        match source {
            "visible" => self.hc_output(&["read", address]),
            "recent" | "recent-unwrapped" => self.hc_output(&["read", address, "--lines", "200"]),
            other => bail!("unknown pane read source '{other}'"),
        }
    }

    pub fn wait_output(&self, address: &str, needle: &str, timeout_ms: u64) -> Result<bool> {
        let secs = timeout_ms.div_ceil(1000).to_string();
        let args = [
            "wait",
            "--json",
            address,
            "--output",
            needle,
            "--timeout",
            &secs,
        ];
        let out = self.hc_command(&args).output().map_err(missing_hc)?;
        if !out.status.success() {
            return Err(HcFailed::from_stderr(args.join(" "), &out.stderr).into());
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        let resp: WaitResponse = serde_json::from_str(stdout.trim())
            .with_context(|| format!("could not parse hc wait response: {}", stdout.trim()))?;
        Ok(resp.reason == "output")
    }

    pub fn agent_prompt(&self, address: &str, text: &str) -> Result<()> {
        self.hc_output(&["send", "--json", address, text])?;
        Ok(())
    }

    pub fn pane_run(&self, address: &str, command: &str) -> Result<()> {
        self.hc_output(&["send", "--json", address, command])?;
        Ok(())
    }

    pub fn pane_send_keys(&self, address: &str, keys: &str) -> Result<()> {
        let words: Vec<&str> = keys.split_whitespace().collect();
        if words.is_empty() {
            bail!("no keys given");
        }
        let mut args = Vec::with_capacity(3 + words.len());
        args.extend(["keys", "--json", address]);
        args.extend(words);
        self.hc_output(&args)?;
        Ok(())
    }

    pub fn pane_send_text(&self, _address: &str, _text: &str) -> Result<()> {
        Err(Unsupported {
            call: "pane_send_text",
            backend: Backend::Hc,
        }
        .into())
    }

    pub fn pane_close(&self, address: &str) -> Result<()> {
        let scoped = self.scoped();
        match scoped.hc_output(&["kill", "--json", address]) {
            Ok(_) => {}
            Err(e)
                if e.downcast_ref::<HcFailed>()
                    .is_some_and(|f| f.kind == "finished") => {}
            Err(e) => return Err(e),
        }
        scoped.hc_output(&["rm", address])?;
        Ok(())
    }

    pub fn notification_show(&self, _title: &str, _body: &str) -> Result<()> {
        Ok(())
    }

    pub fn report_agent(
        &self,
        _address: &str,
        _source: &str,
        _agent: &str,
        _state: &str,
        _seq: u64,
    ) -> Result<()> {
        Err(Unsupported {
            call: "report_agent",
            backend: Backend::Hc,
        }
        .into())
    }

    pub fn release_agent(
        &self,
        _address: &str,
        _source: &str,
        _agent: &str,
        _seq: u64,
    ) -> Result<()> {
        Err(Unsupported {
            call: "release_agent",
            backend: Backend::Hc,
        }
        .into())
    }

    pub fn integration_status(&self) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::model::at_prompt;

    fn parse_terminal(raw: &str) -> Result<Pane, HcFailed> {
        let rec: TerminalRecord = serde_json::from_str(raw).unwrap();
        rec.into_pane("get --json")
    }

    #[cfg(unix)]
    fn hc_script(body: &str) -> (tempfile::TempDir, HcApp) {
        let dir = tempfile::tempdir().unwrap();
        let hc = dir.path().join("hc");
        let app = dir.path().join("herdr-app");
        let sock = dir.path().join("default.sock");
        std::fs::write(&sock, b"").unwrap();
        let warm_guard = "if [ \"$1\" = warm ]; then\n  exit 0\nfi";
        let warm = crate::test_support::write_warmed(
            &hc,
            &format!("#!/bin/sh\n{warm_guard}\n{body}\n"),
            &["warm"],
        );
        assert!(warm.status.success());
        let warm_app = crate::test_support::write_warmed(&app, "#!/bin/sh\nexit 0\n", &["warm"]);
        assert!(warm_app.status.success());
        (dir, HcApp::new(hc, app, sock))
    }

    #[test]
    fn hcb_05_terminal_maps_to_pane() {
        let raw = r#"{
            "address":"t4",
            "argv":["zsh"],
            "cwd":"/Users/op/proj",
            "foreground":null,
            "keep":true,
            "labels":{"herdr.space":"sp0123456789abcdef0123456789abcdef","herdr.tab":"tb0123456789abcdef0123456789abcdef"},
            "name":null,
            "parent":null,
            "pid":4123,
            "reported":true,
            "state":"live",
            "status":{"app":"claude-code","state":"idle"},
            "status_ms":1795138450594,
            "turn":2
        }"#;
        let pane = parse_terminal(raw).unwrap();
        assert_eq!(pane.pane_id, "t4");
        assert_eq!(
            pane.workspace_id.as_deref(),
            Some("sp0123456789abcdef0123456789abcdef")
        );
        assert_eq!(
            pane.tab_id.as_deref(),
            Some("tb0123456789abcdef0123456789abcdef")
        );
        assert_eq!(pane.agent.as_deref(), Some("claude-code"));
        assert_eq!(pane.agent_status.as_deref(), Some("idle"));
        assert_eq!(pane.state_change_seq, Some(1_795_138_450_594));
        assert!(pane.agent_session.is_none());
        assert!(pane.at_prompt());
    }

    #[test]
    fn hcb_05_null_status_is_no_agent() {
        let raw = r#"{
            "address":"t2",
            "state":"live",
            "labels":{"herdr.space":"sp1"},
            "status":null,
            "status_ms":null
        }"#;
        let pane = parse_terminal(raw).unwrap();
        assert_eq!(pane.agent, None);
        assert_eq!(pane.agent_status, None);
        assert_eq!(pane.state_change_seq, None);
        assert!(!pane.at_prompt());
    }

    #[test]
    fn hcb_05_error_status_is_not_at_prompt() {
        let raw = r#"{
            "address":"t3",
            "state":"live",
            "labels":{"herdr.space":"sp1"},
            "status":{"app":"codex","state":"error"},
            "status_ms":500
        }"#;
        let pane = parse_terminal(raw).unwrap();
        assert_eq!(pane.agent.as_deref(), Some("codex"));
        assert_eq!(pane.agent_status.as_deref(), Some("error"));
        assert!(!pane.at_prompt());
        assert!(!at_prompt(Some("error")));
    }

    #[cfg(unix)]
    #[test]
    fn hcb_05_finished_terminal_is_gone() {
        let (_dir, hc) = hc_script(
            r#"case "$1 $3" in
"get t1")
  printf '{"address":"t1","state":"finished","labels":{},"status":null,"status_ms":null}\n'
  exit 0;;
"get t9")
  printf '{"error":{"hint":null,"kind":"not_found","message":"no terminal t9"}}\n' >&2
  exit 3;;
esac
exit 1"#,
        );
        let finished = hc.pane_get("t1").unwrap_err();
        let finished_hc = finished.downcast_ref::<HcFailed>().unwrap();
        assert!(finished_hc.is_gone());
        assert!(!hc.pane_exists("t1"));

        let missing = hc.pane_get("t9").unwrap_err();
        let missing_hc = missing.downcast_ref::<HcFailed>().unwrap();
        assert_eq!(missing_hc.kind, "not_found");
        assert!(missing_hc.is_gone());
        assert!(!hc.pane_exists("t9"));
    }

    #[cfg(unix)]
    #[test]
    fn hcb_05_pane_list_filters_by_space_label() {
        let (dir, hc) = hc_script(
            r#"d=$(dirname "$0")
echo "$*" >> "$d/calls"
case "$1" in
ls)
  printf '{"hidden_finished":0,"terminals":[{"address":"t1","agent":"zsh","cwd":"/a","name":null,"state":"live","status":"idle","turn":0},{"address":"t2","agent":"zsh","cwd":"/a","name":null,"state":"live","status":null,"turn":0}]}\n';;
get)
  case "$3" in
  t1) printf '{"address":"t1","state":"live","labels":{"herdr.space":"sp7","herdr.tab":"tb1"},"status":{"app":"claude-code","state":"idle"},"status_ms":99}\n';;
  t2) printf '{"address":"t2","state":"finished","labels":{"herdr.space":"sp7","herdr.tab":"tb1"},"status":null,"status_ms":null}\n';;
  esac;;
esac"#,
        );
        let panes = hc.pane_list("sp7").unwrap();
        assert_eq!(panes.len(), 1, "finished row omitted");
        assert_eq!(panes[0].pane_id, "t1");
        assert_eq!(panes[0].workspace_id.as_deref(), Some("sp7"));
        let calls = std::fs::read_to_string(dir.path().join("calls")).unwrap();
        assert!(
            calls
                .lines()
                .next()
                .unwrap()
                .contains("ls --json --label herdr.space=sp7"),
            "{calls}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn hcb_05_pane_list_is_bounded_as_a_whole() {
        let (_dir, hc) = hc_script(
            r#"case "$1" in
ls)
  printf '{"hidden_finished":0,"terminals":[{"address":"t1"},{"address":"t2"},{"address":"t3"},{"address":"t4"}]}\n';;
get)
  sleep 0.25
  printf '{"address":"%s","state":"live","labels":{"herdr.space":"sp1"},"status":null,"status_ms":null}\n' "$3";;
esac"#,
        );
        let bounded = hc.with_timeout(Duration::from_millis(500));
        let started = Instant::now();
        let err = bounded.pane_list("sp1").unwrap_err();
        assert!(
            err.chain().any(|c| {
                c.downcast_ref::<std::io::Error>()
                    .is_some_and(|io| io.kind() == std::io::ErrorKind::TimedOut)
            }),
            "expected TimedOut, got: {err:#}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "bounded as one budget, not 500 ms per child: {err}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn hcb_05_status_ms_is_the_change_counter() {
        let (_dir, hc) = hc_script(
            r#"printf '{"address":"t5","state":"live","labels":{"herdr.space":"sp1"},"status":{"app":"codex","state":"working"},"status_ms":424242}\n'"#,
        );
        let p = hc.pane_get("t5").unwrap();
        assert_eq!(p.state_change_seq, Some(424_242));
    }

    #[cfg(unix)]
    #[test]
    fn hcb_05_reachable_does_not_start_a_core() {
        let (dir, hc) = hc_script(
            r#"d=$(dirname "$0")
echo called > "$d/called"
exit 0"#,
        );
        std::fs::remove_file(dir.path().join("default.sock")).unwrap();
        assert!(!hc.server_reachable());
        assert!(
            !dir.path().join("called").exists(),
            "hc must not even be spawned when the socket file is absent"
        );

        std::fs::write(dir.path().join("default.sock"), b"").unwrap();
        assert!(hc.server_reachable());
        assert!(dir.path().join("called").exists());
    }

    #[cfg(unix)]
    #[test]
    fn hcb_06_prompt_is_one_hc_send() {
        let (dir, hc) = hc_script(
            r#"d=$(dirname "$0")
printf '%s\n' "$*" >> "$d/calls"
printf '{"started":true,"status":"idle","turn":1}\n'"#,
        );
        hc.agent_prompt("t4", "[sonnet-1] DONE: ok").unwrap();
        hc.pane_run("t4", "horch worker").unwrap();
        let calls: Vec<String> = std::fs::read_to_string(dir.path().join("calls"))
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect();
        assert_eq!(
            calls,
            [
                "send --json t4 [sonnet-1] DONE: ok",
                "send --json t4 horch worker",
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn hcb_06_blocked_terminal_refuses_the_prompt() {
        let (_dir, hc) = hc_script(
            r#"case "$3" in
t1)
  printf '{"error":{"hint":null,"kind":"blocked","message":"terminal t1 is blocked"}}\n' >&2
  exit 9;;
t2)
  printf '{"error":{"hint":null,"kind":"finished","message":"terminal t2 has finished"}}\n' >&2
  exit 11;;
t3)
  printf '{"error":{"hint":null,"kind":"not_found","message":"terminal t3 not found"}}\n' >&2
  exit 3;;
esac"#,
        );
        let blocked = hc.agent_prompt("t1", "hi").unwrap_err();
        let blocked_hc = blocked.downcast_ref::<HcFailed>().unwrap();
        assert_eq!(blocked_hc.kind, "blocked");
        assert!(!blocked_hc.is_gone());

        for gone in ["t2", "t3"] {
            let err = hc.agent_prompt(gone, "hi").unwrap_err();
            assert!(err.downcast_ref::<HcFailed>().unwrap().is_gone(), "{gone}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn hcb_06_keys_pass_through() {
        let (dir, hc) = hc_script(
            r#"d=$(dirname "$0")
for a in "$@"; do printf '[%s]' "$a" >> "$d/calls"; done
printf '\n' >> "$d/calls"
printf '{"status":"idle","turn":1}\n'"#,
        );
        hc.pane_send_keys("t7", "esc enter").unwrap();
        let calls = std::fs::read_to_string(dir.path().join("calls")).unwrap();
        assert_eq!(calls.trim(), "[keys][--json][t7][esc][enter]");
    }

    #[cfg(unix)]
    #[test]
    fn hcb_06_send_text_is_refused() {
        let (dir, hc) = hc_script("echo bad > \"$(dirname \"$0\")/called\"\nexit 0");
        let err = hc.pane_send_text("t1", "draft").unwrap_err().to_string();
        assert_eq!(err, "hc cannot type text without Enter");
        assert!(!dir.path().join("called").exists());

        let rep = hc
            .report_agent("t1", "horch", "claude", "idle", 1)
            .unwrap_err()
            .to_string();
        assert_eq!(rep, "hc agents report their own status");
        let rel = hc
            .release_agent("t1", "horch", "claude", 2)
            .unwrap_err()
            .to_string();
        assert_eq!(rel, "hc agents report their own status");
        hc.notification_show("title", "body").unwrap();
        assert_eq!(hc.integration_status(), None);
        assert!(!dir.path().join("called").exists());
    }

    #[cfg(unix)]
    #[test]
    fn hcb_06_close_kills_then_removes() {
        let (dir, hc) = hc_script(
            r#"d=$(dirname "$0")
printf '%s\n' "$*" >> "$d/calls"
case "$1 $2 $3" in
"kill --json t1")
  printf '{"address":"t1","state":"finished"}\n';;
"kill --json t2")
  printf '{"error":{"hint":null,"kind":"finished","message":"already finished"}}\n' >&2
  exit 11;;
"kill --json t9")
  printf '{"error":{"hint":null,"kind":"not_found","message":"no terminal t9"}}\n' >&2
  exit 3;;
"rm t1 "*|"rm t1")
  printf 'removed t1\n';;
"rm t2 "*|"rm t2")
  printf 'removed t2\n';;
esac"#,
        );
        hc.pane_close("t1").unwrap();
        hc.pane_close("t2").unwrap();
        let missing = hc.pane_close("t9").unwrap_err();
        assert_eq!(
            missing.downcast_ref::<HcFailed>().unwrap().kind,
            "not_found"
        );

        let calls: Vec<String> = std::fs::read_to_string(dir.path().join("calls"))
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect();
        assert_eq!(
            calls,
            [
                "kill --json t1",
                "rm t1",
                "kill --json t2",
                "rm t2",
                "kill --json t9",
            ]
        );
    }
}
