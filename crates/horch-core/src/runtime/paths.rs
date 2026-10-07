//! Where horch keeps its state, and which project a command belongs to.
//!
//! Pure functions of an [`EnvSource`], moved out of `ledger.rs`. The rules are
//! unchanged: an empty variable counts as unset.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::context::EnvSource;

/// The directories a command works with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// The current directory at bootstrap, when the platform can say.
    pub cwd: Option<PathBuf>,
    /// `$HORCH_PROJECT_DIR`, else the current directory. `None` when neither
    /// is available; [`Paths::project`] reports that.
    pub project_dir: Option<PathBuf>,
    /// `$HORCH_STATE_DIR`, else `${XDG_STATE_HOME:-$HOME/.local/state}/horch`.
    pub state_root: PathBuf,
    /// The explicit `$HORCH_STATE_DIR`, if any. A pane does not inherit this
    /// process's environment, so an explicit state root is passed on to it.
    pub state_override: Option<PathBuf>,
    /// `$HORCH_DATA_DIR`, else `${XDG_DATA_HOME:-$HOME/.local/share}/horch`.
    /// A pane does not inherit this process's environment, so a pane command
    /// sets `HORCH_DATA_DIR` to this value.
    pub data_root: PathBuf,
    /// The system temp dir; the workspace mailboxes live under it.
    pub temp_root: PathBuf,
    /// `$HOME` (`%USERPROFILE%` on Windows), else `.`.
    pub home: PathBuf,
    /// Claude Code's managed settings file: `$HORCH_CLAUDE_MANAGED_SETTINGS`,
    /// else the platform path ([`CLAUDE_MANAGED_SETTINGS`]). A test points
    /// it into a temp dir, so no result depends on the machine's file.
    pub claude_managed_settings: PathBuf,
}

/// Where Claude Code reads managed (organisation) settings on this platform.
pub(crate) const CLAUDE_MANAGED_SETTINGS: &str = if cfg!(target_os = "macos") {
    "/Library/Application Support/ClaudeCode/managed-settings.json"
} else if cfg!(windows) {
    r"C:\Program Files\ClaudeCode\managed-settings.json"
} else {
    "/etc/claude-code/managed-settings.json"
};

impl Paths {
    pub fn from_env(env: &dyn EnvSource) -> Paths {
        let home = home_dir(env);
        Paths {
            cwd: env.current_dir(),
            project_dir: project_dir(env).ok(),
            state_root: state_root(env, &home),
            state_override: nonempty_path(env, "HORCH_STATE_DIR"),
            data_root: data_root(env, &home),
            temp_root: env.temp_dir(),
            home,
            claude_managed_settings: nonempty_path(env, "HORCH_CLAUDE_MANAGED_SETTINGS")
                .unwrap_or_else(|| PathBuf::from(CLAUDE_MANAGED_SETTINGS)),
        }
    }

    /// The project dir, or the error the ambient lookup gave.
    pub fn project(&self) -> Result<PathBuf> {
        self.project_dir
            .clone()
            .context("resolving the current directory")
    }

    /// The current directory, or the error the ambient lookup gave.
    pub fn current_dir(&self) -> Result<PathBuf> {
        self.cwd.clone().context("resolving the current directory")
    }

    /// Point every state lookup at `dir`, as `HORCH_STATE_DIR=<dir>` would.
    pub fn set_state_dir(&mut self, dir: impl Into<PathBuf>) {
        let dir = dir.into();
        self.state_root = dir.clone();
        self.state_override = Some(dir);
    }
}

/// Root directory for ledger files: `$HORCH_STATE_DIR`, else
/// `${XDG_STATE_HOME:-$HOME/.local/state}/horch`.
pub(crate) fn state_root(env: &dyn EnvSource, home: &Path) -> PathBuf {
    if let Some(dir) = nonempty_path(env, "HORCH_STATE_DIR") {
        return dir;
    }
    nonempty_path(env, "XDG_STATE_HOME")
        .unwrap_or_else(|| home.join(".local").join("state"))
        .join("horch")
}

