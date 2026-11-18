//! `horch ledger add-map --dry-run` runs the plan validator on a `MAP.md`
//! or a `plan.yaml` (FR-3, FM-1). The requirements of a map are the ids that
//! the tables of its `Spec:` file define, by the rule of
//! `scripts/check-req-coverage.sh`.

use std::path::{Path, PathBuf};

use horch_core::taskledger::map;

/// A ticket in format C1 that covers `covers` (requirement lines).
fn ticket(title: &str, covers: &str) -> String {
    format!(
        "# {title}\n\n<!-- horch-ticket: 1 -->\n\
         **Persona:** primary `opus` · **Parallel:** yes\n\
         **Kind:** feature\n\
         **Check:** `true`\n\
         **Own:** `src/a.rs`\n\n\
         Spec: `docs/specs/xr.md`\n\n\
         ## Goal\n\n{title}.\n\n\
         ## Requirements\n\n{covers}\n\n\
         ## Steps\n\n1. Do it.\n\n\
         ## Done when\n\nThe Check passes.\n"
    )
}

/// A project in `dir` with the spec `docs/specs/xr.md`, the tickets and the
/// map `tickets/MAP.md`. Each ticket is `(file stem, text)`.
fn project(dir: &Path, rows: &str, tickets: &[(&str, String)]) -> PathBuf {
    let specs = dir.join("docs/specs");
    std::fs::create_dir_all(&specs).unwrap();
    std::fs::write(
        specs.join("xr.md"),
        "# XR\n\n| ID | Requirement | Check |\n|---|---|---|\n\
         | XRA-01 | The tool reads the limit. | A test. |\n\
         | XRA-02 | The tool logs each retry. | A test. |\n",
    )
    .unwrap();
    let tdir = dir.join("tickets");
    std::fs::create_dir_all(&tdir).unwrap();
    for (stem, text) in tickets {
        std::fs::write(tdir.join(format!("{stem}.md")), text).unwrap();
    }
    let map = tdir.join("MAP.md");
    std::fs::write(
        &map,
        format!(
            "# Map: xr\n\nSpec: `docs/specs/xr.md`\n\n\
             | # | Title | Blocked by | Wave |\n|---|---|---|---|\n{rows}"
        ),
    )
    .unwrap();
    map
}

fn problems(path: &Path, root: &Path) -> Vec<String> {
    let (_, problems) = map::check(path, root).expect("the dry run reads the file");
    problems.iter().map(ToString::to_string).collect()
}

#[test]
fn map_with_an_uncovered_requirement_names_the_id() {
    let dir = tempfile::tempdir().unwrap();
    let map = project(
        dir.path(),
        "| 01 | [Read the limit](01-limit.md) |  | 1 |\n",
        &[(
            "01-limit",
            ticket(
                "01: Read the limit",
                "- **XRA-01**: The tool reads the limit.",
            ),
        )],
    );
    let found = problems(&map, dir.path());
    assert!(
        found
            .iter()
            .any(|p| p.contains("XRA-02") && p.contains("coverage")),
        "the dry run does not name the uncovered XRA-02: {found:?}"
    );
    assert!(
        !found.iter().any(|p| p.contains("XRA-01")),
        "the dry run names the covered XRA-01: {found:?}"
    );

    let covered = project(
        dir.path(),
        "| 01 | [Read the limit](01-limit.md) |  | 1 |\n\
         | 02 | [Log each retry](02-log.md) | 01 | 2 |\n",
        &[(
            "02-log",
            ticket("02: Log each retry", "- XRA-02 (the retry log)"),
        )],
    );
    assert_eq!(problems(&covered, dir.path()), Vec::<String>::new());
}

#[test]
fn map_with_a_cycle_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let map = project(
        dir.path(),
        "| 01 | [Read the limit](01-limit.md) | 02 | 1 |\n\
         | 02 | [Log each retry](02-log.md) | 01 | 1 |\n",
        &[
            ("01-limit", ticket("01: Read the limit", "- XRA-01")),
            ("02-log", ticket("02: Log each retry", "- XRA-02")),
        ],
    );
    let found = problems(&map, dir.path());
    assert!(
        found.iter().any(|p| p.contains("cycle")),
        "the dry run accepts a cycle: {found:?}"
    );
}

