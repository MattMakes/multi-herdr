//! The one error type of the marketplace. Each message sits on its variant.

use std::fmt;
use std::io;
use std::path::PathBuf;

use thiserror::Error;

use crate::git::GitError;

pub type Result<T> = std::result::Result<T, MarketplaceError>;

#[derive(Debug, Error)]
pub enum MarketplaceError {
    /// The source spec, skill id or revision is not usable, or the
    /// catalog does not know the skill.
    #[error("invalid skill source: {0}")]
    BadSource(String),
    /// The URL carried userinfo or a token. The text says which, and never
    /// holds the URL, so the secret cannot reach a log.
    #[error("skill source URL carries credentials ({0}); remove them")]
    Credentials(String),
    #[error("{0}")]
    Git(#[source] GitError),
    /// A branch or tag is not on the remote, or a commit could not be
    /// fetched.
    #[error("revision '{revision}' not found in {url}")]
    RevisionNotFound { url: String, revision: String },
    /// SKILL.md is missing, not UTF-8, has no frontmatter, or has an
    /// unknown key.
    #[error("invalid SKILL.md: {0}")]
    Manifest(String),
    /// The SKILL.md name or description breaks a rule.
    #[error("invalid SKILL.md: {0}")]
    SkillMd(String),
    /// A `..`, empty or otherwise unclean relative path.
    #[error("path '{}' is not a clean relative path", .0.display())]
    Traversal(PathBuf),
    #[error("absolute path '{}'", .0.display())]
    AbsolutePath(PathBuf),
    #[error("'{}' is a symlink", .0.display())]
    Symlink(PathBuf),
    /// A device, socket, fifo or git submodule.
    #[error("'{}' is not a regular file", .0.display())]
    NotRegularFile(PathBuf),
    #[error("skill has more than {0} files")]
    TooManyFiles(usize),
    #[error("'{}' is {bytes} bytes, over the limit", .path.display())]
    FileTooLarge { path: PathBuf, bytes: u64 },
    #[error("skill is over {0} bytes in total")]
    TotalTooLarge(u64),
    #[error("digest {actual} does not match the locked {expected}")]
    DigestMismatch { expected: String, actual: String },
    /// `marketplace.lock` cannot be parsed or has another version.
    #[error("marketplace.lock: {0}")]
    Lock(String),
    #[error("{0}")]
    Io(#[source] io::Error),
    /// A `FaultPoint` fired. Only tests and crash drills set one.
    #[error("fault injected at {0}")]
    Fault(&'static str),
}

impl MarketplaceError {
    /// An I/O error with the path or action in its message.
    pub(crate) fn io(context: impl fmt::Display, e: io::Error) -> Self {
        Self::Io(io::Error::new(e.kind(), format!("{context}: {e}")))
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

    #[test]
    fn only_git_and_io_have_a_source() {
        use std::error::Error;
        let git = MarketplaceError::from(GitError::NoDir);
        assert_eq!(
            git.source().unwrap().to_string(),
            "git needs an explicit working directory"
        );
        let io = MarketplaceError::io("ctx", io::Error::other("disk"));
        assert_eq!(io.source().unwrap().to_string(), "ctx: disk");
        assert!(MarketplaceError::Lock("x".into()).source().is_none());
        assert!(MarketplaceError::Symlink("l".into()).source().is_none());
    }
}
