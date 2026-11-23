#![cfg(unix)]

use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::Path;

use horch_core::fleetd::ideas::adapters::idea_folder_files::{FolderError, IdeaFolder};
use horch_core::fleetd::ideas::domain::idea_folder::{IdeaFile, Mark, FILE_LIMIT};
use horch_core::fsx::{Created, FsxError};
use horch_platform::channel::User;

const IDEA: &str = "I-0007";

fn folder(tmp: &tempfile::TempDir) -> IdeaFolder {
    IdeaFolder::create(&tmp.path().join("ideas"), IDEA, User::current().unwrap()).unwrap()
}

fn idea(text: &str) -> IdeaFile {
    IdeaFile {
        id: IDEA.into(),
        created: "2026-11-10T15:02:11-07:00".into(),
        source: "horch idea add".into(),
        priority: 0,
        depends_on: vec![],
        text: text.into(),
    }
}

const ASSUMPTIONS: &str = "# I-0007 assumptions
feature: retry-sync-backoff
1. [confirmed] Retries apply to all sync requests, not uploads. (Q3)
2. [assumed] Max 5 tries is enough; no config option needed.
3. [assumed] Log level INFO for each retry.
";

fn reason_of(err: FolderError) -> String {
    match err {
        FolderError::Invalid { reason, .. } | FolderError::Rejected { reason, .. } => reason,
        other => panic!("not a refused file: {other}"),
    }
}

#[test]
fn idea_md_is_written_once_and_never_changed() {
    let tmp = tempfile::tempdir().unwrap();
    let f = folder(&tmp);
    let first = idea("Add retry with backoff to the sync client.\n");

    assert_eq!(f.write_idea(&first).unwrap(), Created::Written);
    assert_eq!(f.write_idea(&first).unwrap(), Created::AlreadyIdentical);
    let changed = f.write_idea(&idea("Something else.\n")).unwrap_err();
    assert!(
        matches!(changed, FolderError::Write(FsxError::Conflict { .. })),
        "{changed}"
    );
    assert_eq!(f.read_idea().unwrap(), first);
}

#[test]
fn idea_md_of_another_idea_does_not_validate() {
    let tmp = tempfile::tempdir().unwrap();
    let f = folder(&tmp);
    let mut other = idea("text\n");
    other.id = "I-0008".into();
    f.write_idea(&other).unwrap();

    let reason = reason_of(f.read_idea().unwrap_err());
    assert!(reason.contains("I-0008"), "{reason}");
}

#[test]
fn amendments_are_read_in_order() {
    let tmp = tempfile::tempdir().unwrap();
    let f = folder(&tmp);
    assert_eq!(f.read_amendments().unwrap(), Vec::<String>::new());

    f.write_amendment(2, "Second.\n").unwrap();
    f.write_amendment(1, "Also cap the delay at 30 s.\n")
        .unwrap();
    assert_eq!(
        f.write_amendment(1, "Also cap the delay at 30 s.\n")
            .unwrap(),
        Created::AlreadyIdentical
    );
    assert!(f.write_amendment(1, "Changed.\n").is_err());

    assert_eq!(
        f.read_amendments().unwrap(),
        vec!["Also cap the delay at 30 s.\n", "Second.\n"]
    );
}

#[test]
fn interview_answers_are_numbered() {
    let tmp = tempfile::tempdir().unwrap();
    let f = folder(&tmp);
    assert_eq!(f.read_answers().unwrap(), Vec::<String>::new());

    assert_eq!(
        f.append_interview("What is done?", "All tests pass.")
            .unwrap(),
        "Q1"
    );
    assert_eq!(f.append_interview("Scope?", "Sync only.").unwrap(), "Q2");
    assert_eq!(f.read_answers().unwrap(), vec!["Q1", "Q2"]);
}

#[test]
fn assumptions_with_an_open_item_do_not_validate() {
    let tmp = tempfile::tempdir().unwrap();
    let f = folder(&tmp);
    f.write_assumptions(&ASSUMPTIONS.replace("3. [assumed]", "3. [open]"))
        .unwrap();

    let err = f.read_assumptions().unwrap_err();
    assert!(matches!(err, FolderError::Invalid { .. }), "{err}");
    assert_eq!(reason_of(err), "item 3 is [open]");
}

