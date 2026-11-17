//! The port of the launcher: each process of fleetd starts through 1
//! [`ProcessHost`] (`docs/specs/fleetd/design.md`, SR-7, "1 launcher for
//! each process").

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Child;

/// The environment rule of a process. Phase 1: the variables that the
/// caller gives, on top of the environment of fleetd. The launcher then
/// removes the forbidden keys and the git repository variables, also from
/// these variables.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvRule {
    /// The variables to set, in order.
    pub vars: Vec<(OsString, OsString)>,
}

/// 1 process to start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchSpec {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    /// The working folder. `None` keeps the folder of fleetd.
    pub cwd: Option<PathBuf>,
    pub env_rule: EnvRule,
    /// Whether the process leads a new session, so that it outlives the
    /// pane of its starter. Each process leads its own group in both cases.
    pub detach: bool,
    /// The file that stdout and stderr append to. `None` discards them.
    /// stdin is always empty.
    pub output: Option<PathBuf>,
}

/// A started process.
#[derive(Debug)]
pub struct Launched {
    pub pid: u32,
    /// The start time of the process, in the unit of
    /// `horch_platform::process::start_time`. `None` only when the process
    /// ended before the launcher read it.
    pub pid_start: Option<u64>,
    /// The process, unreaped. The caller waits for it or reaps it.
    pub child: Child,
}

/// Why a process did not start.
#[derive(Debug)]
pub enum LaunchError {
    /// A transaction of the ledger door is open on this thread. No effect
    /// outside the ledger starts inside a transaction.
    InsideTransaction,
    /// The output file did not open, the OS did not start the process, or
    /// the launcher could not check the started process. In the last case
    /// the launcher killed the process.
    Io(std::io::Error),
}

impl std::fmt::Display for LaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LaunchError::InsideTransaction => {
                write!(f, "a process start inside a ledger transaction is refused")
            }
            LaunchError::Io(e) => write!(f, "the launch failed: {e}"),
        }
    }
}

impl std::error::Error for LaunchError {}

impl From<std::io::Error> for LaunchError {
    fn from(e: std::io::Error) -> Self {
        LaunchError::Io(e)
    }
}

/// The 1 way to start a process of fleetd: an attempt, a job, the restart
/// of fleetd.
pub trait ProcessHost {
    /// Start `spec` in its own process group. Its environment has no
    /// forbidden key and no git repository variable
    /// ([`scrub_child_env`](crate::runtime::process::scrub_child_env)).
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, LaunchError>;
}
