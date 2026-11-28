use std::path::Path;

use horch_core::roster::{ProjectFacts, Roster};

const RUST_SKILLS: [&str; 3] = ["rust-cargo-workspace", "rust-unsafe-review", "rust-testing"];
const CSHARP_SKILLS: [&str; 2] = ["godot-csharp-godot", "godot-csharp-signals"];

fn repo() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

#[test]
fn grs_10_godot_native_engineer_adds_rust_skills_on_cargo_toml_and_gdext_ref_is_complete() {
    let dir = repo().join("teammates");
    let roster = Roster::load_layered(None, None, Some(dir.to_str().unwrap())).unwrap();
    let t = roster.get("godot-native-engineer").unwrap();
    assert_eq!(
        t.brief_description,
        "Godot 4 native code: GDExtension in C++ (godot-cpp) and Rust (gdext), bindings, SCons/Cargo builds, threads."
    );
    assert_eq!(t.skills_when["Cargo.toml"], RUST_SKILLS);
    assert_eq!(t.skills_when["*.csproj"], CSHARP_SKILLS);
    for s in RUST_SKILLS {
        assert!(t.available_skills.iter().any(|x| x == s), "offers {s}");
        assert!(!t.skills.iter().any(|x| x == s), "expects {s} everywhere");
    }

    let launch = |names: &[&str]| {
        roster
            .clone()
            .with_project_facts(ProjectFacts::from_names(names.iter().copied()))
            .for_launch(t.clone())
    };
    let gd = launch(&["project.godot", "player.gd"]);
    assert!((5..=11).contains(&gd.skills.len()), "gdscript {}", gd.skills.len());
    for s in RUST_SKILLS {
        assert!(!gd.skills.iter().any(|x| x == s), "gdscript expects {s}");
    }
    for names in [
        &["project.godot", "Cargo.toml"][..],
        &["project.godot", "Cargo.toml", "Game.csproj"][..],
    ] {
        let rs = launch(names);
        assert!((5..=11).contains(&rs.skills.len()), "{names:?} {}", rs.skills.len());
        for s in RUST_SKILLS {
            assert!(rs.skills.iter().any(|x| x == s), "{names:?} lacks {s}");
            assert!(!rs.available_skills.iter().any(|x| x == s), "{names:?} offers {s}");
        }
    }

    let body = std::fs::read_to_string(dir.join("godot-native-engineer.md")).unwrap();
    for needle in ["godot-cpp", "gdext", "cargo clippy", "cargo test", "godot-check.sh"] {
        assert!(body.contains(needle), "persona lacks {needle}");
    }

    let reference = std::fs::read_to_string(
        repo().join("skills/godot-gdextension/references/rust-gdext.md"),
    )
    .unwrap();
    for needle in [
        "[libraries]",
        "macos.debug",
        "linux.debug.x86_64",
        "windows.debug.x86_64",
        "target/debug/",
        "reloadable = true",
        "cargo build --target",
        "cargo clippy --target",
        "godot-check.sh",
        "--headless",
    ] {
        assert!(reference.contains(needle), "rust-gdext.md lacks {needle}");
    }
}