#[test]
fn assumptions_give_the_feature_name() {
    let tmp = tempfile::tempdir().unwrap();
    let f = folder(&tmp);
    f.write_assumptions(ASSUMPTIONS).unwrap();

    let read = f.read_assumptions().unwrap();
    assert_eq!(read.feature.as_deref(), Some("retry-sync-backoff"));
    assert_eq!(read.ids(), vec!["A1", "A2", "A3"]);
    assert_eq!(read.items[0].mark, Mark::Confirmed);
    assert_eq!(read.items[0].question.as_deref(), Some("Q3"));
    assert_eq!(read.items[1].question, None);

    f.write_assumptions(&ASSUMPTIONS.replace("retry-sync-backoff", "Retry_Sync"))
        .unwrap();
    let reason = reason_of(f.read_assumptions().unwrap_err());
    assert!(reason.contains("kebab-case"), "{reason}");
}

#[test]
fn spec_needs_a_section() {
    let tmp = tempfile::tempdir().unwrap();
    let f = folder(&tmp);
    assert!(matches!(
        f.read_spec().unwrap_err(),
        FolderError::Missing { .. }
    ));

    f.write_spec("# Spec\nNo section.\n").unwrap();
    assert!(matches!(
        f.read_spec().unwrap_err(),
        FolderError::Invalid { .. }
    ));

    f.write_spec("# Spec\n## Retries\nText.\n").unwrap();
    assert_eq!(f.read_spec().unwrap(), "# Spec\n## Retries\nText.\n");
}

#[test]
fn requirements_cite_assumptions_and_answers() {
    let tmp = tempfile::tempdir().unwrap();
    let f = folder(&tmp);
    f.write_assumptions(ASSUMPTIONS).unwrap();
    f.append_interview("What is done?", "All tests pass.")
        .unwrap();
    f.write_requirements(
        "idea: I-0007
requirements:
  - id: R1
    text: Sync client retries failed requests.
    cites: [A1, A2]
  - id: R2
    text: Existing sync tests still pass.
    cites: [Q1]
",
    )
    .unwrap();

    let reqs = f.read_requirements().unwrap();
    assert_eq!(reqs.len(), 2);
    assert_eq!(reqs[0].id, "R1");
    assert_eq!(reqs[0].cites, vec!["A1", "A2"]);
    assert_eq!(reqs[1].cites, vec!["Q1"]);
}

#[test]
fn requirement_without_a_citation_does_not_validate() {
    let tmp = tempfile::tempdir().unwrap();
    let f = folder(&tmp);
    f.write_assumptions(ASSUMPTIONS).unwrap();
    f.write_requirements(
        "idea: I-0007
requirements:
  - id: R1
    text: Sync client retries failed requests.
",
    )
    .unwrap();

    let err = f.read_requirements().unwrap_err();
    assert!(matches!(err, FolderError::Invalid { .. }), "{err}");
    assert_eq!(reason_of(err), "requirement R1 cites nothing");
}

#[test]
fn requirement_citing_a_missing_assumption_does_not_validate() {
    let tmp = tempfile::tempdir().unwrap();
    let f = folder(&tmp);
    f.write_assumptions(ASSUMPTIONS).unwrap();
    f.write_requirements(
        "idea: I-0007
requirements:
  - id: R1
    text: Sync client retries failed requests.
    cites: [A1, A9, Q2]
",
    )
    .unwrap();

    let err = f.read_requirements().unwrap_err();
    assert!(matches!(err, FolderError::Invalid { .. }), "{err}");
    assert_eq!(
        reason_of(err),
        "requirement R1 cites A9, which is not an assumption or an answer; \
         requirement R1 cites Q2, which is not an assumption or an answer"
    );
}

#[test]
fn assumptions_that_are_a_symlink_are_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let f = folder(&tmp);
    let target = f.dir().join("real.md");
    fs::write(&target, ASSUMPTIONS).unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
    symlink(&target, f.dir().join("assumptions.md")).unwrap();

    let err = f.read_assumptions().unwrap_err();
    assert!(matches!(err, FolderError::Rejected { .. }), "{err}");
    assert!(reason_of(err).contains("link"));
}

#[test]
fn a_file_over_its_limit_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let f = folder(&tmp);
    let big = format!("# Spec\n## Section\n{}", "x".repeat(FILE_LIMIT as usize));
    f.write_spec(&big).unwrap();

    let err = f.read_spec().unwrap_err();
    assert!(matches!(err, FolderError::Rejected { .. }), "{err}");
    assert!(reason_of(err).contains("limit"));
}

#[test]
fn the_folder_is_private() {
    let tmp = tempfile::tempdir().unwrap();
    let f = folder(&tmp);
    f.write_spec("# Spec\n## A\n").unwrap();
    let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(f.dir()), 0o700);
    assert_eq!(mode(&f.dir().join("spec.md")), 0o600);
}
