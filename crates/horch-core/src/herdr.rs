//! Typed wrapper around the `herdr` CLI, replacing the shell's `herdr ... | jq`
//! pipelines.
//!
//! Verified against herdr 0.6.1 and re-checked against 0.8.0. The splitting and
//! messaging surface is deliberately limited to what 0.6.1 advertises: no `pane
//! current`, and no `--env` or `--ratio` on `pane split`.
//!
//! The tab and `pane move` calls that `horch tile` needs arrived later and are
//! verified against 0.8.2 (`ai_docs/reports/horch-tile-herdr-surface.md`). They
//! are additions, so a caller that never tiles still only needs 0.6.1.

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
    /// Which tab holds the pane. `pane list` reports it for every pane, which is
    /// how a caller covers a whole workspace with one call.
    #[serde(default)]
    pub tab_id: Option<String>,
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
    /// Which pane this tab hands the keyboard to. Absent on older herdr.
    #[serde(default)]
    pub focused_pane_id: Option<String>,
    /// True while one pane fills the tab. herdr refuses to move panes into or
    /// out of a zoomed tab (`changed: false`, `reason: "zoomed_tab"`), so a
    /// caller that rearranges panes has to check this first.
    #[serde(default)]
    pub zoomed: bool,
}

/// One tab of a workspace, as `herdr tab list` reports it.
#[derive(Debug, Clone, Deserialize)]
pub struct Tab {
    pub tab_id: String,
    pub workspace_id: String,
    /// 1-based position in the workspace, which is the order the tab strip shows.
    pub number: i64,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub pane_count: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Workspace {
    pub workspace_id: String,
    /// The tab the workspace shows now. Used to put focus back after a
    /// rearrangement.
    #[serde(default)]
    pub active_tab_id: Option<String>,
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
struct TabListResult {
    tabs: Vec<Tab>,
}

#[derive(Debug, Deserialize)]
struct WorkspaceListResult {
    workspaces: Vec<Workspace>,
}

/// What `herdr pane move` reports back.
///
/// `source_layout` is absent when the move emptied the source tab: herdr closes
/// a tab that loses its last pane, so there is no layout left to report.
/// `created_tab` is present only for the `--new-tab` form.
#[derive(Debug, Clone, Deserialize)]
pub struct Move {
    /// False when herdr declined the move. `reason` says why: `same_tab` for a
    /// move into the tab the pane is already in, `zoomed_tab` when either tab is
    /// zoomed.
    pub changed: bool,
    #[serde(default)]
    pub reason: Option<String>,
    pub pane: Pane,
    #[serde(default)]
    pub created_tab: Option<Tab>,
    #[serde(default)]
    pub source_layout: Option<Layout>,
    #[serde(default)]
    pub target_layout: Option<Layout>,
}

#[derive(Debug, Deserialize)]
struct MoveResult {
    move_result: Move,
}

#[derive(Debug, Deserialize)]
struct TabCreateResult {
    tab: Tab,
    root_pane: Pane,
}

/// A freshly created tab and the shell pane herdr starts it with.
#[derive(Debug, Clone)]
pub struct NewTab {
    pub tab_id: String,
    pub root_pane_id: String,
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
    /// The tab the root pane landed in, which is the workspace's first tab.
    pub tab_id: Option<String>,
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

    /// Which pane the tab holding `pane` hands the keyboard to.
    ///
    /// `pane layout` reports the focused pane of the whole tab, not of the pane
    /// asked about, which is what a caller wants before it rearranges the tab.
    pub fn focused_pane(&self, pane: Option<&str>) -> Result<Option<String>> {
        Ok(self.pane_layout(pane)?.focused_pane_id)
    }

    /// `herdr tab list --workspace <ws>`, in tab-strip order.
    ///
    /// Always scoped to a workspace: without `--workspace` herdr returns the tabs
    /// of every workspace it has open.
    pub fn tab_list(&self, workspace_id: &str) -> Result<Vec<Tab>> {
        let r: TabListResult = self.json(&["tab", "list", "--workspace", workspace_id])?;
        let mut tabs = r.tabs;
        tabs.sort_by_key(|t| t.number);
        Ok(tabs)
    }

    /// `herdr workspace list`.
    pub fn workspace_list(&self) -> Result<Vec<Workspace>> {
        let r: WorkspaceListResult = self.json(&["workspace", "list"])?;
        Ok(r.workspaces)
    }

    /// `herdr tab create`.
    ///
    /// Note that herdr starts the new tab with a shell pane, whose id comes back
    /// as `root_pane_id`. A caller that wanted an empty tab has to close it - so
    /// a caller moving an existing pane somewhere new is better served by
    /// [`Self::pane_move_new_tab`], which creates the tab around that pane and
    /// adds no shell.
    pub fn tab_create(&self, workspace_id: &str, label: &str, focus: bool) -> Result<NewTab> {
        let mut args: Vec<&str> = vec![
            "tab",
            "create",
            "--workspace",
            workspace_id,
            "--label",
            label,
        ];
        args.push(if focus { "--focus" } else { "--no-focus" });
        let r: TabCreateResult = self.json(&args)?;
        Ok(NewTab {
            tab_id: r.tab.tab_id,
            root_pane_id: r.root_pane.pane_id,
        })
    }

    /// `herdr tab close <tab>`.
    ///
    /// DESTRUCTIVE: this closes every pane in the tab, killing whatever runs in
    /// them, and closing a workspace's last tab closes the workspace. It is not
    /// how an emptied tab is disposed of - herdr closes a tab that loses its last
    /// pane on its own - so `horch tile` never calls this.
    pub fn tab_close(&self, tab_id: &str) -> Result<()> {
        self.output(&["tab", "close", tab_id])?;
        Ok(())
    }

    /// `herdr tab focus <tab>`. The only way to focus a specific pane's tab:
    /// `pane focus` moves to a NEIGHBOUR, not to an id.
    pub fn tab_focus(&self, tab_id: &str) -> Result<()> {
        self.output(&["tab", "focus", tab_id])?;
        Ok(())
    }

    /// `herdr pane move <pane> --tab <tab> --split <d> [--target-pane <target>]
    /// [--ratio <r>] --no-focus`.
    ///
    /// Keeps the pane id and the running process (verified against herdr 0.8.2;
    /// see `ai_docs/reports/horch-tile-herdr-surface.md`). `ratio` is the share
    /// the TARGET pane keeps, so the moved pane gets `1 - ratio`.
    ///
    /// A move into the tab the pane already occupies is refused with
    /// `changed: false, reason: "same_tab"`: repositioning a pane inside its own
    /// tab means moving it out and back.
    pub fn pane_move(
        &self,
        pane: &str,
        tab: &str,
        direction: Direction,
        target: Option<&str>,
        ratio: Option<f64>,
    ) -> Result<Move> {
        let mut args: Vec<String> = vec![
            "pane".into(),
            "move".into(),
            pane.into(),
            "--tab".into(),
            tab.into(),
            "--split".into(),
            direction.as_str().into(),
        ];
        if let Some(target) = target {
            args.push("--target-pane".into());
            args.push(target.into());
        }
        if let Some(ratio) = ratio {
            args.push("--ratio".into());
            // Match herdr's own four-decimal storage of split ratios.
            args.push(format!("{ratio:.4}"));
        }
        args.push("--no-focus".into());
        let r: MoveResult = self.json(&args)?;
        Ok(r.move_result)
    }

    /// `herdr pane move <pane> --new-tab --label <label> --no-focus`.
    ///
    /// The new tab holds only the moved pane, and its id comes back as
    /// `created_tab`. This form takes neither `--split` nor `--ratio`; the label
    /// flag is `--label`, not `--tab-label`.
    pub fn pane_move_new_tab(&self, pane: &str, label: &str) -> Result<Move> {
        let r: MoveResult = self.json(&[
            "pane", "move", pane, "--new-tab", "--label", label, "--no-focus",
        ])?;
        Ok(r.move_result)
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
            tab_id: r.root_pane.tab_id.clone(),
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

    /// The real 0.8.2 reply, trimmed. Two fields are conditional and must not be
    /// required: `source_layout` is absent when the move emptied and so closed the
    /// source tab, and `created_tab` is present only for `--new-tab`.
    #[test]
    fn parses_a_pane_move_that_closed_its_source_tab() {
        let raw = r#"{"result":{"type":"pane_move","move_result":{
            "changed":true,
            "pane":{"pane_id":"w19:p3","tab_id":"w19:t3","workspace_id":"w19"},
            "previous_pane_id":"w19:p3","previous_tab_id":"w19:t1",
            "created_tab":{"tab_id":"w19:t3","workspace_id":"w19","label":"workers 2",
                           "number":3,"pane_count":1},
            "focused_pane_id":"w19:p3"}}}"#;
        let r: Envelope<MoveResult> = serde_json::from_str(raw).unwrap();
        let m = r.result.move_result;
        assert!(m.changed);
        assert_eq!(m.pane.tab_id.as_deref(), Some("w19:t3"));
        assert_eq!(m.created_tab.unwrap().tab_id, "w19:t3");
        assert!(m.source_layout.is_none(), "the source tab was closed");
        assert!(m.reason.is_none());
    }

