//! FDS-14: `herdr-run` is the old `herdr-fleet` launcher, and `just install`
//! installs it. The briefing tests (FDS-13) live in `cmd/recipes.rs`.

use std::path::PathBuf;

fn repo_file(rel: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::read_to_string(root.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

#[test]
fn fds_14_herdr_run_matches_old_launcher() {
    let run = repo_file("scripts/herdr-run");
    assert!(
        run.lines()
            .any(|l| l == r#"exec horch fleet "$@" --cwd "$PWD""#),
        "{run}"
    );
    assert!(!run.contains("--compete"), "{run}");
    assert!(run.contains("HORCH_TEAMMATES_DIR=\"__TEAMMATES_DIR__\""));
    assert!(run.lines().any(|l| l == "export HORCH_TEAMMATES_DIR"));

    let justfile = repo_file("justfile");
    assert!(justfile.contains("scripts/herdr-run > ~/.local/bin/herdr-run"));
    assert!(justfile.contains("chmod 755 ~/.local/bin/herdr-run"));
}
