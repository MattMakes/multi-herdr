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

/// What a source resolves to, before it is joined with the source itself.
struct Origin {
    origin: ResolvedOrigin,
    expected_name: Option<String>,
    requested_revision: Option<String>,
    resolved_commit: Option<String>,
}

/// What a git revision asks the remote for.
enum Lookup<'a> {
    /// A full commit: no remote lookup.
    Commit(String),
    /// A branch or tag: the ref names to look for, in order.
    Refs { wanted: Vec<String>, name: &'a str },
}

impl Resolver<'_> {
    pub fn resolve(&self, source: &SkillSource) -> Result<ResolvedSource> {
        let found = match source {
            SkillSource::Bundled { name } => self.resolve_bundled(name),
            SkillSource::Local { path } => resolve_local(path),
            SkillSource::Git {
                url,
                revision,
                subdir,
            } => self.resolve_git_source(url, revision, subdir.as_deref()),
        }?;
        Ok(ResolvedSource {
            source: source.clone(),
            origin: found.origin,
            expected_name: found.expected_name,
            requested_revision: found.requested_revision,
            resolved_commit: found.resolved_commit,
        })
    }

    fn resolve_bundled(&self, name: &str) -> Result<Origin> {
        let id = SkillId::parse(name)?;
        let skill = self.catalog.bundled(&id).ok_or_else(|| {
            MarketplaceError::BadSource(format!("unknown bundled skill '{name}'"))
        })?;
        Ok(Origin {
            origin: ResolvedOrigin::Bundled(skill.clone()),
            expected_name: Some(name.to_owned()),
            requested_revision: None,
            resolved_commit: None,
        })
    }

    fn resolve_git_source(
        &self,
        url: &str,
        revision: &GitRevision,
        subdir: Option<&str>,
    ) -> Result<Origin> {
        check_url(url)?;
        subdir.map(check_relative_path).transpose()?;
        let commit = self.resolve_git(url, revision)?;
        Ok(Origin {
            origin: ResolvedOrigin::Git {
                url: url.to_owned(),
                commit: commit.clone(),
                subdir: subdir.map(str::to_owned),
            },
            expected_name: subdir.and_then(|d| d.rsplit('/').next()).map(str::to_owned),
            requested_revision: Some(revision.requested().to_owned()),
            resolved_commit: Some(commit),
        })
    }

    /// A branch or tag becomes a full commit through `git ls-remote`. A
    /// commit is returned as is; the fetch verifies it exists.
    fn resolve_git(&self, url: &str, revision: &GitRevision) -> Result<String> {
        match wanted_refs(revision)? {
            Lookup::Commit(sha) => Ok(sha),
            Lookup::Refs { wanted, name } => self.lookup_remote(url, &wanted, name),
        }
    }

    fn lookup_remote(&self, url: &str, wanted: &[String], name: &str) -> Result<String> {
        check_ref_name(name)?;
        let out = self.git.run(self.scratch, &["ls-remote", url])?;
        let text = out.stdout_text();
        let refs: Vec<(&str, &str)> = text.lines().filter_map(|l| l.split_once('\t')).collect();
        wanted
            .iter()
            .find_map(|want| {
                refs.iter()
                    .find_map(|(sha, r)| (r == want).then_some(*sha))
                    .filter(|sha| is_full_sha(sha))
            })
            .map(str::to_ascii_lowercase)
            .ok_or_else(|| MarketplaceError::RevisionNotFound {
                url: url.to_owned(),
                revision: name.to_owned(),
            })
    }
}

fn resolve_local(path: &Path) -> Result<Origin> {
    if !path.is_absolute() {
        return Err(MarketplaceError::BadSource(format!(
            "local path '{}' is not absolute",
            path.display()
        )));
    }
    Ok(Origin {
        origin: ResolvedOrigin::Local(path.to_owned()),
        expected_name: path.file_name().and_then(|n| n.to_str()).map(str::to_owned),
        requested_revision: None,
        resolved_commit: None,
    })
}

fn wanted_refs(revision: &GitRevision) -> Result<Lookup<'_>> {
    match revision {
        GitRevision::Commit(sha) => {
            if is_full_sha(sha) {
                Ok(Lookup::Commit(sha.to_ascii_lowercase()))
            } else {
                Err(MarketplaceError::BadSource(format!(
                    "commit '{sha}' is not 40 hex characters"
                )))
            }
        }
        GitRevision::Branch(b) => Ok(Lookup::Refs {
            wanted: vec![if b == "HEAD" {
                "HEAD".to_owned()
            } else {
                format!("refs/heads/{b}")
            }],
            name: b,
        }),
        GitRevision::Tag(t) => Ok(Lookup::Refs {
            wanted: vec![
                format!("refs/tags/{t}^{{}}"),
                format!("refs/tags/{t}"),
                format!("refs/heads/{t}"),
            ],
            name: t,
        }),
    }
}

