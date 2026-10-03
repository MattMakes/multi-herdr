//! A9: the versioned skill catalog and the activation plan (SKL-01, SKL-02,
//! SKL-03, SKL-07, SKL-08).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use horch_core::ids::SkillId;
use horch_core::skills::{
    self, briefing, plan_activation, BriefingContext, CatalogSource, InvocationPolicy,
    MaterializedSkills, SkillCatalog, BUNDLED_SKILL_FILES,
};
use horch_core::teammates::{Agent, Phase, Roster, Teammate};
use horch_marketplace::LockEntry;

const PINNED_REPOSITORY: &str = "https://github.com/MattMakes/skill-marketplace";
const PINNED_COMMIT: &str = "d47670328c59a3311a9b4149bc5f8f33f0a92754";

const PHASES: [Option<Phase>; 5] = [
    None,
    Some(Phase::Research),
    Some(Phase::Plan),
    Some(Phase::Implementation),
    Some(Phase::Validation),
];

fn repo_roster() -> Roster {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../teammates");
    Roster::load_with(Some(dir.to_str().unwrap())).unwrap()
}

fn oracles() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/oracles/skills")
}

/// Teammates added after the A0 freeze. They have no oracle file and the
/// oracle loop skips them; a missing file for any other teammate still
/// fails. Never give one of these an oracle file.
const SKIP_NEW_TEAMMATES: &[&str] = &["judge"];

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
    assert_eq!(catalog.len(), 16);
    assert_eq!(catalog, SkillCatalog::bundled().unwrap(), "stable digests");

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

        // `orchestrate` is original to this repository; `skill-creator`
        // names its own upstream; the rest come from the pinned marketplace.
        match id {
            "orchestrate" => assert_eq!(entry.provenance, None),
            _ => {
                let p = entry.provenance.as_ref().unwrap_or_else(|| panic!("{id}"));
                assert_eq!(p.source_sha256.len(), 64, "{id}");
                assert!(is_hex(&p.source_sha256), "{id}");
                assert!(p.source_path.ends_with("SKILL.md"), "{id}");
                if id != "skill-creator" {
                    assert_eq!(p.repository, PINNED_REPOSITORY, "{id}");
                    assert_eq!(p.revision, PINNED_COMMIT, "{id}");
                }
            }
        }
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
        agent: Agent::Claude,
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
                                    namespace: (t.agent == Agent::Claude).then_some("horch"),
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
