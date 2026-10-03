//! The versioned judge rubric and judgment schema (JDG-03), and the judge
//! policy digest that names one exact judging setup.
//!
//! Both texts are compiled in, so a build always judges with the rubric and
//! schema it was tested with. A change to either is a new version: a new
//! file, never an edit of `rubric-1.md` or `judgment-schema-1.0.0.json`.

use sha2::{Digest as _, Sha256};

use super::winner::WinnerPolicy;
use crate::measure::digest::{canonical_json, Digest};

pub const RUBRIC_VERSION: &str = "rubric-1";

/// The lowest and the highest score a rubric component can get.
pub const SCORE_MIN: f64 = 0.0;
pub const SCORE_MAX: f64 = 10.0;

const RUBRIC: &str = include_str!("../../assets/judge/rubric-1.md");
const SCHEMA: &str = include_str!("../../assets/judge/judgment-schema-1.0.0.json");

/// The rubric text the judge reads.
pub fn rubric_text() -> &'static str {
    RUBRIC
}

/// The judgment JSON Schema the judge answers to.
pub fn schema_text() -> &'static str {
    SCHEMA
}

/// The component names, in rubric order: every `### <name>` heading under
/// `## Components`. A judgment scores each candidate on exactly these.
pub fn components() -> Vec<&'static str> {
    let mut in_components = false;
    let mut out = Vec::new();
    for line in RUBRIC.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            in_components = heading.trim() == "Components";
        } else if let Some(name) = line.strip_prefix("### ") {
            if in_components {
                out.push(name.trim());
            }
        }
    }
    out
}

/// sha256(judge.md ‖ rubric ‖ schema ‖ model ‖ effort ‖ WinnerPolicy ‖
/// label_policy_version). Each input enters as an 8-byte big-endian length
/// and then its bytes, so no two input lists give the same byte stream.
/// `policy` enters as its canonical JSON.
pub fn judge_policy_digest(
    judge_md: &str,
    rubric: &str,
    schema: &str,
    model: &str,
    effort: &str,
    policy: &WinnerPolicy,
    label_policy_version: &str,
) -> Digest {
    let policy_value = serde_json::to_value(policy).expect("a WinnerPolicy serializes to JSON");
    let policy_json = canonical_json(&policy_value);
    let mut hasher = Sha256::new();
    for part in [
        judge_md,
        rubric,
        schema,
        model,
        effort,
        policy_json.as_str(),
        label_policy_version,
    ] {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part.as_bytes());
    }
    Digest(hasher.finalize().into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rubric_names_five_components() {
        assert_eq!(
            components(),
            ["correctness", "tests", "scope", "maintainability", "risk"]
        );
    }

    #[test]
    fn the_schema_is_json() {
        let v: serde_json::Value = serde_json::from_str(schema_text()).unwrap();
        assert_eq!(v["title"], "Judgment");
    }
}
