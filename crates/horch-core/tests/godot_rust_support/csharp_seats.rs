use std::path::Path;

use horch_core::roster::{ProjectFacts, Roster};

const CSHARP_SKILLS: [&str; 2] = ["godot-csharp-godot", "godot-csharp-signals"];

#[test]
fn grs_11_csharp_project_adds_csharp_skills_to_godot_reviewer_and_qa_seats() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../teammates");
    let roster = Roster::load_layered(None, None, Some(dir.to_str().unwrap())).unwrap();
    let gdscript = ProjectFacts::from_names(["project.godot", "player.gd"]);
    let csharp = ProjectFacts::from_names(["project.godot", "Game.csproj", "Player.cs"]);
    for name in ["godot-code-reviewer", "godot-qa-engineer"] {
        let t = roster.get(name).unwrap();
        let globs: Vec<&String> = t.skills_when.keys().collect();
        assert_eq!(globs, ["*.csproj"], "{name}");
        assert_eq!(t.skills_when["*.csproj"], CSHARP_SKILLS, "{name}");

        let gd = roster
            .clone()
            .with_project_facts(gdscript.clone())
            .for_launch(t.clone());
        assert!(
            (5..=11).contains(&gd.skills.len()),
            "{name} gdscript {}",
            gd.skills.len()
        );

        let cs = roster
            .clone()
            .with_project_facts(csharp.clone())
            .for_launch(t.clone());
        assert!(
            (5..=11).contains(&cs.skills.len()),
            "{name} csharp {}",
            cs.skills.len()
        );
        for s in CSHARP_SKILLS {
            assert!(cs.skills.iter().any(|x| x == s), "{name} lacks {s}");
            assert!(
                !cs.available_skills.iter().any(|x| x == s),
                "{name} offers {s}"
            );
        }
    }
}
