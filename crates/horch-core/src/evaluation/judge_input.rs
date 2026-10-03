//! The judge's input: an immutable, anonymous bundle (phase B4, JDG-02,
//! JDG-10).
//!
//! The coordinator builds one bundle per round under
//! `artifacts/<round>/judge-input/`, and the judge reads nothing else:
//!
//! ```text
//! task.md  rubric.md  schema.json  manifest.json
//! candidates/<L>/diff.patch  candidates/<L>/validation.json
//! ```
//!
//! `<L>` is `A`, `B`, `C`, ... in an order shuffled from the round id, so a
//! label says nothing about the planner's slot or the harness. The label to
//! original-label map goes back to the caller and never into the bundle.
//! Model, vendor, teammate, cost, latency and history are left out. A diff
//! is copied as it is: a vendor token in it is reported as a
//! [`BlindnessFlag`], never edited away.
//!
//! Every file is written once (`fsx::create_immutable`) at mode 0400; the
//! directories become 0500 after `manifest.json` is written. A second build
//! for the same round verifies the files and writes nothing.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};

use super::rubric::{rubric_text, schema_text, RUBRIC_VERSION};
use super::validator::{GateStatus, ValidationReport};
use crate::fsx;
use crate::ids::RoundId;
use crate::measure::digest::{sha256_bytes, Digest};
use crate::measure::paths::DatasetPaths;
use crate::measure::projection::RoundView;
use crate::measure::testkit::{seed_from_digest, SplitMix64};
use crate::vcs::git::GitClient;
use crate::vcs::worktree::FrozenCandidate;

pub const JUDGE_INPUT_SCHEMA_VERSION: &str = "1.0.0";

/// The most bytes of one candidate's patch that enter the bundle. A longer
/// patch is cut on a UTF-8 boundary and its label is listed in
/// [`JudgeInputManifest::truncated_diffs`].
pub const DIFF_CAP_BYTES: usize = 1024 * 1024;

/// Bundle files are read-only for the owner, and so are its directories once
/// the manifest is written.
pub const BUNDLE_FILE_MODE: u32 = 0o400;
pub const BUNDLE_DIR_MODE: u32 = 0o500;

/// Lowercase tokens that name a vendor, a harness or a model family. The
/// blindness scan matches them case-insensitively anywhere in a diff.
pub const BLINDNESS_TOKENS: [&str; 10] = [
    "claude",
    "anthropic",
    "codex",
    "openai",
    "gpt-",
    "opencode",
    "gemini",
    "qwen",
    "ollama",
    "prime-agent",
];

/// The bundle as built. `label_map` stays with the caller (round state); it
/// is the only link from a bundle label back to a candidate.
#[derive(Debug, Clone, PartialEq)]
pub struct JudgeInput {
    /// `artifacts/<round>/judge-input/`.
    pub dir: PathBuf,
    /// The bundle labels, `A`, `B`, ... in order.
    pub labels: Vec<String>,
    /// Bundle label → the round's original label.
    pub label_map: BTreeMap<String, String>,
    pub manifest: JudgeInputManifest,
    /// sha256 of the `manifest.json` bytes.
    pub digest: Digest,
    /// Vendor tokens found in diffs (JDG-10).
    pub blindness: Vec<BlindnessFlag>,
}

/// `manifest.json`. `files` holds every other bundle file, so the digest of
/// the manifest covers the whole bundle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JudgeInputManifest {
    pub schema_version: String,
    pub round_id: RoundId,
    pub rubric_version: String,
    /// sha256 of `schema.json`.
    pub schema_digest: Digest,
    /// The bundle labels, in order.
    pub labels: Vec<String>,
    /// Bundle labels whose `diff.patch` was cut at [`DIFF_CAP_BYTES`].
    pub truncated_diffs: Vec<String>,
    /// Relative path → sha256 of the file bytes.
    pub files: BTreeMap<String, Digest>,
}

/// One vendor token in one candidate's diff. `file` is the changed path the
/// token appears under, or `diff.patch` before the first file header.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BlindnessFlag {
    pub label: String,
    pub token: String,
    pub file: String,
}

/// `candidates/<L>/validation.json`: the [`ValidationReport`] with every
/// field that could name or time a candidate removed (validation id, head
/// sha, gate durations and log paths).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlindValidation {
    pub label: String,
    pub gates: Vec<BlindGate>,
    pub mechanical_score: f64,
    pub eligible: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlindGate {
    pub name: String,
    pub status: GateStatus,
    pub log_truncated: bool,
}

