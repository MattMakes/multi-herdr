//! Locating the agent CLIs a worker pane launches.
//!
//! A2 moved the resolvers to [`crate::runtime::bins`] and the PATH helpers to
//! [`crate::runtime::process`]. These names stay as thin wrappers that read
//! the process environment through [`ProcessEnv`], for callers that do not
//! take a [`RuntimeContext`](crate::runtime::RuntimeContext) yet. A12 removes
//! them.

use std::path::{Path, PathBuf};

use crate::runtime::bins::{self, BinOverrides};
use crate::runtime::context::{EnvSource, ProcessEnv};
use crate::runtime::{paths, process};

pub use crate::runtime::process::{make_executable, on_path_in, path_with_prepended, which_in};

fn overrides() -> BinOverrides {
    BinOverrides::from_env(&ProcessEnv)
}

/// See [`bins::claude_bin`].
pub fn claude_bin() -> PathBuf {
    bins::claude_bin(
        &overrides(),
        ProcessEnv.var_os("PATH").as_deref(),
        ProcessEnv.var("PATHEXT").as_deref(),
    )
}

/// See [`bins::codex_bin`].
pub fn codex_bin() -> PathBuf {
    bins::codex_bin(&overrides())
}

/// See [`bins::opencode_bin`].
pub fn opencode_bin() -> PathBuf {
    bins::opencode_bin(&overrides())
}

/// See [`bins::pi_bin`].
pub fn pi_bin() -> PathBuf {
    bins::pi_bin(&overrides())
}

/// See [`bins::prime_bin`].
pub fn prime_bin() -> PathBuf {
    bins::prime_bin(&overrides())
}

/// See [`bins::herdr_bin`].
pub fn herdr_bin() -> PathBuf {
    bins::herdr_bin(&overrides())
}

/// See [`bins::sqlite3_bin`].
pub fn sqlite3_bin() -> PathBuf {
    bins::sqlite3_bin(&overrides())
}

/// See [`bins::ollama_bin`].
pub fn ollama_bin() -> PathBuf {
    bins::ollama_bin(&overrides())
}

/// Find `name` on PATH, honouring `PATHEXT` on Windows.
pub fn which(name: &str) -> Option<PathBuf> {
    process::which(
        ProcessEnv.var_os("PATH").as_deref(),
        ProcessEnv.var("PATHEXT").as_deref(),
        name,
    )
}

/// Is `dir` already on PATH?
pub fn on_path(dir: &Path) -> bool {
    ProcessEnv
        .var_os("PATH")
        .map(|paths| on_path_in(&paths, dir))
        .unwrap_or(false)
}

/// The user's home directory. See [`paths::home_dir`].
pub fn home_dir() -> PathBuf {
    paths::home_dir(&ProcessEnv)
}

/// Put this binary's own directory first on this process's PATH.
#[deprecated(note = "A2: put RuntimeContext::prepend_own_dir_to_path on the child command")]
pub fn prepend_own_dir_to_path() -> std::io::Result<()> {
    let exe = std::env::current_exe()?;
    if let Some(next) = process::path_with_own_dir(&exe, ProcessEnv.var_os("PATH").as_deref()) {
        std::env::set_var("PATH", next);
    }
    Ok(())
}
