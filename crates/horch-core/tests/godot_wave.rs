//! The Godot wave (GDW-01 to GDW-09, `docs/specs/godot.md`): the Godot
//! requirement, the 19 `godot-*` seats, the 64 `godot-*` skills and the
//! gate's Godot checks.
//!
//! The seat tests read the teammates through the public roster API and
//! never depend on the context fields (`compact_at`, `compact_window`). The
//! gate tests run the Python checks in `scripts/godot/` with `python3`,
//! against the test fixtures, so no test needs Godot or dotnet.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::harness::HarnessKind;
use horch_core::roster::{offered_in, project_skills, ProjectFacts, Requirement, Roster, Teammate};
use horch_core::runtime::bins::{host_tool_bin, GODOT_APP};
use horch_core::skills::{SkillCatalog, BUNDLED_SKILL_FILES};

/// The C# skills that `skills_when` adds in a project with a `*.csproj`.
const CSHARP_SKILLS: [&str; 2] = ["godot-csharp-godot", "godot-csharp-signals"];

/// The seats that plan or review: they get no `skills_when`.
const NO_CSHARP_SEATS: [&str; 3] = [
    "godot-tech-lead",
    "godot-code-reviewer",
    "godot-qa-engineer",
];

/// The seat that writes C# in every project: its `skills:` names both.
const CSHARP_SEAT: &str = "godot-csharp-engineer";

/// Upstream skills the wave excludes on purpose.
const EXCLUDED: [&str; 6] = [
    "using-godot-prompter",
    "godot-mentor",
    "godot-master",
    "godot-agent-vision",
    "godot-monte-carlo-balancer",
    "godot-theme-easter",
];

/// The bundled size budget (the same numbers as `tests/skills_catalog.rs`).
const MAX_SKILL_MD_BYTES: usize = 12 * 1024;
const MAX_SKILL_DIR_BYTES: usize = 160 * 1024;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn roster() -> Roster {
    let dir = repo().join("teammates");
    Roster::load_layered(None, None, Some(dir.to_str().unwrap())).unwrap()
}

/// Every `godot-*` seat of the repository roster, by name.
fn seats(roster: &Roster) -> Vec<&Teammate> {
    roster
        .names()
        .into_iter()
        .filter(|n| n.starts_with("godot-"))
        .map(|n| roster.get(n).unwrap())
        .collect()
}

/// Every bundled `godot-*` skill id.
fn godot_skills() -> BTreeSet<String> {
    SkillCatalog::bundled()
        .unwrap()
        .entries()
        .map(|e| e.id.to_string())
        .filter(|id| id.starts_with("godot-"))
        .collect()
}

