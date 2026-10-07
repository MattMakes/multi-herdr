//! A9: the versioned skill catalog and the activation plan (SKL-01, SKL-02,
//! SKL-03, SKL-07, SKL-08, SKL-11).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use horch_core::harness::HarnessKind;
use horch_core::ids::SkillId;
use horch_core::roster::{Phase, Roster, Teammate};
use horch_core::skills::catalog::parse_copied;
use horch_core::skills::{
    self, briefing, plan_activation, BriefingContext, CatalogSource, InvocationPolicy,
    MaterializedSkills, SkillCatalog, BUNDLED_SKILL_EXECUTABLES, BUNDLED_SKILL_FILES,
};
use horch_marketplace::LockEntry;

/// Skills written in this repository: their `copied.json` entry lists no
/// copied files.
const REPO_ORIGINAL: &[&str] = &[
    "api-contracts",
    "architecture-review",
    "blender-baking",
    "blender-modeling",
    "blender-rigging",
    "blender-ue-pipeline",
    "data-migrations",
    "godot-build-verify",
    "godot-combat-system",
    "godot-dimension-port",
    "godot-economy-system",
    "godot-gameplay-loops",
    "godot-genre-blueprints",
    "godot-language-choice",
    "godot-project-context",
    "godot-quest-system",
    "godot-scene-files",
    "godot-version-migration",
    "orchestrate",
    "product-requirements",
    "system-design",
    "ue-build-verify",
    "ue-editor-scripting",
];

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
    "app-release-preparer",
    "apple-accessibility-auditor",
    "apple-platform-developer",
    "blender-artist",
    "codex-swift-reviewer",
    "design-critic",
    "design-director",
    "design-system-engineer",
    "godot-ai-programmer",
    "godot-animator",
    "godot-code-reviewer",
    "godot-csharp-engineer",
    "godot-gameplay-programmer",
    "godot-narrative-programmer",
    "godot-native-engineer",
    "godot-network-engineer",
    "godot-performance-engineer",
    "godot-porting-engineer",
    "godot-qa-engineer",
    "godot-release-engineer",
    "godot-systems-programmer",
    "godot-tech-lead",
    "godot-technical-artist",
    "godot-tools-engineer",
    "godot-ui-developer",
    "godot-world-builder",
    "godot-xr-developer",
    "judge",
    "landing-page-builder",
    "motion-engineer",
    "swift-developer",
    "swift-qa-engineer",
    "swift-reviewer",
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

        // The digest is the marketplace tree digest of the skill directory
        // when no file is executable; an execute bit changes it (SKL-11).
        if BUNDLED_SKILL_EXECUTABLES
            .iter()
            .any(|p| p.starts_with(&format!("{id}/")))
        {
            continue;
        }
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

        // A repo-original skill has an entry with no copied files; every
        // other skill names at least one.
        let c = entry.copied.as_ref().unwrap_or_else(|| panic!("{id}"));
        if REPO_ORIGINAL.contains(&id) {
            assert!(c.copied_files.is_empty(), "{id}");
            assert!(!c.verbatim, "{id}");
        } else {
            assert!(!c.copied_files.is_empty(), "{id}");
        }
    }
}

