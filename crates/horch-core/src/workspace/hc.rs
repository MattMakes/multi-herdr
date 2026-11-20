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
use crate::workspace::model::{
    Direction, Focus, FocusDir, Layout, Move, NewWorkspace, Pane, Resize, Tab, Workspace,
};

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
    cwd: Option<String>,
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

#[derive(Debug, Deserialize)]
struct RunResponse {
    address: String,
}

#[derive(Debug, Deserialize)]
struct SpaceNewResponse {
    space: String,
    tab: String,
    terminal: String,
}

#[derive(Debug, Deserialize)]
struct PaneSplitResponse {
    terminal: String,
}

#[derive(Debug, Deserialize)]
struct AppLsResponse {
    spaces: Vec<AppSpace>,
}

#[derive(Debug, Deserialize)]
struct AppSpace {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    tabs: Vec<AppTab>,
}

#[derive(Debug, Deserialize)]
struct AppTab {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    active: bool,
    #[serde(default)]
    terminals: Vec<AppTerminalRef>,
}

#[derive(Debug, Deserialize)]
struct AppTerminalRef {
    terminal: String,
    pane: String,
}

fn terminal_order(address: &str) -> (u64, &str) {
    let num = address
        .strip_prefix('t')
        .and_then(|n| n.parse::<u64>().ok())
        .unwrap_or(u64::MAX);
    (num, address)
}

fn missing_hc(err: std::io::Error) -> anyhow::Error {
    if err.kind() == std::io::ErrorKind::NotFound {
        anyhow!("hc CLI not found on PATH; open the Herdr app or install herdr-core")
    } else {
        anyhow!(err)
    }
}

