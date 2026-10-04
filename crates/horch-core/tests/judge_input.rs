//! B4 judge input: the judge teammate (JDG-01) and the blind bundle
//! (JDG-02, JDG-10). Dataset design §4.7.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use horch_core::competition::model::RoundState;
use horch_core::evaluation::judge_input::{
    blind_labels, build_judge_input, BlindValidation, JudgeInput, JudgeInputManifest,
};
use horch_core::evaluation::rubric::{rubric_text, schema_text};
use horch_core::evaluation::validator::ValidationReport;
use horch_core::harness::HarnessKind;
use horch_core::ids::{ExecutionId, ExperimentId, RoundId};
use horch_core::measure::digest::{sha256_bytes, Digest};
use horch_core::measure::event::RoundCreated;
use horch_core::measure::paths::DatasetPaths;
use horch_core::measure::projection::{CandidateView, JudgeView, PromotionView, RoundView};
use horch_core::measure::NumstatLine;
use horch_core::roster::Roster;
use horch_core::teacher::TeacherRef;
use horch_core::vcs::git::{CheckoutLocation, CherryPick, GitClient, GitIdentity};
use horch_core::vcs::worktree::FrozenCandidate;
use serde_json::{json, Value};

// ─── fixtures ───────────────────────────────────────────────────────────────

fn repo_roster() -> Roster {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../teammates");
    Roster::load_layered(None, None, Some(dir.to_str().unwrap())).unwrap()
}

fn exp_id() -> ExperimentId {
    ExperimentId::new("0199a5b0-0000-7000-8000-000000000001").unwrap()
}

fn round_id() -> RoundId {
    RoundId::new("0199a5b0-0000-7000-8000-000000000002").unwrap()
}

fn exec_id(n: u8) -> ExecutionId {
    ExecutionId::new(format!("0199a5b0-0000-7000-8000-0000000000{n:02x}")).unwrap()
}

fn typed<T: serde::de::DeserializeOwned>(v: Value) -> T {
    serde_json::from_value(v).unwrap()
}

/// The labels hold no hex digit, so no digest or id in the bundle can
/// contain one by accident.
const LABELS: [&str; 3] = ["xq", "xr", "xs"];

/// The identity data of each label: teammate, harness, model, cost.
fn identity(label: &str) -> (&'static str, &'static str, &'static str) {
    match label {
        "xq" => ("sonnet-feature", "claude", "sonnet"),
        "xr" => ("codex-sol", "codex", "gpt-5.6-sol"),
        _ => ("opencode-pickle", "opencode", "opencode/big-pickle"),
    }
}

fn round_view(labels: &[&str]) -> RoundView {
    let created = RoundCreated {
        index: 0,
        base_sha: "a".repeat(40),
        labels: labels.iter().map(|l| l.to_string()).collect(),
        eligible_set: typed(json!(labels
            .iter()
            .map(|l| {
                let (teammate, harness, model) = identity(l);
                json!({"teammate": teammate, "harness": harness, "model": model,
                       "effort": "high", "fallback_index": null, "pool": harness,
                       "pool_state": "ok", "verdict": "eligible"})
            })
            .collect::<Vec<_>>())),
        propensities: labels.iter().map(|l| (l.to_string(), 0.25)).collect(),
        teacher: TeacherRef::none(),
        seed: 42,
        label_policy_version: "lp-1".into(),
    };
    let candidates = labels
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let (teammate, harness, model) = identity(l);
            let view = CandidateView {
                execution_id: Some(exec_id(0x10 + i as u8)),
                planned: Some(typed(json!({
                    "label": l, "teammate": teammate, "harness": harness, "model": model,
                    "effort": "high", "slot": "baseline", "propensity": 0.25,
                    "config_id": format!("{teammate}|{harness}|{model}|high"),
                }))),
                ..CandidateView::default()
            };
            (l.to_string(), view)
        })
        .collect();
    RoundView {
        experiment_id: exp_id(),
        state: RoundState::JudgingBackground,
        created,
        candidates,
        judge: JudgeView::default(),
        winner: None,
        rejected: None,
        promotion: PromotionView::default(),
        needs_intervention: None,
        cleaned_from: None,
        final_outcome: None,
        cleanup_failures: Vec::new(),
        outcomes: Vec::new(),
    }
}

fn head(label: &str) -> String {
    let n = LABELS.iter().position(|l| *l == label).unwrap_or(9);
    format!("{n}").repeat(40)
}

