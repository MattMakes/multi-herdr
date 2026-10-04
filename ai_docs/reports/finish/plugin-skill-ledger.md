# G4 plugin-skill-ledger: the ledger records the plugin skills a launch loads

Plan: `ai_docs/plans/finish/g4-plugin-skill-ledger.md`. Single-branch work on
`design-skills`.

## The gap

F6 (`plugin-filter.md`, "Limits"): a worker's ledger record listed only the
bundle plan. The plugin skills that `plugin_skills` loads were not recorded,
so the launch's skill check (SKL-04, `harness/launch.rs` `skill_drift`) did
not cover them. A plugin skill that changed between the spawn and the launch
ran with bytes that the ledger did not record.

## Design

The decision is also in the SKL design text
(`ai_docs/designs/2026-10-02-architecture-refactor-design.md`, section 4.9,
"Plugin skills in the record").

- Plugin skills enter the catalog, so `execution/plan.rs` and
  `cmd/spawn.rs` did not change. `SkillCatalog::with_operator_skills` now
  also calls the new `SkillCatalog::with_plugin_skills`. The spawn,
  `horch fleet` (`cmd/recipes.rs`, from `ada702d`), the competition
  coordinator and the worker launch (`Bundle::install_from`) all extend the
  catalog through this one call. So the record and the launch see the same
  plugin skills.
- Entry (`CatalogSource::Plugin { plugin, dir }`):
  - id `<plugin>:<skill>`, the name Claude Code lists the skill under. A
    catalog id has no `:`, so a plugin entry never clashes with one;
  - source label `plugin:<plugin>@<marketplace>` (installed) or
    `plugin:<plugin>@inline` (`plugin_dirs`);
  - version `<plugin version>+<digest12>`, from the `installed_plugins.json`
    record, else the manifest, else `plugin+<digest12>`;
  - digest: `horch_marketplace::integrity::tree_digest` of the skill
    directory in the original plugin (the F6 check digest).
- `plan_activation` puts a named plugin entry in `activated` as Explicit.
  An unnamed plugin entry is in neither list. A plugin skill still never
  activates a catalog skill of the same name (SKL-07).
- `MaterializedSkills::materialize` skips plugin entries. The Claude adapter
  loads them from the filtered copy.
- The filtered copy is pinned to the plan. `materialize_filtered` now takes
  the `Bundle` (1-line change in `harness/claude.rs`, which the orchestrator
  approved). Each named skill's copy must hold the digest the plan pinned,
  else the launch fails: "plugin skill 'code:review': the copy holds X,
  the plan pins Y; the plugin changed during the launch". Before, the copy
  was compared only with the source at copy time.
- Only a teammate whose agent loads skills as plugins
  (`skill_namespace().is_some()`), without `disable_skills`, gets plugin
  entries. A plugin or skill that does not resolve now fails the spawn, as
  it already failed the launch.
- `launch.rs` `skill_drift` needed no code change: it compares every
  activated ref, and plugin refs are now in it. The drift message names the
  skill: "'code:review' was 1.2.0+... (digest ...) and is 1.2.0+... now".

## Tests

- `crates/horch-core/tests/skills_catalog.rs`
  `skl_07_named_plugin_skills_enter_the_plan_as_plugin_refs`: the entries,
  id, digest, version label, Explicit policy, source label, unnamed skill
  absent, plugin `tdd` does not activate bundled `tdd`, nothing for codex
  or `disable_skills`, an unresolved plugin fails. `skl_07_plugin_skills_separate`
  is unchanged and passes.
- `harness::launch::tests::plugin_skills_load_a_filtered_copy_of_the_plugin`:
  now installs the bundle with the launch home (`Bundle::install_from`),
  checks that the plan pins `code:review` and not `code:lint`, and that a
  skill edited after the plan fails the launch with "the plan pins".
- `crates/horch-e2e/tests/skills_exposure.rs` (committed in `8873d0d` by
  opus-71 with G3, by accident; the content is mine):
  - `plugin_skills_e2e_ledger_record_lists_them`: the spawn record lists
    `code:review` (source, policy, digest, version `1.2.0+`), and the launch
    loads only `review` from the plugin.
  - `plugin_skills_e2e_changed_since_spawn_fails_the_launch`: SKILL.md
    edited after the spawn; the worker fails with "'code:review' was
    1.2.0+"; claude never ran.
- `crates/horch-e2e/tests/e2e.rs`
  `tel_02_fleet_records_the_orchestrators_plugin_skills`: an edited copy of
  `teammates/` (via `$HORCH_TEAMMATES_DIR`) gives the orchestrator a plugin;
  `horch fleet` records `code:review`, not `code:lint`, and the launch
  accepts the record (claude starts).

Results:
- `cargo test -p horch-core --test skills_catalog`: 21 of 21 pass.
- `cargo test -p horch-e2e --test skills_exposure`: 17 of 17 pass.
- `cargo test -p horch-e2e --test e2e tel_02`: 3 of 3 pass.
- `cargo test -p horch-core --test arch_scan`: 13 of 13 pass.
- `cargo test -p horch-core --test execution_store`: 9 of 9 pass.
- `cargo test -p horch-core --lib`: see the result line in the commit
  report to the orchestrator.
- `cargo clippy -p horch-core -p horch-e2e --all-targets -- -D warnings`:
  clean. `rustfmt --check` on the changed `.rs` files: clean.

## Limits

- `with_operator_skills` now also adds plugin skills. The name is older
  than the behaviour. A rename touches `cmd/spawn.rs` (G3), `recipes.rs`,
  `competition/coordinator.rs` and `roster/validation.rs`, so I did not
  rename it.
- A teammate with only `plugin_skills` (no phase, no skills) now gets a
  bundle, because its plan activates the plugin entries. `horch teammates
  --check` still requires a phase or a skill. The "has no bundle" error in
  `check_filtered` now fires only on a launch path that installs no skills
  bundle.

## Found outside scope (not fixed)

- None found in this unit.
