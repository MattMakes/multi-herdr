use serde::Deserialize;

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
    #[serde(default)]
    pub tab_id: Option<String>,
    #[serde(default)]
    pub agent_session: Option<serde_json::Value>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub agent_status: Option<String>,
    #[serde(default)]
    pub state_change_seq: Option<i64>,
}

pub fn at_prompt(agent_status: Option<&str>) -> bool {
    matches!(agent_status, Some("idle" | "done"))
}

impl Pane {
    pub fn at_prompt(&self) -> bool {
        at_prompt(self.agent_status.as_deref())
    }

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

#[derive(Debug, Clone, Deserialize)]
pub struct LayoutPane {
    pub pane_id: String,
    pub rect: Rect,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Layout {
    pub workspace_id: String,
    pub tab_id: String,
    pub area: Rect,
    pub panes: Vec<LayoutPane>,
    #[serde(default)]
    pub focused_pane_id: Option<String>,
    #[serde(default)]
    pub zoomed: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Tab {
    pub tab_id: String,
    pub workspace_id: String,
    pub number: i64,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub pane_count: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Workspace {
    pub workspace_id: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub active_tab_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Resize {
    pub changed: bool,
    pub layout: Layout,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Move {
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

#[derive(Debug, Clone, Deserialize)]
pub struct Focus {
    pub changed: bool,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub focused_pane_id: Option<String>,
    pub layout: Layout,
}

#[derive(Debug, Clone)]
pub struct NewWorkspace {
    pub workspace_id: String,
    pub root_pane_id: String,
    pub tab_id: Option<String>,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusDir {
    Left,
    Right,
    Up,
    Down,
}

impl FocusDir {
    pub fn as_str(self) -> &'static str {
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