/// `copied.json` parses one entry per skill; a path outside the skill
/// directory, an unknown field and a duplicate name fail.
#[test]
fn skills_copied_parses() {
    let text = r#"{
      "skills": [
        {"name": "copy", "copied_files": ["SKILL.md", "references/a.md"], "verbatim": true},
        {"name": "part", "copied_files": ["SKILL.md"]},
        {"name": "own", "copied_files": []}
      ]
    }"#;
    let parsed = parse_copied(text).unwrap();
    assert_eq!(parsed["copy"].copied_files, ["SKILL.md", "references/a.md"]);
    assert!(parsed["copy"].verbatim);
    assert!(!parsed["part"].verbatim, "verbatim defaults to false");
    assert!(parsed["own"].copied_files.is_empty());

    for bad in ["../x.md", "/abs.md", ""] {
        let text = text.replace("references/a.md", bad);
        assert!(parse_copied(&text).is_err(), "{bad:?}");
    }
    let unknown = text.replace(r#""verbatim": true"#, r#""verbatim": true, "extra": 1"#);
    assert!(parse_copied(&unknown).is_err());
    let dup = text.replace(r#""name": "own""#, r#""name": "part""#);
    assert!(parse_copied(&dup).is_err());

    // U-49: budget_exempt carries a reason, and only on a skill that is not
    // verbatim (a verbatim skill is already exempt).
    assert_eq!(parsed["part"].budget_exempt, None);
    let part = r#"{"name": "part", "copied_files": ["SKILL.md"]}"#;
    let exempt = |field: &str| text.replace(part, &part.replace('}', &format!(", {field}}}")));
    let ok = parse_copied(&exempt(r#""budget_exempt": "combined upstream base""#)).unwrap();
    assert_eq!(
        ok["part"].budget_exempt.as_deref(),
        Some("combined upstream base")
    );
    assert!(parse_copied(&exempt(r#""budget_exempt": " ""#)).is_err());
    let both = text.replace(
        r#""verbatim": true"#,
        r#""verbatim": true, "budget_exempt": "x""#,
    );
    assert!(parse_copied(&both).is_err());
}

/// U-49: `verbatim: true` means an unchanged copy of the whole directory, so
/// `copied_files` lists every file of a verbatim skill.
#[test]
fn skills_verbatim_lists_every_file() {
    let catalog = SkillCatalog::bundled().unwrap();
    let mut verbatim = 0;
    for entry in catalog.entries() {
        let Some(c) = entry.copied.as_ref().filter(|c| c.verbatim) else {
            continue;
        };
        verbatim += 1;
        let prefix = format!("{}/", entry.id);
        for (path, _) in BUNDLED_SKILL_FILES {
            if let Some(rel) = path.strip_prefix(&prefix) {
                assert!(
                    c.copied_files.iter().any(|f| f == rel),
                    "{path}: verbatim skill holds a file that is not in copied_files"
                );
            }
        }
    }
    assert!(verbatim > 0);
}

/// A verbatim skill (`"verbatim": true` in copied.json) is an unchanged
/// copy and is exempt from the size budget. So is a skill whose entry gives a
/// `budget_exempt` reason. Any other skill meets it.
const MAX_SKILL_MD_BYTES: usize = 12 * 1024;
const MAX_SKILL_DIR_BYTES: usize = 160 * 1024;

/// Skill ids exempt from the size budget: every skill whose copied.json
/// entry says `verbatim: true` or gives a `budget_exempt` reason.
///
/// A renamed or combined Godot skill (`godot-*`, Godot wave GW3-GW7) keeps
/// its copied `SKILL.md` at upstream length, so its entry gives a
/// `budget_exempt` reason (U-49). It is not verbatim.
fn budget_exempt() -> BTreeSet<String> {
    let catalog = SkillCatalog::bundled().unwrap();
    catalog
        .entries()
        .filter(|e| {
            e.copied
                .as_ref()
                .is_some_and(|c| c.verbatim || c.budget_exempt.is_some())
        })
        .map(|e| e.id.to_string())
        .collect()
}

/// `(SKILL.md bytes, directory bytes)` of every bundled skill.
fn skill_sizes() -> BTreeMap<&'static str, (usize, usize)> {
    let mut sizes: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for (path, bytes) in BUNDLED_SKILL_FILES {
        let Some((id, rel)) = path.split_once('/') else {
            continue;
        };
        let size = sizes.entry(id).or_default();
        size.1 += bytes.len();
        if rel == "SKILL.md" {
            size.0 = bytes.len();
        }
    }
    sizes
}

/// U-49: a `budget_exempt` reason is for a skill over the budget. When a
/// refresh brings the skill under it, the stale reason fails here.
#[test]
fn skills_budget_exempt_only_over_budget() {
    let catalog = SkillCatalog::bundled().unwrap();
    let sizes = skill_sizes();
    for entry in catalog.entries() {
        if entry
            .copied
            .as_ref()
            .is_some_and(|c| c.budget_exempt.is_some())
        {
            let (md, dir) = sizes[entry.id.as_str()];
            assert!(
                md > MAX_SKILL_MD_BYTES || dir > MAX_SKILL_DIR_BYTES,
                "{}: within the budget ({md} / {dir} bytes); drop budget_exempt",
                entry.id
            );
        }
    }
}

#[test]
fn skills_bundled_size_budget() {
    let exempt = budget_exempt();
    let mut dirs: BTreeMap<&str, usize> = BTreeMap::new();
    for (path, bytes) in BUNDLED_SKILL_FILES {
        // Top-level files (README.md, copied.json) belong to no skill.
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

/// `skill-creator` is a verbatim copy that ships Python scripts and HTML
/// assets. It is the only skill exempt from the text-only rule.
const EXEMPT_FROM_TEXT_ONLY: &[&str] = &["skill-creator"];

/// Single files exempt from the text-only rule; the rest of their skill is
/// still checked. `godot-run.sh` is the fleet wrapper for every Godot call,
/// and SKL-11 keeps its execute bit in a bundle.
const EXEMPT_FILES: &[&str] = &["godot-build-verify/scripts/godot-run.sh"];

/// Skill files are `.md` or `.txt`, except the skills in
/// `EXEMPT_FROM_TEXT_ONLY` and the single files in `EXEMPT_FILES`. Verbatim
/// skills are not exempt from this rule.
#[test]
fn skills_bundled_text_only() {
    for (path, _) in BUNDLED_SKILL_FILES {
        let Some((id, _)) = path.split_once('/') else {
            continue;
        };
        if EXEMPT_FROM_TEXT_ONLY.contains(&id) || EXEMPT_FILES.contains(path) {
            continue;
        }
        assert!(path.ends_with(".md") || path.ends_with(".txt"), "{path}");
    }
}

/// No bundled skill ships a licence or notice file (operator rule,
/// 2026-10-04: the copies are personal and carry no licence or credit).
#[test]
fn skills_bundled_no_licence_files() {
    for (path, _) in BUNDLED_SKILL_FILES {
        let name = path.rsplit('/').next().unwrap().to_ascii_uppercase();
        assert!(
            !["LICENSE", "LICENCE", "NOTICE", "COPYING"]
                .iter()
                .any(|p| name.starts_with(p)),
            "{path}"
        );
    }
}

/// `skills/copied.json` lists its skills sorted by name, so a merge that
/// appends an entry fails here instead of drifting.
#[test]
fn skills_copied_sorted_by_name() {
    let raw = std::fs::read_to_string(skills_dir().join("copied.json")).unwrap();
    let doc: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let names: Vec<&str> = doc["skills"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap())
        .collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(names, sorted, "copied.json skills are not sorted by name");
}

/// Every skill table in `skills/README.md` is sorted by its first column,
/// except the process-order "Source mapping" table, and every bundled skill
/// has exactly one row across the tables.
#[test]
fn skills_readme_tables_sorted_one_row_per_skill() {
    let readme = std::fs::read_to_string(skills_dir().join("README.md")).unwrap();
    let mut section = "";
    let mut table: Vec<&str> = Vec::new();
    let mut all: Vec<&str> = Vec::new();
    let check = |section: &str, table: &mut Vec<&str>| {
        if section != "Source mapping" {
            let mut sorted = table.clone();
            sorted.sort_unstable();
            assert_eq!(
                *table, sorted,
                "README table under '{section}' is not sorted"
            );
        }
        table.clear();
    };
    for line in readme.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            check(section, &mut table);
            section = h;
        }
        // A skill row starts with `| [name](name/SKILL.md)`.
        let name = line
            .strip_prefix("| [")
            .and_then(|r| r.split_once("](").map(|(n, _)| n))
            .filter(|n| line.contains(&format!("]({n}/SKILL.md)")));
        match name {
            Some(n) => {
                table.push(n);
                all.push(n);
            }
            None => check(section, &mut table),
        }
    }
    check(section, &mut table);

    let on_disk: BTreeSet<String> = std::fs::read_dir(skills_dir())
        .unwrap()
        .map(|e| e.unwrap())
        .filter(|e| e.path().join("SKILL.md").is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    let mut listed: Vec<String> = all.iter().map(|s| s.to_string()).collect();
    listed.sort();
    let unique: BTreeSet<String> = listed.iter().cloned().collect();
    assert_eq!(
        listed.len(),
        unique.len(),
        "a skill has more than one README row"
    );
    assert_eq!(unique, on_disk, "README rows and bundled skills differ");
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

/// `SkillCatalog::bundled` is built once and cloned: 2 calls give equal
/// catalogs.
#[test]
fn bundled_catalog_is_the_same_on_every_call() {
    let first = SkillCatalog::bundled().unwrap();
    let second = SkillCatalog::bundled().unwrap();
    assert!(!first.is_empty());
    assert_eq!(first, second);
}

/// A catalog extended for a teammate holds its named plugin skills as
/// `<plugin>:<skill>`, and the plan activates them (Explicit), so the
/// ledger records them (SKL-04). An unnamed plugin skill is in neither
/// list, and a plugin skill named like a bundled one still does not
/// activate the bundled one (SKL-07).
#[test]
fn skl_07_named_plugin_skills_enter_the_plan_as_plugin_refs() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("code");
    std::fs::create_dir_all(root.join(".claude-plugin")).unwrap();
    std::fs::write(
        root.join(".claude-plugin/plugin.json"),
        r#"{"name":"code"}"#,
    )
    .unwrap();
    for skill in ["review", "tdd", "lint"] {
        let dir = root.join("skills").join(skill);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {skill}\ndescription: Plugin {skill}.\n---\n"),
        )
        .unwrap();
    }
    let t = Teammate {
        name: "plugged".into(),
        agent: HarnessKind::Claude,
        phase: Some(Phase::Plan),
        plugin_dirs: vec![root.to_string_lossy().into_owned()],
        plugin_skills: [(
            "code".to_string(),
            vec!["review".to_string(), "tdd".to_string()],
        )]
        .into(),
        ..Teammate::default()
    };
    let bundled = SkillCatalog::bundled().unwrap();
    let catalog = bundled.clone().with_host_skills(&t, None).unwrap();
    assert_eq!(catalog.len(), bundled.len() + 2);
    let entry = catalog.lookup("code:review").unwrap();
    assert!(entry.is_plugin());
    assert_eq!(entry.description, "Plugin review.");
    let digest = horch_marketplace::integrity::tree_digest(&root.join("skills/review")).unwrap();
    assert_eq!(entry.digest.to_string(), digest);
    // No plugin version: the label says plugin.
    assert_eq!(
        entry.version.0,
        format!("plugin+{}", &digest["sha256:".len()..][..12])
    );

    let plan = plan_activation(&t, t.phase, &catalog).unwrap();
    let review = plan
        .activated
        .iter()
        .find(|r| r.id.as_str() == "code:review")
        .unwrap();
    assert_eq!(review.policy, InvocationPolicy::Explicit);
    assert_eq!(review.source, "plugin:code@inline");
    assert_eq!(
        plan.activated_ids(),
        [
            "code:review",
            "code:tdd",
            "create-plan",
            "handoff",
            "pre-flight"
        ]
    );
    let all: Vec<&str> = plan
        .activated
        .iter()
        .chain(&plan.available)
        .map(|r| r.id.as_str())
        .collect();
    assert!(!all.contains(&"code:lint"), "{all:?}");
    let tdd = plan.available.iter().find(|r| r.id.as_str() == "tdd");
    assert_eq!(tdd.unwrap().policy, InvocationPolicy::Available);

    // An agent that does not load skills as plugins, and a teammate with
    // skills off, add nothing.
    for other in [
        Teammate {
            agent: HarnessKind::Codex,
            ..t.clone()
        },
        Teammate {
            disable_skills: true,
            ..t.clone()
        },
    ] {
        let c = bundled.clone().with_host_skills(&other, None).unwrap();
        assert_eq!(c.len(), bundled.len());
    }
    // A plugin that does not resolve fails the extension, as the launch would.
    let missing = Teammate {
        plugin_dirs: Vec::new(),
        ..t.clone()
    };
    let err = format!(
        "{:#}",
        bundled
            .clone()
            .with_host_skills(&missing, None)
            .unwrap_err()
    );
    assert!(err.contains("plugin 'code' is neither"), "{err}");
}

/// `text` without the SKL-10 sentence that names skills outside the
/// bundle: a sanctioned change after the baselines were frozen.
fn without_outside_sentence(text: &str) -> String {
    const START: &str = " Your skills name ";
    const END: &str = " only when your step needs it.";
    match text.find(START) {
        None => text.to_string(),
        Some(i) => {
            let end = i + text[i..].find(END).expect("the sentence ends") + END.len();
            format!("{}{}", &text[..i], &text[end..])
        }
    }
}

/// The briefing equals the frozen baseline with the bundle path replaced,
/// and with the SKL-10 sentence (skills named outside the bundle) removed
/// from both sides: that sentence is a sanctioned change. The baselines
/// share `tests/oracles/skills/` with `oracle_skills_match`, whose bless
/// writes the sentence in; every other byte stays frozen.
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
                            without_outside_sentence(
                                &text.replace(&*root.to_string_lossy(), "<BUNDLE>"),
                            )
                        }
                    }
                }
            };
            let path = oracles().join(format!("{name}-{phase}.txt"));
            let want = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert_eq!(text, without_outside_sentence(&want), "{name}-{phase}");
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

/// A materialized bundle writes an executable skill file with mode 755
/// and any other file without an execute bit.
#[cfg(unix)]
#[test]
fn skl_11_materialized_bundle_keeps_the_execute_bit() {
    use std::os::unix::fs::PermissionsExt as _;
    let catalog = SkillCatalog::bundled().unwrap();
    let t = Teammate {
        name: "exec".into(),
        available_skills: vec!["godot-build-verify".into()],
        phase: Some(Phase::Implementation),
        ..Teammate::default()
    };
    let plan = plan_activation(&t, t.phase, &catalog).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let files = MaterializedSkills::materialize(&plan, &catalog, tmp.path(), "exec")
        .unwrap()
        .unwrap();
    let dir = files.skills_dir().join("godot-build-verify");
    let mode = |rel: &str| {
        std::fs::metadata(dir.join(rel))
            .unwrap()
            .permissions()
            .mode()
            & 0o777
    };
    assert_eq!(mode("scripts/godot-run.sh"), 0o755);
    assert_eq!(mode("SKILL.md") & 0o111, 0);
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
    // A host without the export is not an error: the launch skips the
    // skill, and `--check` warns (see the next test).
    assert_eq!(
        problems(operator("~/no-such-dir", &["test-modernizer"])),
        ""
    );
    assert_eq!(problems(operator("~/.agents/skills", &["missing"])), "");
    // The name rules hold on every host, even where the skill is missing.
    let p = problems(operator("~/no-such-dir", &["tdd"]));
    assert!(p.contains("'tdd' has the name of a catalog skill"), "{p}");
    let p = problems(operator("~/no-such-dir", &["device-interaction"]));
    assert!(
        p.contains("'device-interaction' is a subagent skill"),
        "{p}"
    );
    // A skill directory without a SKILL.md is broken, not missing.
    std::fs::create_dir_all(dir.join("empty")).unwrap();
    let p = problems(operator("~/.agents/skills", &["empty"]));
    assert!(
        p.contains("operator skill 'empty' in '~/.agents/skills' has no readable"),
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

fn check_warnings(t: Teammate, home: Option<&Path>) -> Vec<String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../teammates");
    let mut roster = Roster::load_layered(home, None, Some(dir.to_str().unwrap())).unwrap();
    let who = t.name.clone();
    roster.insert_for_test(t);
    horch_core::roster::validation::operator_skill_warnings(&roster)
        .into_iter()
        .filter(|w| w.starts_with(&format!("{who}: ")))
        .collect()
}

/// A missing directory or a missing name is a warning: each skill that the
/// host lacks gets one line that says how to export it. A present skill
/// gets none.
#[test]
fn operator_skills_missing_on_this_host_warn() {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join(".agents/skills");
    operator_skill(&dir, "test-modernizer", "Modernize tests.");
    let with = |o| Teammate {
        operator_skills: o,
        ..probe()
    };
    let warnings = |o| check_warnings(with(o), Some(home.path()));

    assert_eq!(
        warnings(operator("~/.agents/skills", &["test-modernizer"])),
        Vec::<String>::new()
    );
    assert_eq!(
        warnings(operator(
            "~/.agents/skills",
            &["test-modernizer", "swiftui-whats-new-27"]
        )),
        [
            "probe: operator skill swiftui-whats-new-27 is not installed on this host \
          (no ~/.agents/skills/swiftui-whats-new-27): ask the operator to run \
          `xcrun agent skills export --output-dir <dir>` (Xcode 27 or later)"
        ]
    );
    let w = warnings(operator("~/no-such-dir", &["a-skill", "b-skill"]));
    assert_eq!(w.len(), 2, "{w:?}");
    assert!(
        w[0].contains("operator skill a-skill is not installed"),
        "{w:?}"
    );
    assert!(w[1].contains("(no ~/no-such-dir/b-skill)"), "{w:?}");
}

/// A directory that exists but cannot be read is an error, not a skip.
#[cfg(unix)]
#[test]
fn operator_skills_unreadable_dir_is_an_error() {
    use std::os::unix::fs::PermissionsExt;
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join(".agents/skills");
    operator_skill(&dir, "test-modernizer", "Modernize tests.");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o000)).unwrap();
    // Root reads anything; then there is nothing to test.
    let readable = std::fs::read_dir(&dir).is_ok();
    let t = Teammate {
        operator_skills: operator("~/.agents/skills", &["test-modernizer"]),
        ..probe()
    };
    let p = check_problems(t, Some(home.path())).join("\n");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    if readable {
        return;
    }
    assert!(
        p.contains("operator_skills dir '~/.agents/skills' cannot be read"),
        "{p}"
    );
}

