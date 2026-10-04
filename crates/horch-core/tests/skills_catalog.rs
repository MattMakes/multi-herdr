//! A9: the versioned skill catalog and the activation plan (SKL-01, SKL-02,
//! SKL-03, SKL-07, SKL-08).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use horch_core::harness::HarnessKind;
use horch_core::ids::SkillId;
use horch_core::roster::{Phase, Roster, Teammate};
use horch_core::skills::catalog::{parse_provenance, SourceRef};
use horch_core::skills::{
    self, briefing, plan_activation, BriefingContext, CatalogSource, InvocationPolicy,
    MaterializedSkills, SkillCatalog, BUNDLED_SKILL_FILES,
};
use horch_marketplace::LockEntry;

const PINNED_REPOSITORY: &str = "https://github.com/MattMakes/skill-marketplace";
const PINNED_COMMIT: &str = "d47670328c59a3311a9b4149bc5f8f33f0a92754";
/// Skills written in this repository: their provenance has no sources.
const REPO_ORIGINAL: &[&str] = &["orchestrate", "ue-build-verify", "ue-editor-scripting"];

const PHASES: [Option<Phase>; 5] = [
    None,
    Some(Phase::Research),
    Some(Phase::Plan),
    Some(Phase::Implementation),
    Some(Phase::Validation),
];

fn repo_roster() -> Roster {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../teammates");
    Roster::load_layered(None, None, Some(dir.to_str().unwrap())).unwrap()
}

fn skills_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills")
}

fn oracles() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/oracles/skills")
}

/// Teammates added after the A0 freeze. They have no oracle file and the
/// oracle loop skips them; a missing file for any other teammate still
/// fails. Never give one of these an oracle file.
const SKIP_NEW_TEAMMATES: &[&str] = &[
    "antigravity",
    "design-critic",
    "design-director",
    "design-system-engineer",
    "judge",
    "landing-page-builder",
    "motion-engineer",
    "ue-ai-engineer",
    "ue-character-engineer",
    "ue-code-reviewer",
    "ue-gameplay-engineer",
    "ue-network-engineer",
    "ue-qa-engineer",
    "ue-tech-lead",
    "ue-technical-artist",
    "ue-tools-engineer",
    "ue-ui-engineer",
    "ue-world-engineer",
    "visual-prototyper",
];

