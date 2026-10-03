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
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillManifest {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub metadata: Option<BTreeMap<String, String>>,
    #[serde(default, rename = "allowed-tools")]
    pub allowed_tools: Option<AllowedTools>,
}

/// `allowed-tools` is written either as one string or as a list.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum AllowedTools {
    One(String),
    Many(Vec<String>),
}

impl SkillManifest {
    /// Parse SKILL.md text. `expected_name`, when known, must equal `name`
    /// (the skill directory name).
    pub fn parse(text: &str, expected_name: Option<&str>) -> std::result::Result<Self, String> {
        let text = text.replace("\r\n", "\n");
        let front = text
            .strip_prefix("---\n")
            .and_then(|s| s.split_once("\n---").map(|p| p.0))
            .ok_or("missing YAML frontmatter")?;
        let manifest: Self = serde_yaml::from_str(front).map_err(|e| e.to_string())?;
        if !is_valid_skill_name(&manifest.name) {
            return Err(format!("invalid skill name '{}'", manifest.name));
        }
        if let Some(expected) = expected_name {
            if manifest.name != expected {
                return Err(format!(
                    "name '{}' does not match the directory '{expected}'",
                    manifest.name
                ));
            }
        }
        if manifest.description.trim().is_empty() {
            return Err("description is empty".to_owned());
        }
        if manifest.description.len() > MAX_DESCRIPTION_BYTES {
            return Err(format!("description is over {MAX_DESCRIPTION_BYTES} bytes"));
        }
        Ok(manifest)
    }

    /// Read and validate `<dir>/SKILL.md`. A symlinked SKILL.md is rejected
    /// before it is read.
    pub fn read(dir: &Path, expected_name: Option<&str>) -> Result<Self> {
        let path = dir.join("SKILL.md");
        let invalid = |reason: String| MarketplaceError::InvalidManifest {
            path: path.clone(),
            reason,
        };
        let meta = fs::symlink_metadata(&path).map_err(|e| invalid(e.to_string()))?;
        if !meta.is_file() {
            return Err(invalid("SKILL.md is not a regular file".to_owned()));
        }
        let bytes = fs::read(&path).map_err(|e| invalid(e.to_string()))?;
        let text = String::from_utf8(bytes).map_err(|_| invalid("not UTF-8".to_owned()))?;
        Self::parse(&text, expected_name).map_err(invalid)
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
        assert_eq!(
            m.allowed_tools,
            Some(AllowedTools::One("Read, Grep".into()))
        );
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
            let err = SkillManifest::parse(text, Some("tdd")).unwrap_err();
            assert!(err.contains(why), "{text}: {err}");
        }
        let long = format!("---\nname: tdd\ndescription: {}\n---\n", "x".repeat(1025));
        assert!(SkillManifest::parse(&long, None).is_err());
    }
}