/// The launch goes on without a missing operator skill. The bundle holds
/// the rest, and the briefing names the skipped skill and the fix.
#[test]
fn operator_skills_missing_on_this_host_are_skipped_with_a_briefing_note() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("exported");
    operator_skill(&dir, "test-modernizer", "Modernize tests.");
    let dir_text = dir.to_str().unwrap();
    let t = Teammate {
        name: "swifty".into(),
        skills: vec!["tdd".into()],
        operator_skills: operator(dir_text, &["swiftui-whats-new-27", "test-modernizer"]),
        ..Teammate::default()
    };
    let state = tmp.path().join("state");
    let bundle = skills::Bundle::install(&state, &t).unwrap().unwrap();
    assert_eq!(bundle.plan().activated_ids(), ["tdd", "test-modernizer"]);
    assert!(!bundle.skills_dir().join("swiftui-whats-new-27").exists());
    let brief = bundle.briefing_in(&t, None);
    assert!(
        brief.contains("- horch:test-modernizer: Operator probe skill test-modernizer."),
        "{brief}"
    );
    assert!(!brief.contains("- horch:swiftui-whats-new-27"), "{brief}");
    assert!(
        brief.contains(&format!(
            "Skipped: operator skill swiftui-whats-new-27 is not installed on this host \
             (no {dir_text}/swiftui-whats-new-27): ask the operator to run \
             `xcrun agent skills export --output-dir <dir>` (Xcode 27 or later)."
        )),
        "{brief}"
    );

    // A missing directory skips every name; the launch still succeeds.
    let gone = Teammate {
        operator_skills: operator(&format!("{dir_text}-gone"), &["test-modernizer"]),
        ..t.clone()
    };
    let bundle = skills::Bundle::install(&state, &gone).unwrap().unwrap();
    assert_eq!(bundle.plan().activated_ids(), ["tdd"]);
    assert!(bundle
        .briefing_in(&gone, None)
        .contains("Skipped: operator skill test-modernizer is not installed"),);
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
        .with_host_skills(&t, None)
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

