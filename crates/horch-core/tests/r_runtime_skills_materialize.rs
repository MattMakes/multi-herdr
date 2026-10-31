//! `MaterializedSkills::materialize`: each phase's failure text, and the
//! files a bundled and an operator skill leave in the bundle.

use std::path::Path;

use horch_core::roster::{OperatorSkills, Teammate};
use horch_core::skills::{
    MaterializedSkills, SkillActivationPlan, SkillCatalog, SkillResolution, SupportCheck,
};
use horch_marketplace::LockEntry;

const ANY_AGENT: SupportCheck = |_| Ok(());

fn teammate(skills: &[&str], operator: Option<(&Path, &[&str])>) -> Teammate {
    Teammate {
        name: "probe".into(),
        skills: skills.iter().map(|s| (*s).to_owned()).collect(),
        operator_skills: operator.map(|(dir, names)| OperatorSkills {
            dir: dir.display().to_string(),
            names: names.iter().map(|n| (*n).to_owned()).collect(),
        }),
        ..Teammate::default()
    }
}

fn resolve(base: SkillCatalog, t: &Teammate) -> (SkillCatalog, SkillActivationPlan) {
    let resolution = SkillResolution::read(base, Some(t), None, ANY_AGENT).unwrap();
    let plan = resolution.plan(t).unwrap();
    (resolution.into_catalog(), plan)
}

fn write_operator_skill(dir: &Path, name: &str, notes: &str) {
    let skill = dir.join(name);
    std::fs::create_dir_all(skill.join("references")).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: Probe skill {name}.\n---\nBody.\n"),
    )
    .unwrap();
    std::fs::write(skill.join("references/notes.md"), notes).unwrap();
}

fn error_of(
    plan: &SkillActivationPlan,
    catalog: &SkillCatalog,
    state: &Path,
    execution_id: &str,
) -> String {
    MaterializedSkills::materialize(plan, catalog, state, execution_id)
        .expect_err("materialize must fail")
        .to_string()
}

#[test]
fn a_bundled_skill_is_written_under_skills_id() {
    // `tdd` is 1 file; `app-store-changelog` is 2, one in a folder.
    let t = teammate(&["tdd", "app-store-changelog"], None);
    let (catalog, plan) = resolve(SkillCatalog::bundled().unwrap(), &t);
    let state = tempfile::tempdir().unwrap();
    let out = MaterializedSkills::materialize(&plan, &catalog, state.path(), "run-1")
        .unwrap()
        .unwrap();
    let skill_md = std::fs::read_to_string(out.skills_dir().join("tdd/SKILL.md")).unwrap();
    assert!(skill_md.starts_with("---\n"), "{skill_md}");
    assert!(skill_md.contains("name: tdd"), "{skill_md}");
    // The marketplace's own walk of each written folder gives the digest
    // the plan pins, which build.rs took from the files under `skills/`. A
    // file cut short or left out gives another digest.
    assert_eq!(plan.activated.len(), 2);
    for skill in &plan.activated {
        let written = out.skills_dir().join(skill.id.as_str());
        assert_eq!(
            horch_marketplace::integrity::tree_digest(&written).unwrap(),
            skill.digest.to_string(),
            "{}",
            skill.id
        );
    }
}

#[test]
fn an_operator_skill_is_copied_from_its_directory() {
    let ops = tempfile::tempdir().unwrap();
    write_operator_skill(ops.path(), "probe-op", "notes\n");
    let t = teammate(&[], Some((ops.path(), &["probe-op"])));
    let (catalog, plan) = resolve(SkillCatalog::bundled().unwrap(), &t);
    let state = tempfile::tempdir().unwrap();
    let out = MaterializedSkills::materialize(&plan, &catalog, state.path(), "run-1")
        .unwrap()
        .unwrap();
    let copy = out.skills_dir().join("probe-op");
    assert!(std::fs::read_to_string(copy.join("SKILL.md"))
        .unwrap()
        .contains("Probe skill probe-op."));
    assert_eq!(
        std::fs::read_to_string(copy.join("references/notes.md")).unwrap(),
        "notes\n"
    );
}

