//! `task.yaml` and `START.md` name the idea of the task, its
//! `requirements.yaml` and the decisions file of the feature, so each
//! worker reads them before it starts (RR-12; docs/specs/fleetd/design.md,
//! "Workspace layout and file protocol" (Example: task.yaml) and "Ideas"
//! (How a worker's question is resolved, step 6); MAP.md, Q15;
//! docs/tickets/fleetd-p6/49-task-files.md).

use std::path::{Path, PathBuf};

use horch_core::fleetd::task_execution::adapters::task_files::{
    attempt_dir, write_attempt_files, AttemptFiles,
};
use horch_core::fleetd::task_execution::domain::task_files::TaskFacts;

const TICKET: &str = "docs/tickets/p6/07-retry.md";

/// The facts of attempt `A1` of task `T7` of idea `I-0007` of feature
/// `F-0003`, with a worktree under `root`.
fn facts(root: &Path) -> TaskFacts {
    TaskFacts {
        task: "T7".into(),
        task_id: "0b5f9c1e-6d8a-4c1b-9a51-1f1f0e2d3c4b".into(),
        run: "F-0003".into(),
        attempt: "A1".into(),
        profile: "opus".into(),
        worktree: root.join("work").join("T7").to_string_lossy().into_owned(),
        branch: "fleet/F-0003-T7".into(),
        read_first: TICKET.into(),
        goal: "Log each retry".into(),
        check: Some("cargo test".into()),
        kind: Some("feature".into()),
        ticket_snapshot: "# 07: retry\n\nDo the work.\n".into(),
        extra: None,
        handoff: None,
        unlanded: None,
        idea: Some("I-0007".into()),
        feature: Some("F-0003".into()),
        covers: vec!["R1".into(), "R2".into()],
        verdict: None,
    }
}

/// Write `text` to `path`, with its folders.
fn put(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn requirements(root: &Path) -> PathBuf {
    root.join("ideas").join("I-0007").join("requirements.yaml")
}

fn decisions(root: &Path) -> PathBuf {
    root.join("features").join("F-0003").join("decisions.md")
}

fn yaml(files: &AttemptFiles) -> serde_yaml::Value {
    serde_yaml::from_str(&std::fs::read_to_string(&files.task_yaml).unwrap()).unwrap()
}

fn start(files: &AttemptFiles) -> String {
    std::fs::read_to_string(&files.start_md).unwrap()
}

/// The `read_first` list of a `task.yaml`.
fn read_first(y: &serde_yaml::Value) -> Vec<String> {
    y["read_first"]
        .as_sequence()
        .unwrap_or_else(|| panic!("read_first is a list: {y:?}"))
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect()
}

fn shown(p: &Path) -> String {
    p.display().to_string()
}

#[test]
fn task_yaml_names_its_idea_and_requirements() {
    let root = tempfile::tempdir().unwrap();
    put(&requirements(root.path()), "- id: R1\n  text: a\n");
    let f = facts(root.path());
    let files = write_attempt_files(root.path(), &f, &[]).unwrap();
    let y = yaml(&files);
    assert_eq!(y["idea"], "I-0007");
    assert_eq!(
        read_first(&y),
        [TICKET.to_string(), shown(&requirements(root.path()))],
        "no decisions file yet: the list does not name it"
    );
    let covers: Vec<&str> = y["covers"]
        .as_sequence()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(covers, ["R1", "R2"]);
    assert_eq!(
        attempt_dir(root.path(), &f.task_id, "A1").join("task.yaml"),
        files.task_yaml
    );

    // A task of a run in flight with no idea folder lists only its ticket.
    let root = tempfile::tempdir().unwrap();
    let mut f = facts(root.path());
    f.covers.clear();
    let y = yaml(&write_attempt_files(root.path(), &f, &[]).unwrap());
    assert_eq!(y["idea"], "I-0007");
    assert_eq!(read_first(&y), [TICKET]);
    assert_eq!(y["covers"].as_sequence().map(Vec::len), Some(0));

    // A task with no idea has a null idea.
    let root = tempfile::tempdir().unwrap();
    let mut f = facts(root.path());
    f.idea = None;
    f.feature = None;
    let y = yaml(&write_attempt_files(root.path(), &f, &[]).unwrap());
    assert!(y["idea"].is_null(), "{y:?}");
    assert_eq!(read_first(&y), [TICKET]);
}

#[test]
fn start_md_lists_the_decisions_file_of_the_feature() {
    let root = tempfile::tempdir().unwrap();
    put(&requirements(root.path()), "- id: R1\n  text: a\n");
    put(&decisions(root.path()), "# F-0003 decisions\n");
    let f = facts(root.path());
    let files = write_attempt_files(root.path(), &f, &[]).unwrap();

    let want = [
        TICKET.to_string(),
        shown(&requirements(root.path())),
        shown(&decisions(root.path())),
    ];
    assert_eq!(read_first(&yaml(&files)), want);

    let text = start(&files);
    for path in &want {
        assert!(text.contains(&format!("- `{path}`")), "{path}: {text}");
    }
    let snapshot_end = text.find("Do the work.").unwrap();
    let list = text.find("Read these files first").unwrap();
    assert!(
        snapshot_end < list,
        "the ticket snapshot comes first: {text}"
    );

    // With no idea folder and no decisions file, START.md names no file
    // of an idea.
    let root = tempfile::tempdir().unwrap();
    let f = facts(root.path());
    let text = start(&write_attempt_files(root.path(), &f, &[]).unwrap());
    assert!(!text.contains("requirements.yaml"), "{text}");
    assert!(!text.contains("decisions.md"), "{text}");
}

#[test]
fn fix_attempt_lists_the_verdict() {
    let root = tempfile::tempdir().unwrap();
    put(&decisions(root.path()), "# F-0003 decisions\n");
    let verdict = root.path().join("work").join("T8").join("review.md");
    put(&verdict, "## Verdict\n\ncritical_open\n");
    let mut f = facts(root.path());
    f.attempt = "A2".into();
    f.verdict = Some(shown(&verdict));
    let files = write_attempt_files(root.path(), &f, &[]).unwrap();

    assert_eq!(
        read_first(&yaml(&files)),
        [
            TICKET.to_string(),
            shown(&decisions(root.path())),
            shown(&verdict)
        ]
    );
    let text = start(&files);
    assert!(text.contains(&format!("- `{}`", shown(&verdict))), "{text}");

    // An attempt that is not a fix attempt names no verdict.
    let root = tempfile::tempdir().unwrap();
    let f = facts(root.path());
    let text = start(&write_attempt_files(root.path(), &f, &[]).unwrap());
    assert!(!text.contains("review.md"), "{text}");
}
