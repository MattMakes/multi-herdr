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

impl MarketplaceError {
    /// The message of a variant that names a path.
    fn path_message(&self) -> Option<String> {
        match self {
            Self::Traversal(p) => Some(format!(
                "path '{}' is not a clean relative path",
                p.display()
            )),
            Self::AbsolutePath(p) => Some(format!("absolute path '{}'", p.display())),
            Self::Symlink(p) => Some(format!("'{}' is a symlink", p.display())),
            Self::NotRegularFile(p) => Some(format!("'{}' is not a regular file", p.display())),
            _ => None,
        }
    }

    /// The message of a variant that reports a size or count limit.
    fn limit_message(&self) -> Option<String> {
        match self {
            Self::TooManyFiles(limit) => Some(format!("skill has more than {limit} files")),
            Self::FileTooLarge { path, bytes } => Some(format!(
                "'{}' is {bytes} bytes, over the limit",
                path.display()
            )),
            Self::TotalTooLarge(limit) => Some(format!("skill is over {limit} bytes in total")),
            _ => None,
        }
    }

    /// The message of a variant that carries text or a source and names no
    /// path or limit.
    fn text_message(&self) -> Option<String> {
        match self {
            Self::BadSource(why) => Some(format!("invalid skill source: {why}")),
            Self::Credentials(why) => Some(format!(
                "skill source URL carries credentials ({why}); remove them"
            )),
            Self::RevisionNotFound { url, revision } => {
                Some(format!("revision '{revision}' not found in {url}"))
            }
            Self::Manifest(why) | Self::SkillMd(why) => Some(format!("invalid SKILL.md: {why}")),
            Self::DigestMismatch { expected, actual } => Some(format!(
                "digest {actual} does not match the locked {expected}"
            )),
            Self::Lock(why) => Some(format!("marketplace.lock: {why}")),
            Self::Fault(point) => Some(format!("fault injected at {point}")),
            _ => None,
        }
    }
}

impl fmt::Display for MarketplaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Git(e) => write!(f, "{e}"),
            Self::Io(e) => write!(f, "{e}"),
            other => {
                let text = other
                    .path_message()
                    .or_else(|| other.limit_message())
                    .or_else(|| other.text_message());
                f.write_str(&text.unwrap_or_default())
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variant_keeps_its_message() {
        let cases: Vec<(MarketplaceError, &str)> = vec![
            (
                MarketplaceError::BadSource("x".into()),
                "invalid skill source: x",
            ),
            (
                MarketplaceError::Credentials("a token".into()),
                "skill source URL carries credentials (a token); remove them",
            ),
            (
                MarketplaceError::Git(GitError::NoDir),
                "git needs an explicit working directory",
            ),
            (
                MarketplaceError::RevisionNotFound {
                    url: "u".into(),
                    revision: "r".into(),
                },
                "revision 'r' not found in u",
            ),
            (
                MarketplaceError::Manifest("m".into()),
                "invalid SKILL.md: m",
            ),
            (MarketplaceError::SkillMd("s".into()), "invalid SKILL.md: s"),
            (
                MarketplaceError::Traversal("a/../b".into()),
                "path 'a/../b' is not a clean relative path",
            ),
            (
                MarketplaceError::AbsolutePath("/etc".into()),
                "absolute path '/etc'",
            ),
            (MarketplaceError::Symlink("l".into()), "'l' is a symlink"),
            (
                MarketplaceError::NotRegularFile("f".into()),
                "'f' is not a regular file",
            ),
            (
                MarketplaceError::TooManyFiles(512),
                "skill has more than 512 files",
            ),
            (
                MarketplaceError::FileTooLarge {
                    path: "big".into(),
                    bytes: 9,
                },
                "'big' is 9 bytes, over the limit",
            ),
            (
                MarketplaceError::TotalTooLarge(100),
                "skill is over 100 bytes in total",
            ),
            (
                MarketplaceError::DigestMismatch {
                    expected: "e".into(),
                    actual: "a".into(),
                },
                "digest a does not match the locked e",
            ),
            (
                MarketplaceError::Lock("bad".into()),
                "marketplace.lock: bad",
            ),
            (MarketplaceError::Io(io::Error::other("disk")), "disk"),
            (
                MarketplaceError::Fault("after_rename"),
                "fault injected at after_rename",
            ),
        ];
        assert_eq!(cases.len(), 17);
        for (err, text) in cases {
            assert_eq!(err.to_string(), text);
        }
    }
}
