# A10 a10-exposure: each harness exposes exactly the activated skills

Unit U28. Branch `ard/a10-exposure`. Phase A10. Requirements SKL-05, SKL-06.
Base: `arch-refactor-dataset` at 20861b9.

## Result

- Each harness adapter exposes the bundle in its own native way. The core
  skill model (`skills.rs`, `skills/**`) has no harness code left.
- The launch flow builds the catalog with
  `SkillCatalog::installed(ctx.paths.data_root)`, so installed marketplace
  skills launch. The launch needs no git and no network.
- Roster validation accepts installed marketplace ids.
- `HARNESS_MATCH_PENDING` is empty.
- `HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate` prints `GATE GREEN`.
  `./scripts/check-req-coverage.sh --phase A10` exits 0.
- `oracle_skills_match`, `oracle_launch_matches`, `arc_09_argv_matches_baseline`
  and every `skl_08_*` test pass. No oracle changed.

## Per-harness exposure

| Harness | Mechanism | Where |
|---|---|---|
| claude | `--plugin-dir <bundle>` (pushed to `plugin_dirs`), `.claude-plugin/plugin.json` (name `horch`), and the `--settings` overlay merged into the teammate's own settings | `Claude::expose_skills`, `skills_settings` in `harness/claude.rs` |
| pi, prime | 1 flag `--skill <bundle>/skills`, in `args`, before `--` | `pi::with_skill_flag`, called by `Pi`/`Prime::expose_skills` |
| opencode | `skills.paths` in `OPENCODE_CONFIG_CONTENT`, merged into the value the builder set (effort variant), else the teammate's `env`, else the inherited value | `OpenCode::expose_skills_env`, `skills_config` in `harness/opencode.rs` |
| codex | a private `CODEX_HOME` whose `skills` link points at `<bundle>/skills` (`Rules::attach_skills`) | `Codex::prepare` (unchanged) and `Codex::ensure_skills_supported` (Windows refusal) |
| none | refuses skills (`skills need an agent harness`) | `NoAgent::expose_skills` |

The bundle holds only the activated skills (`MaterializedSkills` writes
`plan.activated`). `available` skills are named in the briefing only.

New `Harness` trait methods (all with defaults): `ensure_skills_supported`,
`skill_namespace` (claude: `Some("horch")`), `expose_skills(teammate, bundle,
home) -> Teammate` (before the build) and `expose_skills_env(cmd, teammate,
bundle, inherited)` (after the build). `Bundle::configure` and
`Bundle::apply_env` keep their signatures and delegate to the adapter,
because `tests/baseline_oracles.rs` (not mine) calls them.

## Catalog wiring

- `harness/launch.rs` `install_skills(ctx, req)`: catalog =
  `SkillCatalog::installed(&ctx.paths.data_root)`, then
  `Bundle::install_from(state_root, teammate, catalog, name)`.
- Bundle dir name (`bundle_name`): the execution id = the request's ledger
  record id, when it is 1 plain path component (no `/ \ : NUL`, not `.`/`..`)
  and `skill-bundles/<id>` does not exist yet. Otherwise a fresh
  `ExecutionId::mint`. The "exists" case covers a resume after a crash
  left the old directory. `skl_06_e2e_exposure_claude` checks the name.
- `Roster::with_skill_catalog(catalog)` and `roster.skill_catalog()`.
  The field is in `roster/repository.rs`. `check_teammate` uses
  `skills::selected_in(t, catalog)` when a catalog is attached.
- `cmd/mod.rs` `load_roster` attaches `SkillCatalog::installed(data_root)`.
  When the lock does not read, the roster keeps the compiled-in catalog, so
  `horch doctor` still runs. A launch still fails loudly on that lock.
- `skills::ensure_supported_in(teammate, catalog)` is new.
  `ensure_supported(teammate)` keeps its old behavior (bundled catalog).

## Decisions (orchestrator answers)

1. Pi and Prime keep 1 flag `--skill <bundle>/skills`, not 1 flag per
   skill. The dir holds only set A, so the exposure is exact, and the
   frozen `pi.json`/`prime.json` launch oracles stay unchanged.
2. `cmd/mod.rs` got the 1-line `load_roster` change. `cmd/spawn.rs` is NOT
   changed: U26 (opus-17) moves that code to `execution/plan.rs` and
   switches the call to `ensure_supported_in(teammate, &roster.skill_catalog()?)`.
   If U26 merges first, this branch rebases and changes that 1 call in its
   new place.
3. `crates/horch-e2e/src/lib.rs` got 1 additive function,
   `write_skills_report`.

## Deviations from the plan

- Exposure is not all in `prepare`. OpenCode must merge after the builder
  sets its effort variant, and the oracle path (`command_with_skills`)
  never calls `prepare`. So exposure is 2 adapter hooks around the build,
  plus Codex's `prepare` for the private home. `PrepareRequest.skills` still
  carries the bundle.
- The plugin manifest is written by `Claude::expose_skills` (idempotent),
  not by `Bundle::install`. A codex/pi/opencode bundle no longer gets
  `.claude-plugin/`.
