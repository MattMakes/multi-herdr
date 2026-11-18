//! ARC-08: the roster split keeps strict serde, the overlay order and every
//! frontmatter shape the fleet has shipped.

use std::path::{Path, PathBuf};

use super::parser::{md_files, parse_base, parse_teammate};
use super::Roster;
use crate::fleetd::worker_profiles::domain::profile::{Liveness, Mode, OnUsageLimit, Profile};

fn manifest_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn repo_teammates() -> PathBuf {
    manifest_dir().join("../../teammates")
}

/// The built-in `opus.md`, with its `brief_description` replaced.
fn opus_saying(brief: &str) -> String {
    let text = std::fs::read_to_string(repo_teammates().join("opus.md")).unwrap();
    text.lines()
        .map(|line| {
            if line.starts_with("brief_description:") {
                format!("brief_description: {brief}")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn write_opus(dir: &Path, brief: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("opus.md"), opus_saying(brief)).unwrap();
}

#[test]
fn arc_08_unknown_field_rejected() {
    let text = "---\nname: x\nbrief_description: X\nagent: claude\nmodel: opus\n\
                permision_mode: auto\n---\nbody";
    let err = parse_teammate("x", text).unwrap_err();
    let message = format!("{err:#}");
    assert!(message.contains("permision_mode"), "{message}");
    assert!(message.contains("unknown field"), "{message}");
}

#[test]
fn arc_08_overlay_precedence() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let roster_override = tmp.path().join("override");
    let explicit = tmp.path().join("explicit");
    write_opus(
        &home.join(".config/horch/teammates"),
        "from the user overlay",
    );
    write_opus(&roster_override, "from the roster override");
    write_opus(&explicit, "from the explicit dir");
    let explicit = explicit.to_str().unwrap();

    let brief = |r: Roster| r.require("opus").unwrap().brief_description.clone();
    let builtin = brief(Roster::builtin().unwrap());
    assert!(builtin.starts_with("Generic worker"), "{builtin}");

    let all = Roster::load_layered(Some(&home), Some(&roster_override), Some(explicit)).unwrap();
    assert_eq!(brief(all), "from the explicit dir");
    let no_explicit = Roster::load_layered(Some(&home), Some(&roster_override), None).unwrap();
    assert_eq!(brief(no_explicit), "from the roster override");
    let user = Roster::load_layered(Some(&home), None, None).unwrap();
    assert_eq!(user.sources, vec![home.join(".config/horch/teammates")]);
    assert_eq!(brief(user), "from the user overlay");
    let none = Roster::load_layered(Some(&tmp.path().join("empty-home")), None, None).unwrap();
    assert!(none.sources.is_empty());
    assert_eq!(brief(none), builtin);
}

#[test]
fn arc_08_legacy_frontmatter_corpus_parses() {
    let dir = repo_teammates();
    let mut parsed = 0;
    for (stem, path) in md_files(&dir).unwrap() {
        if stem.starts_with('_') {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        parse_teammate(&stem, &text).unwrap_or_else(|e| panic!("{}: {e:#}", path.display()));
        parsed += 1;
    }
    assert!(parsed >= 33, "only {parsed} teammates parsed");
    for (stem, path) in md_files(&dir.join("_base")).unwrap() {
        let text = std::fs::read_to_string(&path).unwrap();
        parse_base(&stem, &text).unwrap_or_else(|e| panic!("{}: {e:#}", path.display()));
    }

    // Before the usage-pool fallbacks (c3e673d~1).
    let fixtures = manifest_dir().join("tests/fixtures/roster");
    let text = std::fs::read_to_string(fixtures.join("pre-fallbacks/opus.md")).unwrap();
    let opus = parse_teammate("opus", &text).unwrap();
    assert!(opus.fallbacks.is_empty());
    assert_eq!(opus.effort.as_deref(), Some("medium"));

    // Before codex teammates stated an effort (a1e6798).
    let text = std::fs::read_to_string(fixtures.join("pre-effort/codex-sol.md")).unwrap();
    let sol = parse_teammate("codex-sol", &text).unwrap();
    assert_eq!(sol.effort, None);
    assert_eq!(sol.phase, None);
    assert!(sol.fallbacks.is_empty());
}

/// The roster reads files and nothing else: it never talks to the terminal
/// server, starts a process or reads the environment. This test file is
/// skipped: it is test code, and the test's own name holds a needle.
#[test]
fn arc_08_roster_never_calls_herdr() {
    let needles = ["herdr", "Herdr", "std::process", "std::env::var"];
    let dir = manifest_dir().join("src/roster");
    let mut scanned = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs")
            || path.file_name() == Some("tests.rs".as_ref())
        {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        for needle in &needles {
            assert!(
                !text.contains(needle),
                "{} contains '{needle}'",
                path.display()
            );
        }
        scanned += 1;
    }
    assert!(scanned >= 9, "only {scanned} files scanned");
}

/// F7: a teammate file whose `requires` names a value this binary does not
/// know, as when the live roster is ahead of the installed `horch`.
fn write_ahead(dir: &Path, name: &str) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let text = opus_saying("ahead of the binary")
        .replacen("\n---\n", "\nrequires: [no-such-tool]\n---\n", 1)
        .replace("name: opus", &format!("name: {name}"));
    let path = dir.join(format!("{name}.md"));
    std::fs::write(&path, text).unwrap();
    path
}

#[test]
fn f7_builtin_roster_always_parses() {
    let r = Roster::builtin().expect("every compiled-in teammate and base parses");
    assert!(r.load_warnings().is_empty());
    assert!(r.names().len() >= 33, "{:?}", r.names());
}

#[test]
fn f7_bad_overlay_file_skips_only_that_teammate() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("teammates");
    let path = write_ahead(&dir, "newcomer");
    write_opus(&dir, "overlaid opus");

    let r = Roster::load_layered(None, Some(&dir), None).unwrap();
    assert_eq!(
        r.require("opus").unwrap().brief_description,
        "overlaid opus"
    );
    assert_eq!(r.sources, vec![dir.clone()]);
    assert!(!r.names().contains(&"newcomer"));

    let warnings = r.load_warnings();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    let w = &warnings[0];
    assert!(w.contains(&path.display().to_string()), "{w}");
    assert!(w.contains("unknown variant `no-such-tool`"), "{w}");
    assert!(w.contains("teammate 'newcomer' is not available"), "{w}");

    // Spawn by name: the parse error, not "unknown teammate".
    let err = format!("{:#}", r.require("newcomer").unwrap_err());
    assert!(!err.contains("unknown teammate"), "{err}");
    assert!(err.contains("unknown variant `no-such-tool`"), "{err}");
    assert!(err.contains(&path.display().to_string()), "{err}");

    // The gate stays strict.
    assert!(r.check().contains(w), "{:?}", r.check());
}

#[test]
fn f7_bad_override_of_builtin_falls_back_to_builtin() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("teammates");
    write_ahead(&dir, "opus");

    let r = Roster::load_layered(None, Some(&dir), None).unwrap();
    let builtin = Roster::builtin().unwrap();
    assert_eq!(
        r.require("opus").unwrap().brief_description,
        builtin.require("opus").unwrap().brief_description
    );
    let warnings = r.load_warnings();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0].contains("using the built-in 'opus' instead"),
        "{warnings:?}"
    );
    assert!(r.check().contains(&warnings[0]), "{:?}", r.check());
}