fn frozen(label: &str, i: u8) -> FrozenCandidate {
    FrozenCandidate {
        label: label.into(),
        execution_id: exec_id(0x10 + i),
        worktree: PathBuf::from(format!("/wt/{label}")),
        branch: format!("mh/exp/0199a5b0/r0/{label}"),
        base_sha: "a".repeat(40),
        head_sha: head(label),
        numstat: vec![NumstatLine {
            added: Some(1),
            deleted: Some(0),
            path: "src/lib.rs".into(),
        }],
        diff_digest: sha256_bytes(label.as_bytes()),
        frozen_at: "2026-10-02T12:00:00Z".into(),
    }
}

fn report(label: &str) -> ValidationReport {
    typed(json!({
        "validation_id": "0199a5b0-0000-7000-8000-0000000000f1",
        "label": label,
        "head_sha": head(label),
        "gates": [{"name": "test", "status": {"kind": "passed"}, "duration_ms": 4321,
                   "log_ref": format!("validation/{label}/gate-1-test.log"),
                   "log_digest": sha256_bytes(b"log").to_string(), "log_truncated": false}],
        "mechanical_score": 1.0,
        "eligible": true,
    }))
}

fn inputs(labels: &[&str]) -> Vec<(String, FrozenCandidate, ValidationReport)> {
    labels
        .iter()
        .enumerate()
        .map(|(i, l)| (l.to_string(), frozen(l, i as u8), report(l)))
        .collect()
}

/// A git client that only answers `diff_patch`, from canned patches keyed
/// by head sha. It records the `dir` of every call.
struct FakeGit {
    patches: BTreeMap<String, String>,
    dirs: std::cell::RefCell<Vec<PathBuf>>,
}

impl FakeGit {
    fn new(patches: &[(&str, &str)]) -> Self {
        FakeGit {
            patches: patches
                .iter()
                .map(|(l, p)| (head(l), p.to_string()))
                .collect(),
            dirs: Default::default(),
        }
    }
}

fn plain_patch(label: &str) -> String {
    format!(
        "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n\
         @@ -1 +1,2 @@\n fn main() {{}}\n+// change {label}\n"
    )
}

impl GitClient for FakeGit {
    fn diff_patch(
        &self,
        dir: &Path,
        _base: &str,
        head: &str,
        cap_bytes: usize,
    ) -> anyhow::Result<(String, bool)> {
        self.dirs.borrow_mut().push(dir.to_path_buf());
        let patch = self.patches.get(head).cloned().unwrap_or_default();
        if patch.len() > cap_bytes {
            Ok((patch[..cap_bytes].to_string(), true))
        } else {
            Ok((patch, false))
        }
    }
    fn toplevel(&self, _: &Path) -> anyhow::Result<PathBuf> {
        unimplemented!()
    }
    fn head(&self, _: &Path) -> anyhow::Result<String> {
        unimplemented!()
    }
    fn current_branch(&self, _: &Path) -> anyhow::Result<Option<String>> {
        unimplemented!()
    }
    fn status_porcelain(&self, _: &Path) -> anyhow::Result<String> {
        unimplemented!()
    }
    fn version(&self) -> anyhow::Result<String> {
        unimplemented!()
    }
    fn worktree_add(&self, _: &Path, _: &Path, _: &str, _: &str) -> anyhow::Result<()> {
        unimplemented!()
    }
    fn worktree_remove(&self, _: &Path, _: &Path, _: bool) -> anyhow::Result<()> {
        unimplemented!()
    }
    fn worktree_list(&self, _: &Path) -> anyhow::Result<Vec<(PathBuf, Option<String>)>> {
        unimplemented!()
    }
    fn commit_all(&self, _: &Path, _: &str, _: &GitIdentity) -> anyhow::Result<Option<String>> {
        unimplemented!()
    }
    fn rev_parse(&self, _: &Path, _: &str) -> anyhow::Result<Option<String>> {
        unimplemented!()
    }
    fn rev_list(&self, _: &Path, _: &str) -> anyhow::Result<Vec<String>> {
        unimplemented!()
    }
    fn diff_numstat(&self, _: &Path, _: &str, _: &str) -> anyhow::Result<Vec<NumstatLine>> {
        unimplemented!()
    }
    fn diff_digest(&self, _: &Path, _: &str, _: &str) -> anyhow::Result<Digest> {
        unimplemented!()
    }
    fn is_ancestor(&self, _: &Path, _: &str, _: &str) -> anyhow::Result<bool> {
        unimplemented!()
    }
    fn update_ref_cas(&self, _: &Path, _: &str, _: &str, _: &str) -> anyhow::Result<bool> {
        unimplemented!()
    }
    fn cherry_pick(&self, _: &Path, _: &str, _: &GitIdentity) -> anyhow::Result<CherryPick> {
        unimplemented!()
    }
    fn branch_checkout_location(&self, _: &Path, _: &str) -> anyhow::Result<CheckoutLocation> {
        unimplemented!()
    }
}

