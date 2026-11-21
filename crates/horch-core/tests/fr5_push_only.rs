//! FR-5: the supervisor assigns tasks to workers (push), and no agent
//! queries the ledger for open tasks. The choice of a task is
//! `dispatch::choices` (the rules of `dispatch::choose`), and only the bind
//! module and the dispatch point call it, before the pane exists: fleetd
//! (`dispatch_due`). `horch spawn` no longer chooses a task (ticket 48 of
//! fleetd-p6). `horch status` calls `choices` only to read the holds of
//! each task (`holds.rs`) and binds nothing.
//!
//! `pick_task`, the search of `horch ledger claim`, and `tasks::claim` are
//! gone (T-871). `CLAIM_TASK`, the first line that told a worker to search,
//! is gone.

use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Each `.rs` file under `crates/*/src`, with its path relative to the
/// workspace root.
fn sources() -> Vec<(String, String)> {
    let root = workspace_root();
    let mut files = Vec::new();
    for krate in std::fs::read_dir(root.join("crates")).unwrap() {
        let src = krate.unwrap().path().join("src");
        if src.is_dir() {
            rust_files(&src, &mut files);
        }
    }
    files
        .into_iter()
        .map(|p| {
            let relative = p
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            (relative, std::fs::read_to_string(&p).unwrap())
        })
        .collect()
}

/// The lines of `text` before its test module, each with its comment cut.
fn code_lines(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        if line.trim_start().starts_with("#[cfg(test)]")
            && lines
                .peek()
                .is_some_and(|l| l.trim_start().starts_with("mod "))
        {
            break;
        }
        let code = match line.find("//") {
            Some(i) => &line[..i],
            None => line,
        };
        out.push(code.to_string());
    }
    out
}

/// Whether `line` calls `name`: `name(` with no identifier character
/// before it, and not its definition `fn name(`.
fn calls(line: &str, name: &str) -> bool {
    let pattern = format!("{name}(");
    line.match_indices(&pattern).any(|(i, _)| {
        let before = line[..i].chars().next_back();
        let ident = before.is_some_and(|c| c.is_alphanumeric() || c == '_');
        !ident && !line[..i].trim_end().ends_with("fn")
    })
}

/// Each `file:line` of production code that calls `name`.
fn callers(name: &str) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    for (file, text) in sources() {
        for (n, line) in code_lines(&text).iter().enumerate() {
            if calls(line, name) {
                out.push((file.clone(), n + 1));
            }
        }
    }
    out
}

const APP_DISPATCH: &str = "crates/horch-core/src/fleetd/task_execution/application/dispatch.rs";
const DOMAIN_DISPATCH: &str = "crates/horch-core/src/fleetd/task_execution/domain/dispatch.rs";
const HOLDS: &str = "crates/horch-core/src/fleetd/fleet_runtime/application/holds.rs";
const TASKS: &str = "crates/horch-core/src/taskledger/tasks.rs";

#[test]
fn the_scan_finds_a_call_and_skips_a_definition_and_a_comment() {
    assert!(calls("    let c = choices(tx, &h, &s, None)?;", "choices"));
    assert!(calls("dispatch::choices(tx)", "choices"));
    assert!(!calls("pub fn choices(", "choices"));
    assert!(!calls("let my_choices(x)", "choices"));
    assert_eq!(code_lines("a // choices(x)\n"), ["a "]);
    assert_eq!(
        code_lines("x\n#[cfg(test)]\nmod tests {\n choices(x)\n}\n"),
        ["x"]
    );
}

#[test]
fn only_the_bind_and_its_dispatch_points_choose_a_task() {
    let mut offenders: Vec<String> = callers("choose")
        .into_iter()
        .filter(|(f, _)| f != APP_DISPATCH && f != DOMAIN_DISPATCH)
        .map(|(f, n)| format!("{f}:{n}: choose("))
        .collect();
    let choosers = callers("choices");
    offenders.extend(
        choosers
            .iter()
            .filter(|(f, _)| f != APP_DISPATCH && f != HOLDS)
            .map(|(f, n)| format!("{f}:{n}: choices(")),
    );
    assert!(
        offenders.is_empty(),
        "FR-5: only the bind module, fleetd and the holds view choose a task:\n{}",
        offenders.join("\n")
    );
    assert!(
        choosers.iter().any(|(f, _)| f == APP_DISPATCH),
        "fleetd chooses the task that it binds"
    );
}

#[test]
fn the_claim_search_is_gone() {
    let left: Vec<String> = sources()
        .into_iter()
        .flat_map(|(f, text)| {
            code_lines(&text)
                .into_iter()
                .filter(|l| l.contains("pick_task") || l.contains("tasks::claim("))
                .map(move |l| format!("{f}: {}", l.trim()))
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(
        left.is_empty(),
        "FR-5: no code searches for a task or claims one:\n{}",
        left.join("\n")
    );
    let tasks = std::fs::read_to_string(workspace_root().join(TASKS)).unwrap();
    assert!(
        !code_lines(&tasks).iter().any(|l| l.contains("fn claim(")),
        "FR-5: {TASKS} has no claim"
    );
}

#[test]
fn claim_task_is_gone() {
    let left: Vec<String> = sources()
        .into_iter()
        .filter(|(_, text)| text.contains("CLAIM_TASK"))
        .map(|(f, _)| f)
        .collect();
    assert!(left.is_empty(), "CLAIM_TASK is still in {left:?}");
}