// ─── skills named outside the bundle (SKL-10) ──────────────────────────────

/// The briefing of a teammate whose only skill is `skill`, with no phase.
fn briefing_of(skill: &str) -> String {
    let catalog = SkillCatalog::bundled().unwrap();
    let t = Teammate {
        name: "named".into(),
        skills: vec![skill.into()],
        ..Teammate::default()
    };
    let plan = plan_activation(&t, None, &catalog).unwrap();
    briefing::render(
        &plan,
        &catalog,
        &BriefingContext {
            phase: None,
            declared: &t.skills,
            namespace: None,
            plugin_lines: &[],
            skills_dir: Path::new("/bundle/skills"),
        },
    )
}

/// SKL-10: a skill text that names catalog skills outside the bundle gets 1
/// sentence: the distinct count, and the 3 most-named ids, ties in name
/// order. `godot-save-load` names 4, once each.
#[test]
fn skl_10_briefing_names_skills_outside_the_bundle() {
    let text = briefing_of("godot-save-load");
    let want = " Your skills name 4 skills that are not in this bundle, for example \
                godot-inventory-system, godot-popochiu, godot-project-setup. Read one with \
                horch skills read <id> only when your step needs it. Report unresolved \
                dependencies through horch tell orchestrator.\n";
    assert!(text.ends_with(want), "{text}");
    let catalog = SkillCatalog::bundled().unwrap();
    assert_eq!(
        skills::reading::outside_bundle(&catalog, &["godot-save-load"]),
        [
            ("godot-inventory-system".to_string(), 1),
            ("godot-popochiu".to_string(), 1),
            ("godot-project-setup".to_string(), 1),
            ("godot-resource-pattern".to_string(), 1),
        ]
    );
}

