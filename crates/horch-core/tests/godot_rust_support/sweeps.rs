use std::path::PathBuf;

use horch_core::taskledger::finish::{codesweep, comments};

fn files(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| n.to_string()).collect()
}

fn repo_file(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

#[test]
fn grs_12_codesweep_triages_godot_and_csharp_signals_and_selects_domain_reviewer() {
    for word in [
        "class_name Player",
        "signal died",
        "@export var speed",
        "public class Player",
        "public struct Hit",
        "public interface IDamage",
        "#[godot_api]",
    ] {
        let diff = format!("+++ b/x\n+{word}\n");
        let lenses = codesweep::triage(&diff, &files(&["README.md"]));
        assert!(
            lenses.contains(&"g"),
            "{word} must pick the g lens: {lenses:?}"
        );
    }
    for manifest in [
        "game/Game.csproj",
        "Directory.Packages.props",
        "project.godot",
        "rust/game.gdextension",
        "addons/tool/plugin.cfg",
    ] {
        let lenses = codesweep::triage("", &files(&[manifest]));
        assert!(
            lenses.contains(&"d"),
            "{manifest} must pick the d lens: {lenses:?}"
        );
    }
    for ui in ["scenes/main.tscn", "themes/ui.tres"] {
        let lenses = codesweep::triage("", &files(&[ui]));
        assert!(
            lenses.contains(&"u"),
            "{ui} must pick the u lens: {lenses:?}"
        );
    }

    assert_eq!(
        codesweep::domain_reviewer(&files(&["src/lib.rs", "player.gd"])),
        Some("godot-code-reviewer")
    );
    assert_eq!(
        codesweep::domain_reviewer(&files(&["project.godot"])),
        Some("godot-code-reviewer")
    );
    assert_eq!(
        codesweep::domain_reviewer(&files(&["src/lib.rs"])),
        Some("rust-reviewer")
    );
    assert_eq!(
        codesweep::domain_reviewer(&files(&["Cargo.toml"])),
        Some("rust-reviewer")
    );
    assert_eq!(
        codesweep::domain_reviewer(&files(&["App/Main.swift"])),
        Some("swift-reviewer")
    );
    assert_eq!(codesweep::domain_reviewer(&files(&["web/app.ts"])), None);

    let gd = files(&["scripts/player.gd", "scripts/enemy.gd"]);
    let domain = codesweep::domain_reviewer(&gd);
    let slots =
        codesweep::slot_reviewers(domain, 1, "codex-reviewer", "architect-reviewer", &[], 3);
    assert_eq!(
        slots,
        vec![
            ("godot-code-reviewer", "codex-reviewer"),
            ("codex-reviewer", "architect-reviewer"),
            ("codex-reviewer", "architect-reviewer"),
        ]
    );
    let cross_vendor = ["codex-reviewer", "architect-reviewer"];
    for (persona, _) in &slots[1..] {
        assert!(
            cross_vendor.contains(persona),
            "{persona} is not cross-vendor"
        );
    }

    let down = vec![("godot-code-reviewer".to_string(), "no seat".to_string())];
    assert_eq!(
        codesweep::slot_reviewers(domain, 1, "codex-reviewer", "architect-reviewer", &down, 2),
        vec![
            ("codex-reviewer", "architect-reviewer"),
            ("codex-reviewer", "architect-reviewer"),
        ]
    );
    assert_eq!(
        codesweep::slot_reviewers(
            domain,
            2,
            "codex-reviewer-light",
            "architect-reviewer-light",
            &[],
            2
        ),
        vec![
            ("codex-reviewer-light", "architect-reviewer-light"),
            ("codex-reviewer-light", "architect-reviewer-light"),
        ]
    );
}

#[test]
fn grs_13_comment_sweep_skips_godot_data_files_and_keeps_safety_and_doc_comments() {
    for data in [
        "scenes/main.tscn",
        "themes/ui.tres",
        "art/hero.png.import",
        "scripts/player.gd.uid",
        "rust/game.gdextension",
        "project.godot",
    ] {
        assert!(!comments::is_code(data), "{data} must not be swept");
    }
    for code in ["scripts/player.gd", "src/lib.rs", "Game/Player.cs"] {
        assert!(comments::is_code(code), "{code} must be swept");
    }

    let pass = repo_file("skills/comment-pruning/references/pruning-pass.md");
    let keep = pass
        .split("## Keep list")
        .nth(1)
        .and_then(|rest| rest.split("\n## ").next())
        .expect("pruning-pass.md has a Keep list section");
    for marker in ["// SAFETY:", "///", "//!", "##"] {
        assert!(keep.contains(marker), "the Keep list must name {marker}");
    }

    let lint = repo_file("scripts/comments/comment_lint.py");
    assert!(
        lint.contains("\".cs\": \"csharp\""),
        "comment_lint.py must scan .cs files"
    );
}
