//! The one error type of the marketplace. Written by hand: the crate
//! allowlist has no `thiserror`.

use std::fmt;
use std::io;
use std::path::PathBuf;

use crate::git::GitError;

pub type Result<T> = std::result::Result<T, MarketplaceError>;

#[derive(Debug)]
pub enum MarketplaceError {
    /// The source spec, skill id or revision is not usable, or the
    /// catalog does not know the skill.
    BadSource(String),
    /// The URL carried userinfo or a token. The text says which, and never
    /// holds the URL, so the secret cannot reach a log.
    Credentials(String),
    Git(GitError),
    /// A branch or tag is not on the remote, or a commit could not be
    /// fetched.
    RevisionNotFound {
        url: String,
        revision: String,
    },
    /// SKILL.md is missing, not UTF-8, has no frontmatter, or has an
    /// unknown key.
    Manifest(String),
    /// The SKILL.md name or description breaks a rule.
    SkillMd(String),
    /// A `..`, empty or otherwise unclean relative path.
    Traversal(PathBuf),
    AbsolutePath(PathBuf),
    Symlink(PathBuf),
    /// A device, socket, fifo or git submodule.
    NotRegularFile(PathBuf),
    TooManyFiles(usize),
    FileTooLarge {
        path: PathBuf,
        bytes: u64,
    },
    TotalTooLarge(u64),
    DigestMismatch {
        expected: String,
        actual: String,
    },
    /// `marketplace.lock` cannot be parsed or has another version.
    Lock(String),
    Io(io::Error),
    /// A `FaultPoint` fired. Only tests and crash drills set one.
    Fault(&'static str),
}

impl MarketplaceError {
    /// An I/O error with the path or action in its message.
    pub(crate) fn io(context: impl fmt::Display, e: io::Error) -> Self {
        Self::Io(io::Error::new(e.kind(), format!("{context}: {e}")))
    }
}

impl fmt::Display for MarketplaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadSource(why) => write!(f, "invalid skill source: {why}"),
            Self::Credentials(why) => write!(
                f,
                "skill source URL carries credentials ({why}); remove them"
            ),
            Self::Git(e) => write!(f, "{e}"),
            Self::RevisionNotFound { url, revision } => {
                write!(f, "revision '{revision}' not found in {url}")
            }
            Self::Manifest(why) => write!(f, "invalid SKILL.md: {why}"),
            Self::SkillMd(why) => write!(f, "invalid SKILL.md: {why}"),
            Self::Traversal(p) => write!(f, "path '{}' is not a clean relative path", p.display()),
            Self::AbsolutePath(p) => write!(f, "absolute path '{}'", p.display()),
            Self::Symlink(p) => write!(f, "'{}' is a symlink", p.display()),
            Self::NotRegularFile(p) => write!(f, "'{}' is not a regular file", p.display()),
            Self::TooManyFiles(limit) => write!(f, "skill has more than {limit} files"),
            Self::FileTooLarge { path, bytes } => {
                write!(f, "'{}' is {bytes} bytes, over the limit", path.display())
            }
            Self::TotalTooLarge(limit) => write!(f, "skill is over {limit} bytes in total"),
            Self::DigestMismatch { expected, actual } => {
                write!(f, "digest {actual} does not match the locked {expected}")
            }
            Self::Lock(why) => write!(f, "marketplace.lock: {why}"),
            Self::Io(e) => write!(f, "{e}"),
            Self::Fault(point) => write!(f, "fault injected at {point}"),
        }
    }
}

impl std::error::Error for MarketplaceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Git(e) => Some(e),
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<GitError> for MarketplaceError {
    fn from(e: GitError) -> Self {
        Self::Git(e)
    }
}
