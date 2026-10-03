//! The pre-A3 path of the roster. Everything now lives in [`crate::roster`];
//! this module re-exports it so old paths keep resolving. Phase A12 removes it.
//!
//! The functions below are the old zero-argument forms of the roster calls
//! that now take the home directory and the roster override as parameters.
//! They read the process environment so callers that A3 does not own
//! (`launch.rs`, `skills.rs`, `plugins.rs`, the commands) keep compiling.
//! A4 or A12 moves those callers to `RuntimeContext` and deletes them.

use std::path::PathBuf;

use anyhow::Result;

pub use crate::roster::*;

// A2: from RuntimeContext
fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

impl Roster {
    /// [`Roster::load_layered`] under `$HOME` and `$HORCH_TEAMMATES_DIR`.
    pub fn load() -> Result<Roster> {
        Roster::load_with(None)
    }

    /// As [`Roster::load`], with `explicit` as the highest-precedence overlay.
    pub fn load_with(explicit: Option<&str>) -> Result<Roster> {
        // A2: from RuntimeContext
        let roster_override = std::env::var_os("HORCH_TEAMMATES_DIR").map(PathBuf::from);
        Roster::load_layered(home().as_deref(), roster_override.as_deref(), explicit)
    }
}

/// [`crate::roster::operator_status_line`] under `$HOME`.
pub fn operator_status_line() -> Option<serde_json::Value> {
    crate::roster::operator_status_line(home().as_deref())
}

/// [`crate::roster::operator_enabled_plugins`] under `$HOME`.
pub fn operator_enabled_plugins() -> Vec<String> {
    crate::roster::operator_enabled_plugins(home().as_deref())
}

/// [`crate::roster::operator_effort_warnings`] under `$HOME`, `$CODEX_HOME`
/// and `$CLAUDE_CODE_EFFORT_LEVEL`.
pub fn operator_effort_warnings() -> Vec<String> {
    crate::roster::operator_effort_warnings(
        home().as_deref(),
        &crate::codex::codex_home(&crate::agent::home_dir()),
        // A2: from RuntimeContext
        std::env::var("CLAUDE_CODE_EFFORT_LEVEL").ok().as_deref(),
    )
}

/// [`crate::roster::expand_home`] against `$HOME`.
pub fn expand_home(path: &str) -> PathBuf {
    crate::roster::expand_home(path, home().as_deref())
}
