# U16 a9a-skills: bundled skills become a versioned catalog; activation plan

Unit slug: `a9a-skills`. Branch: `ard/a9a-skills`. Phase: A9 (part a).
Requirements: SKL-01, SKL-02, SKL-03, SKL-07, SKL-08.

## GOAL

Skills have immutable resolved identities: a catalog merges the compiled-in
bundled skills (versioned `bundled+<digest12>`, with upstream provenance) with
marketplace lock entries, and a pure `plan_activation` decides which skills a
teammate in a phase gets, and why. The rendered briefing text is unchanged.
The legacy `Bundle` keeps working for launch (A10 replaces the exposure).

## CONTEXT

- Read first: `ai_docs/plans/arch-refactor-dataset/01-fleet-conventions.md`.
- Read in `00-master-plan.md`: §1 (layout:
  `skills/{catalog,selection,activation,briefing,materialize}.rs`), §2
  ("Skill" row of the concept map), §3 "A9", §4 SKL rows and Spec A §17
  items 9 and 11.
- Design doc: `ai_docs/designs/2026-10-02-architecture-refactor-design.md`,
  key types for skills (`SkillId`, `SkillVersion`, `ResolvedSkillRef`,
  `SkillActivationPlan`, `InvocationPolicy`, `plan_activation`) and the A9
  phase section. The design wins over this plan where they differ.
- Today: `crates/horch-core/src/skills.rs` (625 lines): `phase_skills`,
  private `catalog()` (validates bundled SKILL.md files from
  `BUNDLED_SKILL_FILES`), `selected(teammate)`, `ensure_supported`,
  `describe(phase)` (the `horch skills --json` data), `Bundle` (installs a
  per-launch plugin dir; `configure`, `native_args`, `opencode_config`).
  Callers: `teammates.rs:680`, `codex.rs:394`, `launch.rs:100` and tests,
  `crates/horch/src/main.rs:405`, `cmd/worker.rs:116`, `cmd/spawn.rs:327`,
  `cmd/recipes.rs:578`. `crates/horch-core/build.rs` embeds the skill files.
  `skills/provenance.json` pins upstream `MattMakes/skill-marketplace` at
  `d47670328c59a3311a9b4149bc5f8f33f0a92754` with per-skill `source_sha256`.
- Oracles you must keep green, unchanged: `oracle_skills_match` (briefing
  text per teammate × phase in `crates/horch-core/tests/oracles/skills/`),
  `oracle_cli_skills_match` (`horch skills` output in
  `crates/horch/tests/oracles/skills/`). Read `a0-oracles.md`.
- Marketplace API: `horch_marketplace` lock entries (`a8-marketplace.md`;
  `marketplace.lock` JSON, 7 string fields per entry). `horch-core` needs a
  dependency on `horch-marketplace`. U14 `b2-vcs` adds the same dependency
  in parallel. Add exactly these lines so the two edits are identical:
  `horch-marketplace.workspace = true` in `crates/horch-core/Cargo.toml`
  `[dependencies]` (alphabetical position), `horch-marketplace` appended to
  `allowed_core` in `scripts/check-deps.sh`, and `"horch-marketplace"`
  appended to the core list in `nfr_05` in `crates/horch-core/tests/nfr.rs`.
  If U14 has merged when you rebase, keep its version.
- Parallel units: U10 `a2-runtime` edits 1 env line in `skills.rs` (about
  line 241, `OPENCODE_CONFIG_CONTENT`) and calls in `worker.rs`/`recipes.rs`.
  U11 `a3-roster` moves `Teammate`/`Phase` into `roster/` (old paths keep
  working). Keep your `skills.rs` edits to new `pub mod` lines at the top and
  re-exports; put new logic in the new files.
