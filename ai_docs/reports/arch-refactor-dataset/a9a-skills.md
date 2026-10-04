# A9 a9a-skills: bundled skills become a versioned catalog; activation plan

Branch `ard/a9a-skills`. Phase A9 (part a). Requirements SKL-01, SKL-02,
SKL-03, SKL-07, SKL-08. SKL-04 (executions record refs) is out of scope
and still has no test; `check-req-coverage.sh --phase A9` reports it.

## What changed

- `crates/horch-core/build.rs`: also emits
  `pub static BUNDLED_PROVENANCE: &str` (`skills/provenance.json`) into
  `bundled_skills.rs`, next to `BUNDLED_SKILL_FILES`.
- `crates/horch-core/src/skills.rs`: new `pub mod` lines and re-exports.
  `phase_skills` and `selected` moved to `selection.rs` and are
  re-exported. The private `catalog()` shim now reads `SkillCatalog`.
  `Bundle` keeps its public API (`install`, `skills_dir`, `configure`,
  `apply_env`, `briefing`) and gains `plan()`. Internally it plans,
  materializes and renders through the new modules. `Bundle` no longer
  has its own `Drop`; its `MaterializedSkills` field removes the directory.
- New `crates/horch-core/src/skills/{catalog,selection,activation,briefing,materialize}.rs`.
- `horch-core` depends on `horch-marketplace`: the exact lines from the
  plan in `crates/horch-core/Cargo.toml`, `scripts/check-deps.sh` and
  `nfr_05` in `crates/horch-core/tests/nfr.rs`.
- `crates/horch-core/tests/skills_catalog.rs`: 7 tests.

## API for A10 and A11

```rust
use horch_core::skills::{
    SkillCatalog, CatalogEntry, CatalogSource, Provenance, SkillVersion,   // catalog.rs
    plan_activation, InvocationPolicy, ResolvedSkillRef, SkillActivationPlan, // activation.rs
    briefing::render, BriefingContext,                                     // briefing.rs
    MaterializedSkills,                                                    // materialize.rs
    phase_skills, selected,                                                // selection.rs
};

SkillCatalog::bundled() -> Result<SkillCatalog>          // validated, versioned, with provenance
catalog.with_lock(&[horch_marketplace::LockEntry]) -> Result<SkillCatalog>
catalog.get(&SkillId) / catalog.lookup(&str) / catalog.entries() / len()

CatalogEntry { id: SkillId, version: SkillVersion /* = horch_marketplace::SkillVersion */,
               source: CatalogSource, digest: Digest /* measure::digest */,
               description: String, provenance: Option<Provenance>,
               skill_file_bytes: usize, files: pub(crate) }
CatalogSource { Bundled, Marketplace { source: String, resolved_commit: Option<String> } }
entry.source_label()  // "bundled" | "<spec>@<commit>" | "<spec>"

plan_activation(&Teammate, Option<Phase>, &SkillCatalog) -> Result<SkillActivationPlan>
SkillActivationPlan { activated: Vec<ResolvedSkillRef> /* sorted by id */,
                      available: Vec<ResolvedSkillRef>,
                      plugin_skills: BTreeMap<String, Vec<String>> }
ResolvedSkillRef { id, version, digest: Digest /* serializes "sha256:<hex>" */, source: String, policy }
InvocationPolicy { Explicit, Deterministic, Available }   // serde snake_case

MaterializedSkills::materialize(&plan, &catalog, state_root, execution_id: &str)
    -> Result<Option<MaterializedSkills>>   // <state_root>/skill-bundles/<execution_id>/skills/<id>/...
materialized.root / materialized.skills_dir(); Drop removes root

briefing::render(&plan, &catalog, &BriefingContext {
    phase, declared: &teammate.skills, namespace: Some("horch") /* Claude */ | None,
    plugin_lines: &[..] /* "- <plugin>:<skill>: <description>" */, skills_dir })
```

## Decisions

- **Lock override rule.** A lock entry whose `source` starts with
  `bundled:` records an install of the compiled-in copy. It never replaces
  the bundled entry. Any other lock entry (git or local) is an explicit
  operator install. It replaces a bundled skill with the same id, and an
  entry with a new id is added. `with_lock` rejects a lock id that fails
  the marketplace SKILL.md name rules, an empty version, and a digest
  that does not parse.
- **Digest.** The rule is the marketplace tree digest, computed in memory
  over the compiled-in files of `skills/<id>/`. `skl_01` checks it against
  `horch_marketplace::integrity::tree_digest`. Version is
  `bundled+<digest12>`.
