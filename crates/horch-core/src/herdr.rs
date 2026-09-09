//! Typed wrapper around the `herdr` CLI, replacing the shell's `herdr ... | jq`
//! pipelines.
//!
//! Verified against herdr 0.6.1 and re-checked against 0.8.0. Deliberately
//! limited to the surface 0.6.1 advertises: no `pane current`, and no `--env` or
//! `--ratio` on `pane split`.

use std::ffi::OsStr;
use std::process::Command;

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;

/// Geometry of a pane or of the tab area that contains it.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
pub struct Rect {
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Pane {
    pub pane_id: String,
    #[serde(default)]
    pub workspace_id: Option<String>,
    /// Present when herdr's claude/codex integration is installed. Either a bare
    /// string or an object carrying the id under one of several keys, hence
    /// `Value`; use [`Pane::agent_session_id`] to read it.
    #[serde(default)]
    pub agent_session: Option<serde_json::Value>,
}

impl Pane {
    /// The agent session id herdr reports for this pane, if any.
    ///
    /// Mirrors the jq fallback chain `.session_id // .id // .ref` used by the
    /// bash implementation, because the key has moved between herdr versions.
    pub fn agent_session_id(&self) -> Option<String> {
        match self.agent_session.as_ref()? {
            serde_json::Value::String(s) if !s.is_empty() => Some(s.clone()),
            serde_json::Value::Object(map) => ["session_id", "id", "ref"]
                .iter()
                .filter_map(|k| map.get(*k))
                .filter_map(|v| v.as_str())
                .find(|s| !s.is_empty())
                .map(str::to_owned),
            _ => None,
        }
    }
}

/// One pane's slot in a tab layout.
#[derive(Debug, Clone, Deserialize)]
pub struct LayoutPane {
    pub pane_id: String,
    pub rect: Rect,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Layout {
    pub workspace_id: String,
    pub tab_id: String,
    /// The whole tab region the panes are packed into.
    pub area: Rect,
    pub panes: Vec<LayoutPane>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Workspace {
    pub workspace_id: String,
}

#[derive(Debug, Deserialize)]
struct Envelope<T> {
    result: T,
}

#[derive(Debug, Deserialize)]
struct PaneResult {
    pane: Pane,
}

#[derive(Debug, Deserialize)]
struct PaneListResult {
    panes: Vec<Pane>,
}

#[derive(Debug, Deserialize)]
struct LayoutResult {
    layout: Layout,
}

/// What `herdr pane resize` reports back.
#[derive(Debug, Clone, Deserialize)]
pub struct Resize {
    /// False when herdr declined the move, which in practice means the divider is
    /// against the minimum pane width. The loop-breaker for any resize sequence.
    pub changed: bool,
    pub layout: Layout,
}

#[derive(Debug, Deserialize)]
struct ResizeResult {
    resize: Resize,
}

#[derive(Debug, Deserialize)]
struct WorkspaceCreateResult {
    workspace: Workspace,
    root_pane: Pane,
}

/// A freshly created workspace and the pane it starts with.
#[derive(Debug, Clone)]
pub struct NewWorkspace {
    pub workspace_id: String,
    pub root_pane_id: String,
}

/// Which way `pane split` grows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Right,
    Down,
}

impl Direction {
    pub fn as_str(self) -> &'static str {
        match self {
            Direction::Right => "right",
            Direction::Down => "down",
        }
    }
}

impl std::str::FromStr for Direction {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "right" => Ok(Direction::Right),
            "down" => Ok(Direction::Down),
            other => Err(format!("unknown direction '{other}' (expected right or down)")),
        }
    }
}

impl std::fmt::Display for Direction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Handle to the `herdr` executable.
#[derive(Debug, Clone, Default)]
pub struct Herdr;

impl Herdr {
    pub fn new() -> Self {
        Self
    }

