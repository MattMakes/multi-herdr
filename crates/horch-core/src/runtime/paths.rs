use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::context::EnvSource;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub cwd: Option<PathBuf>,
    pub project_dir: Option<PathBuf>,
    pub state_root: PathBuf,
    pub state_override: Option<PathBuf>,
    pub default_state_root: PathBuf,
    pub data_root: PathBuf,
    pub ledger_db: PathBuf,
    pub ledger_override: Option<PathBuf>,
    pub temp_root: PathBuf,
    pub home: PathBuf,
    pub claude_managed_settings: PathBuf,
}

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
            default_state_root: default_state_root(env, &home),
            data_root: data_root(env, &home),
            ledger_db: nonempty_path(env, "HORCH_LEDGER_DB")
                .unwrap_or_else(|| crate::taskledger::store::default_path(&home)),
            ledger_override: nonempty_path(env, "HORCH_LEDGER_DB"),
            temp_root: env.temp_dir(),
            home,
            claude_managed_settings: nonempty_path(env, "HORCH_CLAUDE_MANAGED_SETTINGS")
                .unwrap_or_else(|| PathBuf::from(CLAUDE_MANAGED_SETTINGS)),
        }
    }

    pub fn project(&self) -> Result<PathBuf> {
        self.project_dir
            .clone()
            .context("resolving the current directory")
    }

    pub fn current_dir(&self) -> Result<PathBuf> {
        self.cwd.clone().context("resolving the current directory")
    }

    pub fn ledger_isolation_warning(&self) -> Option<String> {
        let moved = self.state_root != self.default_state_root;
        (moved && self.ledger_override.is_none()).then(|| {
            format!(
                "the task ledger is still {}; set HORCH_LEDGER_DB to isolate it",
                self.ledger_db.display()
            )
        })
    }

    pub fn set_state_dir(&mut self, dir: impl Into<PathBuf>) {
        let dir = dir.into();
        self.state_root = dir.clone();
        self.state_override = Some(dir);
    }
}

pub(crate) fn state_root(env: &dyn EnvSource, home: &Path) -> PathBuf {
    nonempty_path(env, "HORCH_STATE_DIR").unwrap_or_else(|| default_state_root(env, home))
}

fn default_state_root(env: &dyn EnvSource, home: &Path) -> PathBuf {
    nonempty_path(env, "XDG_STATE_HOME")
        .unwrap_or_else(|| home.join(".local").join("state"))
        .join("horch")
}

pub(crate) fn data_root(env: &dyn EnvSource, home: &Path) -> PathBuf {
    if let Some(dir) = nonempty_path(env, "HORCH_DATA_DIR") {
        return dir;
    }
    nonempty_path(env, "XDG_DATA_HOME")
        .unwrap_or_else(|| home.join(".local").join("share"))
        .join("horch")
}

pub(crate) fn project_dir(env: &dyn EnvSource) -> Result<PathBuf> {
    if let Some(dir) = nonempty_path(env, "HORCH_PROJECT_DIR") {
        return Ok(dir);
    }
    env.current_dir().context("resolving the current directory")
}

pub(crate) fn home_dir(env: &dyn EnvSource) -> PathBuf {
    let key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    nonempty_path(env, key).unwrap_or_else(|| PathBuf::from("."))
}

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
        let env = env.with("HORCH_DATA_DIR", "/data");
        assert_eq!(data_root(&env, home), PathBuf::from("/data"));
        assert_eq!(Paths::from_env(&env).data_root, PathBuf::from("/data"));
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
    fn a_state_dir_without_a_ledger_override_warns_with_the_ledger_path() {
        let home = MapEnv::new("/cwd").with("HOME", "/h");
        assert_eq!(Paths::from_env(&home).ledger_isolation_warning(), None);
        let scratch = home.clone().with("HORCH_STATE_DIR", "/scratch");
        let expected = format!(
            "the task ledger is still {}; set HORCH_LEDGER_DB to isolate it",
            Path::new("/h")
                .join(".horch")
                .join("ledger.sqlite3")
                .display()
        );
        assert_eq!(
            Paths::from_env(&scratch).ledger_isolation_warning(),
            Some(expected)
        );
        let default = Path::new("/h").join(".local").join("state").join("horch");
        let pane = home
            .clone()
            .with("HORCH_STATE_DIR", &default.to_string_lossy());
        assert_eq!(Paths::from_env(&pane).ledger_isolation_warning(), None);
        let xdg_pane = home.clone().with("XDG_STATE_HOME", "/xdg").with(
            "HORCH_STATE_DIR",
            &Path::new("/xdg").join("horch").to_string_lossy(),
        );
        assert_eq!(Paths::from_env(&xdg_pane).ledger_isolation_warning(), None);
        let isolated = scratch.with("HORCH_LEDGER_DB", "/scratch/ledger.sqlite3");
        assert_eq!(Paths::from_env(&isolated).ledger_isolation_warning(), None);
        let ledger_only = home.with("HORCH_LEDGER_DB", "/scratch/ledger.sqlite3");
        assert_eq!(
            Paths::from_env(&ledger_only).ledger_isolation_warning(),
            None
        );
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
