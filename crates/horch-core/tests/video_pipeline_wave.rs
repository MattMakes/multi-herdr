//! The video pipeline wave: the 6 `video-*` teammates, the 9 `video-*`
//! skills with their script entrypoints, the orchestrator recipe and the
//! checker self-tests.
//!
//! The roster tests read the teammates through the public roster API. The
//! self-test runs the bundled checkers with `python3`, and they skip when
//! `python3` or `ffmpeg` is not on `PATH`. The end-to-end check is
//! `scripts/live/video.sh`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::roster::{Phase, Roster, Teammate};
use horch_core::skills::{SkillCatalog, BUNDLED_SKILL_EXECUTABLES, BUNDLED_SKILL_FILES};

/// One seat: name, phase, model, effort, skills.
type Seat = (
    &'static str,
    Phase,
    &'static str,
    &'static str,
    &'static [&'static str],
);

const SEATS: [Seat; 6] = [
    (
        "video-director",
        Phase::Plan,
        "opus",
        "high",
        &["video-direction", "video-storyboard"],
    ),
    (
        "video-capture",
        Phase::Implementation,
        "sonnet",
        "medium",
        &["video-capture"],
    ),
    (
        "video-audio",
        Phase::Implementation,
        "sonnet",
        "medium",
        &["video-audio", "video-storyboard"],
    ),
    (
        "video-animator",
        Phase::Implementation,
        "opus",
        "medium",
        &[
            "video-motion",
            "video-character",
            "video-reanimate",
            "motion-gsap",
        ],
    ),
    (
        "video-editor",
        Phase::Implementation,
        "sonnet",
        "medium",
        &["video-render"],
    ),
    (
        "video-critic",
        Phase::Validation,
        "opus",
        "high",
        &["video-qa", "video-direction"],
    ),
];

const VIDEO_SKILLS: [&str; 9] = [
    "video-direction",
    "video-storyboard",
    "video-capture",
    "video-audio",
    "video-motion",
    "video-character",
    "video-reanimate",
    "video-render",
    "video-qa",
];

/// The script entrypoints the pipeline calls, each with an execute bit.
const ENTRYPOINTS: [&str; 5] = [
    "video-storyboard/scripts/check_timeline.py",
    "video-audio/scripts/check_audio.py",
    "video-reanimate/scripts/compare_frames.py",
    "video-render/scripts/render_frames.mjs",
    "video-qa/scripts/qa_video.py",
];

/// The checkers whose `--self-test` needs only python3 and ffmpeg.
const PYTHON_SELF_TESTS: [&str; 4] = [
    "skills/video-storyboard/scripts/check_timeline.py",
    "skills/video-audio/scripts/check_audio.py",
    "skills/video-reanimate/scripts/compare_frames.py",
    "skills/video-qa/scripts/qa_video.py",
];

/// The hand-off files `video-direction` writes. A lowercase name works on
/// macOS and breaks on Linux.
const HANDOFF_NAMES: [&str; 3] = ["brief", "direction", "storyboard"];

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn roster() -> Roster {
    let dir = repo().join("teammates");
    Roster::load_layered(None, None, Some(dir.to_str().unwrap())).unwrap()
}

fn seat(roster: &Roster, name: &str) -> Teammate {
    roster
        .get(name)
        .unwrap_or_else(|| panic!("{name} is not in the roster"))
        .clone()
}

fn on_path(tool: &str) -> bool {
    Command::new(tool)
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Every `skills/video-*` and `teammates/video-*` file that is text, by
/// repository-relative path and content.
fn video_text_files() -> Vec<(String, String)> {
    let mut files = Vec::new();
    for (path, bytes) in BUNDLED_SKILL_FILES {
        if path.starts_with("video-") {
            files.push((
                format!("skills/{path}"),
                String::from_utf8_lossy(bytes).into(),
            ));
        }
    }
    let teammates = repo().join("teammates");
    for entry in std::fs::read_dir(&teammates).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if name.starts_with("video-") && name.ends_with(".md") {
            files.push((
                format!("teammates/{name}"),
                std::fs::read_to_string(&path).unwrap(),
            ));
        }
    }
    files
}

/// True when `text` holds `<stem>.md` as a whole file name: not part of a
/// longer name, and not in uppercase.
fn names_file_lowercase(text: &str, stem: &str) -> bool {
    let needle = format!("{stem}.md");
    text.match_indices(&needle).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        !before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    })
}

#[test]
fn vpw_01_six_video_seats_have_the_pinned_shape() {
    let roster = roster();
    let found: BTreeSet<&str> = roster
        .names()
        .into_iter()
        .filter(|n| n.starts_with("video-"))
        .collect();
    let want: BTreeSet<&str> = SEATS.iter().map(|s| s.0).collect();
    assert_eq!(found, want, "the video-* seats");

    for (name, phase, model, effort, skills) in SEATS {
        let t = seat(&roster, name);
        assert_eq!(t.phase, Some(phase), "{name} phase");
        assert_eq!(t.model.as_deref(), Some(model), "{name} model");
        assert_eq!(t.effort.as_deref(), Some(effort), "{name} effort");
        assert_eq!(t.skills, skills, "{name} skills");
        // Chrome, ffmpeg and macOS `say` are not live-checked in the Codex
        // sandbox, so no seat may fall back to Codex.
        assert!(t.fallbacks.is_empty(), "{name} fallbacks");
        // The video seats are offered in every project.
        assert!(t.offer_when.is_empty(), "{name} offer_when");
        assert!(t.requires.is_empty(), "{name} requires");
        assert!(
            t.disallowed_tools.iter().any(|d| d == "Agent"),
            "{name} must forbid Agent"
        );
    }
}

