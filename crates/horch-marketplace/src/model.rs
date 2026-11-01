//! Skill identity and sources. Parsing is pure; nothing here touches the
//! network or the filesystem.

use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{MarketplaceError, Result};

/// A skill name under the SKILL.md rules: lowercase ASCII letters, digits
/// and `-`, no leading, trailing or doubled `-`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SkillId(String);

impl SkillId {
    pub fn parse(s: &str) -> Result<Self> {
        if is_valid_skill_name(s) {
            Ok(Self(s.to_owned()))
        } else {
            Err(MarketplaceError::BadSource(format!(
                "invalid skill id '{s}': use lowercase ASCII letters, digits and single '-'"
            )))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub(crate) fn is_valid_skill_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--")
        && name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
}

impl TryFrom<String> for SkillId {
    type Error = MarketplaceError;
    fn try_from(s: String) -> Result<Self> {
        Self::parse(&s)
    }
}

impl From<SkillId> for String {
    fn from(id: SkillId) -> Self {
        id.0
    }
}

impl fmt::Display for SkillId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// An installed version: `git+<commit12>`, `local+<digest12>` or
/// `bundled+<digest12>`. It names the directory `skills/<id>/<version>/`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SkillVersion(pub String);

impl fmt::Display for SkillVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum SkillSource {
    /// A skill compiled into the binary. The caller supplies its files
    /// through `Catalog`.
    Bundled { name: String },
    /// An absolute path to a skill directory.
    Local { path: PathBuf },
    Git {
        url: String,
        revision: GitRevision,
        /// The skill directory inside the repository; `None` is the root.
        subdir: Option<String>,
    },
}

/// A requested git revision. `Tag` resolves to a tag first and falls back
/// to a branch of the same name, because `owner/repo@name` does not say
/// which it is. `Branch("HEAD")` is the remote's default branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum GitRevision {
    Branch(String),
    Tag(String),
    Commit(String),
}

impl GitRevision {
    /// The text the user asked for; the lock records it as
    /// `requested_revision`.
    pub fn requested(&self) -> &str {
        match self {
            Self::Branch(s) | Self::Tag(s) | Self::Commit(s) => s,
        }
    }

    /// The revision for `@rev` text, as `parse_source` reads it. The lock's
    /// `requested_revision` parses back to the same revision.
    pub fn from_requested(rev: &str) -> Result<Self> {
        Self::from_spec(Some(rev))
    }

    fn from_spec(rev: Option<&str>) -> Result<Self> {
        match rev {
            None | Some("HEAD") => Ok(Self::Branch("HEAD".to_owned())),
            Some(r) if is_full_sha(r) => Ok(Self::Commit(r.to_ascii_lowercase())),
            Some(r) if is_safe_ref_name(r) => Ok(Self::Tag(r.to_owned())),
            Some(r) => Err(MarketplaceError::BadSource(format!(
                "'{r}' is not a valid revision"
            ))),
        }
    }
}

pub(crate) fn is_full_sha(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|c| c.is_ascii_hexdigit())
}

const BAD_REF_PREFIXES: [&str; 2] = ["-", "/"];
const BAD_REF_SUFFIXES: [&str; 2] = ["/", ".lock"];
const BAD_REF_PARTS: [&str; 3] = ["..", "//", "@{"];
const BAD_REF_BYTES: [u8; 7] = *b"~^:?*[\\";

/// A ref name git accepts and that cannot be read as an option or a range.
pub(crate) fn is_safe_ref_name(s: &str) -> bool {
    !s.is_empty()
        && !has_bad_shape(s)
        && s.bytes()
            .all(|c| c.is_ascii_graphic() && !BAD_REF_BYTES.contains(&c))
}

fn has_bad_shape(s: &str) -> bool {
    BAD_REF_PREFIXES.iter().any(|p| s.starts_with(p))
        || BAD_REF_SUFFIXES.iter().any(|p| s.ends_with(p))
        || BAD_REF_PARTS.iter().any(|p| s.contains(p))
}

impl SkillSource {
    /// Parse a source spec:
    /// - `owner/repo[@rev]`: a GitHub repository over https;
    /// - `https://host/path[@rev]` or `file:///path[@rev]`: a git repository;
    /// - `/absolute/path`: a local skill directory;
    /// - `bundled:<name>`: a bundled skill.
    ///
    /// A 40-hex `@rev` is a commit. Any other `@rev` resolves as a tag, then
    /// as a branch. Without `@rev` the remote's default branch is used. The
    /// last `@` in the path starts `@rev`, so a path that contains `@` needs
    /// an explicit `@rev`. A git spec can end in `#<subdir>`; `with_subdir`
    /// sets it too.
    pub fn parse(spec: &str) -> Result<Self> {
        parse_source(spec)
    }
}