impl BlindValidation {
    fn new(label: &str, report: &ValidationReport) -> Self {
        BlindValidation {
            label: label.to_string(),
            gates: report
                .gates
                .iter()
                .map(|g| BlindGate {
                    name: g.name.clone(),
                    status: g.status.clone(),
                    log_truncated: g.log_truncated,
                })
                .collect(),
            mechanical_score: report.mechanical_score,
            eligible: report.eligible,
        }
    }
}

/// The `n`th bundle label: `A` .. `Z`, then `AA`, `AB`, ...
fn bundle_label(mut n: usize) -> String {
    let mut out = Vec::new();
    loop {
        out.push(b'A' + (n % 26) as u8);
        if n < 26 {
            break;
        }
        n = n / 26 - 1;
    }
    out.reverse();
    String::from_utf8(out).expect("ASCII letters")
}

/// Shuffle `originals` with SplitMix64 seeded by sha256 of the round id,
/// then name them `A`, `B`, ... in the shuffled order. Returns
/// `(bundle label, original label)` pairs. The same round id and the same
/// labels always give the same pairs.
pub fn blind_labels(round_id: &RoundId, originals: &[String]) -> Vec<(String, String)> {
    let seed = seed_from_digest(&sha256_bytes(round_id.as_str().as_bytes()));
    let mut shuffled = originals.to_vec();
    SplitMix64::new(seed).shuffle(&mut shuffled);
    shuffled
        .into_iter()
        .enumerate()
        .map(|(i, original)| (bundle_label(i), original))
        .collect()
}

/// Every vendor token in `patch`, once per changed file it appears under.
pub fn scan_blindness(label: &str, patch: &str) -> Vec<BlindnessFlag> {
    let mut found = BTreeSet::new();
    let mut file = "diff.patch".to_string();
    for line in patch.lines() {
        if let Some(header) = line.strip_prefix("diff --git ") {
            if let Some((_, b)) = header.rsplit_once(" b/") {
                file = b.to_string();
            }
        }
        let lower = line.to_ascii_lowercase();
        for token in BLINDNESS_TOKENS {
            if lower.contains(token) {
                found.insert(BlindnessFlag {
                    label: label.to_string(),
                    token: token.to_string(),
                    file: file.clone(),
                });
            }
        }
    }
    found.into_iter().collect()
}

fn to_json<T: Serialize>(value: &T) -> Vec<u8> {
    let mut bytes = serde_json::to_vec_pretty(value).expect("a bundle file serializes");
    bytes.push(b'\n');
    bytes
}

#[cfg(unix)]
fn seal_dir(dir: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(BUNDLE_DIR_MODE))
        .with_context(|| format!("sealing {}", dir.display()))
}

#[cfg(not(unix))]
fn seal_dir(_dir: &Path) -> anyhow::Result<()> {
    Ok(())
}