    /// Run `herdr <args>`, returning stdout. Errors carry herdr's own stderr,
    /// which is what actually explains a failure.
    fn output<S: AsRef<OsStr>>(&self, args: &[S]) -> Result<String> {
        let out = Command::new("herdr").args(args).output().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                anyhow!("herdr CLI not found on PATH. Install it with `herdr-install`, or see https://herdr.dev/docs/install/")
            } else {
                anyhow!(e)
            }
        })?;
        if !out.status.success() {
            let rendered: Vec<String> = args
                .iter()
                .map(|a| a.as_ref().to_string_lossy().into_owned())
                .collect();
            bail!(
                "herdr {} failed ({}): {}",
                rendered.join(" "),
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// Run `herdr <args>` and decode the `{"result": ...}` envelope it prints.
    fn json<T: for<'de> Deserialize<'de>, S: AsRef<OsStr>>(&self, args: &[S]) -> Result<T> {
        let stdout = self.output(args)?;
        let envelope: Envelope<T> = serde_json::from_str(stdout.trim())
            .with_context(|| format!("could not parse herdr response: {}", stdout.trim()))?;
        Ok(envelope.result)
    }

    /// True when the herdr server answers at all. Cheap reachability probe.
    pub fn server_reachable(&self) -> bool {
        Command::new("herdr")
            .args(["workspace", "list"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// `herdr pane get <id>`. Accepts the internal id herdr exports as
    /// `HERDR_PANE_ID` (e.g. `p_2`) and upgrades it to the public ids.
    pub fn pane_get(&self, id: &str) -> Result<Pane> {
        let r: PaneResult = self.json(&["pane", "get", id])?;
        Ok(r.pane)
    }

    pub fn pane_list(&self, workspace_id: &str) -> Result<Vec<Pane>> {
        let r: PaneListResult = self.json(&["pane", "list", "--workspace", workspace_id])?;
        Ok(r.panes)
    }

    pub fn pane_split(&self, from: &str, direction: Direction) -> Result<String> {
        let r: PaneResult = self.json(&[
            "pane",
            "split",
            from,
            "--direction",
            direction.as_str(),
            "--no-focus",
        ])?;
        Ok(r.pane.pane_id)
    }

    /// `herdr pane run` submits text and Enter atomically. Safe for launching a
    /// command into a fresh shell; not safe for a TUI in raw mode, which is why
    /// [`Self::send_line`] exists.
    pub fn pane_run(&self, pane: &str, command: &str) -> Result<()> {
        self.output(&["pane", "run", pane, command])?;
        Ok(())
    }

    pub fn pane_send_text(&self, pane: &str, text: &str) -> Result<()> {
        self.output(&["pane", "send-text", pane, text])?;
        Ok(())
    }

    pub fn pane_send_keys(&self, pane: &str, keys: &str) -> Result<()> {
        self.output(&["pane", "send-keys", pane, keys])?;
        Ok(())
    }

    pub fn pane_close(&self, pane: &str) -> Result<()> {
        self.output(&["pane", "close", pane])?;
        Ok(())
    }

    /// True when the pane still exists.
    pub fn pane_exists(&self, pane: &str) -> bool {
        self.pane_get(pane).is_ok()
    }

    pub fn pane_read(&self, pane: &str, source: &str) -> Result<String> {
        self.output(&["pane", "read", pane, "--source", source])
    }

    pub fn pane_layout(&self, pane: Option<&str>) -> Result<Layout> {
        let r: LayoutResult = match pane {
            Some(p) => self.json(&["pane", "layout", "--pane", p])?,
            None => self.json(&["pane", "layout", "--current"])?,
        };
        Ok(r.layout)
    }

    /// Move the divider on `direction` side of `pane` by `amount`, a fraction of
    /// the split container that owns it.
    ///
    /// The reply carries the post-resize layout, so a caller adjusting several
    /// dividers can re-plan against real geometry instead of predicting it. It
    /// also carries `changed`, which goes false when herdr refuses the move -
    /// that is how a caller discovers the minimum pane width rather than
    /// hard-coding one.
    pub fn pane_resize(&self, pane: &str, direction: &str, amount: f64) -> Result<Resize> {
        // herdr stores split ratios to four decimal places; matching that here
        // keeps the request and what it can actually honour in step.
        let amount = format!("{amount:.4}");
        let r: ResizeResult = self.json(&[
            "pane",
            "resize",
            "--pane",
            pane,
            "--direction",
            direction,
            "--amount",
            &amount,
        ])?;
        Ok(r.resize)
    }

    pub fn workspace_create(
        &self,
        label: &str,
        cwd: Option<&str>,
        focus: bool,
    ) -> Result<NewWorkspace> {
        let mut args: Vec<&str> = vec!["workspace", "create", "--label", label];
        if let Some(cwd) = cwd {
            args.push("--cwd");
            args.push(cwd);
        }
        if !focus {
            args.push("--no-focus");
        }
        let r: WorkspaceCreateResult = self.json(&args)?;
        Ok(NewWorkspace {
            workspace_id: r.workspace.workspace_id,
            root_pane_id: r.root_pane.pane_id,
        })
    }

    pub fn workspace_close(&self, workspace_id: &str) -> Result<()> {
        self.output(&["workspace", "close", workspace_id])?;
        Ok(())
    }

    /// `herdr wait output <pane> --match <needle> --timeout <ms>`.
    /// Returns false on timeout rather than erroring.
    pub fn wait_output(&self, pane: &str, needle: &str, timeout_ms: u64) -> Result<bool> {
        let timeout = timeout_ms.to_string();
        let status = Command::new("herdr")
            .args(["wait", "output", pane, "--match", needle, "--timeout"])
            .arg(&timeout)
            .output()
            .context("running herdr wait output")?;
        Ok(status.status.success())
    }

    /// `herdr integration status`, or None when the subcommand is unavailable.
    pub fn integration_status(&self) -> Option<String> {
        Command::new("herdr")
            .args(["integration", "status"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
    }

    /// Type a line into another pane's terminal and submit it.
    ///
    /// Two calls rather than `pane run`: the orchestrator and workers are TUIs in
    /// raw mode, where an Enter arriving in the same write as the text can be
    /// consumed as part of the paste instead of submitting it. The settle delay
    /// scales with length, and the second Enter is a no-op-safe retry (Enter on
    /// an empty input does nothing).
    pub fn send_line(&self, pane: &str, message: &str) -> Result<()> {
        self.pane_send_text(pane, message)?;
        let settle = if message.len() > 1500 { 2 } else { 1 };
        std::thread::sleep(std::time::Duration::from_secs(settle));
        self.pane_send_keys(pane, "enter")?;
        std::thread::sleep(std::time::Duration::from_secs(1));
        self.pane_send_keys(pane, "enter")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pane_get_envelope() {
        let raw = r#"{"result":{"pane":{"pane_id":"w1-3","workspace_id":"w1"}}}"#;
        let r: Envelope<PaneResult> = serde_json::from_str(raw).unwrap();
        assert_eq!(r.result.pane.pane_id, "w1-3");
        assert_eq!(r.result.pane.workspace_id.as_deref(), Some("w1"));
        assert_eq!(r.result.pane.agent_session_id(), None);
    }

    #[test]
    fn parses_workspace_create_envelope() {
        let raw = r#"{"result":{"workspace":{"workspace_id":"w7"},
                                 "root_pane":{"pane_id":"w7-1"}}}"#;
        let r: Envelope<WorkspaceCreateResult> = serde_json::from_str(raw).unwrap();
        assert_eq!(r.result.workspace.workspace_id, "w7");
        assert_eq!(r.result.root_pane.pane_id, "w7-1");
    }

    /// herdr has reported the agent session under several shapes across
    /// versions; all of them must resolve.
    #[test]
    fn agent_session_id_handles_every_known_shape() {
        let cases = [
            (r#""abc-123""#, Some("abc-123")),
            (r#"{"session_id":"s1"}"#, Some("s1")),
            (r#"{"id":"s2"}"#, Some("s2")),
            (r#"{"ref":"s3"}"#, Some("s3")),
            (r#"{"other":"s4"}"#, None),
            (r#"null"#, None),
            (r#""""#, None),
        ];
        for (json, expected) in cases {
            let pane: Pane = serde_json::from_str(&format!(
                r#"{{"pane_id":"p","agent_session":{json}}}"#
            ))
            .unwrap();
            assert_eq!(pane.agent_session_id().as_deref(), expected, "for {json}");
        }
    }

    #[test]
    fn unknown_fields_in_herdr_responses_are_ignored() {
        let raw = r#"{"result":{"layout":{"workspace_id":"w1","tab_id":"t1",
            "area":{"x":0,"y":0,"width":200,"height":50},
            "panes":[{"pane_id":"w1-1","rect":{"x":0,"y":0,"width":50,"height":50},
                      "title":"zsh"}],
            "extra":true}},"jsonrpc":"2.0"}"#;
        let r: Envelope<LayoutResult> = serde_json::from_str(raw).unwrap();
        assert_eq!(r.result.layout.panes.len(), 1);
        assert_eq!(r.result.layout.area.width, 200);
    }
}