fn is_hex(s: &str) -> bool {
    s.bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn lock_entry(id: &str, source: &str, version: &str) -> LockEntry {
    LockEntry {
        id: id.into(),
        source: source.into(),
        requested_revision: Some("v1".into()),
        resolved_commit: Some("ab".repeat(20)),
        version: version.into(),
        digest: format!("sha256:{}", "cd".repeat(32)),
        installed_at: "2026-10-02T12:00:00Z".into(),
    }
}

#[test]
fn skl_01_bundled_catalog_versions_and_digests() {
    let catalog = SkillCatalog::bundled().unwrap();
    assert_eq!(catalog, SkillCatalog::bundled().unwrap(), "stable digests");

    // Every skill directory is in the catalog, and nothing else is.
    let on_disk: BTreeSet<String> = std::fs::read_dir(skills_dir())
        .unwrap()
        .map(|e| e.unwrap())
        .filter(|e| e.path().join("SKILL.md").is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    let in_catalog: BTreeSet<String> = catalog.entries().map(|e| e.id.to_string()).collect();
    assert_eq!(in_catalog, on_disk);

    let tmp = tempfile::tempdir().unwrap();
    for entry in catalog.entries() {
        let id = entry.id.as_str();
        let short = entry.version.0.strip_prefix("bundled+").unwrap();
        assert_eq!(short.len(), 12, "{id}: {}", entry.version);
        assert!(is_hex(short), "{id}: {}", entry.version);
        assert_eq!(short, entry.digest.short12(), "{id}");
        assert_eq!(entry.source, CatalogSource::Bundled);
        assert_eq!(entry.source_label(), "bundled");
        assert!(!entry.description.trim().is_empty(), "{id}");

        // The digest is the marketplace tree digest of the skill directory.
        let dir = tmp.path().join(id);
        for (path, bytes) in BUNDLED_SKILL_FILES {
            if let Some(rel) = path.strip_prefix(&format!("{id}/")) {
                let target = dir.join(rel);
                std::fs::create_dir_all(target.parent().unwrap()).unwrap();
                std::fs::write(target, bytes).unwrap();
            }
        }
        assert_eq!(
            horch_marketplace::integrity::tree_digest(&dir).unwrap(),
            entry.digest.to_string(),
            "{id}"
        );

        // A repo-original skill has an entry with no sources; every other
        // skill names pinned upstream files.
        let p = entry.provenance.as_ref().unwrap_or_else(|| panic!("{id}"));
        assert!(!p.adaptation.trim().is_empty(), "{id}");
        if REPO_ORIGINAL.contains(&id) {
            assert!(p.sources.is_empty(), "{id}");
            continue;
        }
        assert!(!p.sources.is_empty(), "{id}");
        for src in &p.sources {
            assert_eq!(src.sha256.len(), 64, "{id}");
            assert!(is_hex(&src.sha256), "{id}");
            assert!(!src.path.is_empty(), "{id}");
            // The skill-marketplace upstream has one exact pin. Any other
            // upstream (design, vendored) is checked by shape: a GitHub
            // repository, a full commit and a licence.
            if src.repository == PINNED_REPOSITORY {
                assert_eq!(src.revision, PINNED_COMMIT, "{id}: {}", src.path);
            } else {
                assert_pinned_upstream_shape(id, src);
            }
        }
    }
}

/// A non-marketplace source names `https://github.com/<owner>/<repo>`, a
/// 40-hex revision, a 64-hex sha256 and a non-empty licence.
fn assert_pinned_upstream_shape(id: &str, src: &SourceRef) {
    let repo = src
        .repository
        .strip_prefix("https://github.com/")
        .unwrap_or_else(|| panic!("{id}: {} is not a GitHub repository", src.repository));
    let parts: Vec<&str> = repo.split('/').collect();
    assert!(
        parts.len() == 2 && parts.iter().all(|p| !p.is_empty()),
        "{id}: {} is not https://github.com/<owner>/<repo>",
        src.repository
    );
    assert!(
        src.revision.len() == 40 && is_hex(&src.revision),
        "{id}: {} revision {} is not a full commit",
        src.repository,
        src.revision
    );
    assert!(
        src.sha256.len() == 64 && is_hex(&src.sha256),
        "{id}: {} sha256 is not 64 hex digits",
        src.path
    );
    assert!(
        src.license.as_deref().is_some_and(|l| !l.trim().is_empty()),
        "{id}: {} has no licence",
        src.path
    );
}

/// A skill entry may list several upstream sources; the single-source
/// fields still parse as a 1-item list; an original skill has none.
#[test]
fn skills_multi_source_provenance_parses() {
    let text = r#"{
      "skills": [
        {
          "name": "multi",
          "sources": [
            {"repository": "https://example.com/a", "revision": "aa11", "path": "x/SKILL.md",
             "sha256": "SHA", "license": "MIT"},
            {"repository": "https://example.com/b", "revision": "bb22", "path": "y/rules.md",
             "sha256": "SHA"}
          ],
          "adaptation": "Merged.",
          "vendored": true
        },
        {"name": "old", "source_repository": "https://example.com/c", "source_revision": "cc33",
         "source_path": "z/SKILL.md", "source_sha256": "SHA", "adaptation": "Kept."},
        {"name": "own", "source_path": null, "adaptation": "Original."}
      ]
    }"#
    .replace("SHA", &"ab".repeat(32));
    let parsed = parse_provenance(&text).unwrap();
    let multi = &parsed["multi"];
    assert_eq!(multi.sources.len(), 2);
    assert_eq!(multi.sources[0].license.as_deref(), Some("MIT"));
    assert!(multi.vendored);
    assert!(!parsed["old"].vendored, "vendored defaults to false");
    assert_eq!(multi.sources[1].repository, "https://example.com/b");
    assert_eq!(multi.sources[1].license, None);
    assert_eq!(parsed["old"].sources.len(), 1);
    assert_eq!(parsed["old"].sources[0].revision, "cc33");
    assert!(parsed["own"].sources.is_empty());
    assert_eq!(parsed["own"].adaptation, "Original.");

    // Mixing both shapes, a bad digest and a duplicate name fail.
    let mixed = text.replace(
        r#""name": "own", "source_path": null"#,
        r#""name": "own", "sources": [], "source_path": "p""#,
    );
    assert!(parse_provenance(&mixed).is_err());
    let bad = text.replacen(&"ab".repeat(32), "abc", 1);
    assert!(parse_provenance(&bad).is_err());
    let dup = text.replace(r#""name": "own""#, r#""name": "old""#);
    assert!(parse_provenance(&dup).is_err());
}

/// A vendored skill (`"vendored": true` in provenance.json) is an upstream
/// copy and is exempt from the size budget. Any other skill meets it.
const MAX_SKILL_MD_BYTES: usize = 12 * 1024;
const MAX_SKILL_DIR_BYTES: usize = 160 * 1024;

/// Skill ids exempt from the size budget: every skill whose provenance says
/// `vendored: true`.
fn budget_exempt() -> BTreeSet<String> {
    let catalog = SkillCatalog::bundled().unwrap();
    catalog
        .entries()
        .filter(|e| e.provenance.as_ref().is_some_and(|p| p.vendored))
        .map(|e| e.id.to_string())
        .collect()
}

#[test]
fn skills_bundled_size_budget() {
    let exempt = budget_exempt();
    let mut dirs: BTreeMap<&str, usize> = BTreeMap::new();
    for (path, bytes) in BUNDLED_SKILL_FILES {
        // Top-level files (README.md, provenance.json) belong to no skill.
        let Some((id, rel)) = path.split_once('/') else {
            continue;
        };
        *dirs.entry(id).or_default() += bytes.len();
        if rel == "SKILL.md" && !exempt.contains(id) {
            assert!(
                bytes.len() <= MAX_SKILL_MD_BYTES,
                "{id}/SKILL.md: {} bytes",
                bytes.len()
            );
        }
    }
    for (id, total) in dirs {
        if !exempt.contains(id) {
            assert!(total <= MAX_SKILL_DIR_BYTES, "{id}: {total} bytes");
        }
    }
}

/// `skill-creator` is a verbatim upstream copy that ships Python scripts,
/// HTML assets and `LICENSE.txt`. It is the only text-only exemption.
const EXEMPT_FROM_TEXT_ONLY: &[&str] = &["skill-creator"];

/// Skill files are `.md` or `.txt`. A file named exactly `LICENSE` is
/// allowed in any skill directory: a vendored or adapted skill keeps its
/// upstream licence notice. Vendored skills are not exempt from this rule.
#[test]
fn skills_bundled_text_only() {
    for (path, _) in BUNDLED_SKILL_FILES {
        let Some((id, _)) = path.split_once('/') else {
            continue;
        };
        if EXEMPT_FROM_TEXT_ONLY.contains(&id) {
            continue;
        }
        let name = path.rsplit('/').next().unwrap();
        assert!(
            name == "LICENSE" || path.ends_with(".md") || path.ends_with(".txt"),
            "{path}"
        );
    }
}

/// `build.rs` never bundles a dotfile or a file under a dot directory. The
/// fixture `skills/.dotfile-fixture` exists on disk and must not appear.
#[test]
fn skills_bundled_skip_dotfiles() {
    assert!(skills_dir().join(".dotfile-fixture").is_file());
    for (path, _) in BUNDLED_SKILL_FILES {
        assert!(
            !path.split('/').any(|part| part.starts_with('.')),
            "{path} is a dotfile"
        );
    }
}

/// The lock override rule: a `bundled:` lock entry never replaces the
/// compiled-in copy; any other lock entry does, and new ids are added.
#[test]
fn skl_01_lock_entries_merge_by_the_override_rule() {
    let bundled = SkillCatalog::bundled().unwrap();
    let tdd = SkillId::new("tdd").unwrap();
    let merged = bundled
        .clone()
        .with_lock(&[
            lock_entry("tdd", "bundled:tdd", "bundled+000000000000"),
            lock_entry("debug", "https://example.com/r.git", "git+abababababab"),
            lock_entry("fresh", "https://example.com/r.git", "git+abababababab"),
        ])
        .unwrap();
    assert_eq!(merged.get(&tdd), bundled.get(&tdd));
    let debug = merged.lookup("debug").unwrap();
    assert_eq!(debug.version.0, "git+abababababab");
    assert_eq!(
        debug.source_label(),
        format!("https://example.com/r.git@{}", "ab".repeat(20))
    );
    assert!(matches!(debug.source, CatalogSource::Marketplace { .. }));
    assert!(merged.lookup("fresh").is_some());
    assert_eq!(merged.len(), bundled.len() + 1);

    for bad in [
        lock_entry("../escape", "https://example.com/r.git", "git+abababababab"),
        lock_entry("ok", "https://example.com/r.git", ""),
        LockEntry {
            digest: "md5:00".into(),
            ..lock_entry("ok", "https://example.com/r.git", "git+abababababab")
        },
    ] {
        assert!(bundled.clone().with_lock(&[bad]).is_err());
    }
}

#[test]
fn skl_02_activation_matches_legacy_selection() {
    let catalog = SkillCatalog::bundled().unwrap();
    let roster = repo_roster();
    let mut compared = 0;
    for name in roster.names() {
        for phase in PHASES {
            let mut t = roster.get(name).unwrap().clone();
            t.phase = phase;
            let legacy = skills::selected(&t);
            let plan = plan_activation(&t, phase, &catalog);
            match (legacy, plan) {
                (Ok(legacy), Ok(plan)) => {
                    assert_eq!(plan.activated_ids(), legacy, "{name} {phase:?}");
                    let all: BTreeSet<&str> = plan
                        .activated
                        .iter()
                        .chain(&plan.available)
                        .map(|r| r.id.as_str())
                        .collect();
                    assert_eq!(all.len(), catalog.len(), "{name} {phase:?}");
                }
                (Err(legacy), Err(plan)) => {
                    assert_eq!(format!("{plan:#}"), format!("{legacy:#}"), "{name}")
                }
                (legacy, plan) => panic!("{name} {phase:?}: {legacy:?} vs {plan:?}"),
            }
            compared += 1;
        }
    }
    assert!(compared >= 33 * PHASES.len(), "{compared}");
}

#[test]
fn skl_03_policy_mapping() {
    let catalog = SkillCatalog::bundled().unwrap();
    // `tdd` is both the teammate's own and an implementation phase skill.
    let t = Teammate {
        name: "mapper".into(),
        skills: vec!["trace".into(), "tdd".into()],
        phase: Some(Phase::Implementation),
        ..Teammate::default()
    };
    let plan = plan_activation(&t, t.phase, &catalog).unwrap();
    let policy: BTreeMap<&str, InvocationPolicy> = plan
        .activated
        .iter()
        .chain(&plan.available)
        .map(|r| (r.id.as_str(), r.policy))
        .collect();
    assert_eq!(
        plan.activated.len() + plan.available.len(),
        policy.len(),
        "no skill appears twice"
    );
    assert_eq!(policy.len(), catalog.len());
    for (id, want) in [
        ("trace", InvocationPolicy::Explicit),
        ("tdd", InvocationPolicy::Explicit),
        ("execute", InvocationPolicy::Deterministic),
        ("debug", InvocationPolicy::Deterministic),
        ("check", InvocationPolicy::Deterministic),
        ("handoff", InvocationPolicy::Deterministic),
    ] {
        assert_eq!(policy[id], want, "{id}");
    }
    assert!(plan
        .available
        .iter()
        .all(|r| r.policy == InvocationPolicy::Available));
    assert_eq!(
        plan.activated_ids(),
        ["check", "debug", "execute", "handoff", "tdd", "trace"],
        "activated is sorted by id"
    );
    for r in plan.activated.iter().chain(&plan.available) {
        let entry = catalog.get(&r.id).unwrap();
        assert_eq!((&r.version, r.digest), (&entry.version, entry.digest));
    }

    // No phase: only the teammate's own skills activate.
    let plan = plan_activation(&t, None, &catalog).unwrap();
    assert_eq!(plan.activated_ids(), ["tdd", "trace"]);

    // The plan is data: it round-trips through JSON.
    let json = serde_json::to_string(&plan).unwrap();
    assert!(json.contains(r#""policy":"explicit""#), "{json}");
    assert_eq!(
        serde_json::from_str::<skills::SkillActivationPlan>(&json).unwrap(),
        plan
    );
}

#[test]
fn skl_07_plugin_skills_separate() {
    let catalog = SkillCatalog::bundled().unwrap();
    // One plugin skill shares a bundled skill's name; one does not exist in
    // the catalog at all.
    let plugin_skills: BTreeMap<String, Vec<String>> = [
        ("superpowers".to_string(), vec!["brainstorming".to_string()]),
        ("dev".to_string(), vec!["tdd".to_string()]),
    ]
    .into();
    let t = Teammate {
        name: "plugged".into(),
        agent: HarnessKind::Claude,
        phase: Some(Phase::Plan),
        plugin_skills: plugin_skills.clone(),
        ..Teammate::default()
    };
    let plan = plan_activation(&t, t.phase, &catalog).unwrap();
    assert_eq!(plan.plugin_skills, plugin_skills);
    assert!(catalog.lookup("brainstorming").is_none());
    let ids: Vec<&str> = plan
        .activated
        .iter()
        .chain(&plan.available)
        .map(|r| r.id.as_str())
        .collect();
    assert!(!ids.contains(&"brainstorming"));
    assert_eq!(ids.len(), catalog.len());
    // The plugin's `tdd` does not activate the bundled `tdd`.
    let tdd = plan.available.iter().find(|r| r.id.as_str() == "tdd");
    assert_eq!(tdd.unwrap().policy, InvocationPolicy::Available);
    assert_eq!(
        plan.activated_ids(),
        ["create-plan", "handoff", "pre-flight"]
    );
    let without = Teammate {
        plugin_skills: BTreeMap::new(),
        ..t.clone()
    };
    let plain = plan_activation(&without, t.phase, &catalog).unwrap();
    assert_eq!(
        (&plain.activated, &plain.available),
        (&plan.activated, &plan.available)
    );
}

#[test]
fn skl_08_briefing_matches_baseline_modulo_path() {
    let catalog = SkillCatalog::bundled().unwrap();
    let roster = repo_roster();
    let tmp = tempfile::tempdir().unwrap();
    let mut compared = 0;
    for name in roster.names() {
        if SKIP_NEW_TEAMMATES.contains(&name)
            && !oracles().join(format!("{name}-research.txt")).exists()
        {
            continue;
        }
        for phase in PHASES.into_iter().flatten() {
            let mut t = roster.get(name).unwrap().clone();
            t.phase = Some(phase);
            let execution = format!("{name}-{phase}");
            let text = match plan_activation(&t, t.phase, &catalog) {
                Err(e) => format!("ERROR: {e:#}\n"),
                Ok(plan) => {
                    // The A0 roster has no plugin_skills; their lines need
                    // the operator's installed plugins.
                    assert!(plan.plugin_skills.is_empty(), "{name}");
                    match MaterializedSkills::materialize(&plan, &catalog, tmp.path(), &execution)
                        .unwrap()
                    {
                        None => "NO BUNDLE\n".to_string(),
                        Some(files) => {
                            assert!(files.root.ends_with(format!("skill-bundles/{execution}")));
                            for r in &plan.activated {
                                assert!(files.skills_dir().join(r.id.as_str()).is_dir());
                            }
                            for r in &plan.available {
                                assert!(!files.skills_dir().join(r.id.as_str()).exists());
                            }
                            let text = briefing::render(
                                &plan,
                                &catalog,
                                &BriefingContext {
                                    phase: t.phase,
                                    declared: &t.skills,
                                    namespace: (t.agent == HarnessKind::Claude).then_some("horch"),
                                    plugin_lines: &[],
                                    skills_dir: &files.skills_dir(),
                                },
                            );
                            let root = files.root.clone();
                            drop(files);
                            assert!(!root.exists(), "dropped bundle is removed");
                            text.replace(&*root.to_string_lossy(), "<BUNDLE>")
                        }
                    }
                }
            };
            let path = oracles().join(format!("{name}-{phase}.txt"));
            let want = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert_eq!(text, want, "{name}-{phase}");
            compared += 1;
        }
    }
    assert_eq!(compared, 33 * 4);
}

#[test]
fn skl_08_materialize_rejects_path_like_execution_ids() {
    let catalog = SkillCatalog::bundled().unwrap();
    let t = Teammate {
        phase: Some(Phase::Plan),
        ..Teammate::default()
    };
    let plan = plan_activation(&t, t.phase, &catalog).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    for bad in ["", ".", "..", "../x", "a/b", "a\\b"] {
        assert!(
            MaterializedSkills::materialize(&plan, &catalog, tmp.path(), bad).is_err(),
            "{bad:?}"
        );
    }
    let a = MaterializedSkills::materialize(&plan, &catalog, tmp.path(), "x").unwrap();
    assert!(a.is_some());
    assert!(
        MaterializedSkills::materialize(&plan, &catalog, tmp.path(), "x").is_err(),
        "two executions never share a directory"
    );
}

// ─── available_skills and operator_skills ──────────────────────────────────

/// The `{who}:` problems `--check` reports for `t`, on the repo roster with
/// `home` as the launch home.
fn check_problems(t: Teammate, home: Option<&Path>) -> Vec<String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../teammates");
    let mut roster = Roster::load_layered(home, None, Some(dir.to_str().unwrap())).unwrap();
    let who = t.name.clone();
    roster.insert_for_test(t);
    roster
        .check()
        .into_iter()
        .filter(|p| p.contains(&who))
        .collect()
}

/// A valid fleet worker to vary: the repo's `backend-developer`, renamed.
fn probe() -> Teammate {
    Teammate {
        name: "probe".into(),
        ..repo_roster().require("backend-developer").unwrap().clone()
    }
}

/// `available_skills:` are activated as Offered, so they are materialized,
/// and the briefing names them under "Also available" without their
/// description. A phase skill named there stays Deterministic.
#[test]
fn available_skills_are_materialized_and_named_without_description() {
    let catalog = SkillCatalog::bundled().unwrap();
    let t = Teammate {
        name: "offered".into(),
        skills: vec!["tdd".into()],
        available_skills: vec!["trace".into(), "debug".into()],
        phase: Some(Phase::Implementation),
        ..Teammate::default()
    };
    let plan = plan_activation(&t, t.phase, &catalog).unwrap();
    let policy: BTreeMap<&str, InvocationPolicy> = plan
        .activated
        .iter()
        .map(|r| (r.id.as_str(), r.policy))
        .collect();
    assert_eq!(policy["tdd"], InvocationPolicy::Explicit);
    assert_eq!(policy["trace"], InvocationPolicy::Offered);
    assert_eq!(policy["debug"], InvocationPolicy::Deterministic);
    assert!(plan.available.iter().all(|r| r.id.as_str() != "trace"));
    assert_eq!(
        skills::selected(&t).unwrap(),
        ["check", "debug", "execute", "handoff", "tdd", "trace"]
    );

    let tmp = tempfile::tempdir().unwrap();
    let files = MaterializedSkills::materialize(&plan, &catalog, tmp.path(), "offered")
        .unwrap()
        .unwrap();
    assert!(files.skills_dir().join("trace/SKILL.md").is_file());
    let text = briefing::render(
        &plan,
        &catalog,
        &BriefingContext {
            phase: t.phase,
            declared: &t.skills,
            namespace: Some("horch"),
            plugin_lines: &[],
            skills_dir: &files.skills_dir(),
        },
    );
    let also = text
        .lines()
        .find(|l| l.starts_with("Also available in this phase: "))
        .unwrap_or_else(|| panic!("{text}"));
    assert!(also.contains("horch:trace"), "{also}");
    let trace = catalog.lookup("trace").unwrap();
    let words: String = trace
        .description
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(!text.contains(&words), "no description for trace:\n{text}");
    assert!(text.contains("- horch:tdd: "), "{text}");

    // Without a phase or skills, the offered skill alone makes a bundle.
    let only = Teammate {
        name: "only-offered".into(),
        available_skills: vec!["trace".into()],
        ..Teammate::default()
    };
    let plan = plan_activation(&only, None, &catalog).unwrap();
    assert_eq!(plan.activated_ids(), ["trace"]);
    let text = briefing::render(
        &plan,
        &catalog,
        &BriefingContext {
            phase: None,
            declared: &only.skills,
            namespace: None,
            plugin_lines: &[],
            skills_dir: tmp.path(),
        },
    );
    assert!(text.contains("Available native skills: trace."), "{text}");
    assert!(!text.contains(&words), "{text}");
}

/// `--check` refuses an unknown offered skill, one already in `skills:`,
/// and an orchestrator-only one.
#[test]
fn available_skills_check_errors() {
    let unknown = Teammate {
        available_skills: vec!["no-such-skill".into()],
        ..probe()
    };
    let problems = check_problems(unknown, None);
    assert!(
        problems
            .iter()
            .any(|p| p.contains("unknown bundled skill 'no-such-skill'")),
        "{problems:?}"
    );

    let mut twice = probe();
    twice.available_skills = vec![twice.skills[0].clone()];
    let problems = check_problems(twice, None);
    assert!(
        problems
            .iter()
            .any(|p| p.contains("both in skills and in available_skills")),
        "{problems:?}"
    );

    let orchestrate = Teammate {
        available_skills: vec!["orchestrate".into()],
        ..probe()
    };
    let problems = check_problems(orchestrate, None);
    assert!(
        problems
            .iter()
            .any(|p| p.contains("'orchestrate' belongs to the orchestrator only")),
        "{problems:?}"
    );

    let fine = Teammate {
        available_skills: vec!["trace".into()],
        ..probe()
    };
    assert_eq!(check_problems(fine, None), Vec::<String>::new());
}

/// Write `<dir>/<name>/SKILL.md` with `body` after the frontmatter, plus one
/// reference file.
fn operator_skill(dir: &Path, name: &str, body: &str) {
    let skill = dir.join(name);
    std::fs::create_dir_all(skill.join("references")).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: Operator probe skill {name}.\n---\n{body}\n"),
    )
    .unwrap();
    std::fs::write(skill.join("references/notes.md"), "notes\n").unwrap();
}

fn operator(dir: &str, names: &[&str]) -> Option<horch_core::roster::OperatorSkills> {
    Some(horch_core::roster::OperatorSkills {
        dir: dir.into(),
        names: names.iter().map(|n| (*n).to_owned()).collect(),
    })
}

/// `--check` reads the operator directory the launch will read, with `~/`
/// expanded against the roster's home, and refuses a missing directory, a
/// missing name, a name clash with a catalog skill, and a subagent skill.
#[test]
fn operator_skills_check_errors() {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join(".agents/skills");
    operator_skill(&dir, "test-modernizer", "Modernize tests.");
    operator_skill(&dir, "device-interaction", "Drive the device.");
    operator_skill(
        &dir,
        "helper",
        "You are a SUBAGENT skill. Start it with the Agent tool.",
    );
    operator_skill(&dir, "tdd", "A clash.");
    let with = |o| Teammate {
        operator_skills: o,
        ..probe()
    };
    let problems = |o| check_problems(with(o), Some(home.path())).join("\n");

    assert_eq!(
        problems(operator("~/.agents/skills", &["test-modernizer"])),
        ""
    );
    let p = problems(operator("~/no-such-dir", &["test-modernizer"]));
    assert!(
        p.contains("operator_skills dir '~/no-such-dir' does not exist"),
        "{p}"
    );
    let p = problems(operator("~/.agents/skills", &["missing"]));
    assert!(
        p.contains("operator skill 'missing' is not in '~/.agents/skills'"),
        "{p}"
    );
    let p = problems(operator("~/.agents/skills", &["device-interaction"]));
    assert!(
        p.contains("'device-interaction' is a subagent skill"),
        "{p}"
    );
    let p = problems(operator("~/.agents/skills", &["helper"]));
    assert!(p.contains("'helper' is a subagent skill"), "{p}");
    assert!(p.contains("SUBAGENT skill"), "{p}");
    let p = problems(operator("~/.agents/skills", &["tdd"]));
    assert!(p.contains("'tdd' has the name of a catalog skill"), "{p}");
    let p = problems(operator("~/.agents/skills", &[]));
    assert!(p.contains("operator_skills names no skill"), "{p}");
    let p = problems(operator(
        "~/.agents/skills",
        &["test-modernizer", "test-modernizer"],
    ));
    assert!(p.contains("'test-modernizer' is named twice"), "{p}");

    // A harness without skills cannot load them.
    let none = Teammate {
        agent: HarnessKind::None,
        model: None,
        skills: Vec::new(),
        phase: None,
        operator_skills: operator("~/.agents/skills", &["test-modernizer"]),
        ..probe()
    };
    let p = check_problems(none, Some(home.path())).join("\n");
    assert!(
        p.contains("cannot load with disabled skills or no agent"),
        "{p}"
    );
}

/// An operator skill is copied into the bundle with its references, is
/// versioned by its tree digest, and is EXPECTED in the briefing with its
/// description. A source that changes between plan and copy fails.
#[test]
fn operator_skills_materialize_with_a_digest_and_an_expected_line() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("exported");
    operator_skill(&dir, "test-modernizer", "Modernize tests.");
    let t = Teammate {
        name: "swifty".into(),
        skills: vec!["tdd".into()],
        operator_skills: operator(dir.to_str().unwrap(), &["test-modernizer"]),
        ..Teammate::default()
    };
    let state = tmp.path().join("state");
    let bundle = skills::Bundle::install(&state, &t).unwrap().unwrap();
    let plan = bundle.plan();
    let modernizer = plan
        .activated
        .iter()
        .find(|r| r.id.as_str() == "test-modernizer")
        .unwrap();
    assert_eq!(modernizer.policy, InvocationPolicy::Explicit);
    assert!(
        modernizer.version.0.starts_with("operator+"),
        "{modernizer:?}"
    );
    assert!(modernizer.source.starts_with("operator:"), "{modernizer:?}");
    let copied = bundle.skills_dir().join("test-modernizer");
    assert!(copied.join("SKILL.md").is_file());
    assert!(copied.join("references/notes.md").is_file());
    assert_eq!(
        horch_marketplace::integrity::tree_digest(&copied).unwrap(),
        modernizer.digest.to_string()
    );
    let brief = bundle.briefing_in(&t, None);
    assert!(
        brief.contains("- horch:tdd: ")
            && brief.contains("- horch:test-modernizer: Operator probe skill test-modernizer."),
        "{brief}"
    );
    // The source is never touched.
    assert!(dir.join("test-modernizer/SKILL.md").is_file());

    // The plan pins the digest; a source edited after planning fails.
    let catalog = SkillCatalog::bundled()
        .unwrap()
        .with_operator_skills(&t, None)
        .unwrap();
    let plan = plan_activation(&t, None, &catalog).unwrap();
    std::fs::write(dir.join("test-modernizer/references/notes.md"), "changed\n").unwrap();
    let err = MaterializedSkills::materialize(&plan, &catalog, &state, "changed").unwrap_err();
    assert!(
        format!("{err:#}").contains("the operator directory changed"),
        "{err:#}"
    );

    // Without the extension, the field activates nothing: the catalog
    // decides, so `plan_launch` needs no filesystem.
    let plain = plan_activation(&t, None, &SkillCatalog::bundled().unwrap()).unwrap();
    assert_eq!(plain.activated_ids(), ["tdd"]);
}
