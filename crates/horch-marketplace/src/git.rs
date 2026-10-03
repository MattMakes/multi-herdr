//! The low-level git runner. Every git process this crate starts goes
//! through `GitRunner::run` or `GitRunner::output`, in an explicit directory,
//! with prompts, system config and hooks off. The core crate's VCS layer
//! wraps this module later, so it stays general: any argv, any directory.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

/// Removed from every git child. Git never needs the API key, and the
/// operator's shell can carry an invalid, billed one.
const FORBIDDEN_ENV: [&str; 1] = ["ANTHROPIC_API_KEY"];

/// Removed from every git child, so the directory alone selects the
/// repository. A caller inside a git hook would otherwise leak these.
const REPO_ENV: [&str; 7] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_NAMESPACE",
    "GIT_COMMON_DIR",
];

/// Set on every git child.
const FIXED_ENV: [(&str, &str); 3] = [
    ("GIT_TERMINAL_PROMPT", "0"),
    ("LC_ALL", "C"),
    ("GIT_CONFIG_NOSYSTEM", "1"),
];

/// Passed as `-c` before every subcommand.
const FIXED_CONFIG: [&str; 1] = ["core.hooksPath=/dev/null"];

#[derive(Debug, Clone)]
pub struct GitRunner {
    bin: PathBuf,
    env: Vec<(OsString, OsString)>,
}

#[derive(Debug, Clone)]
pub struct GitOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl GitOutput {
    /// Stdout as UTF-8 (lossy), with the trailing newline trimmed.
    pub fn stdout_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).trim_end().to_owned()
    }
}

#[derive(Debug)]
pub enum GitError {
    /// `run` was called with an empty directory path.
    NoDir,
    Spawn {
        bin: PathBuf,
        source: io::Error,
    },
    Failed {
        args: Vec<String>,
        code: Option<i32>,
        stderr: String,
    },
}

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDir => write!(f, "git needs an explicit working directory"),
            Self::Spawn { bin, source } => write!(f, "cannot run {}: {source}", bin.display()),
            Self::Failed { args, code, stderr } => {
                let code = code.map_or_else(|| "signal".to_owned(), |c| c.to_string());
                write!(f, "git {} failed ({code}): {stderr}", args.join(" "))
            }
        }
    }
}

impl std::error::Error for GitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl GitRunner {
    /// `bin` is the git executable. The caller resolves it (for example from
    /// `HORCH_GIT_BIN`); this crate never reads the process environment.
    pub fn new(bin: impl Into<PathBuf>) -> Self {
        Self {
            bin: bin.into(),
            env: Vec::new(),
        }
    }

    /// Add one variable to every child, for example `GIT_CONFIG_GLOBAL` in
    /// a hermetic test. The fixed variables still win.
    pub fn with_env(mut self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> Self {
        self.env
            .push((key.as_ref().to_owned(), value.as_ref().to_owned()));
        self
    }

    pub fn bin(&self) -> &Path {
        &self.bin
    }

    /// Run `git <args>` in `dir` and return its output whatever the exit code.
    pub fn output(&self, dir: &Path, args: &[&str]) -> Result<GitOutput, GitError> {
        if dir.as_os_str().is_empty() {
            return Err(GitError::NoDir);
        }
        let mut cmd = Command::new(&self.bin);
        cmd.current_dir(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (k, v) in &self.env {
            cmd.env(k, v);
        }
        for (k, v) in FIXED_ENV {
            cmd.env(k, v);
        }
        for k in FORBIDDEN_ENV.iter().chain(&REPO_ENV) {
            cmd.env_remove(k);
        }
        for c in FIXED_CONFIG {
            cmd.arg("-c").arg(c);
        }
        cmd.args(args);
        let out = cmd.output().map_err(|source| GitError::Spawn {
            bin: self.bin.clone(),
            source,
        })?;
        Ok(GitOutput {
            status: out.status,
            stdout: out.stdout,
            stderr: out.stderr,
        })
    }

    /// Run `git <args>` in `dir`; a non-zero exit is an error.
    pub fn run(&self, dir: &Path, args: &[&str]) -> Result<GitOutput, GitError> {
        let out = self.output(dir, args)?;
        if !out.status.success() {
            return Err(GitError::Failed {
                args: args.iter().map(|a| (*a).to_owned()).collect(),
                code: out.status.code(),
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
            });
        }
        Ok(out)
    }
}