/// A temp state dir that makes the sealed (0500) bundle dirs writable again
/// before it is removed.
struct State(tempfile::TempDir);

impl State {
    fn new() -> Self {
        State(tempfile::tempdir().unwrap())
    }
    fn paths(&self) -> DatasetPaths {
        DatasetPaths::new(self.0.path(), Path::new("/work/alpha"))
    }
}

impl Drop for State {
    fn drop(&mut self) {
        #[cfg(unix)]
        fn unseal(dir: &Path) {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
            for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                if entry.file_type().is_ok_and(|t| t.is_dir()) {
                    unseal(&entry.path());
                }
            }
        }
        #[cfg(unix)]
        unseal(self.0.path());
    }
}

fn build(state: &State, git: &FakeGit, labels: &[&str]) -> JudgeInput {
    build_judge_input(
        &round_id(),
        &round_view(&LABELS),
        "Add a greeting to main.\n",
        &inputs(labels),
        &state.paths(),
        git,
        Path::new("/main/repo"),
    )
    .unwrap()
}

/// Every file under `dir`, relative path → bytes.
fn bundle_files(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let rel = path.strip_prefix(root).unwrap().to_string_lossy();
                out.insert(rel.replace('\\', "/"), std::fs::read(&path).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

// ─── JDG-01 ─────────────────────────────────────────────────────────────────

#[test]
fn jdg_01_judge_teammate_check() {
    let roster = repo_roster();
    let t = roster.require("judge").unwrap();
    assert_eq!(
        t.brief_description,
        "Blind evaluator for dataset rounds. Headless only; never spawned."
    );
    assert!(t.hidden);
    assert_eq!(t.base, None);
    assert_eq!(t.agent, HarnessKind::Claude);
    assert_eq!(t.model.as_deref(), Some("opus"));
    assert_eq!(t.effort.as_deref(), Some("high"));
    assert!(!t.inherit_plugins);
    assert_eq!(t.mcp_servers, Some(BTreeMap::new()));
    assert_eq!(
        t.tools,
        Some(vec!["Read".into(), "Grep".into(), "Glob".into()])
    );
    assert_eq!(
        t.disallowed_tools,
        ["Agent", "Edit", "Write", "NotebookEdit", "Bash"]
    );
    assert_eq!(t.phase, None);
    assert!(t.skills.is_empty());
    // The evaluator prose of design §6 B4 (Spec B §10): no placeholder, and
    // it names every file of the bundle that `build_judge_input` writes.
    assert!(t
        .persona
        .trim_start()
        .starts_with("You are a blind evaluator."));
    // Split so a repository grep for the marker does not match this test.
    assert!(!t.persona.contains(&["SPEC", "-TODO"].concat()));
    for file in [
        "`task.md`",
        "`rubric.md`",
        "`schema.json`",
        "`candidates/<label>/diff.patch`",
        "`candidates/<label>/validation.json`",
        "`manifest.json`",
    ] {
        assert!(t.persona.contains(file), "judge.md does not name {file}");
    }
    let lines: Vec<&str> = t.persona.lines().map(str::trim).collect();
    assert!(lines.contains(&"{rubric}") && lines.contains(&"{schema}"));
    // `horch teammates --check` runs exactly this.
    assert!(roster.check().is_empty(), "{:?}", roster.check());
    let refused = Roster::is_spawnable(t).unwrap_err().to_string();
    assert!(refused.contains("HEADLESS_ONLY"), "{refused}");
}

// ─── JDG-02 ─────────────────────────────────────────────────────────────────

#[test]
fn jdg_02_bundle_digest_and_readonly() {
    let state = State::new();
    let git = FakeGit::new(&[
        ("xq", &plain_patch("q")),
        ("xr", &plain_patch("r")),
        ("xs", &plain_patch("s")),
    ]);
    let input = build(&state, &git, &LABELS);

    // The patch is read from the main repo, never from a worktree.
    assert!(git
        .dirs
        .borrow()
        .iter()
        .all(|d| d == Path::new("/main/repo")));

    let dir = state
        .paths()
        .judge_input_dir(&exp_id(), &round_id())
        .unwrap();
    assert_eq!(input.dir, dir);
    assert_eq!(input.labels, ["A", "B", "C"]);
    let files = bundle_files(&dir);
    let mut want: BTreeSet<String> = ["task.md", "rubric.md", "schema.json", "manifest.json"]
        .into_iter()
        .map(String::from)
        .collect();
    for l in ["A", "B", "C"] {
        want.insert(format!("candidates/{l}/diff.patch"));
        want.insert(format!("candidates/{l}/validation.json"));
    }
    assert_eq!(files.keys().cloned().collect::<BTreeSet<_>>(), want);
    assert_eq!(files["task.md"], b"Add a greeting to main.\n");
    assert_eq!(files["rubric.md"], rubric_text().as_bytes());
    assert_eq!(files["schema.json"], schema_text().as_bytes());

    // The digest is over the manifest bytes, and the manifest digests every
    // other file.
    assert_eq!(input.digest, sha256_bytes(&files["manifest.json"]));
    let manifest: JudgeInputManifest = serde_json::from_slice(&files["manifest.json"]).unwrap();
    assert_eq!(manifest, input.manifest);
    assert_eq!(manifest.schema_version, "1.0.0");
    assert_eq!(manifest.round_id, round_id());
    assert_eq!(manifest.rubric_version, "rubric-2");
    assert_eq!(
        manifest.schema_digest,
        sha256_bytes(schema_text().as_bytes())
    );
    assert!(manifest.truncated_diffs.is_empty());
    assert_eq!(manifest.files.len(), files.len() - 1);
    for (rel, digest) in &manifest.files {
        assert_eq!(*digest, sha256_bytes(&files[rel]), "{rel}");
    }

    // Each bundle label maps back to the original label of its diff.
    for (blind, original) in &input.label_map {
        let patch = String::from_utf8(files[&format!("candidates/{blind}/diff.patch")].clone());
        let suffix = &original[1..];
        assert!(patch.unwrap().contains(&format!("change {suffix}")));
        let v: BlindValidation =
            serde_json::from_slice(&files[&format!("candidates/{blind}/validation.json")]).unwrap();
        assert_eq!(v.label, *blind);
        assert!(v.eligible);
    }

    #[cfg(unix)]
    {
        for rel in files.keys() {
            assert_eq!(mode(&dir.join(rel)), 0o400, "{rel}");
        }
        for d in [
            "",
            "candidates",
            "candidates/A",
            "candidates/B",
            "candidates/C",
        ] {
            assert_eq!(mode(&dir.join(d)), 0o500, "{d}");
        }
    }

    // A second build for the same round verifies and writes nothing.
    let again = build(&state, &git, &LABELS);
    assert_eq!(again.digest, input.digest);
    assert_eq!(again.label_map, input.label_map);
    assert_eq!(bundle_files(&dir), files);

    // A different diff for the same round is a conflict, not a rewrite.
    let changed = FakeGit::new(&[
        ("xq", &plain_patch("other")),
        ("xr", &plain_patch("r")),
        ("xs", &plain_patch("s")),
    ]);
    let err = build_judge_input(
        &round_id(),
        &round_view(&LABELS),
        "Add a greeting to main.\n",
        &inputs(&LABELS),
        &state.paths(),
        &changed,
        Path::new("/main/repo"),
    )
    .unwrap_err();
    assert!(format!("{err:#}").contains("different content"), "{err:#}");
    assert_eq!(bundle_files(&dir), files);
}

#[test]
fn jdg_02_labels_seeded_shuffle() {
    let originals: Vec<String> = ["c0", "c1", "c2", "c3"].map(String::from).to_vec();
    let rid = |n: u32| RoundId::new(format!("0199a5b0-0000-7000-8000-{n:012x}")).unwrap();

    // The same round id gives the same mapping.
    assert_eq!(
        blind_labels(&rid(7), &originals),
        blind_labels(&rid(7), &originals)
    );
    for pairs in (0..20).map(|n| blind_labels(&rid(n), &originals)) {
        let blind: Vec<&str> = pairs.iter().map(|(b, _)| b.as_str()).collect();
        assert_eq!(blind, ["A", "B", "C", "D"]);
        let mut seen: Vec<&str> = pairs.iter().map(|(_, o)| o.as_str()).collect();
        seen.sort();
        assert_eq!(seen, ["c0", "c1", "c2", "c3"]);
    }

    // Different round ids give different mappings.
    let distinct: BTreeSet<Vec<(String, String)>> =
        (0..20).map(|n| blind_labels(&rid(n), &originals)).collect();
    assert!(distinct.len() >= 2, "{} mappings", distinct.len());

    // The caller's candidate order does not change the bundle.
    let state = State::new();
    let git = FakeGit::new(&[
        ("xq", &plain_patch("q")),
        ("xr", &plain_patch("r")),
        ("xs", &plain_patch("s")),
    ]);
    let forward = build(&state, &git, &LABELS);
    let other = State::new();
    let reversed = build(&other, &git, &["xs", "xr", "xq"]);
    assert_eq!(forward.digest, reversed.digest);
    assert_eq!(forward.label_map, reversed.label_map);
    let shuffled: Vec<(String, String)> = blind_labels(&round_id(), &LABELS.map(String::from));
    assert_eq!(
        forward.label_map,
        shuffled.into_iter().collect::<BTreeMap<_, _>>()
    );
}

#[test]
fn jdg_02_no_identity_cost_latency() {
    let state = State::new();
    let git = FakeGit::new(&[
        ("xq", &plain_patch("q")),
        ("xr", &plain_patch("r")),
        ("xs", &plain_patch("s")),
    ]);
    let input = build(&state, &git, &LABELS);
    let files = bundle_files(&input.dir);

    let mut forbidden: Vec<String> = Vec::new();
    for (i, label) in LABELS.iter().enumerate() {
        let (teammate, harness, model) = identity(label);
        forbidden.extend([
            exec_id(0x10 + i as u8).to_string(),
            teammate.to_string(),
            harness.to_string(),
            model.to_string(),
            label.to_string(),
            format!("mh/exp/0199a5b0/r0/{label}"),
            format!("/wt/{label}"),
            head(label),
        ]);
    }
    forbidden.extend(
        [
            "0199a5b0-0000-7000-8000-0000000000f1", // validation id
            "validation_id",
            "execution_id",
            "teammate",
            "harness",
            "model",
            "cost",
            "usd",
            "latency",
            "duration",
            "4321",
            "log_ref",
            "frozen_at",
            "propensit",
            "history",
        ]
        .map(String::from),
    );
    for (rel, bytes) in &files {
        // The rubric and schema are compiled-in texts and the same for
        // every round; check that they hold no identity either.
        let text = String::from_utf8_lossy(bytes).to_ascii_lowercase();
        for f in &forbidden {
            assert!(
                !text.contains(&f.to_ascii_lowercase()),
                "{rel} contains '{f}'"
            );
        }
    }
    assert!(input.blindness.is_empty(), "{:?}", input.blindness);
}

// ─── JDG-10 ─────────────────────────────────────────────────────────────────

#[test]
fn jdg_10_identity_leak_flagged() {
    let state = State::new();
    let leaky = "diff --git a/NOTES.md b/NOTES.md\n--- a/NOTES.md\n+++ b/NOTES.md\n\
                 @@ -0,0 +1,2 @@\n+change r\n+Co-Authored-By: Claude <noreply@example.com>\n";
    let git = FakeGit::new(&[
        ("xq", &plain_patch("q")),
        ("xr", leaky),
        ("xs", &plain_patch("s")),
    ]);
    let input = build(&state, &git, &LABELS);
    let blind = input
        .label_map
        .iter()
        .find(|(_, o)| o.as_str() == "xr")
        .map(|(b, _)| b.clone())
        .unwrap();
    assert_eq!(input.blindness.len(), 1, "{:?}", input.blindness);
    let flag = &input.blindness[0];
    assert_eq!(flag.label, blind);
    assert_eq!(flag.token, "claude");
    assert_eq!(flag.file, "NOTES.md");

    // The diff is stored as it was: flagged, not edited.
    let stored = std::fs::read_to_string(input.dir.join(format!("candidates/{blind}/diff.patch")));
    assert_eq!(stored.unwrap(), leaky);
}