/// SKL-10: `tdd` names no other catalog skill, so its briefing has no
/// sentence.
#[test]
fn skl_10_briefing_has_no_sentence_without_outside_names() {
    let text = briefing_of("tdd");
    assert!(!text.contains("not in this bundle"), "{text}");
    assert!(!text.contains("horch skills read"), "{text}");
}

fn mention_counts(text: &str, ids: &[&str]) -> BTreeMap<String, usize> {
    let ids: BTreeSet<&str> = ids.iter().copied().collect();
    skills::reading::mentions([text.as_bytes()], &ids)
}

/// SKL-10: an id matches a whole word only: `code-review` is not named in
/// `godot-code-review`, nor `godot-ui` in `godot-ui-theming`.
#[test]
fn skl_10_whole_word_match_only() {
    let ids = ["code-review", "godot-code-review", "godot-ui"];
    let got = mention_counts(
        "Run godot-code-review, then godot-ui-theming and Xgodot-ui. See godot-ui.",
        &ids,
    );
    assert_eq!(
        got,
        BTreeMap::from([("godot-code-review".into(), 1), ("godot-ui".into(), 1)])
    );
    assert_eq!(
        mention_counts("code-review (see ../code-review/SKILL.md)", &ids),
        BTreeMap::from([("code-review".into(), 2)])
    );
}

/// SKL-10: an id without a hyphen counts in backticks.
#[test]
fn skl_10_plain_id_counts_in_backticks() {
    assert_eq!(
        mention_counts("Use `tdd` first.", &["tdd"]),
        BTreeMap::from([("tdd".into(), 1)])
    );
}

/// SKL-10: an id without a hyphen counts in a link path.
#[test]
fn skl_10_plain_id_counts_in_a_link_path() {
    assert_eq!(
        mention_counts("See [it](../tdd/SKILL.md).", &["tdd"]),
        BTreeMap::from([("tdd".into(), 1)])
    );
}

/// SKL-10: an id without a hyphen counts when a namespace qualifies it.
#[test]
fn skl_10_plain_id_counts_qualified() {
    assert_eq!(
        mention_counts("Load horch:tdd now.", &["tdd"]),
        BTreeMap::from([("tdd".into(), 1)])
    );
}

/// SKL-10: an id without a hyphen does not count in plain prose.
#[test]
fn skl_10_plain_id_in_prose_does_not_count() {
    assert!(mention_counts(
        "check the build, then check: it passes. A check/ path, `check it`.",
        &["check"]
    )
    .is_empty());
}