/// A plan.yaml of 2 tasks; `a_after` is the `after:` list of task `a`.
fn plan_yaml(dir: &Path, a_after: &str, a_kind: &str) -> PathBuf {
    let path = dir.join("plan.yaml");
    std::fs::write(
        &path,
        format!(
            "idea: I-0001\nrepo: /fleet/repos/app\nfeature: F-0001\nbase_branch: feature/x\n\
             tasks:\n\
             \x20 - key: a\n    title: Task a\n    kind: {a_kind}\n    profile: coder-codex\n\
             \x20   covers: [R1]\n    after: [{a_after}]\n    acceptance:\n      - run: cargo test\n\
             \x20 - key: b\n    title: Task b\n    kind: feature\n    profile: coder-claude\n\
             \x20   covers: [R2]\n    after: [a]\n    acceptance:\n      - run: cargo test\n"
        ),
    )
    .unwrap();
    path
}

#[test]
fn plan_yaml_runs_the_same_validator() {
    let dir = tempfile::tempdir().unwrap();
    let valid = plan_yaml(dir.path(), "", "feature");
    assert_eq!(problems(&valid, dir.path()), Vec::<String>::new());
    let (parsed, _) = map::check(&valid, dir.path()).unwrap();
    assert_eq!(
        parsed.items.len(),
        2,
        "the dry run counts each task of the plan"
    );

    let broken = plan_yaml(dir.path(), "b", "chore");
    let found = problems(&broken, dir.path());
    assert!(
        found.iter().any(|p| p.starts_with("plan.yaml:")
            && p.contains("dag: task a:")
            && p.contains("cycle")),
        "the dry run does not report the dag rule: {found:?}"
    );
    assert!(
        found
            .iter()
            .any(|p| p.contains("schema: task a:") && p.contains("`chore` is not a kind")),
        "the dry run does not report the schema rule: {found:?}"
    );
}

/// The ids that the script defines in the fixture spec: it runs in a copy of
/// the repository layout, with a `cargo` that lists no test, so each id that
/// it defines is printed as `MISSING <id>:`.
#[cfg(unix)]
fn script_ids(spec: &str) -> Vec<String> {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let repo_script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/check-req-coverage.sh");
    std::fs::create_dir_all(root.join("scripts")).unwrap();
    std::fs::create_dir_all(root.join("docs/specs")).unwrap();
    std::fs::create_dir_all(root.join("bin")).unwrap();
    std::fs::copy(&repo_script, root.join("scripts/check-req-coverage.sh")).unwrap();
    std::fs::write(root.join("docs/specs/fixture.md"), spec).unwrap();
    let cargo = root.join("bin/cargo");
    std::fs::write(&cargo, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&cargo, std::fs::Permissions::from_mode(0o755)).unwrap();
    for args in [&["init", "-q"][..], &["add", "docs/specs/fixture.md"][..]] {
        let ok = Command::new("git")
            .args(args)
            .current_dir(root)
            .status()
            .unwrap();
        assert!(ok.success(), "git {args:?}");
    }
    let path = format!(
        "{}:{}",
        root.join("bin").display(),
        std::env::var("PATH").unwrap()
    );
    let out = Command::new("bash")
        .arg(root.join("scripts/check-req-coverage.sh"))
        .env("PATH", path)
        .current_dir(root)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut ids: Vec<String> = stdout
        .lines()
        .filter_map(|l| l.strip_prefix("MISSING "))
        .filter_map(|l| l.split(':').next())
        .map(str::to_string)
        .collect();
    assert!(
        !ids.is_empty(),
        "the script defines no id: {stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    ids.sort();
    ids
}

#[cfg(unix)]
#[test]
fn script_and_validator_find_the_same_ids_in_docs_specs() {
    let spec = "# Fixture\n\n\
        | ID | Requirement | Phase |\n|---|---|---|\n\
        | ABC-01 | Defined. | A1 |\n\
        | ABC-02 | Defined. | - |\n\
        | AB-03 | Two letters: not an id. | A1 |\n\
        | ABC-4 | One digit: not an id. | A1 |\n\
        |ABC-05 | No space: not an id. | A1 |\n\
        Text ends the table.\n\
        | ABC-06 | A row of a table whose header is this row. |\n\
        | ABC-07 | Not defined: the header's second cell is not Requirement. |\n\n\
        | Id | Check |\n|---|---|\n\
        | DEF-01 | Defined by a Check table. |\n\n\
        | Test | Covers |\n|---|---|\n\
        | GHI-01 | A test mapping only refers to it. |\n\n\
        | ID | Requirement |\n|---|---|\n\
        | JKL-01 | Defined. |\n\
        | `JKL-02` | Code: not an id. |\n";
    let mut rust: Vec<String> = map::requirement_ids(spec)
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    rust.sort();
    assert_eq!(rust, ["ABC-01", "ABC-02", "DEF-01", "JKL-01"]);
    assert_eq!(script_ids(spec), rust);
}
