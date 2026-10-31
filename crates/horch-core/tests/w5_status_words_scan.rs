//! Issue #240: the ledger SQL builds its status words from the enums in
//! `taskledger/model.rs`, so a word added to an enum reaches every reader.
//!
//! The scan reads the source as text. Comment lines and the `#[cfg(test)]`
//! tail of a file are skipped: a test may seed a row with a literal word,
//! and the `CHECK` in the schema rejects a bad one.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use horch_core::taskledger::model::{
    FindingStatus, RunStatus, StepStatus, TaskStatus, WorkerStatus, WorktreeStatus,
};

/// Files that define the words: the enums, and the schema's frozen text.
const DEFINING: &[&str] = &["model.rs", "schema.rs"];

fn status_words() -> BTreeSet<&'static str> {
    let mut words = BTreeSet::new();
    words.extend(RunStatus::ALL.iter().map(|s| s.as_str()));
    words.extend(TaskStatus::ALL.iter().map(|s| s.as_str()));
    words.extend(WorkerStatus::ALL.iter().map(|s| s.as_str()));
    words.extend(WorktreeStatus::ALL.iter().map(|s| s.as_str()));
    words.extend(StepStatus::ALL.iter().map(|s| s.as_str()));
    words.extend(FindingStatus::ALL.iter().map(|s| s.as_str()));
    words
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

/// The `'word'` tokens in `line` that name a status.
fn quoted_status_words<'a>(line: &'a str, words: &BTreeSet<&str>) -> Vec<&'a str> {
    line.split('\'')
        .enumerate()
        .filter(|(i, piece)| i % 2 == 1 && words.contains(piece))
        .map(|(_, piece)| piece)
        .collect()
}

fn scan(path: &Path, words: &BTreeSet<&str>, found: &mut Vec<String>) {
    let name = path.file_name().unwrap().to_string_lossy();
    if DEFINING.contains(&name.as_ref()) || name == "tests.rs" {
        return;
    }
    let text = std::fs::read_to_string(path).unwrap();
    for (i, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("#[cfg(test)]") {
            break;
        }
        if trimmed.starts_with("//") {
            continue;
        }
        for word in quoted_status_words(line, words) {
            found.push(format!("{}:{}: '{word}'", path.display(), i + 1));
        }
    }
}

#[test]
fn no_status_literal_in_ledger_sql() {
    let words = status_words();
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    rust_files(&manifest.join("src/taskledger"), &mut files);
    let cmd = manifest.join("../horch/src/cmd");
    for entry in std::fs::read_dir(&cmd).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if name.starts_with("taskledger") || name == "compact.rs" {
            files.push(path);
        }
    }
    assert!(files.len() > 20, "scanned only {} files", files.len());

    let mut found = Vec::new();
    for path in &files {
        scan(path, &words, &mut found);
    }
    assert!(
        found.is_empty(),
        "status words written as literals; build them from the enums in taskledger/model.rs:\n{}",
        found.join("\n")
    );
}

#[test]
fn scan_sees_a_quoted_status_word() {
    let words = status_words();
    assert_eq!(
        quoted_status_words(
            "WHERE status IN ('open', 'landing') AND kind = 'work'",
            &words
        ),
        ["open", "landing"]
    );
    assert!(quoted_status_words("WHERE origin = 'native'", &words).is_empty());
}
