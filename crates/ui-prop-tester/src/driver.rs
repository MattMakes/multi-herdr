use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DriverKind {
    Browser,
    Terminal,
}

pub type Action = Value;

#[derive(Debug, Clone)]
pub struct Observation {
    pub timestamp_ms: u64,
    pub state: Value,
    pub screenshot: Option<Vec<u8>>,
    pub ended: bool,
}

#[derive(Debug, Clone)]
pub struct RemoteExtractor {
    pub id: u32,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExtractOutcome {
    Value(Value),
    Thrown { message: String, stack: String },
}

#[derive(Debug, Clone, Default)]
pub struct SpecSetup {
    pub custom_actions: Vec<CustomAction>,
}

#[derive(Debug, Clone)]
pub struct CustomAction {
    pub name: String,
    pub source: String,
}

#[derive(Debug)]
pub enum DriverError {
    Setup(String),
    Stalled(String),
    Failed(String),
}

impl fmt::Display for DriverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DriverError::Setup(message) => write!(f, "setup failed: {message}"),
            DriverError::Stalled(message) => write!(f, "target stalled: {message}"),
            DriverError::Failed(message) => write!(f, "driver failed: {message}"),
        }
    }
}

impl std::error::Error for DriverError {}

pub trait Driver {
    fn kind(&self) -> DriverKind;

    fn prepare(&mut self, _setup: &SpecSetup) -> Result<(), DriverError> {
        Ok(())
    }

    fn start(&mut self) -> Result<Observation, DriverError>;

    fn apply(&mut self, action: &Action) -> Result<Observation, DriverError>;

    fn extracts_remotely(&self) -> bool {
        false
    }

    fn extract(
        &mut self,
        _extractors: &[RemoteExtractor],
    ) -> Result<Vec<ExtractOutcome>, DriverError> {
        Ok(Vec::new())
    }

    fn match_recorded(&self, recorded: &Action, offered: &[Action]) -> Option<Action> {
        default_match_recorded(recorded, offered)
    }

    fn coverage_symbols(&mut self) -> Option<Value> {
        None
    }

    fn shutdown(&mut self) -> Result<(), DriverError>;
}

pub fn default_match_recorded(recorded: &Action, offered: &[Action]) -> Option<Action> {
    let kind = recorded.get("kind")?;
    offered
        .iter()
        .any(|candidate| candidate.get("kind") == Some(kind))
        .then(|| recorded.clone())
}

pub trait DriverFactory: Send + Sync {
    fn browser(&self, config: &BrowserConfig) -> Result<Box<dyn Driver>, DriverError>;
    fn terminal(&self, config: &TerminalConfig) -> Result<Box<dyn Driver>, DriverError>;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrowserConfig {
    pub origin: String,
    pub headless: bool,
    pub no_sandbox: bool,
    pub width: u32,
    pub height: u32,
    pub device_scale_factor: f64,
    pub grant_permissions: Vec<String>,
    pub virtual_time_policy: Option<String>,
    pub headers: Vec<(String, String)>,
    pub cookies: Vec<String>,
    pub allow_urls: Vec<String>,
    pub instrument_javascript: bool,
    pub coverage_symbols: Option<PathBuf>,
    pub remote_debugger: Option<String>,
    pub create_target: bool,
    pub chrome: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TerminalConfig {
    pub program: String,
    pub args: Vec<String>,
    pub columns: u16,
    pub rows: u16,
    pub scrollback_lines_max: usize,
    pub quiescence_timeout_ms: u64,
}
