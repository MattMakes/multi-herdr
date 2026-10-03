//! The one error type of the marketplace. Written by hand: the crate
//! allowlist has no `thiserror`.

use std::fmt;
use std::io;
use std::path::PathBuf;

use crate::git::GitError;

pub type Result<T> = std::result::Result<T, MarketplaceError>;

/// A rule that a staged skill tree broke. See `integrity`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntegrityViolation {
    AbsolutePath(String),
    ParentComponent(String),
    InvalidPath(String),
    Symlink(String),
    NotRegularFile(String),
    TooManyFiles {
        limit: usize,
    },
    FileTooLarge {
        path: String,
        bytes: u64,
        limit: u64,
    },
    TotalTooLarge {
        limit: u64,
    },
}

impl fmt::Display for IntegrityViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AbsolutePath(p) => write!(f, "absolute path '{p}'"),
            Self::ParentComponent(p) => write!(f, "path '{p}' has a '..' component"),
            Self::InvalidPath(p) => write!(f, "path '{p}' is not a clean relative UTF-8 path"),
            Self::Symlink(p) => write!(f, "'{p}' is a symlink"),
            Self::NotRegularFile(p) => write!(f, "'{p}' is not a regular file or directory"),
            Self::TooManyFiles { limit } => write!(f, "more than {limit} files"),
            Self::FileTooLarge { path, bytes, limit } => {
                write!(f, "'{path}' is {bytes} bytes; the limit is {limit}")
            }
            Self::TotalTooLarge { limit } => write!(f, "total size is over {limit} bytes"),
        }
    }
}

#[derive(Debug)]
pub enum MarketplaceError {
    InvalidSkillId(String),
    InvalidSource(String),
    /// The URL carried userinfo or a token. The URL itself is never stored
    /// here, so the secret cannot reach a log.
    CredentialsInUrl,
    RevisionNotFound {
        url: String,
        revision: String,
    },
    UnknownBundled(String),
    InvalidManifest {
        path: PathBuf,
        reason: String,
    },
    Integrity(IntegrityViolation),
    DigestMismatch {
        id: String,
        expected: String,
        actual: String,
    },
    Lockfile {
        path: PathBuf,
        reason: String,
    },
    /// A `FaultPoint` fired. Only tests and crash drills set one.
    Fault(&'static str),
    Git(GitError),
    Io {
        context: String,
        source: io::Error,
    },
}

impl MarketplaceError {
    pub(crate) fn io(context: impl Into<String>, source: io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }
}

impl fmt::Display for MarketplaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSkillId(id) => write!(
                f,
                "invalid skill id '{id}': use lowercase ASCII letters, digits and single '-'"
            ),
            Self::InvalidSource(why) => write!(f, "invalid skill source: {why}"),
            Self::CredentialsInUrl => write!(
                f,
                "skill source URL carries credentials; remove the user, password or token"
            ),
            Self::RevisionNotFound { url, revision } => {
                write!(f, "revision '{revision}' not found in {url}")
            }
            Self::UnknownBundled(name) => write!(f, "unknown bundled skill '{name}'"),
            Self::InvalidManifest { path, reason } => {
                write!(f, "{}: invalid SKILL.md: {reason}", path.display())
            }
            Self::Integrity(v) => write!(f, "skill tree rejected: {v}"),
            Self::DigestMismatch {
                id,
                expected,
                actual,
            } => write!(
                f,
                "skill '{id}': digest {actual} does not match locked {expected}"
            ),
            Self::Lockfile { path, reason } => write!(f, "{}: {reason}", path.display()),
            Self::Fault(point) => write!(f, "fault injected at {point}"),
            Self::Git(e) => write!(f, "{e}"),
            Self::Io { context, source } => write!(f, "{context}: {source}"),
        }
    }
}

impl std::error::Error for MarketplaceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Git(e) => Some(e),
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl From<GitError> for MarketplaceError {
    fn from(e: GitError) -> Self {
        Self::Git(e)
    }
}

impl From<IntegrityViolation> for MarketplaceError {
    fn from(v: IntegrityViolation) -> Self {
        Self::Integrity(v)
    }
}