#[test]
fn f7_bad_layer_keeps_the_earlier_layer_and_a_later_layer_heals() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let user = home.join(".config/horch/teammates");
    let roster_override = tmp.path().join("override");
    let explicit = tmp.path().join("explicit");
    write_opus(&user, "from the user overlay");
    write_ahead(&roster_override, "opus");

    let r = Roster::load_layered(Some(&home), Some(&roster_override), None).unwrap();
    assert_eq!(
        r.require("opus").unwrap().brief_description,
        "from the user overlay"
    );
    let warnings = r.load_warnings();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    let earlier = format!(
        "using the earlier {} instead",
        user.join("opus.md").display()
    );
    assert!(warnings[0].contains(&earlier), "{warnings:?}");

    write_opus(&explicit, "from the explicit dir");
    let r = Roster::load_layered(
        Some(&home),
        Some(&roster_override),
        Some(explicit.to_str().unwrap()),
    )
    .unwrap();
    assert_eq!(
        r.require("opus").unwrap().brief_description,
        "from the explicit dir"
    );
    assert!(r.load_warnings().is_empty(), "{:?}", r.load_warnings());
}

#[test]
fn f7_bad_base_file_keeps_the_builtin_base() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("teammates");
    std::fs::create_dir_all(dir.join("_base")).unwrap();
    let path = dir.join("_base/fleet-worker.md");
    std::fs::write(&path, "no frontmatter here\n").unwrap();

    let r = Roster::load_layered(None, Some(&dir), None).unwrap();
    let builtin = Roster::builtin().unwrap();
    assert_eq!(
        r.base("fleet-worker").unwrap().body,
        builtin.base("fleet-worker").unwrap().body
    );
    let warnings = r.load_warnings();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("base 'fleet-worker'"), "{warnings:?}");
    assert!(
        warnings[0].contains(&path.display().to_string()),
        "{warnings:?}"
    );
    assert!(r.check().contains(&warnings[0]), "{:?}", r.check());
}