/// See `SkillSource::parse`. A git spec can end in `#<subdir>`, the skill
/// directory inside the repository: `owner/repo@v1#skills/tdd`.
pub fn parse_source(spec: &str) -> Result<SkillSource> {
    let spec = spec.trim();
    if let Some(name) = spec.strip_prefix("bundled:") {
        parse_bundled(name)
    } else if spec.starts_with('/') {
        Ok(SkillSource::Local {
            path: PathBuf::from(spec),
        })
    } else {
        let (spec, subdir) = split_off(spec, spec.split_once('#'));
        let subdir = subdir.map(str::to_owned);
        match spec.split_once("://") {
            Some((scheme, rest)) => parse_git_url(spec, scheme, rest, subdir),
            None => parse_github(spec, subdir),
        }
    }
}

/// `head` and the optional suffix of a split of `s`; `s` whole when there
/// is no split.
fn split_off<'a>(s: &'a str, parts: Option<(&'a str, &'a str)>) -> (&'a str, Option<&'a str>) {
    parts.map_or((s, None), |(head, tail)| (head, Some(tail)))
}

fn parse_bundled(name: &str) -> Result<SkillSource> {
    SkillId::parse(name)?;
    Ok(SkillSource::Bundled {
        name: name.to_owned(),
    })
}

fn parse_git_url(
    spec: &str,
    scheme: &str,
    rest: &str,
    subdir: Option<String>,
) -> Result<SkillSource> {
    check_url(spec)?;
    let (authority, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let (path, rev) = split_off(path, path.rsplit_once('@'));
    Ok(SkillSource::Git {
        url: format!("{}://{authority}{path}", scheme.to_ascii_lowercase()),
        revision: GitRevision::from_spec(rev)?,
        subdir,
    })
}

fn parse_github(spec: &str, subdir: Option<String>) -> Result<SkillSource> {
    let (repo, rev) = split_off(spec, spec.split_once('@'));
    let mut parts = repo.split('/');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(owner), Some(name), None) if is_github_name(owner) && is_github_name(name) => {
            Ok(SkillSource::Git {
                url: format!("https://github.com/{owner}/{name}"),
                revision: GitRevision::from_spec(rev)?,
                subdir,
            })
        }
        _ => Err(MarketplaceError::BadSource(format!(
            "'{spec}' is not owner/repo, an https:// or file:// URL, an absolute path, \
                 or bundled:<name>"
        ))),
    }
}

fn is_github_name(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('-')
        && !s.starts_with('.')
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
}

/// Reject a URL that git must not see: a scheme other than https or file,
/// userinfo (`user:pass@host`, `token@host`), or a query with `token=`.
pub fn check_url(url: &str) -> Result<()> {
    let (scheme, rest) = url.split_once("://").ok_or_else(|| {
        MarketplaceError::BadSource("a git URL needs https:// or file://".to_owned())
    })?;
    let authority = &rest[..rest.find(['/', '?', '#']).unwrap_or(rest.len())];
    match credential_in(url, authority) {
        Some(why) => Err(MarketplaceError::Credentials(why.to_owned())),
        None => check_scheme(scheme, authority),
    }
}

/// Why the URL carries a credential, or `None`.
fn credential_in(url: &str, authority: &str) -> Option<&'static str> {
    let token_in_query = url.split_once('?').is_some_and(|(_, query)| {
        query
            .split(['&', ';', '#'])
            .any(|kv| kv.to_ascii_lowercase().contains("token="))
    });
    if authority.contains('@') {
        Some("userinfo in the URL authority")
    } else if token_in_query {
        Some("a token in the URL query")
    } else {
        None
    }
}

