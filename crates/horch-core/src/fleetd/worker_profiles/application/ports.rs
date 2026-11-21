use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Child;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvRule {
    pub vars: Vec<(OsString, OsString)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchSpec {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub cwd: Option<PathBuf>,
    pub env_rule: EnvRule,
    pub detach: bool,
    pub output: Option<PathBuf>,
}

#[derive(Debug)]
pub struct Launched {
    pub pid: u32,
    pub pid_start: Option<u64>,
    pub child: Child,
}

#[derive(Debug)]
pub enum LaunchError {
    InsideTransaction,
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

pub trait ProcessHost {
    fn launch(&self, spec: &LaunchSpec) -> Result<Launched, LaunchError>;
}