/// Every built-in teammate on `model: opus` carries the compiled-in
/// `sr_eng_system_prompt`, and an overlay `system_prompt/` file replaces it.
#[test]
fn opus_teammates_carry_the_sr_eng_system_prompt() {
    let source =
        std::fs::read_to_string(repo_teammates().join("system_prompt/sr_eng_system_prompt.md"))
            .unwrap();
    let builtin = Roster::builtin().unwrap();
    let opus: Vec<_> = builtin
        .teammates
        .values()
        .filter(|t| t.model.as_deref() == Some("opus"))
        .collect();
    assert!(!opus.is_empty());
    for t in opus {
        assert_eq!(
            t.system_prompt.as_deref(),
            Some("sr_eng_system_prompt"),
            "{}",
            t.name
        );
        assert_eq!(
            t.system_prompt_text.as_deref(),
            Some(source.as_str()),
            "{}",
            t.name
        );
    }

    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("teammates");
    std::fs::create_dir_all(dir.join("system_prompt")).unwrap();
    std::fs::write(
        dir.join("system_prompt/sr_eng_system_prompt.md"),
        "overlaid\n",
    )
    .unwrap();
    let r = Roster::load_layered(None, Some(&dir), None).unwrap();
    assert_eq!(
        r.require("opus").unwrap().system_prompt_text.as_deref(),
        Some("overlaid\n")
    );
}

/// `--check` fails a prompt name with no file, and a non-Claude agent.
#[test]
fn check_refuses_an_unknown_or_non_claude_system_prompt() {
    let mut r = Roster::builtin().unwrap();
    r.teammates.get_mut("opus").unwrap().system_prompt = Some("nope".into());
    r.teammates.get_mut("codex-sol").unwrap().system_prompt = Some("sr_eng_system_prompt".into());
    let problems = r.check();
    assert!(
        problems
            .iter()
            .any(|p| p.starts_with("opus: system_prompt 'nope' is not a file")),
        "{problems:#?}"
    );
    assert!(
        problems
            .iter()
            .any(|p| p.starts_with("codex-sol:") && p.contains("system_prompt")),
        "{problems:#?}"
    );
}

// Ticket fleetd-p2/06: the profile of a persona and the required fallback.

/// The built-in roster with `probe`: a copy of `opus.md` whose `fallbacks`
/// line is replaced by `lines`.
fn roster_with_probe(lines: &str) -> Roster {
    let text = std::fs::read_to_string(repo_teammates().join("opus.md")).unwrap();
    let text = text
        .replace("name: opus", "name: probe")
        .replace("fallbacks: [codex-sol]", lines);
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("probe.md"), text).unwrap();
    let mut roster = Roster::builtin().unwrap();
    roster.overlay(dir.path()).unwrap();
    roster
}