- **Provenance.** Per-skill `source_repository`/`source_revision`
  override the file-level values (`skill-creator` names
  `anthropics/claude-plugins-official`). A skill with
  `source_path: null` (`orchestrate`, original to this repository) has
  `provenance: None`.
- **Design over plan.** The names follow the design §4.9: `SkillCatalog`,
  `activated`, a `plugin_skills` field in the plan, `source` on
  `ResolvedSkillRef`, and `Option` from `materialize`. `plan_activation`
  takes 3 arguments, as the plan says. The design's 4th `lock: &LockView`
  argument does not exist anywhere, and `with_lock` already merges the
  lock into the catalog.
- **Explicit order.** `activated` is sorted by id. The briefing lists
  expected skills in the teammate's declared order (`backend-developer`
  declares `tdd` before `security-review`). So `BriefingContext.declared`
  carries that order.
- **Plugin skills (SKL-07).** `plan.plugin_skills` copies
  `teammate.plugin_skills` unchanged. These skills never enter
  `activated`/`available`, and a plugin skill named like a bundled skill
  does not activate it. Plugin descriptions need the operator's installed
  plugins, which means I/O. So `Bundle::briefing` resolves them, and only
  for Claude, as before. It passes the lines to `render` as
  `plugin_lines`.
- **Validation.** One copy of the SKILL.md rules exists, in `catalog.rs`
  (`validate_skill_md`). The name and loadability checks exist once, in
  `selection::check`. Both `selected` and `plan_activation` use them, so
  error messages stay identical.

## Gotchas for A10 and A11

- `materialize` refuses `CatalogSource::Marketplace` entries. It writes
  only compiled-in files. A10/A11 must copy from the marketplace store
  `skills/<id>/<version>/`. Validate `version` before you build a path
  from it: `with_lock` checks only that it is not empty.
- A marketplace `CatalogEntry` has an empty `description` and
  `skill_file_bytes: 0`, because the lock does not carry them. Read the
  installed SKILL.md if the briefing or `horch skills` needs them.
- `.claude-plugin/plugin.json` is still written by `Bundle::install`, not
  by `materialize`: it is Claude exposure (A10).
- `selection.rs` still checks `Agent::None` (the legacy rule). A10's
  `skl_05` scan forbids `HarnessKind` in `skills/**`, and the
  `namespace` field already keeps `briefing.rs` harness-free.
- `ExecutionId` accepts `/` and `..` (`validate_plain`). `materialize`
  rejects an empty id, `.`, `..`, `/`, `\` and NUL itself.
- `Bundle::install` still names the directory with `mint_uuid()`. A6/A10
  pass the real execution id.

## Security review (horch:security-review, focused)

Scope: `skills/catalog.rs` and `skills/materialize.rs` at `eccbbeb`. Trust
boundaries: `marketplace.lock` entries (operator-writable file) into
`with_lock`, and the execution id and state root into filesystem paths.
I did a source review only and ran no scanner.

- DISMISSED: path traversal through a lock id. `with_lock` applies
  `horch_marketplace::SkillId::parse` (`[a-z0-9-]` rules) before it
  accepts the id. Test: `skl_01_lock_entries_merge_by_the_override_rule`
  (`../escape` is rejected).
- DISMISSED: path traversal through the execution id. `materialize` rejects
  path-like ids. Test: `skl_08_materialize_rejects_path_like_execution_ids`.
- DISMISSED: deletion of foreign files on drop. `create_dir` fails on an
  existing directory or symlink before `MaterializedSkills` exists, so
  `Drop` only removes a directory that this value created.
- UNVERIFIED, low: on Windows, an execution id such as `C:x` could change
  the join prefix. Real ids are UUIDs. I did not test this on Windows.
- Note for A10: the lock `version` is not path-validated (see gotchas).

## Tests (7, all in `crates/horch-core/tests/skills_catalog.rs`)

`skl_01_bundled_catalog_versions_and_digests`,
`skl_01_lock_entries_merge_by_the_override_rule`,
`skl_02_activation_matches_legacy_selection` (every teammate × 5 phases,
including none, with Ok/Err parity), `skl_03_policy_mapping`,
`skl_07_plugin_skills_separate`,
`skl_08_briefing_matches_baseline_modulo_path` (all 132 A0 oracle files;
a mutation of the render text makes it fail),
`skl_08_materialize_rejects_path_like_execution_ids`.
`oracle_skills_match` and `oracle_cli_skills_match` pass unchanged.
