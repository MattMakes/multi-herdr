//! The low-level git runner. Every git process this crate starts goes
//! through `GitRunner::run` or `GitRunner::output`, in an explicit directory,
//! with prompts, system config and hooks off. The core crate's VCS layer
//! wraps this module later, so it stays general: any argv, any directory.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Removed from every git child. Git never needs the API key, and the
/// operator's shell can carry an invalid, billed one.
const FORBIDDEN_ENV: [&str; 1] = ["ANTHROPIC_API_KEY"];

/// Removed from every git child, so the `-C` directory alone selects the
/// repository and its config. A caller inside a git hook, or under
/// `git rebase --exec`, has `GIT_DIR` set: on 2026-10-03 a gate run under
/// `git rebase -x` let test fixtures `git init` and commit into the real
/// repository. Every git child that horch or its tests start removes these
/// with [`scrub_repo_env`].
///
/// The list holds every name of `git rev-parse --local-env-vars` (a test
/// checks the host git), plus `GIT_NAMESPACE` and `GIT_CEILING_DIRECTORIES`.
/// [`scrub_repo_env`] also removes the numbered `GIT_CONFIG_KEY_<n>` and
/// `GIT_CONFIG_VALUE_<n>`.
pub const REPO_ENV: [&str; 18] = [
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CEILING_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_CONFIG",
    "GIT_CONFIG_COUNT",
    "GIT_CONFIG_PARAMETERS",
    "GIT_DIR",
    "GIT_GRAFT_FILE",
    "GIT_IMPLICIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_INTERNAL_SUPER_PREFIX",
    "GIT_NAMESPACE",
    "GIT_NO_REPLACE_OBJECTS",
    "GIT_OBJECT_DIRECTORY",
    "GIT_PREFIX",
    "GIT_REPLACE_REF_BASE",
    "GIT_SHALLOW_FILE",
    "GIT_WORK_TREE",
];

/// The prefixes of the numbered config variables that `GIT_CONFIG_COUNT`
/// counts.
const REPO_ENV_NUMBERED: [&str; 2] = ["GIT_CONFIG_KEY_", "GIT_CONFIG_VALUE_"];

/// Remove every [`REPO_ENV`] variable and every `GIT_CONFIG_KEY_<n>` and
/// `GIT_CONFIG_VALUE_<n>` from `cmd`, a git child: those this process has
/// and those set on `cmd`. Call it after any `env` call.
pub fn scrub_repo_env(cmd: &mut Command) {
    for k in REPO_ENV {
        cmd.env_remove(k);
    }
    let numbered: Vec<OsString> = std::env::vars_os()
        .map(|(k, _)| k)
        .chain(cmd.get_envs().map(|(k, _)| k.to_os_string()))
        .filter(|k| {
            k.to_str()
                .is_some_and(|k| REPO_ENV_NUMBERED.iter().any(|p| k.starts_with(p)))
        })
        .collect();
    for k in numbered {
        cmd.env_remove(k);
    }
}

/// Set on every git child.
const FIXED_ENV: [(&str, &str); 3] = [
    ("GIT_TERMINAL_PROMPT", "0"),
    ("LC_ALL", "C"),
    ("GIT_CONFIG_NOSYSTEM", "1"),
];

/// Passed as `-c` before every subcommand.
const FIXED_CONFIG: [&str; 1] = ["core.hooksPath=/dev/null"];

/// The files that show which repository `git_dir` is: its config, `HEAD`,
/// `packed-refs` and every loose ref, as one text. The [`REPO_ENV`]
/// regression tests compare it before and after git runs with `GIT_DIR`
/// aimed at `git_dir`.
pub fn repo_state(git_dir: &Path) -> String {
    let mut out = String::new();
    for name in ["config", "HEAD", "packed-refs"] {
        out += &format!(
            "{name}: {:?}\n",
            std::fs::read_to_string(git_dir.join(name)).ok()
        );
    }
    let mut stack = vec![git_dir.join("refs")];
    let mut refs = Vec::new();
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            if e.path().is_dir() {
                stack.push(e.path());
            } else {
                refs.push(format!(
                    "{}: {:?}",
                    e.path().display(),
                    std::fs::read_to_string(e.path()).ok()
                ));
            }
        }
    }
    refs.sort();
    out + &refs.join("\n")
}

#[derive(Debug, Clone)]
pub struct GitRunner {
    bin: PathBuf,
    env: Vec<(OsString, OsString)>,
}

#[derive(Debug, Clone)]
pub struct GitOutput {
    /// The exit code; -1 when a signal ended git.
    pub status: i32,
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
        status: i32,
        stderr: String,
    },
}

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDir => write!(f, "git needs an explicit working directory"),
            Self::Spawn { bin, source } => write!(f, "cannot run {}: {source}", bin.display()),
            Self::Failed {
                args,
                status,
                stderr,
            } => write!(f, "git {} failed ({status}): {stderr}", args.join(" ")),
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
        for k in FORBIDDEN_ENV {
            cmd.env_remove(k);
        }
        scrub_repo_env(&mut cmd);
        // The directory again as `-C`, so the repository never depends on
        // how git searches from its working directory.
        let at = std::path::absolute(dir).map_err(|source| GitError::Spawn {
            bin: self.bin.clone(),
            source,
        })?;
        cmd.arg("-C").arg(at);
        for c in FIXED_CONFIG {
            cmd.arg("-c").arg(c);
        }
        cmd.args(args);
        let out = cmd.output().map_err(|source| GitError::Spawn {
            bin: self.bin.clone(),
            source,
        })?;
        Ok(GitOutput {
            status: out.status.code().unwrap_or(-1),
            stdout: out.stdout,
            stderr: out.stderr,
        })
    }

    /// Run `git <args>` in `dir`; a non-zero exit is an error.
    pub fn run(&self, dir: &Path, args: &[&str]) -> Result<GitOutput, GitError> {
        let out = self.output(dir, args)?;
        if out.status != 0 {
            return Err(GitError::Failed {
                args: args.iter().map(|a| (*a).to_owned()).collect(),
                status: out.status,
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
            });
        }
        Ok(out)
    }
}