/// The lines of `check` about the file `probe.md`.
fn probe_problems(roster: &Roster) -> Vec<String> {
    roster
        .check()
        .into_iter()
        .filter(|p| p.starts_with("probe.md: "))
        .collect()
}

#[test]
fn profile_without_fallback_is_refused() {
    for lines in [
        "fallbacks: []",
        "",
        "fallbacks: []\nno_fallback_reason: \"  \"",
    ] {
        let problems = probe_problems(&roster_with_probe(lines));
        assert!(
            problems.iter().any(|p| p.contains("names no fallback")),
            "{lines:?}: {problems:?}"
        );
    }
}

#[test]
fn a_written_reason_for_no_fallback_is_accepted_and_the_task_waits() {
    let roster =
        roster_with_probe("fallbacks: []\nno_fallback_reason: the trust wall needs claude");
    assert_eq!(probe_problems(&roster), Vec::<String>::new());
    let profile = roster.require("probe").unwrap().profile();
    assert_eq!(profile.fallback(), None);
    assert_eq!(profile.on_usage_limit(), OnUsageLimit::WaitQuota);
}

#[test]
fn a_reason_beside_a_fallback_is_refused() {
    let roster = roster_with_probe("fallbacks: [codex-sol]\nno_fallback_reason: stale");
    let problems = probe_problems(&roster);
    assert!(
        problems.iter().any(|p| p.contains("no_fallback_reason")),
        "{problems:?}"
    );
}

#[test]
fn top_tier_seat_orchestrator_recipe_pane_and_agent_none_need_no_fallback() {
    let roster = Roster::builtin().unwrap();
    for name in [
        "fable-creative",
        "astra-creative",
        "orchestrator",
        "orchestrator-codex",
        "orchestration-orchestrator",
        "orchestration-worker",
        "smoke",
    ] {
        let t = roster.require(name).unwrap();
        assert!(t.fallbacks.is_empty(), "{name}");
        assert!(t.no_fallback_reason.is_none(), "{name}");
        let file = format!("{name}.md: ");
        let problems: Vec<String> = roster
            .check()
            .into_iter()
            .filter(|p| p.starts_with(&file))
            .collect();
        assert!(problems.is_empty(), "{problems:?}");
    }
}

#[test]
fn liveness_and_retry_absent_give_the_design_defaults() {
    let roster = Roster::builtin().unwrap();
    let profile = roster.require("opus").unwrap().profile();
    assert_eq!(
        profile,
        Profile {
            fallback: Some("codex-sol".to_string()),
            ..Profile::default()
        }
    );
    assert_eq!(profile.liveness.lease_s, 120);
    assert_eq!(profile.liveness.max_wait_s, 3600);
    assert_eq!(profile.retry.max_attempts, None);
    assert_eq!(profile.retry.backoff_s, [30, 120, 480]);

    // A partial block keeps the default of each field it leaves out.
    let roster = roster_with_probe(
        "fallbacks: [codex-sol]\nliveness: {max_wait_s: 600}\nretry: {max_attempts: 5}",
    );
    let profile = roster.require("probe").unwrap().profile();
    assert_eq!(
        profile.liveness,
        Liveness {
            max_wait_s: 600,
            ..Liveness::default()
        }
    );
    assert_eq!(profile.retry.max_attempts, Some(5));
    assert_eq!(profile.retry.backoff_s, [30, 120, 480]);
    assert_eq!(probe_problems(&roster), Vec::<String>::new());
}

#[test]
fn mode_absent_is_exec_in_phase_7() {
    let roster = Roster::builtin().unwrap();
    assert_eq!(roster.require("opus").unwrap().profile().mode, Mode::Exec);
    let roster = roster_with_probe("fallbacks: [codex-sol]\nmode: interactive");
    assert_eq!(
        roster.require("probe").unwrap().profile().mode,
        Mode::Interactive
    );
}

#[test]
fn lease_not_below_stall_is_refused_with_the_field() {
    let roster = roster_with_probe("fallbacks: [codex-sol]\nliveness: {lease_s: 300}");
    assert_eq!(
        probe_problems(&roster),
        ["probe.md: liveness.lease_s (300) must be below liveness.stall_after_s (300)"]
    );
    let roster = roster_with_probe("fallbacks: [codex-sol]\nretry: {backoff_s: []}");
    assert_eq!(
        probe_problems(&roster),
        ["probe.md: retry.backoff_s must not be empty"]
    );
}