/// Build (or verify) the judge input bundle of one round.
///
/// `candidates` holds `(original label, frozen candidate, validation)` for
/// every candidate the judge compares; each label must be one of the round's.
/// `repo` is the MAIN repository: the patch is read from there, never from a
/// candidate worktree, whose `.git` file the candidate controls.
pub fn build_judge_input(
    round_id: &RoundId,
    round: &RoundView,
    task_text: &str,
    candidates: &[(String, FrozenCandidate, ValidationReport)],
    paths: &DatasetPaths,
    git: &dyn GitClient,
    repo: &Path,
) -> anyhow::Result<JudgeInput> {
    if candidates.is_empty() {
        bail!("round {round_id}: no candidates to judge");
    }
    // Round order first, so the caller's slice order cannot change the
    // shuffle.
    let mut by_label = BTreeMap::new();
    for entry @ (label, frozen, report) in candidates {
        if !round.created.labels.contains(label) {
            bail!("round {round_id}: '{label}' is not one of the round's labels");
        }
        if frozen.label != *label || report.label != *label {
            bail!(
                "round {round_id}: candidate '{label}' carries frozen label '{}' and \
                 validation label '{}'",
                frozen.label,
                report.label
            );
        }
        if report.head_sha != frozen.head_sha {
            bail!(
                "round {round_id}: candidate '{label}' was validated at {} but frozen at {}",
                report.head_sha,
                frozen.head_sha
            );
        }
        if by_label.insert(label.clone(), entry).is_some() {
            bail!("round {round_id}: candidate '{label}' is given twice");
        }
    }
    let originals: Vec<String> = round
        .created
        .labels
        .iter()
        .filter(|l| by_label.contains_key(*l))
        .cloned()
        .collect();
    let pairs = blind_labels(round_id, &originals);

    // Every file's bytes, before anything touches the disk.
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    files.insert("task.md".into(), task_text.as_bytes().to_vec());
    files.insert("rubric.md".into(), rubric_text().as_bytes().to_vec());
    files.insert("schema.json".into(), schema_text().as_bytes().to_vec());
    let mut blindness = Vec::new();
    let mut truncated_diffs = Vec::new();
    for (blind, original) in &pairs {
        let (_, frozen, report) = by_label[original];
        let (patch, cut) = git
            .diff_patch(repo, &frozen.base_sha, &frozen.head_sha, DIFF_CAP_BYTES)
            .with_context(|| format!("reading the patch of candidate '{original}'"))?;
        if cut {
            truncated_diffs.push(blind.clone());
        }
        blindness.extend(scan_blindness(blind, &patch));
        files.insert(format!("candidates/{blind}/diff.patch"), patch.into_bytes());
        files.insert(
            format!("candidates/{blind}/validation.json"),
            to_json(&BlindValidation::new(blind, report)),
        );
    }
    let labels: Vec<String> = pairs.iter().map(|(b, _)| b.clone()).collect();
    let manifest = JudgeInputManifest {
        schema_version: JUDGE_INPUT_SCHEMA_VERSION.into(),
        round_id: round_id.clone(),
        rubric_version: RUBRIC_VERSION.into(),
        schema_digest: sha256_bytes(schema_text().as_bytes()),
        labels: labels.clone(),
        truncated_diffs,
        files: files
            .iter()
            .map(|(rel, bytes)| (rel.clone(), sha256_bytes(bytes)))
            .collect(),
    };
    let manifest_bytes = to_json(&manifest);
    let dir = paths.judge_input_dir(&round.experiment_id, round_id);

    if dir.join("manifest.json").exists() {
        // Already sealed: verify, write nothing.
        files.insert("manifest.json".into(), manifest_bytes.clone());
        for (rel, bytes) in &files {
            let path = dir.join(rel);
            let existing =
                std::fs::read(&path).with_context(|| format!("verifying {}", path.display()))?;
            if existing != *bytes {
                return Err(fsx::FsxError::Conflict { path }.into());
            }
        }
    } else {
        fsx::ensure_private_dir(&dir)?;
        for label in &labels {
            fsx::ensure_private_dir(&dir.join("candidates").join(label))?;
        }
        for (rel, bytes) in &files {
            fsx::create_immutable(&dir.join(rel), bytes, BUNDLE_FILE_MODE)?;
        }
        // The manifest goes last: its presence means the bundle is whole.
        fsx::create_immutable(
            &dir.join("manifest.json"),
            &manifest_bytes,
            BUNDLE_FILE_MODE,
        )?;
        for label in &labels {
            seal_dir(&dir.join("candidates").join(label))?;
        }
        seal_dir(&dir.join("candidates"))?;
        seal_dir(&dir)?;
    }

    Ok(JudgeInput {
        dir,
        labels,
        label_map: pairs.into_iter().collect(),
        manifest,
        digest: sha256_bytes(&manifest_bytes),
        blindness,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_labels_continue_past_z() {
        let names: Vec<String> = [0, 1, 25, 26, 27, 51, 52, 701, 702]
            .into_iter()
            .map(bundle_label)
            .collect();
        assert_eq!(names, ["A", "B", "Z", "AA", "AB", "AZ", "BA", "ZZ", "AAA"]);
    }

    #[test]
    fn scan_names_the_changed_file() {
        let patch = "diff --git a/src/x.rs b/src/x.rs\n+// GPT-5 wrote this\n\
                     diff --git a/README.md b/README.md\n+Built with OpenAI and openai\n";
        let flags = scan_blindness("A", patch);
        let got: Vec<(&str, &str)> = flags
            .iter()
            .map(|f| (f.token.as_str(), f.file.as_str()))
            .collect();
        assert_eq!(got, [("gpt-", "src/x.rs"), ("openai", "README.md")]);
    }
}