/// Runs `python3 -c <code>` in the repository with `scripts/godot` on
/// `sys.path`. Returns the exit code and stdout.
fn python(code: &str) -> (i32, String) {
    let out = Command::new("python3")
        .current_dir(repo())
        .arg("-c")
        .arg(format!(
            "import sys; sys.path.insert(0, 'scripts/godot')\n{code}"
        ))
        .output()
        .expect("python3 runs (the gate needs it too)");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("Traceback"), "{stderr}");
    (
        out.status.code().unwrap(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

fn executable(path: &Path) {
    std::fs::write(path, "#!/bin/sh\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[test]
fn gdw_01_godot_requirement_is_spelled_godot_and_every_seat_requires_it() {
    assert_eq!(Requirement::Godot.as_str(), "godot");
    let parsed: Requirement = serde_json::from_str("\"godot\"").unwrap();
    assert_eq!(parsed, Requirement::Godot);

    let roster = roster();
    let seats = seats(&roster);
    assert!(!seats.is_empty());
    for t in seats {
        assert!(t.requires.contains(&Requirement::Godot), "{}", t.name);
    }
}

#[cfg(unix)]
#[test]
fn gdw_01_godot_lookup_is_the_variable_then_path_then_the_app() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    executable(&bin.join("godot"));
    let app = tmp.path().join("Godot");
    executable(&app);
    let empty = tmp.path().join("empty");
    std::fs::create_dir(&empty).unwrap();

    let var = OsStr::new("/opt/godot/Godot");
    let lookup = |var: Option<&OsStr>, path: &Path, app: Option<&Path>| {
        host_tool_bin(var, Some(path.as_os_str()), None, "godot", app)
    };
    // GODOT_PATH wins, unchecked: doctor reports a broken one.
    assert_eq!(
        lookup(Some(var), &bin, Some(&app)),
        Some(PathBuf::from(var))
    );
    // An empty GODOT_PATH counts as unset.
    assert_eq!(
        lookup(Some(OsStr::new("")), &bin, Some(&app)),
        Some(bin.join("godot"))
    );
    assert_eq!(lookup(None, &bin, Some(&app)), Some(bin.join("godot")));
    assert_eq!(lookup(None, &empty, Some(&app)), Some(app.clone()));
    assert_eq!(
        lookup(None, &empty, Some(&tmp.path().join("missing"))),
        None
    );
    assert_eq!(lookup(None, &empty, None), None);

    if cfg!(target_os = "macos") {
        assert_eq!(
            GODOT_APP,
            Some("/Applications/Godot.app/Contents/MacOS/Godot")
        );
    } else {
        assert_eq!(GODOT_APP, None);
    }
}

#[test]
fn gdw_02_godot_seats_are_offered_only_in_a_godot_project() {
    let roster = roster();
    let seats = seats(&roster);
    let godot = ProjectFacts::from_names(["project.godot", "scenes", "README.md"]);
    let other = ProjectFacts::from_names(["Cargo.toml", "src", "project.godot.bak"]);
    for t in &seats {
        assert_eq!(t.offer_when, ["project.godot"], "{}", t.name);
        assert!(offered_in(t, Some(&godot)), "{}", t.name);
        assert!(!offered_in(t, Some(&other)), "{}", t.name);
        // No facts gathered: `horch teammates` lists every seat.
        assert!(offered_in(t, None), "{}", t.name);
    }

    let offered = |facts: ProjectFacts| -> BTreeSet<String> {
        roster
            .clone()
            .with_project_facts(facts)
            .offered()
            .into_iter()
            .map(|t| t.name.clone())
            .filter(|n| n.starts_with("godot-"))
            .collect()
    };
    assert_eq!(offered(godot).len(), seats.len());
    assert!(offered(other).is_empty());
}

#[test]
fn gdw_03_a_csharp_project_adds_the_csharp_skills_to_the_builder_seats() {
    let roster = roster();
    let gdscript = ProjectFacts::from_names(["project.godot", "player.gd"]);
    let csharp = ProjectFacts::from_names(["project.godot", "Game.csproj", "Player.cs"]);
    let csharp_set: BTreeSet<String> = CSHARP_SKILLS.iter().map(|s| s.to_string()).collect();
    let mut builders = 0;
    for t in seats(&roster) {
        assert!(project_skills(t, &gdscript).is_empty(), "{}", t.name);
        if NO_CSHARP_SEATS.contains(&t.name.as_str()) {
            assert!(t.skills_when.is_empty(), "{}", t.name);
            continue;
        }
        if t.name == CSHARP_SEAT {
            assert!(t.skills_when.is_empty(), "{}", t.name);
            for s in CSHARP_SKILLS {
                assert!(t.skills.iter().any(|x| x == s), "{} lacks {s}", t.name);
            }
            continue;
        }
        builders += 1;
        assert_eq!(
            t.skills_when
                .get("*.csproj")
                .map(|v| v.iter().cloned().collect()),
            Some(csharp_set.clone()),
            "{}",
            t.name
        );
        let added: BTreeSet<String> = project_skills(t, &csharp).into_iter().collect();
        assert_eq!(added, csharp_set, "{}", t.name);

        // At launch the added skills are expected skills, not available.
        let launched = roster
            .clone()
            .with_project_facts(csharp.clone())
            .for_launch(t.clone());
        for s in CSHARP_SKILLS {
            assert!(launched.skills.iter().any(|x| x == s), "{} {s}", t.name);
            assert!(
                !launched.available_skills.iter().any(|x| x == s),
                "{} {s}",
                t.name
            );
        }
    }
    assert_eq!(builders, 15);
}

#[test]
fn gdw_04_a_godot_skill_over_the_size_budget_gives_a_reason() {
    let catalog = SkillCatalog::bundled().unwrap();
    let mut dir_bytes = std::collections::BTreeMap::<&str, usize>::new();
    for (path, bytes) in BUNDLED_SKILL_FILES {
        if let Some((id, _)) = path.split_once('/') {
            *dir_bytes.entry(id).or_default() += bytes.len();
        }
    }
    let mut exempt = 0;
    for e in catalog.entries() {
        let id = e.id.to_string();
        if !id.starts_with("godot-") {
            continue;
        }
        let copied = e
            .copied
            .as_ref()
            .unwrap_or_else(|| panic!("{id}: no copied.json entry"));
        let over =
            e.skill_file_bytes > MAX_SKILL_MD_BYTES || dir_bytes[id.as_str()] > MAX_SKILL_DIR_BYTES;
        let reason = copied.budget_exempt.as_deref().map(str::trim);
        assert!(!copied.verbatim, "{id}: a renamed skill is not verbatim");
        if over {
            assert!(
                reason.is_some_and(|r| !r.is_empty()),
                "{id}: over budget, no reason"
            );
        } else {
            assert_eq!(reason, None, "{id}: under budget, stale reason");
        }
        if reason.is_some() {
            exempt += 1;
        }
    }
    assert!(exempt > 0);
}

#[test]
fn gdw_05_api_check_flags_unknown_names_against_the_dump() {
    let fixture = "scripts/godot/tests/fixtures/api_check";
    let (code, out) = python(&format!(
        "import api_check\nsys.exit(api_check.main(['--doctool', '{fixture}/doctool', \
         '{fixture}/content/good.md']))"
    ));
    assert_eq!((code, out.as_str()), (0, ""));
    let (code, out) = python(&format!(
        "import api_check\nsys.exit(api_check.main(['--doctool', '{fixture}/doctool', \
         '{fixture}/content/bad.md']))"
    ));
    assert_eq!(code, 1);
    assert!(out.contains("bad.md:4: unknown OS.gc"), "{out}");
    assert!(out.contains("bad.md:5: unknown Node.no_method"), "{out}");
}

#[test]
fn gdw_05_api_check_skips_without_godot() {
    let (code, out) = python(
        "import api_check\napi_check.find_godot = lambda: None\n\
         sys.exit(api_check.main(['skills/godot-ui']))",
    );
    assert_eq!((code, out.as_str()), (0, "skipped: no Godot\n"));
}

#[test]
fn gdw_05_the_gate_checks_names_against_the_pinned_4_7_2_data() {
    let dir = repo().join("scripts/godot");
    let deprecated: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("deprecated-4.7.2.json")).unwrap())
            .unwrap();
    assert!(deprecated.as_object().is_some_and(|m| !m.is_empty()));
    assert!(dir.join("known_classes.txt").is_file());

    // The auto mode picks the data file of the Godot version it finds.
    let (code, out) =
        python("import api_check\nprint(api_check.pinned_deprecations('4.7.2').name)");
    assert_eq!((code, out.trim()), (0, "deprecated-4.7.2.json"));

    let gate = std::fs::read_to_string(repo().join("scripts/phase-gate.sh")).unwrap();
    assert!(gate.contains("api_check.py --strict-own skills/copied.json"));
    assert!(gate.contains("step godot_skills\necho \"GATE GREEN\""));
}

#[test]
fn gdw_06_xref_check_passes_on_the_bundled_godot_skills() {
    let (code, _) = python("import xref_check\nsys.exit(xref_check.main([]))");
    assert_eq!(code, 0);

    let gate = std::fs::read_to_string(repo().join("scripts/phase-gate.sh")).unwrap();
    for check in [
        "python3 -m unittest discover -s scripts/godot/tests",
        "python3 scripts/godot/xref_check.py",
        "gdscript_blocks_check.py --strict-own skills/copied.json",
        "csharp_blocks_check.py --strict-own skills/copied.json",
    ] {
        assert!(gate.contains(check), "the gate lacks {check}");
    }
}

#[test]
fn gdw_06_block_checks_skip_without_godot_or_dotnet() {
    let (code, out) = python(
        "import gdscript_blocks_check as g\ng.find_godot = lambda: None\n\
         sys.exit(g.main(['skills/godot-ui']))",
    );
    assert_eq!((code, out.as_str()), (0, "skipped: no Godot\n"));
    let (code, out) = python(
        "import shutil, csharp_blocks_check as c\nshutil.which = lambda name: None\n\
         sys.exit(c.main(['skills/godot-ui']))",
    );
    assert_eq!((code, out.as_str()), (0, "skipped: no dotnet\n"));
}

#[test]
fn gdw_06_copied_failures_fail_only_above_the_baseline() {
    let baseline: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(repo().join("scripts/godot/copied-baseline.json")).unwrap(),
    )
    .unwrap();
    for check in ["api_check", "csharp_blocks_check", "gdscript_blocks_check"] {
        assert!(
            baseline.get(check).is_some_and(|v| v.is_object()),
            "{check}"
        );
    }

    let tmp = tempfile::tempdir().unwrap();
    let base = tmp.path().join("baseline.json");
    std::fs::write(&base, r#"{"gdscript_blocks_check": {"godot-ui": 2}}"#).unwrap();
    let run = |n: usize| {
        python(&format!(
            "import io, copied_ratchet as r\nfrom pathlib import Path\n\
             f = Path('skills/godot-ui/SKILL.md').resolve()\n\
             code = r.apply('gdscript_blocks_check', 'blocks', [(f, 'x')] * {n}, [f], \
             {{f: 'godot-ui'}}, Path('{}'), out=io.StringIO())\nsys.exit(code)",
            base.display()
        ))
        .0
    };
    assert_eq!(run(1), 0, "below the baseline passes");
    assert_eq!(run(2), 0, "at the baseline passes");
    assert_eq!(run(3), 1, "above the baseline fails");
}

#[test]
fn gdw_07_the_catalog_has_64_godot_skills_and_no_excluded_one() {
    let skills = godot_skills();
    assert_eq!(skills.len(), 64, "{skills:?}");
    for name in EXCLUDED {
        assert!(!skills.contains(name), "{name} is excluded");
    }
}

#[test]
fn gdw_07_godot_skills_carry_no_licence_or_credit_line() {
    let word = |text: &str, w: &str| {
        text.match_indices(w).any(|(i, _)| {
            let after = text[i + w.len()..].chars().next();
            !after.is_some_and(|c| c.is_ascii_alphanumeric())
        })
    };
    let mut files = 0;
    for (path, bytes) in BUNDLED_SKILL_FILES {
        if !path.starts_with("godot-") {
            continue;
        }
        files += 1;
        let name = path.rsplit('/').next().unwrap().to_ascii_uppercase();
        assert!(
            !["LICENSE", "LICENCE", "NOTICE", "COPYING"]
                .iter()
                .any(|p| name.starts_with(p)),
            "{path}"
        );
        let text = String::from_utf8_lossy(bytes).to_lowercase();
        for credit in [
            "godotprompter",
            "gd-agentic",
            "thedivergentai",
            "jame581",
            "lgpl",
        ] {
            assert!(!text.contains(credit), "{path} names {credit}");
        }
        for w in ["license", "licence"] {
            assert!(!word(&text, w), "{path} has the word {w}");
        }
    }
    assert!(files > 64);
}

#[test]
fn gdw_08_every_godot_seat_expects_5_to_10_skills_from_the_catalog() {
    let roster = roster();
    let seats = seats(&roster);
    assert_eq!(seats.len(), 19);
    let catalog = SkillCatalog::bundled().unwrap();
    for t in seats {
        let n = t.skills.len();
        assert!((5..=10).contains(&n), "{} expects {n} skills", t.name);
        for s in t.skills.iter().chain(&t.available_skills) {
            assert!(
                catalog.lookup(s).is_some(),
                "{}: {s} is not a bundled skill",
                t.name
            );
        }
    }
}

#[test]
fn gdw_08_every_godot_skill_has_a_seat() {
    let roster = roster();
    let owned: BTreeSet<&String> = seats(&roster)
        .into_iter()
        .flat_map(|t| t.skills.iter().chain(&t.available_skills))
        .collect();
    let orphans: Vec<String> = godot_skills()
        .into_iter()
        .filter(|s| !owned.contains(s))
        .collect();
    assert!(orphans.is_empty(), "no seat expects or offers {orphans:?}");
}

#[test]
fn gdw_09_godot_seats_are_claude_fleet_workers_without_mcp_servers() {
    let roster = roster();
    for t in seats(&roster) {
        assert_eq!(t.agent, HarnessKind::Claude, "{}", t.name);
        assert_eq!(t.base.as_deref(), Some("fleet-worker"), "{}", t.name);
        assert!(
            t.mcp_servers.as_ref().is_some_and(|m| m.is_empty()),
            "{}",
            t.name
        );
    }
    let reviewer = roster.require("godot-code-reviewer").unwrap();
    for tool in ["Edit", "Write", "NotebookEdit"] {
        assert!(
            reviewer.disallowed_tools.iter().any(|x| x == tool),
            "godot-code-reviewer may use {tool}"
        );
    }
}