/// horch's data directory (the skill store): `$HORCH_DATA_DIR`, else
/// `${XDG_DATA_HOME:-$HOME/.local/share}/horch`.
pub(crate) fn data_root(env: &dyn EnvSource, home: &Path) -> PathBuf {
    if let Some(dir) = nonempty_path(env, "HORCH_DATA_DIR") {
        return dir;
    }
    nonempty_path(env, "XDG_DATA_HOME")
        .unwrap_or_else(|| home.join(".local").join("share"))
        .join("horch")
}

/// The project dir a ledger belongs to: `$HORCH_PROJECT_DIR`, else the cwd.
pub(crate) fn project_dir(env: &dyn EnvSource) -> Result<PathBuf> {
    if let Some(dir) = nonempty_path(env, "HORCH_PROJECT_DIR") {
        return Ok(dir);
    }
    env.current_dir().context("resolving the current directory")
}

/// The user's home directory: `$HOME` (`%USERPROFILE%` on Windows), else `.`.
pub(crate) fn home_dir(env: &dyn EnvSource) -> PathBuf {
    let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    nonempty_path(env, key).unwrap_or_else(|| PathBuf::from("."))
}

/// A variable as a path, with an empty value counting as unset.
pub(crate) fn nonempty_path(env: &dyn EnvSource, key: &str) -> Option<PathBuf> {
    env.var_os(key).filter(|v| !v.is_empty()).map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::context::MapEnv;

    #[test]
    fn state_root_prefers_horch_state_dir_then_xdg_then_home() {
        let home = Path::new("/home/a");
        let env = MapEnv::new("/cwd");
        assert_eq!(
            state_root(&env, home),
            PathBuf::from("/home/a/.local/state/horch")
        );
        let env = env.with("XDG_STATE_HOME", "/xdg");
        assert_eq!(state_root(&env, home), PathBuf::from("/xdg/horch"));
        let env = env.with("HORCH_STATE_DIR", "/state");
        assert_eq!(state_root(&env, home), PathBuf::from("/state"));
        // Empty counts as unset.
        let env = MapEnv::new("/cwd")
            .with("HORCH_STATE_DIR", "")
            .with("XDG_STATE_HOME", "");
        assert_eq!(
            state_root(&env, home),
            PathBuf::from("/home/a/.local/state/horch")
        );
    }

    #[test]
    fn data_root_prefers_horch_data_dir_then_xdg_then_home() {
        let home = Path::new("/home/a");
        let env = MapEnv::new("/cwd");
        assert_eq!(
            data_root(&env, home),
            PathBuf::from("/home/a/.local/share/horch")
        );
        let env = env.with("XDG_DATA_HOME", "/xdg");
        assert_eq!(data_root(&env, home), PathBuf::from("/xdg/horch"));
        // The variable names the store itself: no `horch` is appended.
        let env = env.with("HORCH_DATA_DIR", "/data");
        assert_eq!(data_root(&env, home), PathBuf::from("/data"));
        assert_eq!(Paths::from_env(&env).data_root, PathBuf::from("/data"));
        // Empty counts as unset.
        let env = MapEnv::new("/cwd")
            .with("HORCH_DATA_DIR", "")
            .with("XDG_DATA_HOME", "");
        assert_eq!(
            data_root(&env, home),
            PathBuf::from("/home/a/.local/share/horch")
        );
    }

    #[test]
    fn project_dir_prefers_the_variable_over_the_cwd() {
        let env = MapEnv::new("/cwd");
        assert_eq!(project_dir(&env).unwrap(), PathBuf::from("/cwd"));
        let env = env.with("HORCH_PROJECT_DIR", "/proj");
        assert_eq!(project_dir(&env).unwrap(), PathBuf::from("/proj"));
        let env = MapEnv::default();
        assert!(project_dir(&env).is_err(), "no cwd and no variable");
        assert!(Paths::from_env(&env).project().is_err());
    }

    #[test]
    fn home_falls_back_to_dot() {
        let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
        assert_eq!(home_dir(&MapEnv::new("/cwd")), PathBuf::from("."));
        assert_eq!(
            home_dir(&MapEnv::new("/cwd").with(key, "")),
            PathBuf::from(".")
        );
        assert_eq!(
            home_dir(&MapEnv::new("/cwd").with(key, "/h")),
            PathBuf::from("/h")
        );
    }
}
