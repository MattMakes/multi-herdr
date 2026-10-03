//! `SkillManifest`: the SKILL.md YAML frontmatter, validated with the same
//! rules as the bundled catalog.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::error::{MarketplaceError, Result};
use crate::model::is_valid_skill_name;

pub const MAX_DESCRIPTION_BYTES: usize = 1024;

/// SPEC-TODO(Spec A §10): the allowed key list is `name`, `description`,
/// `license`, `metadata` and `allowed-tools` until Spec A §10 is in the
/// repository. Any other key, `hooks` included, is rejected.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillManifest {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
    /// A string or a list in practice; kept as YAML.
    #[serde(default, rename = "allowed-tools")]
    pub allowed_tools: Option<serde_yaml::Value>,
}

impl SkillManifest {
    /// Parse SKILL.md text. `expected_name`, when known, must equal `name`
    /// (the skill directory name). Frontmatter errors and unknown keys are
    /// `Manifest`; name and description rules are `SkillMd`.
    pub fn parse(text: &str, expected_name: Option<&str>) -> Result<Self> {
        let text = text.replace("\r\n", "\n");
        let front = text
            .strip_prefix("---\n")
            .and_then(|s| s.split_once("\n---").map(|p| p.0))
            .ok_or_else(|| MarketplaceError::Manifest("missing YAML frontmatter".to_owned()))?;
        let manifest: Self =
            serde_yaml::from_str(front).map_err(|e| MarketplaceError::Manifest(e.to_string()))?;
        let rule = |why: String| Err(MarketplaceError::SkillMd(why));
        if !is_valid_skill_name(&manifest.name) {
            return rule(format!("invalid skill name '{}'", manifest.name));
        }
        if let Some(expected) = expected_name {
            if manifest.name != expected {
                return rule(format!(
                    "name '{}' does not match the directory '{expected}'",
                    manifest.name
                ));
            }
        }
        if manifest.description.trim().is_empty() {
            return rule("description is empty".to_owned());
        }
        if manifest.description.len() > MAX_DESCRIPTION_BYTES {
            return rule(format!("description is over {MAX_DESCRIPTION_BYTES} bytes"));
        }
        Ok(manifest)
    }

    /// Read and validate `<dir>/SKILL.md`. A symlinked SKILL.md is rejected
    /// before it is read.
    pub fn read(dir: &Path, expected_name: Option<&str>) -> Result<Self> {
        let path = dir.join("SKILL.md");
        let invalid = |why: &dyn std::fmt::Display| {
            MarketplaceError::Manifest(format!("{}: {why}", path.display()))
        };
        let meta = fs::symlink_metadata(&path).map_err(|e| invalid(&e))?;
        if !meta.is_file() {
            return Err(invalid(&"not a regular file"));
        }
        let bytes = fs::read(&path).map_err(|e| invalid(&e))?;
        let text = String::from_utf8(bytes).map_err(|_| invalid(&"not UTF-8"))?;
        Self::parse(&text, expected_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_known_keys() {
        let m = SkillManifest::parse(
            "---\nname: tdd\ndescription: Use it.\nlicense: MIT\nmetadata:\n  a: b\nallowed-tools: Read, Grep\n---\nbody\n",
            Some("tdd"),
        )
        .unwrap();
        assert_eq!(m.allowed_tools, Some("Read, Grep".into()));
        assert_eq!(m.metadata.get("a").map(String::as_str), Some("b"));
    }

    #[test]
    fn rejects_bad_frontmatter() {
        for (text, why) in [
            ("no front", "missing"),
            (
                "---\nname: tdd\ndescription: x\nhooks: {}\n---\n",
                "unknown field",
            ),
            ("---\nname: tdd\ndescription: '  '\n---\n", "empty"),
            (
                "---\nname: Tdd\ndescription: x\n---\n",
                "invalid skill name",
            ),
            ("---\nname: other\ndescription: x\n---\n", "does not match"),
        ] {
            let err = SkillManifest::parse(text, Some("tdd"))
                .unwrap_err()
                .to_string();
            assert!(err.contains(why), "{text}: {err}");
        }
        let long = format!("---\nname: tdd\ndescription: {}\n---\n", "x".repeat(1025));
        assert!(SkillManifest::parse(&long, None).is_err());
    }
}