#[test]
fn fallback_is_the_first_entry_of_fallbacks() {
    let roster = roster_with_probe("fallbacks: [codex-sol, codex-terra]");
    let t = roster.require("probe").unwrap();
    assert_eq!(t.profile().fallback(), Some("codex-sol"));
    assert_eq!(
        t.profile().on_usage_limit(),
        OnUsageLimit::RestartOn("codex-sol")
    );
}

/// No fallback lowers confidentiality, for the parts of the ladder a teammate
/// file states: a local persona never falls back to a cloud pool, a persona
/// that does not train on its input never falls back to one that does, and a
/// persona with a Claude `sandbox:` block that walls more than the fleet
/// worker block, or with `horch_access: report`, names no fallback, because
/// a fallback launches without that block. The fleet worker block only
/// turns the sandbox on with the network of today (ticket 39); profile
/// validation refuses a fallback whose own sandbox does not deny the
/// operator path. The sandbox of codex and agy comes from the harness, not
/// from the file, so this test cannot read it.
#[test]
fn no_fallback_lowers_confidentiality() {
    use crate::routing::quota::{pool_for, POOL_LOCAL};
    let roster = Roster::builtin().unwrap();
    let pool =
        |t: &super::Teammate| pool_for(t.agent.as_str(), t.model.as_deref().unwrap_or_default());
    for t in roster.teammates.values() {
        let fleet_worker = serde_json::json!({"network": {"allowedDomains": ["*"]}});
        let walls = |s: &serde_json::Map<String, serde_json::Value>| {
            serde_json::Value::Object(s.clone()) != fleet_worker
        };
        let walled =
            t.sandbox.as_ref().is_some_and(walls) || t.horch_access == super::HorchAccess::Report;
        assert!(
            !walled || t.fallbacks.is_empty(),
            "{}: a sandboxed persona names fallback {:?}",
            t.name,
            t.fallbacks
        );
        for name in &t.fallbacks {
            let f = roster.require(name).unwrap();
            assert!(
                pool(t) != POOL_LOCAL || pool(f) == POOL_LOCAL,
                "{}: the local persona falls back to {name} on pool {}",
                t.name,
                pool(f)
            );
            assert!(
                t.trains_on_input || !f.trains_on_input,
                "{}: falls back to {name}, which trains on its input",
                t.name
            );
        }
    }
}

/// The escape from D2 cannot grow in silence: the exact list of shipped
/// personas whose file gave a written reason for no fallback before ticket
/// fleetd-p2/06 (confidentiality, trust, safety or capability).
#[test]
fn the_shipped_personas_with_no_fallback_reason_are_pinned() {
    let roster = Roster::builtin().unwrap();
    let with_reason: Vec<&str> = roster
        .teammates
        .values()
        .filter(|t| t.no_fallback_reason.is_some())
        .map(|t| t.name.as_str())
        .collect();
    assert_eq!(
        with_reason,
        [
            "app-release-preparer",
            "apple-platform-developer",
            "audio-reviewer",
            "blender-3d-printer",
            "blender-artist",
            "blender-builder",
            "blender-qa",
            "idea-lead",
            "intake-analyst",
            "intake-auditor",
            "intake-reviewer",
            "intake-sanitizer",
            "intake-writer",
            "opus-creative",
            "pi",
            "skill-importer",
            "sound-engineer",
            "swift-developer",
            "swift-qa-engineer",
            "ue-ai-engineer",
            "ue-character-engineer",
            "ue-gameplay-engineer",
            "ue-network-engineer",
            "ue-qa-engineer",
            "ue-technical-artist",
            "ue-tools-engineer",
            "ue-ui-engineer",
            "ue-world-engineer",
            "video-animator",
            "video-audio",
            "video-capture",
            "video-critic",
            "video-director",
            "video-editor",
            "video-producer",
            "worktree-steward",
        ]
    );
}
