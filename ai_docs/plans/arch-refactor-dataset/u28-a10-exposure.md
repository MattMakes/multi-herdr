# U28 a10-exposure: each harness exposes exactly the activated skills

Unit slug: `a10-exposure`. Branch: `ard/a10-exposure`. Phase: A10.
Requirements: SKL-05, SKL-06.

## GOAL

The skill activation plan decides what a worker sees, and each harness
adapter's `prepare` exposes exactly the activated skills in its own native
way (Claude plugin dir plus settings overlay, Pi and Prime `--skill <dir>`,
OpenCode `OPENCODE_CONFIG_CONTENT.skills.paths`, Codex a private
`CODEX_HOME`). Non-activated skills are not exposed. The core skill model
has no harness flags. Marketplace skills work end to end, offline.

## CONTEXT

- Read first: `ai_docs/plans/arch-refactor-dataset/01-fleet-conventions.md`.
- Read in `00-master-plan.md`: §3 "A9" and "A10", §4 SKL rows, Spec A §17
  items 9 and 11, §5 LA-4 and LA-5.
- Design doc `ai_docs/designs/2026-10-02-architecture-refactor-design.md`:
  skills key types, harness capabilities (`skill_exposure`), the A10 section.
- Merged reports (`ai_docs/reports/arch-refactor-dataset/`): `a9a-skills.md`
  (`plan_activation(teammate, phase, catalog)`, `SkillActivationPlan` with
  `activated`, `available`, `plugin_skills`; `MaterializedSkills::materialize`;
  `Bundle` still drives exposure; `Bundle::install` writes
  `.claude-plugin/plugin.json` and uses `mint_uuid` as the dir name — switch
  to the execution id), `a11-marketplace-cli.md` (`SkillCatalog::installed(data_root)`;
  materialize copies marketplace skills from the store and checks digests;
  gotchas: `roster/validation.rs` (about line 46) and `skills::selected`
  accept bundled ids only, so a teammate that lists a marketplace skill fails
  roster load; `cmd/worker.rs` and `cmd/recipes.rs` must pass `data_root`;
  an e2e spawn with `HORCH_GIT_BIN=/nonexistent` must prove offline use),
  `a4-harness.md` (the `Harness` trait, `prepare`, `Capabilities.skill_exposure`,
  the launch flow), `a6b-service.md` if merged (the service records
  `skills` refs), `e2e-fakes.md` (fakes record argv and env).
- The `arc_10_harness_match_only_in_harness` scan has a PENDING entry for
  `skills.rs` that this unit removes: after this unit no file under
  `skills/` (or `skills.rs`) mentions `HarnessKind`, `Agent::` or a harness
  flag.

## FILES

own:
- `crates/horch-core/src/skills.rs`, `crates/horch-core/src/skills/**`
  (remove harness-specific code: `native_args`, `opencode_config`, the
  Claude plugin layout move into the harness modules)
- `crates/horch-core/src/harness/{claude,claude_plugins,codex,opencode,pi,prime}.rs`
  (`prepare` exposes the plan)
- `crates/horch-core/src/roster/validation.rs` and `skills/selection.rs`
  (accept a catalog that includes the lock)
- `crates/horch/src/cmd/worker.rs`, `crates/horch/src/cmd/recipes.rs`
  (pass `data_root` and the catalog; minimal edits)
- `crates/horch-core/tests/arch_scan.rs` (remove the `skills.rs` PENDING entry; add the SKL-05 scan)
- `crates/horch-e2e/src/bin/fake-*.rs` (add the `inspect_skills` scenario)
- `crates/horch-e2e/tests/skills_exposure.rs` (new)
- `ai_docs/reports/arch-refactor-dataset/a10-exposure.md`

do not touch: oracle and golden data (the skills briefing oracle must stay
green), routing, workspace, measure, competition.

## STEPS

1. Create the worktree (conventions §2).
2. Move exposure into adapters: each `Harness::prepare` receives the
   materialized activated skills (a dir per skill) and exposes them:
   Claude: `--plugin-dir <bundle>` with the plugin manifest and the
   settings overlay; Pi and Prime: one `--skill <dir>` per activated skill;
   OpenCode: `skills.paths` in `OPENCODE_CONFIG_CONTENT` (merged with the
   inherited value from context); Codex: a private `CODEX_HOME` with the
   skills attached via `Rules::attach_skills` (create it if needed). Only
   `activated` skills are materialized; `available` ones are not.
   The bundle dir name is the execution id (validated as a path component).
3. Marketplace skills: catalog = `SkillCatalog::installed(data_root)`
   (bundled + lock); roster validation and `skills::selected` accept ids in
   that catalog. `worker.rs` and `recipes.rs` pass it.
4. Fakes: an `inspect_skills` scenario in every fake that writes the skill
   dirs, flags and env it received (for Codex, the files under its
   `CODEX_HOME`) to `$HORCH_FAKE_LOG.skills.json`.
5. Tests:
   - `skl_05_core_skill_model_has_no_harness_flags` (in `arch_scan.rs`): no
     file under `skills/` or `skills.rs` contains `HarnessKind`, `Agent::`,
     `--plugin-dir`, `--skill`, `OPENCODE_CONFIG_CONTENT`, `CODEX_HOME`.
   - `skl_06_e2e_exposure_claude`, `..._codex`, `..._opencode`, `..._pi`,
     `..._prime` (in `crates/horch-e2e/tests/skills_exposure.rs`): spawn a
     teammate of that harness whose activation plan has a known set A and a
     known non-activated skill B; the fake sees exactly A, never B.
   - `skl_06_e2e_marketplace_offline`: install a skill from a local fixture
     repo (`Harness::with_git()`), then spawn with `HORCH_GIT_BIN=/nonexistent`;
     the fake sees the skill.
   - `oracle_skills_match`, `skl_08_*`, `arc_09_argv_matches_baseline`
     stay green. If the argv oracle changes because the bundle path or a
     flag order moved, send `QUESTION:` before touching anything.
6. Gate after each step. Commits: `A10: Move skill exposure into harness adapters`,
   `A10: Accept marketplace skills in roster validation`,
   `A10: Add inspect_skills to the fakes`, `A10: Add SKL-05 and SKL-06 tests`.
7. Write and commit the report. Follow conventions §6.

## DONE WHEN

- The named tests pass; `just gate` is green; `check-req-coverage.sh --phase A10` exits 0.

## REPORT

- `horch note` after each commit.
- `horch done` summary: the per-harness exposure, the catalog wiring, gotchas.
