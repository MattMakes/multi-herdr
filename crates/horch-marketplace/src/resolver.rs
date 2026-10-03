//! Resolve a `SkillSource` to a pinned `ResolvedSource`. Together with the
//! fetch in `source`, this is the only place that dispatches on the source
//! kind, so a new source is added here and there and nowhere else.

use std::path::{Path, PathBuf};

use crate::catalog::{BundledSkill, Catalog};
use crate::error::{MarketplaceError, Result};
use crate::git::GitRunner;
use crate::integrity::{check_relative_path, digest12};
use crate::model::{check_url, is_full_sha, GitRevision, SkillId, SkillSource, SkillVersion};

/// A source with everything pinned that the fetch needs.
#[derive(Debug, Clone)]
pub struct ResolvedSource {
    pub source: SkillSource,
    pub origin: ResolvedOrigin,
    /// The skill directory name, when the source names one. SKILL.md
    /// `name` must equal it.
    pub expected_name: Option<String>,
    pub requested_revision: Option<String>,
    pub resolved_commit: Option<String>,
}

#[derive(Debug, Clone)]
pub enum ResolvedOrigin {
    Bundled(BundledSkill),
    Local(PathBuf),
    Git {
        url: String,
        commit: String,
        subdir: Option<String>,
    },
}

impl ResolvedSource {
    /// The store version for a verified tree digest.
    pub fn version(&self, digest: &str) -> SkillVersion {
        SkillVersion(match &self.origin {
            ResolvedOrigin::Bundled(_) => format!("bundled+{}", digest12(digest)),
            ResolvedOrigin::Local(_) => format!("local+{}", digest12(digest)),
            ResolvedOrigin::Git { commit, .. } => format!("git+{}", &commit[..12]),
        })
    }
}

pub struct Resolver<'a> {
    pub git: &'a GitRunner,
    pub catalog: &'a Catalog,
    /// A directory git can run in; `ls-remote` does not touch it.
    pub scratch: &'a Path,
}

impl Resolver<'_> {
    pub fn resolve(&self, source: &SkillSource) -> Result<ResolvedSource> {
        let (origin, expected_name, requested_revision, resolved_commit) = match source {
            SkillSource::Bundled { name } => {
                let id = SkillId::parse(name)?;
                let skill = self.catalog.bundled(&id).ok_or_else(|| {
                    MarketplaceError::BadSource(format!("unknown bundled skill '{name}'"))
                })?;
                (
                    ResolvedOrigin::Bundled(skill.clone()),
                    Some(name.clone()),
                    None,
                    None,
                )
            }
            SkillSource::Local { path } => {
                if !path.is_absolute() {
                    return Err(MarketplaceError::BadSource(format!(
                        "local path '{}' is not absolute",
                        path.display()
                    )));
                }
                let name = path.file_name().and_then(|n| n.to_str()).map(str::to_owned);
                (ResolvedOrigin::Local(path.clone()), name, None, None)
            }
            SkillSource::Git {
                url,
                revision,
                subdir,
            } => {
                check_url(url)?;
                if let Some(dir) = subdir {
                    check_relative_path(dir)?;
                }
                let commit = self.resolve_git(url, revision)?;
                let name = subdir
                    .as_deref()
                    .and_then(|d| d.rsplit('/').next())
                    .map(str::to_owned);
                (
                    ResolvedOrigin::Git {
                        url: url.clone(),
                        commit: commit.clone(),
                        subdir: subdir.clone(),
                    },
                    name,
                    Some(revision.requested().to_owned()),
                    Some(commit),
                )
            }
        };
        Ok(ResolvedSource {
            source: source.clone(),
            origin,
            expected_name,
            requested_revision,
            resolved_commit,
        })
    }

    /// A branch or tag becomes a full commit through `git ls-remote`. A
    /// commit is returned as is; the fetch verifies it exists.
    fn resolve_git(&self, url: &str, revision: &GitRevision) -> Result<String> {
        let (wanted, name): (Vec<String>, &str) = match revision {
            GitRevision::Commit(sha) if is_full_sha(sha) => return Ok(sha.to_ascii_lowercase()),
            GitRevision::Commit(sha) => {
                return Err(MarketplaceError::BadSource(format!(
                    "commit '{sha}' is not 40 hex characters"
                )))
            }
            GitRevision::Branch(b) if b == "HEAD" => (vec!["HEAD".to_owned()], b),
            GitRevision::Branch(b) => (vec![format!("refs/heads/{b}")], b),
            GitRevision::Tag(t) => (
                vec![
                    format!("refs/tags/{t}^{{}}"),
                    format!("refs/tags/{t}"),
                    format!("refs/heads/{t}"),
                ],
                t,
            ),
        };
        if name != "HEAD" && !crate::model::is_safe_ref_name(name) {
            return Err(MarketplaceError::BadSource(format!(
                "'{name}' is not a valid revision"
            )));
        }
        let out = self.git.run(self.scratch, &["ls-remote", url])?;
        let refs: Vec<(String, String)> = out
            .stdout_text()
            .lines()
            .filter_map(|l| l.split_once('\t'))
            .map(|(sha, r)| (sha.to_owned(), r.to_owned()))
            .collect();
        for want in &wanted {
            if let Some((sha, _)) = refs.iter().find(|(_, r)| r == want) {
                if is_full_sha(sha) {
                    return Ok(sha.to_ascii_lowercase());
                }
            }
        }
        Err(MarketplaceError::RevisionNotFound {
            url: url.to_owned(),
            revision: name.to_owned(),
        })
    }
}

impl SkillSource {
    /// Set the skill directory inside a git repository. Other sources are
    /// returned unchanged.
    pub fn with_subdir(self, dir: impl Into<String>) -> Self {
        match self {
            SkillSource::Git { url, revision, .. } => SkillSource::Git {
                url,
                revision,
                subdir: Some(dir.into()),
            },
            other => other,
        }
    }

    /// The same source at another revision: a pinned commit for a rebuild
    /// from the lock, or the requested revision again for an update. Other
    /// sources are returned unchanged.
    pub fn with_revision(self, revision: GitRevision) -> Self {
        match self {
            SkillSource::Git { url, subdir, .. } => SkillSource::Git {
                url,
                revision,
                subdir,
            },
            other => other,
        }
    }

    /// The spec the lock stores as `source`, without the revision. It
    /// parses back to this source at the default revision.
    pub fn to_spec(&self) -> String {
        match self {
            SkillSource::Bundled { name } => format!("bundled:{name}"),
            SkillSource::Local { path } => path.display().to_string(),
            SkillSource::Git {
                url,
                subdir: Some(dir),
                ..
            } => format!("{url}#{dir}"),
            SkillSource::Git { url, .. } => url.clone(),
        }
    }
}