- 2 unit tests left `skills.rs`: `skills_native_loaders_receive_explicit_paths`
  is deleted (it tested the removed `native_args`;
  `phase_skills_are_native_on_all_harnesses_and_resume_keeps_prompt_last` in
  `launch.rs` still covers each harness), and
  `skills_opencode_overlay_preserves_provider_and_denials` moved unchanged to
  `harness/opencode.rs`.
- The marketplace e2e adds `demo` to the brief's `resolved` teammate after
  `spawn`, because `spawn` still uses the bundled-only `ensure_supported`.
  After U26 switches that call, the test can name `demo` in the teammate
  file before spawn.

## Tests added (9 new, 1 moved)

- `skl_05_core_skill_model_has_no_harness_flags` (`tests/arch_scan.rs`):
  the whole text of `skills.rs` and `skills/**`, comments and tests too.
- `crates/horch-e2e/tests/skills_exposure.rs`: `skl_06_e2e_exposure_claude`,
  `_codex`, `_opencode`, `_pi`, `_prime` (set A = `debug`, `tdd`; set B =
  `execute`) and `skl_06_e2e_marketplace_offline` (local bare repo,
  `skills install`, `teammates --check`, then a launch with
  `HORCH_GIT_BIN=/nonexistent`).
- `marketplace_skills_pass_the_check_with_the_installed_catalog`
  (`roster/validation.rs`).
- `bundle_name_is_the_record_id_when_it_is_safe_and_free` (`harness/launch.rs`).
- Moved, not new: `skills_opencode_overlay_preserves_provider_and_denials`.

Red checks: `skl_05` fails on the base (`skills.rs` had `Agent::` and
`--plugin-dir`). `skl_06_e2e_marketplace_offline` fails on the base:
`run_flow` used the bundled catalog, so the launch reported "unknown bundled
skill 'demo'". `bundle_name_...` fails with the first version of
`install_skills`, which returned `../escape` unchanged. The 5 per-harness
SKL-06 tests were green on the base too: the base already exposed exactly
set A. They guard the contract after the move.

## The `inspect_skills` scenario

Every agent fake writes `$HORCH_FAKE_LOG.skills.json` on a launch (the last
launch wins): `fake`, `dirs`, `skills` (sorted ids found in the dirs), plus
`flags` (claude, pi, prime), `plugins` and `settings` (claude), `env`
(opencode, codex) and `files` (codex). The fake writes it while it runs,
because horch removes the bundle and the codex home after the agent exits.
fake-herdr records the spawn's `pane split` as a violation by design, so
the e2e test ignores `herdr: mutating call` violations.

## Security review (horch:security-review, focused)

Scope: the A10 diff 20861b9..HEAD. Trust boundaries: the record id from the
brief (a file in the state dir) into a directory name; `marketplace.lock`
and the store (operator-writable) into the launch; third-party SKILL.md
text into every harness. Source review only; no scanner.

- CONFIRMED, fixed, low: `ExecutionId::new` accepts `/` and `..`. A legacy
  record id such as `../x` reached `materialize`, which refused it, so the
  launch failed. No traversal happened. `bundle_name` now mints a fresh id
  for a non-plain name, and checks with `symlink_metadata`, so a planted
  symlink is never followed. Test: `bundle_name_is_the_record_id_when_it_is_safe_and_free`.
- DISMISSED: race between the existence check and `create_dir`. `create_dir`
  fails on an existing entry, so the launch fails; it never writes into a
  foreign directory.
- ACCEPTED, low: `Claude::expose_skills` writes `.claude-plugin/plugin.json`
  with `create_dir_all`/`write`, which follow a symlink. The bundle root is
  a directory this launch created; planting a link needs write access to the
  state dir.
- DISMISSED: JSON injection through skill paths. `skills_config` and the
  manifest use `serde_json`, not string building.
- NOT IN SCOPE (inherent): marketplace SKILL.md bodies are third-party
  prompt text, now exposed to every harness. The copy's digest must equal
  the lock (A11).
- NOTE: `load_roster` hides a lock read error from `teammates --check`.
  `horch skills doctor` reports it.

`ANTHROPIC_API_KEY`: unchanged; `run_flow` and `build_command` still strip
`FORBIDDEN_ENV` last.

## Gotchas for later phases

- U26/A6b: switch `cmd/spawn.rs:362` (or its new place) to
  `skills::ensure_supported_in(teammate, &roster.skill_catalog()?)`. Then
  simplify `skl_06_e2e_marketplace_offline`.
- A6b: when `LaunchRequest` carries the real execution id, pass it to
  `bundle_name` instead of `record.record_id`.
- A12: `Bundle::configure`/`apply_env` exist for `baseline_oracles.rs` and
  `command_with_skills_in`. They can go when those callers use the adapter
  hooks directly.
- `launch.rs` lost the `pub(crate) use super::claude::overlay_skill_switches`
  re-export; nothing used it after the move.

## Outside my scope (noticed, not fixed)

- `crates/horch-core/tests/execution_store.rs` warns about an unused import
  `KIND_ORCHESTRATOR` (in the base).

## SPEC-RESOLVED (closed: ai_docs/designs/2026-10-02-dataset-competition-design.md §3-§11, ai_docs/reports/finish/spec-b-preflight.md)

None added.
