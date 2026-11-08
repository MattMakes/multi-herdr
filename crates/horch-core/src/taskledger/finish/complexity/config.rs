//! The analyser config of each changed file: the nearest `static-analysis`
//! config from the file's folder up to the work dir (T-35). The analyser
//! itself reads a config only from its current directory, so the step runs
//! it once per config with `--config`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The file names cosmiconfig 9 (the analyser's config loader) searches in
/// 1 folder, in its order, for the module name `static-analysis`.
/// `package.json` counts only when it has a `static-analysis` field.
const SEARCH_PLACES: &[&str] = &[
    "package.json",
    ".static-analysisrc",
    ".static-analysisrc.json",
    ".static-analysisrc.yaml",
    ".static-analysisrc.yml",
    ".static-analysisrc.js",
    ".static-analysisrc.ts",
    ".static-analysisrc.cjs",
    ".static-analysisrc.mjs",
    ".config/static-analysisrc",
    ".config/static-analysisrc.json",
    ".config/static-analysisrc.yaml",
    ".config/static-analysisrc.yml",
    ".config/static-analysisrc.js",
    ".config/static-analysisrc.ts",
    ".config/static-analysisrc.cjs",
    ".config/static-analysisrc.mjs",
    "static-analysis.config.js",
    "static-analysis.config.ts",
    "static-analysis.config.cjs",
    "static-analysis.config.mjs",
];

/// The config cosmiconfig takes from `folder`, when it holds one.
fn config_in(folder: &Path) -> Option<PathBuf> {
    SEARCH_PLACES
        .iter()
        .map(|name| folder.join(name))
        .find(|p| p.is_file() && (!p.ends_with("package.json") || has_field(p)))
}

/// Whether the `package.json` at `path` has a `static-analysis` field.
fn has_field(path: &Path) -> bool {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .is_some_and(|v| v.get("static-analysis").is_some())
}

/// The nearest config of `file` (relative to `dir`): the first folder from
/// the file's folder up to `dir` that holds one. `None` when no folder
/// holds one.
pub(super) fn nearest(dir: &Path, file: &str) -> Option<PathBuf> {
    let mut folder = dir.join(file).parent()?.to_path_buf();
    loop {
        if let Some(found) = config_in(&folder) {
            return Some(found);
        }
        if folder == dir || !folder.pop() || !folder.starts_with(dir) {
            return None;
        }
    }
}

/// `files` grouped by their nearest config, in a stable order: the group
/// without a config first, then by config path. Each group keeps the order
/// of `files`.
pub(super) fn groups(dir: &Path, files: &[String]) -> Vec<(Option<PathBuf>, Vec<String>)> {
    let mut by: BTreeMap<Option<PathBuf>, Vec<String>> = BTreeMap::new();
    for f in files {
        by.entry(nearest(dir, f)).or_default().push(f.clone());
    }
    by.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn t_35_the_nearest_config_wins_and_the_search_stops_at_the_work_dir() {
        let top = tempfile::tempdir().unwrap();
        let dir = top.path().join("repo");
        // A config above the work dir is not the work dir's config.
        write(&top.path().join(".static-analysisrc.json"), "{}");
        write(&dir.join("a/.static-analysisrc.json"), "{}");
        write(&dir.join("a/b/.config/static-analysisrc.yml"), "x: 1");
        write(&dir.join("c/package.json"), r#"{"name":"c"}"#);
        write(&dir.join("d/package.json"), r#"{"static-analysis":{}}"#);

        assert_eq!(nearest(&dir, "top.py"), None);
        assert_eq!(
            nearest(&dir, "a/x.py"),
            Some(dir.join("a/.static-analysisrc.json"))
        );
        assert_eq!(
            nearest(&dir, "a/b/c/x.py"),
            Some(dir.join("a/b/.config/static-analysisrc.yml"))
        );
        assert_eq!(nearest(&dir, "c/x.ts"), None, "no static-analysis field");
        assert_eq!(nearest(&dir, "d/e/x.ts"), Some(dir.join("d/package.json")));
    }

    #[test]
    fn t_35_files_group_by_their_nearest_config() {
        let top = tempfile::tempdir().unwrap();
        let dir = top.path();
        write(&dir.join("engine/.static-analysisrc.json"), "{}");
        let files: Vec<String> = ["lib.ts", "engine/a.py", "x/b.py", "engine/z/c.py"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(
            groups(dir, &files),
            vec![
                (None, vec!["lib.ts".to_string(), "x/b.py".to_string()]),
                (
                    Some(dir.join("engine/.static-analysisrc.json")),
                    vec!["engine/a.py".to_string(), "engine/z/c.py".to_string()]
                ),
            ]
        );
    }
}
