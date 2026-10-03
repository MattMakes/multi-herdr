# D10 followups: load-proof dataset e2e tests, Antigravity loose ends, docs

Unit slug: `followups`. Branch: `ds/followups`.

## GOAL

The full gate passes reliably on a loaded machine (several gates in parallel),
Antigravity is complete in the places D08 left out, and the skills README
describes the multi-source catalog.

## CONTEXT

- Read first: `00-conventions.md`, and the reports in
  `ai_docs/reports/design-skills/` (`antigravity-harness.md`,
  `agent-list.md`, `skills-infra.md`, and the "Gotchas" of every skill
  report).
- Flakes seen only under load (each passes alone): in
  `crates/horch-e2e/tests/dataset.rs`: `cmp_04_*`, `cmp_05_e2e_candidates_in_dataset_workspace`
  (spawned 1 of 2), `cmp_13_crash_every_boundary` (PRE-06 codex version
  unresolved), `mea_10_every_spawn_has_terminal_event` (a lock race),
  `sec_03_no_transcript_copies_by_default` (exit 4 = preflight failed); in
  `crates/horch/tests/agent_list.rs`: `agent_list_no_probe_runs_no_binary`.
  The orchestrator already raised the `harness_version` timeout to 15 s and
  made it read stderr (`routing/quota_probe.rs`). Find each remaining root
  cause. Reproduce with load: run the target in a loop while 2 other
  `cargo test --workspace` runs go on in other worktrees (or a CPU burner).
  Fix the cause (a real race in product code is a product fix; a too-tight
  test deadline is a test fix). Do not add retries that hide a product bug.
- Antigravity gaps from D08: `crates/horch/src/dataset/preflight.rs`
  `kind_of` lacks Antigravity; `crates/horch-e2e/src/harness.rs` `FAKES`
  does not install `fake-antigravity`; the quota `POOLS` display does not
  show `google`; `horch cost` reads no agy usage (only fix if agy writes a
  local usage or transcript file per the research report; else document the
  gap).
- `horch agent-list` prints `skills=PluginDir`: add `SkillExposure::as_str`
  with kebab-case names (`plugin-dir`, `codex-home`, `config-paths`,
  `skill-flag`, `none`).
- `skills/README.md` intro and "Source mapping" still describe one upstream
  only. Rewrite the intro for 2 upstream families (skill-marketplace and the
  7 design repos), keep both tables.
- Parallel unit: D07 `design-personas` (teammates and
  `skills/orchestrate/SKILL.md`). Do not touch those.

## FILES

own:
- `crates/horch-e2e/tests/dataset.rs`, `crates/horch-e2e/src/**`,
  `crates/horch/tests/agent_list.rs` (flake fixes)
- product files that a flake root cause needs (name each in a `NOTE:` to
  the orchestrator before you edit it)
- `crates/horch/src/dataset/preflight.rs` (`kind_of`), the quota pool
  display, `crates/horch-core/src/harness/capabilities.rs` (`as_str`),
  `crates/horch/src/cmd/agentlist.rs` (use it)
- `skills/README.md` (intro and headings only; not the table rows)
- `ai_docs/reports/design-skills/followups.md`

do not touch: teammates, `skills/<id>/`, oracles.

## STEPS

0. Create the worktree (conventions §3).
1. Antigravity gaps and `as_str`. Gate. Commit.
2. Skills README intro. Commit.
3. Flakes: reproduce under load, fix causes one at a time, prove each with
   10 of 10 passes under load. Commit per cause.
4. Full gate 3 times in a row under load: all green. Write and commit the
   report (cause, fix, evidence per flake). Follow conventions §7.

## DONE WHEN

- 3 consecutive green gates under load; the Antigravity gaps are closed or
  documented.