fn missing_app(err: std::io::Error) -> anyhow::Error {
    if err.kind() == std::io::ErrorKind::NotFound {
        anyhow!("herdr-app CLI not found on PATH; open the Herdr app")
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

    fn app_command<S: AsRef<OsStr>>(&self, args: &[S]) -> Command {
        let mut command = Command::new(&self.app);
        command.env("HC_TERMINAL_SOCKET", &self.core_socket);
        if let Some(home) = &self.app_home {
            command.env("HERDR_APP_HOME", home);
        }
        command.args(args);
        command
    }

    fn run_app<S: AsRef<OsStr>>(&self, args: &[S]) -> std::io::Result<Output> {
        let command = self.app_command(args);
        match (self.timeout, self.deadline) {
            (None, None) => {
                let mut cmd = command;
                cmd.output()
            }
            (Some(timeout), deadline) => output_within(command, timeout, deadline),
            (None, Some(deadline)) => output_within(
                command,
                deadline.saturating_duration_since(Instant::now()),
                Some(deadline),
            ),
        }
    }

    fn app_output<S: AsRef<OsStr>>(&self, args: &[S]) -> Result<String> {
        let out = self.run_app(args).map_err(missing_app)?;
        if !out.status.success() {
            let rendered = args
                .iter()
                .map(|a| a.as_ref().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join(" ");
            return Err(HcFailed {
                command: format!("herdr-app {rendered}"),
                kind: "herdr_app".into(),
                message: String::from_utf8_lossy(&out.stderr).trim().to_string(),
            }
            .into());
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn app_json<T: for<'de> Deserialize<'de>, S: AsRef<OsStr>>(&self, args: &[S]) -> Result<T> {
        let stdout = self.app_output(args)?;
        serde_json::from_str(stdout.trim())
            .with_context(|| format!("could not parse herdr-app response: {}", stdout.trim()))
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

    pub fn window_open(&self) -> bool {
        self.app_command(&["status"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    pub fn workspace_create(
        &self,
        label: &str,
        cwd: Option<&str>,
        focus: bool,
    ) -> Result<NewWorkspace> {
        let scoped = self.scoped();
        let mut payload = serde_json::Map::new();
        payload.insert("name".into(), serde_json::Value::String(label.to_string()));
        if let Some(folder) = cwd {
            payload.insert(
                "folder".into(),
                serde_json::Value::String(folder.to_string()),
            );
        }
        payload.insert("focus".into(), serde_json::Value::Bool(focus));
        let arg = serde_json::Value::Object(payload).to_string();
        match scoped.app_json::<SpaceNewResponse, _>(&["do", "space.new", &arg]) {
            Ok(resp) => Ok(NewWorkspace {
                workspace_id: resp.space,
                root_pane_id: resp.terminal,
                tab_id: Some(resp.tab),
            }),
            Err(e) => {
                if !self.window_open() {
                    bail!("the Herdr app window is not open; open Herdr and run this again");
                }
                Err(e)
            }
        }
    }

    pub fn workspace_close(&self, workspace_id: &str) -> Result<()> {
        let scoped = self.scoped();
        let arg = serde_json::json!({"id": workspace_id, "confirmed": true}).to_string();
        match scoped.app_output(&["do", "space.close", &arg]) {
            Ok(_) => return Ok(()),
            Err(e) if !is_app_unavailable(&e) => return Err(e),
            Err(_) => {}
        }
        let label = format!("herdr.space={workspace_id}");
        let listed: LsResponse = scoped.hc_json(&["ls", "--json", "--label", &label])?;
        for row in listed.terminals {
            match scoped.pane_close(&row.address) {
                Ok(()) => {}
                Err(e) if e.downcast_ref::<HcFailed>().is_some_and(HcFailed::is_gone) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    pub fn workspace_list(&self) -> Result<Vec<Workspace>> {
        let scoped = self.scoped();
        match scoped.app_json::<AppLsResponse, _>(&["ls"]) {
            Ok(app) => {
                return Ok(app
                    .spaces
                    .into_iter()
                    .map(|s| {
                        let active_tab_id = s.tabs.into_iter().find(|t| t.active).map(|t| t.id);
                        Workspace {
                            workspace_id: s.id,
                            label: s.name,
                            active_tab_id,
                        }
                    })
                    .collect());
            }
            Err(e) if !is_app_unavailable(&e) => return Err(e),
            Err(_) => {}
        }
        let listed: LsResponse = scoped.hc_json(&["ls", "--json"])?;
        let mut spaces = std::collections::BTreeSet::new();
        for row in listed.terminals {
            match scoped.hc_json::<TerminalRecord, _>(&["get", "--json", &row.address]) {
                Ok(rec) if rec.state != "finished" => {
                    if let Some(sp) = rec.labels.get("herdr.space").filter(|s| !s.is_empty()) {
                        spaces.insert(sp.clone());
                    }
                }
                Ok(_) => {}
                Err(e) if e.downcast_ref::<HcFailed>().is_some_and(HcFailed::is_gone) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(spaces
            .into_iter()
            .map(|workspace_id| Workspace {
                workspace_id,
                label: None,
                active_tab_id: None,
            })
            .collect())
    }

    pub fn tab_list(&self, workspace_id: &str) -> Result<Vec<Tab>> {
        let scoped = self.scoped();
        match scoped.app_json::<AppLsResponse, _>(&["ls"]) {
            Ok(app) => {
                let space = app
                    .spaces
                    .into_iter()
                    .find(|s| s.id == workspace_id)
                    .with_context(|| format!("space {workspace_id} not found in herdr-app ls"))?;
                return Ok(space
                    .tabs
                    .into_iter()
                    .enumerate()
                    .map(|(i, t)| Tab {
                        tab_id: t.id,
                        workspace_id: workspace_id.to_string(),
                        number: (i + 1) as i64,
                        label: t.name,
                        pane_count: Some(t.terminals.len() as i64),
                    })
                    .collect());
            }
            Err(e) if !is_app_unavailable(&e) => return Err(e),
            Err(_) => {}
        }
        let mut panes = scoped.pane_list(workspace_id)?;
        panes.sort_by(|a, b| terminal_order(&a.pane_id).cmp(&terminal_order(&b.pane_id)));
        let mut seen: Vec<(String, usize)> = Vec::new();
        for pane in panes {
            let Some(tb) = pane.tab_id.filter(|s| !s.is_empty()) else {
                continue;
            };
            match seen.iter_mut().find(|(id, _)| *id == tb) {
                Some((_, count)) => *count += 1,
                None => seen.push((tb, 1)),
            }
        }
        Ok(seen
            .into_iter()
            .enumerate()
            .map(|(i, (tab_id, count))| Tab {
                tab_id,
                workspace_id: workspace_id.to_string(),
                number: (i + 1) as i64,
                label: None,
                pane_count: Some(count as i64),
            })
            .collect())
    }

    pub fn pane_split(&self, from: &str, direction: Direction) -> Result<String> {
        let scoped = self.scoped();
        match scoped.split_via_app(from, direction) {
            Ok(term) => return Ok(term),
            Err(e) if !is_app_unavailable(&e) => return Err(e),
            Err(_) => {}
        }
        let from_rec: TerminalRecord = scoped.hc_json(&["get", "--json", from])?;
        if from_rec.state == "finished" {
            return Err(HcFailed {
                command: format!("get --json {from}"),
                kind: "finished".into(),
                message: format!("terminal {from} has finished"),
            }
            .into());
        }
        let mut args: Vec<String> = vec![
            "run".into(),
            "--bg".into(),
            "--json".into(),
            "--keep".into(),
            "--no-parent".into(),
        ];
        if let Some(cwd) = from_rec.cwd.as_deref().filter(|s| !s.is_empty()) {
            args.push("--cwd".into());
            args.push(cwd.to_string());
        }
        if let Some(sp) = from_rec.labels.get("herdr.space").filter(|s| !s.is_empty()) {
            args.push("--label".into());
            args.push(format!("herdr.space={sp}"));
        }
        if let Some(tb) = from_rec.labels.get("herdr.tab").filter(|s| !s.is_empty()) {
            args.push("--label".into());
            args.push(format!("herdr.tab={tb}"));
        }
        let ran: RunResponse = scoped.hc_json(&args)?;
        Ok(ran.address)
    }

    fn split_via_app(&self, from: &str, direction: Direction) -> Result<String> {
        let app: AppLsResponse = self.app_json(&["ls"])?;
        let app_pane = app
            .spaces
            .into_iter()
            .flat_map(|s| s.tabs)
            .flat_map(|t| t.terminals)
            .find(|t| t.terminal == from)
            .map(|t| t.pane)
            .ok_or_else(|| HcFailed {
                command: "herdr-app ls".into(),
                kind: "herdr_app".into(),
                message: format!("terminal {from} has no app pane"),
            })?;
        let arg = serde_json::json!({
            "direction": direction.as_str(),
            "pane": app_pane,
        })
        .to_string();
        let resp: PaneSplitResponse = self.app_json(&["do", "pane.split", &arg])?;
        Ok(resp.terminal)
    }

    pub fn tab_new(
        &self,
        workspace_id: &str,
        label: &str,
        cwd: Option<&str>,
    ) -> Result<(String, String)> {
        let scoped = self.scoped();
        let mut payload = serde_json::Map::new();
        payload.insert(
            "space".into(),
            serde_json::Value::String(workspace_id.to_string()),
        );
        payload.insert("name".into(), serde_json::Value::String(label.to_string()));
        if let Some(folder) = cwd {
            payload.insert(
                "folder".into(),
                serde_json::Value::String(folder.to_string()),
            );
        }
        payload.insert("focus".into(), serde_json::Value::Bool(false));
        let arg = serde_json::Value::Object(payload).to_string();
        match scoped.app_json::<SpaceNewResponse, _>(&["do", "tab.new", &arg]) {
            Ok(resp) => Ok((resp.tab, resp.terminal)),
            Err(e) => {
                if !self.window_open() {
                    bail!("the Herdr app window is not open; open Herdr and run this again");
                }
                Err(e)
            }
        }
    }

    pub fn tab_focus(&self, tab_id: &str) -> Result<()> {
        let arg = serde_json::json!({"id": tab_id}).to_string();
        self.app_output(&["do", "tab.select", &arg])?;
        Ok(())
    }

    pub fn pane_focus_walk(&self, _tab: &str, target: &str) -> Result<bool> {
        let arg = serde_json::json!({"terminal": target}).to_string();
        let out = self
            .run_app(&["do", "pane.focus", &arg])
            .map_err(missing_app)?;
        Ok(out.status.success())
    }

    pub fn pane_layout(&self, _pane: Option<&str>) -> Result<Layout> {
        Err(Unsupported {
            call: "pane_layout",
            backend: Backend::Hc,
        }
        .into())
    }

    pub(crate) fn pane_resize(
        &self,
        _pane: &str,
        _direction: &str,
        _amount: f64,
    ) -> Result<Resize> {
        Err(Unsupported {
            call: "pane_resize",
            backend: Backend::Hc,
        }
        .into())
    }

    pub(crate) fn pane_move(
        &self,
        _pane: &str,
        _tab: &str,
        _direction: Direction,
        _target: Option<&str>,
        _ratio: Option<f64>,
    ) -> Result<Move> {
        Err(Unsupported {
            call: "pane_move",
            backend: Backend::Hc,
        }
        .into())
    }

    pub(crate) fn pane_move_new_tab(&self, _pane: &str, _label: &str) -> Result<Move> {
        Err(Unsupported {
            call: "pane_move_new_tab",
            backend: Backend::Hc,
        }
        .into())
    }

    pub fn pane_focus(&self, _origin: &str, _direction: FocusDir) -> Result<Focus> {
        Err(Unsupported {
            call: "pane_focus",
            backend: Backend::Hc,
        }
        .into())
    }
}

/// True when `herdr-app` exited non-zero (`HcFailed { kind: "herdr_app", .. }`)
/// and NOT when a timeout / push deadline stopped the call.
fn is_app_unavailable(err: &anyhow::Error) -> bool {
    err.downcast_ref::<HcFailed>()
        .is_some_and(|f| f.kind == "herdr_app")
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

    #[cfg(unix)]
    fn hc_and_app_scripts(hc_body: &str, app_body: &str) -> (tempfile::TempDir, HcApp) {
        let dir = tempfile::tempdir().unwrap();
        let hc = dir.path().join("hc");
        let app = dir.path().join("herdr-app");
        let sock = dir.path().join("default.sock");
        std::fs::write(&sock, b"").unwrap();
        let warm_guard = "if [ \"$1\" = warm ]; then\n  exit 0\nfi";
        let warm_hc = crate::test_support::write_warmed(
            &hc,
            &format!("#!/bin/sh\n{warm_guard}\n{hc_body}\n"),
            &["warm"],
        );
        assert!(warm_hc.status.success());
        let warm_app = crate::test_support::write_warmed(
            &app,
            &format!("#!/bin/sh\n{warm_guard}\n{app_body}\n"),
            &["warm"],
        );
        assert!(warm_app.status.success());
        (dir, HcApp::new(hc, app, sock))
    }

    #[cfg(unix)]
    #[test]
    fn hcb_07_space_new_returns_the_root_terminal() {
        let (dir, hc) = hc_and_app_scripts(
            "exit 0",
            r#"d=$(dirname "$0")
printf '%s\n' "$*" >> "$d/app_calls"
case "$1 $2" in
"do space.new")
  printf '{"space":"sp11","tab":"tb11","terminal":"t1"}\n';;
"do tab.new")
  printf '{"space":"sp11","tab":"tb22","terminal":"t5"}\n';;
"do tab.select")
  printf '{"ok":true}\n';;
"do pane.focus")
  printf '{"ok":true}\n';;
status)
  printf '{"ok":true}\n';;
esac"#,
        );
        let ws = hc
            .workspace_create("fleet", Some("/work/alpha"), false)
            .unwrap();
        assert_eq!(ws.workspace_id, "sp11");
        assert_eq!(ws.root_pane_id, "t1");
        assert_eq!(ws.tab_id.as_deref(), Some("tb11"));

        let (tab, term) = hc
            .tab_new("sp11", "workers 2", Some("/work/alpha"))
            .unwrap();
        assert_eq!((tab.as_str(), term.as_str()), ("tb22", "t5"));
        hc.tab_focus("tb22").unwrap();
        assert!(hc.pane_focus_walk("tb22", "t5").unwrap());

        let calls = std::fs::read_to_string(dir.path().join("app_calls")).unwrap();
        assert!(
            calls.contains(r#"do space.new {"focus":false,"folder":"/work/alpha","name":"fleet"}"#)
                || calls.contains(r#""name":"fleet""#),
            "{calls}"
        );
        assert!(calls.contains(r#""name":"workers 2""#), "{calls}");
    }

    #[cfg(unix)]
    #[test]
    fn hcb_07_split_uses_the_app_pane_id() {
        let (dir, hc) = hc_and_app_scripts(
            "echo hc >> \"$(dirname \"$0\")/hc_called\"\nexit 1",
            r#"d=$(dirname "$0")
printf '%s\n' "$*" >> "$d/app_calls"
case "$1" in
ls)
  printf '{"spaces":[{"id":"sp1","name":"fleet","folder":"/w","active":true,"tabs":[{"id":"tb1","name":"1","active":true,"settled":false,"terminals":[{"terminal":"t1","pane":"p77","status":"idle","app":"claude-code","name":null,"sub_agents":[]}]}]}]}\n';;
do)
  printf '{"pane":"p78","terminal":"t2"}\n';;
esac"#,
        );
        let new_term = hc.pane_split("t1", Direction::Right).unwrap();
        assert_eq!(new_term, "t2");
        let calls = std::fs::read_to_string(dir.path().join("app_calls")).unwrap();
        assert!(
            calls.contains(r#""pane":"p77""#),
            "must resolve t1 -> p77: {calls}"
        );
        assert!(calls.contains(r#""direction":"right""#), "{calls}");
        assert!(!dir.path().join("hc_called").exists());
    }

    #[cfg(unix)]
    #[test]
    fn hcb_07_window_closed_fails_fleet_start_with_a_clear_message() {
        let (_dir, hc) = hc_and_app_scripts(
            "exit 0",
            r#"printf 'herdr-app: no Herdr window\n' >&2
exit 2"#,
        );
        let err = hc
            .workspace_create("fleet", Some("/work/alpha"), false)
            .unwrap_err()
            .to_string();
        assert_eq!(
            err,
            "the Herdr app window is not open; open Herdr and run this again"
        );
    }

    #[cfg(unix)]
    #[test]
    fn hcb_07_geometry_calls_are_refused() {
        let (_dir, hc) = hc_script("exit 0");
        let host = crate::workspace::PaneHost::hc_app(hc);
        assert!(!host.caps().geometry);
        for res in [
            host.pane_layout(Some("t1")).map(|_| ()),
            host.pane_resize("t1", "right", 0.5).map(|_| ()),
            host.pane_move("t1", "tb1", Direction::Right, None, None)
                .map(|_| ()),
            host.pane_move_new_tab("t1", "workers 2").map(|_| ()),
            host.pane_focus("t1", FocusDir::Right).map(|_| ()),
        ] {
            let err = res.unwrap_err();
            let unsup = err
                .downcast_ref::<Unsupported>()
                .expect("Unsupported error");
            assert_eq!(unsup.backend, Backend::Hc);
        }
        let herdr = crate::workspace::PaneHost::herdr_bin("no-herdr");
        let tab_err = herdr.tab_new("w1", "workers 2", None).unwrap_err();
        assert_eq!(
            tab_err.downcast_ref::<Unsupported>(),
            Some(&Unsupported {
                call: "tab_new",
                backend: Backend::Herdr,
            })
        );
    }

    #[cfg(unix)]
    #[test]
    fn hcb_08_split_falls_back_to_a_labelled_run() {
        let (dir, hc) = hc_and_app_scripts(
            r#"d=$(dirname "$0")
printf '%s\n' "$*" >> "$d/hc_calls"
case "$1" in
get)
  printf '{"address":"t1","cwd":"/work/alpha","keep":true,"parent":null,"state":"live","labels":{"herdr.space":"sp9","herdr.tab":"tb9"},"status":null,"status_ms":null}\n';;
run)
  printf '{"address":"t2","agent":"zsh","cwd":"/work/alpha","state":"live","turn":0}\n';;
esac"#,
            "exit 2",
        );
        let addr = hc.pane_split("t1", Direction::Down).unwrap();
        assert_eq!(addr, "t2");
        let calls = std::fs::read_to_string(dir.path().join("hc_calls")).unwrap();
        let run_line = calls.lines().last().unwrap();
        assert!(
            run_line.contains("run --bg --json --keep --no-parent"),
            "{calls}"
        );
        assert!(run_line.contains("--cwd /work/alpha"), "{calls}");
        assert!(run_line.contains("--label herdr.space=sp9"), "{calls}");
        assert!(run_line.contains("--label herdr.tab=tb9"), "{calls}");
    }

    #[cfg(unix)]
    #[test]
    fn hcb_08_fallback_terminal_has_no_parent_and_is_kept() {
        let (dir, hc) = hc_and_app_scripts(
            r#"d=$(dirname "$0")
case "$1" in
get)
  if [ "$3" = t1 ]; then
    printf '{"address":"t1","cwd":"/w","keep":true,"parent":null,"state":"live","labels":{"herdr.space":"sp1","herdr.tab":"tb1"},"status":null,"status_ms":null}\n'
  else
    cat "$d/created.json"
  fi;;
run)
  keep=false
  parent='"t1"'
  for a in "$@"; do
    [ "$a" = "--keep" ] && keep=true
    [ "$a" = "--no-parent" ] && parent=null
  done
  printf '{"address":"t2","cwd":"/w","keep":%s,"parent":%s,"state":"live","labels":{"herdr.space":"sp1","herdr.tab":"tb1"},"status":null,"status_ms":null}\n' "$keep" "$parent" > "$d/created.json"
  printf '{"address":"t2","agent":"zsh","cwd":"/w","state":"live","turn":0}\n';;
esac"#,
            "exit 2",
        );
        let addr = hc.pane_split("t1", Direction::Right).unwrap();
        assert_eq!(addr, "t2");
        let created: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join("created.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(created["keep"], true);
        assert!(created["parent"].is_null(), "{created}");
    }

    #[cfg(unix)]
    #[test]
    fn hcb_08_close_with_the_window_closed_kills_by_label() {
        let (dir, hc) = hc_and_app_scripts(
            r#"d=$(dirname "$0")
printf '%s\n' "$*" >> "$d/hc_calls"
case "$1" in
ls)
  printf '{"hidden_finished":0,"terminals":[{"address":"t1"},{"address":"t3"}]}\n';;
kill)
  printf '{"address":"%s","state":"finished"}\n' "$3";;
rm)
  printf 'removed %s\n' "$2";;
esac"#,
            "exit 2",
        );
        hc.workspace_close("sp4").unwrap();
        let calls: Vec<String> = std::fs::read_to_string(dir.path().join("hc_calls"))
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect();
        assert_eq!(
            calls,
            [
                "ls --json --label herdr.space=sp4",
                "kill --json t1",
                "rm t1",
                "kill --json t3",
                "rm t3",
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn hcb_08_list_with_the_window_closed_reads_labels() {
        let (_dir, hc) = hc_and_app_scripts(
            r#"case "$1" in
ls)
  printf '{"hidden_finished":0,"terminals":[{"address":"t1"},{"address":"t2"},{"address":"t3"}]}\n';;
get)
  case "$3" in
  t1) printf '{"address":"t1","state":"live","labels":{"herdr.space":"sp2","herdr.tab":"tb1"},"status":null,"status_ms":null}\n';;
  t2) printf '{"address":"t2","state":"live","labels":{"herdr.space":"sp1","herdr.tab":"tb1"},"status":null,"status_ms":null}\n';;
  t3) printf '{"address":"t3","state":"live","labels":{"herdr.space":"sp2","herdr.tab":"tb2"},"status":null,"status_ms":null}\n';;
  esac;;
esac"#,
            "exit 2",
        );
        let spaces = hc.workspace_list().unwrap();
        let ids: Vec<&str> = spaces.iter().map(|w| w.workspace_id.as_str()).collect();
        assert_eq!(ids, ["sp1", "sp2"]);
        assert!(spaces.iter().all(|w| w.label.is_none()));

        let tabs = hc.tab_list("sp2").unwrap();
        let tab_ids: Vec<&str> = tabs.iter().map(|t| t.tab_id.as_str()).collect();
        assert_eq!(tab_ids, ["tb1", "tb2"]);
    }
}