fn check_scheme(scheme: &str, authority: &str) -> Result<()> {
    match scheme.to_ascii_lowercase().as_str() {
        "https" if !authority.is_empty() => Ok(()),
        "file" if authority.is_empty() => Ok(()),
        _ => Err(MarketplaceError::BadSource(format!(
            "unsupported git URL scheme '{scheme}': use https:// or file:///"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_specs() {
        let sha = "d47670328c59a3311a9b4149bc5f8f33f0a92754";
        assert_eq!(
            SkillSource::parse(&format!("MattMakes/skill-marketplace@{sha}")).unwrap(),
            SkillSource::Git {
                url: "https://github.com/MattMakes/skill-marketplace".into(),
                revision: GitRevision::Commit(sha.into()),
                subdir: None,
            }
        );
        assert_eq!(
            SkillSource::parse("file:///tmp/r.git@v1").unwrap(),
            SkillSource::Git {
                url: "file:///tmp/r.git".into(),
                revision: GitRevision::Tag("v1".into()),
                subdir: None,
            }
        );
        assert_eq!(
            SkillSource::parse("https://example.com/r").unwrap(),
            SkillSource::Git {
                url: "https://example.com/r".into(),
                revision: GitRevision::Branch("HEAD".into()),
                subdir: None,
            }
        );
        assert_eq!(
            SkillSource::parse("o/r@HEAD#skills/tdd").unwrap(),
            SkillSource::Git {
                url: "https://github.com/o/r".into(),
                revision: GitRevision::Branch("HEAD".into()),
                subdir: Some("skills/tdd".into()),
            }
        );
        assert_eq!(
            serde_json::to_string(&GitRevision::Tag("v1".into())).unwrap(),
            r#"{"type":"tag","value":"v1"}"#
        );
        assert_eq!(
            SkillSource::parse("/x/tdd").unwrap(),
            SkillSource::Local {
                path: "/x/tdd".into()
            }
        );
        assert_eq!(
            SkillSource::parse("bundled:tdd").unwrap(),
            SkillSource::Bundled { name: "tdd".into() }
        );
        for bad in [
            "relative/a/b",
            "http://x/y",
            "ssh://x/y",
            "o/r@-x",
            "o/r@a..b",
            "bundled:Bad",
        ] {
            assert!(SkillSource::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn skill_id_rules() {
        for ok in ["a", "tdd", "code-review", "x2"] {
            assert!(SkillId::parse(ok).is_ok(), "{ok}");
        }
        for bad in ["", "-a", "a-", "a--b", "A", "a_b", "a/b"] {
            assert!(SkillId::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn url_scheme_is_lowercased_and_last_at_is_the_revision() {
        assert_eq!(
            SkillSource::parse("HTTPS://example.com/a@b/r@v2#sk/x").unwrap(),
            SkillSource::Git {
                url: "https://example.com/a@b/r".into(),
                revision: GitRevision::Tag("v2".into()),
                subdir: Some("sk/x".into()),
            }
        );
        // `@` in the authority is userinfo and is refused.
        assert!(matches!(
            SkillSource::parse("https://tok@example.com/r"),
            Err(MarketplaceError::Credentials(_))
        ));
    }

    #[test]
    fn github_form_rules() {
        assert_eq!(
            SkillSource::parse(" o/r.git@v1 ").unwrap(),
            SkillSource::Git {
                url: "https://github.com/o/r.git".into(),
                revision: GitRevision::Tag("v1".into()),
                subdir: None,
            }
        );
        for bad in ["o", "o/r/x", "-o/r", "o/.r", "o/", "o/r@"] {
            assert!(
                matches!(SkillSource::parse(bad), Err(MarketplaceError::BadSource(_))),
                "{bad}"
            );
        }
    }

    #[test]
    fn bundled_and_local_forms() {
        let err = SkillSource::parse("bundled:a-b#x").unwrap_err();
        assert!(err.to_string().contains("invalid skill id"), "{err}");
        assert_eq!(
            SkillSource::parse("/a/b#c").unwrap(),
            SkillSource::Local {
                path: "/a/b#c".into()
            }
        );
    }

    #[test]
    fn check_url_rules() {
        for ok in [
            "https://example.com/r",
            "HTTPS://example.com",
            "file:///tmp/r.git",
            "https://example.com/r?ref=main",
        ] {
            assert!(check_url(ok).is_ok(), "{ok}");
        }
        for (bad, kind) in [
            ("example.com/r", "source"),
            ("https://user:pw@example.com/r", "credentials"),
            ("https://token@example.com/r", "credentials"),
            ("https://example.com/r?Token=abc", "credentials"),
            ("https://example.com/r?a=1&token=abc", "credentials"),
            ("https://example.com/r?x=1#a;token=z", "credentials"),
            ("http://example.com/r", "source"),
            ("ssh://example.com/r", "source"),
            ("https:///r", "source"),
            ("file://host/r", "source"),
        ] {
            let err = check_url(bad).unwrap_err();
            let got = match err {
                MarketplaceError::Credentials(_) => "credentials",
                MarketplaceError::BadSource(_) => "source",
                other => panic!("{bad}: {other:?}"),
            };
            assert_eq!(got, kind, "{bad}");
        }
        // The text never holds the URL, so a secret cannot reach a log.
        let msg = check_url("https://sekret@example.com/r")
            .unwrap_err()
            .to_string();
        assert!(!msg.contains("sekret"), "{msg}");
    }

    #[test]
    fn safe_ref_names() {
        for ok in ["main", "v1.2.3", "feat/x", "a@b", "x.lockfile", "a.b"] {
            assert!(is_safe_ref_name(ok), "{ok}");
        }
        for bad in [
            "", "-x", "/x", "x/", "x.lock", "a..b", "a//b", "a@{1}", "a b", "a~1", "a^", "a:b",
            "a?", "a*", "a[", "a\\b", "a\u{7f}", "é",
        ] {
            assert!(!is_safe_ref_name(bad), "{bad:?}");
        }
    }
}