#[test]
fn vpw_01_only_the_critic_is_read_only() {
    let roster = roster();
    let critic = seat(&roster, "video-critic");
    for tool in ["Agent", "Edit", "NotebookEdit"] {
        assert!(
            critic.disallowed_tools.iter().any(|d| d == tool),
            "video-critic must forbid {tool}"
        );
    }
    for (name, ..) in SEATS.iter().filter(|s| s.0 != "video-critic") {
        let t = seat(&roster, name);
        assert_eq!(t.disallowed_tools, ["Agent"], "{name} disallowed_tools");
    }
}

#[test]
fn vpw_02_every_seat_skill_is_bundled() {
    let catalog = SkillCatalog::bundled().unwrap();
    let ids: BTreeSet<String> = catalog.entries().map(|e| e.id.to_string()).collect();
    for skill in VIDEO_SKILLS {
        assert!(ids.contains(skill), "{skill} is not in the bundled catalog");
        let md = format!("{skill}/SKILL.md");
        assert!(
            BUNDLED_SKILL_FILES.iter().any(|(p, _)| *p == md),
            "{md} is not bundled"
        );
    }
    for (name, .., skills) in SEATS {
        for skill in skills {
            assert!(
                ids.contains(*skill),
                "{name} names {skill}, which is not bundled"
            );
        }
    }
}

#[test]
fn vpw_02_the_script_entrypoints_are_bundled_executable() {
    for path in ENTRYPOINTS {
        assert!(
            BUNDLED_SKILL_FILES.iter().any(|(p, _)| *p == path),
            "{path} is not bundled"
        );
        assert!(
            BUNDLED_SKILL_EXECUTABLES.contains(&path),
            "{path} has no execute bit"
        );
    }
}

#[test]
fn vpw_02_a_seat_bundle_holds_every_skill_its_skills_call() {
    // A SKILL.md that runs `<skill-dir>/../<other>/...` needs <other> in the
    // same seat's `skills:`, because the bundle holds only the seat's skills.
    let roster = roster();
    for (name, .., skills) in SEATS {
        let t = seat(&roster, name);
        for skill in skills {
            let md = format!("{skill}/SKILL.md");
            let Some((_, bytes)) = BUNDLED_SKILL_FILES.iter().find(|(p, _)| *p == md) else {
                continue;
            };
            let text = String::from_utf8_lossy(bytes);
            for (at, _) in text.match_indices("../") {
                let sibling: String = text[at + 3..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
                    .collect();
                assert!(
                    t.skills.contains(&sibling),
                    "{md} calls ../{sibling}, which {name} does not carry"
                );
            }
        }
    }
}

#[test]
fn vpw_03_the_recipe_is_reachable_from_the_orchestrate_skill() {
    let recipe = repo().join("docs/recipes/video-pipeline.md");
    let text = std::fs::read_to_string(&recipe).expect("docs/recipes/video-pipeline.md exists");
    // The recipe ships inside the orchestrate skill; the docs file points to it.
    assert!(
        text.contains("skills/orchestrate/references/video-pipeline.md"),
        "docs/recipes/video-pipeline.md must link the shipped recipe"
    );
    let orchestrate = std::fs::read_to_string(repo().join("skills/orchestrate/SKILL.md")).unwrap();
    assert!(
        orchestrate.contains("references/video-pipeline.md"),
        "skills/orchestrate/SKILL.md must reference references/video-pipeline.md"
    );
    assert!(
        BUNDLED_SKILL_FILES
            .iter()
            .any(|(p, _)| *p == "orchestrate/references/video-pipeline.md"),
        "the recipe is not bundled with the orchestrate skill"
    );
}

#[test]
fn vpw_04_checker_self_tests_pass() {
    if !on_path("python3") || !on_path("ffmpeg") {
        eprintln!("skip: python3 and ffmpeg are both needed");
        return;
    }
    for script in PYTHON_SELF_TESTS {
        let out = Command::new("python3")
            .current_dir(repo())
            .args(["-I", script, "--self-test"])
            .output()
            .unwrap_or_else(|e| panic!("{script}: {e}"));
        assert!(
            out.status.success(),
            "{script} --self-test failed\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn vpw_05_scenes_use_the_seek_name_and_sec_fields() {
    for (path, text) in video_text_files() {
        assert!(
            !text.contains("__renderAt"),
            "{path} uses __renderAt; the name is __seekToFrame"
        );
        // Field names carry `_sec`: `duration_sec`, `start_sec`, `beats_sec`.
        for field in ["duration_s", "start_s", "end_s", "beats_s", "t_s"] {
            let bad = format!("{field}\"");
            let bare = format!("{field}:");
            let hit =
                text.contains(&bad) || text.contains(&bare) || text.contains(&format!("`{field}`"));
            assert!(!hit, "{path} uses the field {field}; use {field}ec");
        }
    }
}

#[test]
fn vpw_06_handoff_file_names_are_uppercase() {
    for (path, text) in video_text_files() {
        for stem in HANDOFF_NAMES {
            assert!(
                !names_file_lowercase(&text, stem),
                "{path} names {stem}.md in lowercase; the file is {}.md",
                stem.to_uppercase()
            );
        }
    }
}

#[test]
fn vpw_06_the_lowercase_name_matcher_is_exact() {
    assert!(names_file_lowercase("read `brief.md` first", "brief"));
    assert!(names_file_lowercase("brief.md", "brief"));
    assert!(!names_file_lowercase("read BRIEF.md", "brief"));
    assert!(!names_file_lowercase("see sub-brief.md", "brief"));
    assert!(!names_file_lowercase("a_brief.md", "brief"));
}
