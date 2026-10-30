//! `docs/architecture.md` is the "what lives where" map. These tests fail
//! when a module or a command file has no row in it (#213).

use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn architecture_doc() -> String {
    let path = repo().join("docs/architecture.md");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// The first cell of every table row in `doc`, without its backticks.
fn first_cells(doc: &str) -> Vec<String> {
    doc.lines()
        .filter_map(|line| line.strip_prefix("| `"))
        .filter_map(|rest| rest.split_once('`'))
        .map(|(cell, _)| cell.to_string())
        .collect()
}

/// Every `pub mod <name>;` line of `crates/horch-core/src/lib.rs`.
fn core_modules() -> Vec<String> {
    let lib = std::fs::read_to_string(repo().join("crates/horch-core/src/lib.rs")).unwrap();
    lib.lines()
        .filter_map(|l| l.trim().strip_prefix("pub mod "))
        .filter_map(|l| l.strip_suffix(';'))
        .map(str::to_string)
        .collect()
}

#[test]
fn every_core_module_has_a_row_in_the_architecture_doc() {
    let cells = first_cells(&architecture_doc());
    let modules = core_modules();
    assert!(modules.len() > 20, "lib.rs lists {} modules", modules.len());
    let missing: Vec<_> = modules.iter().filter(|m| !cells.contains(m)).collect();
    assert!(
        missing.is_empty(),
        "docs/architecture.md has no module row for {missing:?}"
    );
}

#[test]
fn every_cli_file_has_a_row_in_the_architecture_doc() {
    let cells = first_cells(&architecture_doc());
    let dir = repo().join("crates/horch/src/cmd");
    let mut missing = Vec::new();
    let mut seen = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        seen += 1;
        let listed = if entry.file_type().unwrap().is_dir() {
            // `cost/` holds the child modules of `cost.rs`; either row names it.
            cells.contains(&format!("{name}/")) || cells.contains(&format!("{name}.rs"))
        } else {
            cells.contains(&name)
        };
        if !listed {
            missing.push(name);
        }
    }
    assert!(seen > 20, "crates/horch/src/cmd has {seen} entries");
    missing.sort();
    assert!(
        missing.is_empty(),
        "docs/architecture.md has no CLI row for {missing:?}"
    );
}