- Out of scope: recording `skills: [ResolvedSkillRef]` on executions
  (SKL-04, needs A6's execution store) and harness exposure (A10). Leave
  `Bundle` working.

## FILES

own:
- `crates/horch-core/src/skills/{catalog,selection,activation,briefing,materialize}.rs` (new; declared from `skills.rs`)
- `crates/horch-core/src/skills.rs` (add `pub mod` lines; make the old
  functions delegate to the new modules where that keeps output identical)
- `crates/horch-core/build.rs` (also embed `skills/provenance.json`)
- `crates/horch-core/Cargo.toml`, `Cargo.lock`, `scripts/check-deps.sh`,
  `crates/horch-core/tests/nfr.rs` (the dependency lines above only)
- `crates/horch-core/tests/skills_catalog.rs` (new)
- `ai_docs/reports/arch-refactor-dataset/a9a-skills.md`

do not touch: `launch.rs`, `codex.rs`, `teammates.rs`, `roster/**`, `cmd/*`,
oracle and golden files.

## STEPS

1. Create the worktree (conventions §2).
2. `build.rs`: embed `skills/provenance.json` as a `&str` constant
   (`BUNDLED_PROVENANCE`), next to the skill files.
3. `skills/catalog.rs`: `CatalogEntry { id: SkillId, version: SkillVersion, source: CatalogSource, digest: Digest, description: String, provenance: Option<Provenance> }`
   with `CatalogSource { Bundled, Marketplace { source: String, resolved_commit: String } }`.
   `Catalog::bundled()` builds entries from `BUNDLED_SKILL_FILES` (digest =
   the marketplace tree-digest rule: sha256 over sorted
   `<relative path>\0<sha256 of bytes>\n` of the skill's files; version
   `bundled+<digest12>`), attaching provenance from `BUNDLED_PROVENANCE`.
   `Catalog::with_lock(lock_entries)` merges marketplace entries; a
   marketplace entry with the same id overrides the bundled one only when
   the lock says so (document the rule you choose in the report).
   Validation reuses the existing SKILL.md rules (keep one copy; move
   `catalog()`'s validation here and call it from `skills.rs`).
4. `skills/selection.rs`: `phase_skills` and the deterministic selectors
   (moved from `skills.rs`; `skills.rs` re-exports).
5. `skills/activation.rs`: `InvocationPolicy { Explicit, Deterministic, Available }`
   (or the design's names), `ResolvedSkillRef { id, version, digest, policy }`,
   `SkillActivationPlan { active: Vec<ResolvedSkillRef>, available: Vec<ResolvedSkillRef> }`
   and the pure
   `plan_activation(teammate: &Teammate, phase: Option<Phase>, catalog: &Catalog) -> Result<SkillActivationPlan>`:
   teammate `skills:` → Explicit, phase skills → Deterministic, every other
   catalog skill → Available (not active). No NLP, no model call.
6. `skills/briefing.rs`: the function that renders the skills part of the
   worker prompt (find where today's text is produced: `Bundle` or
   `prompts.rs`), now taking a `SkillActivationPlan`. Output must equal
   today's text byte-for-byte, except the bundle path.
7. `skills/materialize.rs`: `materialize(plan, state_root, execution_id) -> Result<MaterializedSkills>`
   writing to `<state_root>/skill-bundles/<execution_id>/` and removing it on
   drop, reusing the existing `Bundle::install` logic. `Bundle` stays as the
   launch-facing type in this unit; it may call `materialize` internally if
   that keeps the oracle green.
8. Tests in `crates/horch-core/tests/skills_catalog.rs`:
   - `skl_01_bundled_catalog_versions_and_digests`: every bundled skill has
     version `bundled+<12 hex>` equal to its digest prefix, provenance with
     the pinned commit, and digests are stable across 2 builds of the catalog.
   - `skl_02_activation_matches_legacy_selection`: for every repo teammate ×
     phase (including none), the active ids equal `skills::selected()`.
   - `skl_03_policy_mapping`: a teammate with an explicit skill in a phase
     that also adds skills gets Explicit for its own, Deterministic for the
     phase's, Available for the rest; no skill appears twice.
   - `skl_07_plugin_skills_separate`: Claude `plugin_skills` (find the
     field on `Teammate`) never enter the catalog or the plan.
   - `skl_08_briefing_matches_baseline_modulo_path`: for every teammate ×
     phase, the new briefing equals the A0 oracle file after replacing the
     bundle path with `<BUNDLE>`.
9. Gate after each step. Commits: `A9: Embed skill provenance`,
   `A9: Add versioned skill catalog`, `A9: Add activation plan`,
   `A9: Render briefing from the activation plan`, `A9: Add SKL tests`.
10. Write and commit the report (API for A10 and A11, override rule,
    gotchas). Follow conventions §6.

## DONE WHEN

- The 5 named tests pass; `oracle_skills_match` and `oracle_cli_skills_match`
  pass unchanged.
- `just gate` is green.

## REPORT

- `horch note` after each commit.
- `horch done` summary: API, the lock override rule, gotchas for A10/A11.