    /// herdr declines a move rather than failing the call, so the reason is the
    /// only way a caller learns that nothing happened.
    #[test]
    fn parses_a_declined_pane_move_with_its_reason() {
        let raw = r#"{"result":{"move_result":{"changed":false,"reason":"same_tab",
            "pane":{"pane_id":"w1:p2"}}}}"#;
        let r: Envelope<MoveResult> = serde_json::from_str(raw).unwrap();
        assert!(!r.result.move_result.changed);
        assert_eq!(r.result.move_result.reason.as_deref(), Some("same_tab"));
    }

    #[test]
    fn parses_a_tab_list_and_keeps_tab_strip_order() {
        let raw = r#"{"result":{"type":"tab_list","tabs":[
            {"tab_id":"w0:t2","workspace_id":"w0","label":"workers 2","number":2,"pane_count":6},
            {"tab_id":"w0:t1","workspace_id":"w0","label":"1","number":1,"pane_count":5}]}}"#;
        let r: Envelope<TabListResult> = serde_json::from_str(raw).unwrap();
        let mut tabs = r.result.tabs;
        tabs.sort_by_key(|t| t.number);
        assert_eq!(
            tabs.iter().map(|t| t.tab_id.as_str()).collect::<Vec<_>>(),
            ["w0:t1", "w0:t2"]
        );
    }

    /// A tiler has to know the focused pane to hand focus back, and the zoom
    /// state because herdr refuses to move panes in a zoomed tab.
    #[test]
    fn a_layout_reports_the_focused_pane_and_the_zoom_state() {
        let raw = r#"{"result":{"layout":{"workspace_id":"w0","tab_id":"w0:t1",
            "area":{"x":26,"y":1,"width":184,"height":53},
            "focused_pane_id":"w0:p1","zoomed":true,
            "panes":[{"pane_id":"w0:p1","rect":{"x":26,"y":1,"width":184,"height":53}}],
            "splits":[]}}}"#;
        let r: Envelope<LayoutResult> = serde_json::from_str(raw).unwrap();
        assert_eq!(r.result.layout.focused_pane_id.as_deref(), Some("w0:p1"));
        assert!(r.result.layout.zoomed);
    }

    /// Older herdr omits both, and the wrapper still has to parse the reply.
    #[test]
    fn a_layout_without_focus_or_zoom_still_parses() {
        let raw = r#"{"result":{"layout":{"workspace_id":"w0","tab_id":"w0:t1",
            "area":{"x":0,"y":0,"width":80,"height":24},"panes":[]}}}"#;
        let r: Envelope<LayoutResult> = serde_json::from_str(raw).unwrap();
        assert_eq!(r.result.layout.focused_pane_id, None);
        assert!(!r.result.layout.zoomed);
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