#[test]
fn a_missing_operator_directory_names_teammates_check() {
    let ops = tempfile::tempdir().unwrap();
    write_operator_skill(ops.path(), "probe-op", "notes\n");
    let t = teammate(&[], Some((ops.path(), &["probe-op"])));
    let (catalog, plan) = resolve(SkillCatalog::bundled().unwrap(), &t);
    std::fs::remove_dir_all(ops.path().join("probe-op")).unwrap();
    let state = tempfile::tempdir().unwrap();
    let msg = error_of(&plan, &catalog, state.path(), "run-1");
    let version = &plan.activated[0].version;
    let missing = ops.path().join("probe-op");
    assert_eq!(
        msg,
        format!(
            "skill 'probe-op' {version}: {} is missing; run `horch teammates --check`",
            missing.display()
        )
    );
}

#[test]
fn an_operator_directory_that_changed_fails_the_digest_check() {
    let ops = tempfile::tempdir().unwrap();
    write_operator_skill(ops.path(), "probe-op", "notes\n");
    let t = teammate(&[], Some((ops.path(), &["probe-op"])));
    let (catalog, plan) = resolve(SkillCatalog::bundled().unwrap(), &t);
    std::fs::write(ops.path().join("probe-op/references/notes.md"), "changed\n").unwrap();
    let state = tempfile::tempdir().unwrap();
    let msg = error_of(&plan, &catalog, state.path(), "run-1");
    let r = &plan.activated[0];
    assert!(
        msg.starts_with(&format!(
            "skill 'probe-op' {}: the copy holds sha256:",
            r.version
        )),
        "{msg}"
    );
    assert!(
        msg.ends_with(&format!(
            ", the plan pins {}; the operator directory changed during the launch",
            r.digest
        )),
        "{msg}"
    );
}

#[test]
fn a_plan_version_the_catalog_lacks_fails() {
    let ops = tempfile::tempdir().unwrap();
    write_operator_skill(ops.path(), "probe-op", "notes\n");
    let t = teammate(&[], Some((ops.path(), &["probe-op"])));
    let (_, old_plan) = resolve(SkillCatalog::bundled().unwrap(), &t);
    write_operator_skill(ops.path(), "probe-op", "different\n");
    let (new_catalog, new_plan) = resolve(SkillCatalog::bundled().unwrap(), &t);
    let state = tempfile::tempdir().unwrap();
    let msg = error_of(&old_plan, &new_catalog, state.path(), "run-1");
    assert_eq!(
        msg,
        format!(
            "skill 'probe-op': the plan has {} but the catalog has {}",
            old_plan.activated[0].version, new_plan.activated[0].version
        )
    );
}

#[test]
fn a_plan_skill_the_catalog_lacks_fails() {
    let ops = tempfile::tempdir().unwrap();
    write_operator_skill(ops.path(), "probe-op", "notes\n");
    let t = teammate(&[], Some((ops.path(), &["probe-op"])));
    let (_, plan) = resolve(SkillCatalog::bundled().unwrap(), &t);
    let state = tempfile::tempdir().unwrap();
    let msg = error_of(
        &plan,
        &SkillCatalog::bundled().unwrap(),
        state.path(),
        "run-1",
    );
    assert_eq!(msg, "skill 'probe-op' is not in the catalog");
}

#[test]
fn a_marketplace_skill_without_a_store_fails() {
    let lock = LockEntry {
        id: "fresh".into(),
        source: "https://example.com/r.git".into(),
        requested_revision: Some("v1".into()),
        resolved_commit: Some("ab".repeat(20)),
        version: "git+abababababab".into(),
        digest: format!("sha256:{}", "cd".repeat(32)),
        installed_at: "2026-10-02T12:00:00Z".into(),
    };
    let base = SkillCatalog::bundled().unwrap().with_lock(&[lock]).unwrap();
    let t = teammate(&["fresh"], None);
    let (catalog, plan) = resolve(base, &t);
    let state = tempfile::tempdir().unwrap();
    let msg = error_of(&plan, &catalog, state.path(), "run-1");
    assert_eq!(
        msg,
        "skill 'fresh' git+abababababab comes from the marketplace store, and this catalog has none"
    );
}