fn check_ref_name(name: &str) -> Result<()> {
    if name == "HEAD" || crate::model::is_safe_ref_name(name) {
        Ok(())
    } else {
        Err(MarketplaceError::BadSource(format!(
            "'{name}' is not a valid revision"
        )))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::BundledFile;

    const SHA: &str = "d47670328c59a3311a9b4149bc5f8f33f0a92754";

    fn resolve(source: &SkillSource) -> Result<ResolvedSource> {
        let catalog = Catalog::new().with_bundled(BundledSkill {
            id: SkillId::parse("tdd").unwrap(),
            files: vec![BundledFile {
                path: "SKILL.md".into(),
                bytes: b"x"[..].into(),
                executable: false,
            }],
        });
        // The commit and refused cases never start git, so the binary is
        // never run.
        let git = GitRunner::new("git-is-never-run");
        let scratch = std::env::temp_dir();
        Resolver {
            git: &git,
            catalog: &catalog,
            scratch: &scratch,
        }
        .resolve(source)
    }

    fn git_source(revision: GitRevision, subdir: Option<&str>) -> SkillSource {
        SkillSource::Git {
            url: "https://example.com/r".into(),
            revision,
            subdir: subdir.map(str::to_owned),
        }
    }

    #[test]
    fn bundled_resolves_with_its_name() {
        let r = resolve(&SkillSource::Bundled { name: "tdd".into() }).unwrap();
        assert!(matches!(r.origin, ResolvedOrigin::Bundled(_)));
        assert_eq!(r.expected_name.as_deref(), Some("tdd"));
        assert_eq!((r.requested_revision, r.resolved_commit), (None, None));
    }

    #[test]
    fn unknown_bundled_is_refused() {
        let err = resolve(&SkillSource::Bundled {
            name: "nope".into(),
        })
        .unwrap_err();
        assert!(
            err.to_string().contains("unknown bundled skill 'nope'"),
            "{err}"
        );
    }

    #[test]
    fn local_resolves_with_its_directory_name() {
        let r = resolve(&SkillSource::Local {
            path: "/x/demo".into(),
        })
        .unwrap();
        assert!(matches!(&r.origin, ResolvedOrigin::Local(p) if p == Path::new("/x/demo")));
        assert_eq!(r.expected_name.as_deref(), Some("demo"));
        assert_eq!((r.requested_revision, r.resolved_commit), (None, None));
    }

    #[test]
    fn relative_local_path_is_refused() {
        let err = resolve(&SkillSource::Local {
            path: "x/demo".into(),
        })
        .unwrap_err();
        assert!(
            err.to_string().contains("'x/demo' is not absolute"),
            "{err}"
        );
    }

    #[test]
    fn full_commit_resolves_lowercase_without_git() {
        let upper = SHA.to_ascii_uppercase();
        let source = git_source(GitRevision::Commit(upper.clone()), Some("skills/tdd"));
        let r = resolve(&source).unwrap();
        assert_eq!(r.resolved_commit.as_deref(), Some(SHA));
        assert_eq!(r.requested_revision.as_deref(), Some(upper.as_str()));
        assert_eq!(r.expected_name.as_deref(), Some("tdd"));
        assert!(matches!(
            &r.origin,
            ResolvedOrigin::Git { commit, subdir, .. }
                if commit == SHA && subdir.as_deref() == Some("skills/tdd")
        ));
    }

    #[test]
    fn short_commit_is_refused() {
        let err = resolve(&git_source(GitRevision::Commit("abc123".into()), None)).unwrap_err();
        assert!(
            err.to_string()
                .contains("'abc123' is not 40 hex characters"),
            "{err}"
        );
    }

    #[test]
    fn unsafe_ref_name_is_refused_before_git_runs() {
        for revision in [
            GitRevision::Branch("a..b".into()),
            GitRevision::Tag("-x".into()),
        ] {
            let err = resolve(&git_source(revision, None)).unwrap_err();
            assert!(matches!(err, MarketplaceError::BadSource(_)), "{err:?}");
            assert!(err.to_string().contains("is not a valid revision"), "{err}");
        }
    }

    #[test]
    fn git_source_checks_url_and_subdir() {
        let bad_url = SkillSource::Git {
            url: "http://example.com/r".into(),
            revision: GitRevision::Commit(SHA.into()),
            subdir: None,
        };
        assert!(matches!(
            resolve(&bad_url).unwrap_err(),
            MarketplaceError::BadSource(_)
        ));
        let bad_dir = git_source(GitRevision::Commit(SHA.into()), Some("../x"));
        assert!(matches!(
            resolve(&bad_dir).unwrap_err(),
            MarketplaceError::Traversal(_)
        ));
    }
}
