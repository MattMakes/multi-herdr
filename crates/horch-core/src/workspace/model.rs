//! Public model types of the herdr wire protocol.
//!
//! The JSON parsing that fills them lives in [`crate::workspace::herdr`]; the serde derives
//! here are the wire shape.

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
    /// `Value`; use `Pane::agent_session_id` to read it.
    #[serde(default)]
    pub agent_session: Option<serde_json::Value>,
    /// The agent herdr detects in the pane (`claude`, `opencode`), if any.
    #[serde(default)]
    pub agent: Option<String>,
    /// herdr's view of that agent: `idle`, `working`, `blocked`, `done` or
    /// `unknown`. An agent that is still starting reads `unknown`, and text
    /// typed into it then is lost.
    #[serde(default)]
    pub agent_status: Option<String>,
}

impl Pane {
    /// The agent session id herdr reports for this pane, if any.
    ///
    /// Mirrors the jq fallback chain `.session_id // .id // .ref` used by the
    /// bash implementation, because the key has moved between herdr versions.
    pub(crate) fn agent_session_id(&self) -> Option<String> {
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
    /// The label it was created with, e.g. `horch telemetry`.
    #[serde(default)]
    pub label: Option<String>,
    /// The tab the workspace shows now. Used to put focus back after a
    /// rearrangement.
    #[serde(default)]
    pub active_tab_id: Option<String>,
}

/// What `herdr pane resize` reports back.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Resize {
    /// False when herdr declined the move, which in practice means the divider is
    /// against the minimum pane width. The loop-breaker for any resize sequence.
    pub changed: bool,
    pub layout: Layout,
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

/// What `herdr pane focus --direction` reports back.
///
/// The reply carries the whole post-move layout, so a walk that takes several
/// steps needs no `pane layout` call between them.
#[derive(Debug, Clone, Deserialize)]
pub struct Focus {
    /// False when herdr left the focus alone. `reason` says why: `no_neighbor`
    /// when nothing sits on that side of the origin pane. That is the loop
    /// breaker for any walk.
    pub changed: bool,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub focused_pane_id: Option<String>,
    pub layout: Layout,
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
            other => Err(format!(
                "unknown direction '{other}' (expected right or down)"
            )),
        }
    }
}

impl std::fmt::Display for Direction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which way `pane focus` steps to a NEIGHBOUR pane.
///
/// Deliberately separate from [`Direction`]: that one is split geometry, where
/// `left` and `up` are not words herdr accepts, and its `FromStr` rejects them
/// on purpose. herdr 0.8.2 has no focus-by-id for an ordinary pane
/// (`herdr plugin pane focus` answers `plugin_pane_not_found`), so stepping is
/// the only way to put a named pane back in focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FocusDir {
    Left,
    Right,
    Up,
    Down,
}

impl FocusDir {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            FocusDir::Left => "left",
            FocusDir::Right => "right",
            FocusDir::Up => "up",
            FocusDir::Down => "down",
        }
    }
}

impl std::fmt::Display for FocusDir {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
