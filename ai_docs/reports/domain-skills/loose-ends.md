# V3 loose-ends report

Unit `loose-ends`, branch `ds/loose-ends`, worker opus-46.
Plan: `ai_docs/plans/domain-skills/v3-loose-ends.md`.

## Result

| Item | Commit | State |
|---|---|---|
| 3. `skill-creator` multi-source vendored provenance | 0185aa5 | Done |
| 4. Fake flakes: root cause and fix | 260ac86 | Done (see "Item 4") |
| 2. `horch teammates --matrix` shows the 4 fields | 60420d0 | Done |
| 1. The ledger record lists operator skills | e671a99 | Done |

Gate: `HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate` is green after
each item and after the rebase on `design-skills` (b0e816a).

## Item 3: skill-creator provenance

- `skills/provenance.json`: the `skill-creator` entry now has `sources` (18
  files, one per bundled file) on each source, and
  `vendored: true`. Revision `fa59bc9…` and the adaptation text are unchanged.
- Evidence that the copy is verbatim: `diff -r` of
  `~/.claude/plugins/marketplaces/claude-plugins-official/plugins/skill-creator/skills/skill-creator`
  against `skills/skill-creator` shows no difference. The `SKILL.md` sha256
  equals the old recorded `dcd4803e…`. The local marketplace clone is
  shallow at `bdb4f28`, so it does not hold the pinned commit `fa59bc9`.
  Because the content is equal, the sha256 values are those of the upstream
  files.
- `crates/horch-core/tests/skills_catalog.rs`: `EXEMPT_FROM_BUDGET` is
  removed. `vendored: true` now exempts `skill-creator` from the size budget.
  The pin-check skip for `skill-creator` is removed, so its sources pass
  `assert_pinned_upstream_shape`. A new `EXEMPT_FROM_TEXT_ONLY` holds only
  `skill-creator`, because it ships `.py` and `.html` files.
- No skill digest changed (provenance.json is a top-level file).

## Item 4: fake flakes

### Root cause

`syspolicyd` (Gatekeeper/XProtect) kills the fake with SIGKILL. The unified
log shows the kill at the time of a failure:

```
19:53:53.234 syspolicyd: Unable (errno: 2) to read file at <private> for pid: 59103 ...
19:53:53.234 syspolicyd: Terminating process due to Malware rejection: 59103, <private>
```

The test failed at the same time with
`herdr pane split ... failed (signal: 9 (SIGKILL))`.

Mechanism. Since a4a0640 every harness hard-links the same built fake inode
into its own `bin/`, and `Harness::drop` deletes that `bin/`. Under load,
`syspolicyd` scans an exec late. It reads the executable by a path that it
gets from the inode. With many hard links, that path can be the link of
another harness that is already deleted. The read fails with ENOENT, and
`syspolicyd` kills the running process as a "Malware rejection". In the
micro-experiment the killed process's own path still existed, so the read
path was another link.

Load type matters. CPU load alone (16 `yes`) did not reproduce it: 0 of 90
targeted runs, 0 of 30 full binary runs. Concurrent cargo builds (many new
executables for `syspolicyd`) did. That matches the reports: the flakes
appeared while other workers ran gates.

All 3 assigned flakes fit this cause. A killed fake gives a non-success
status (`launch.status.success()` false), or empty stdout before it prints
(`opencode --version`), or `workspace create` killed by SIGKILL. I
reproduced the kill with `fake-opencode` and with `fake-herdr`. I did not
see the 3 named tests themselves fail during my runs.

### Fix

`crates/horch-e2e/src/harness.rs`: `Harness::new` links each fake with a
symlink on unix (new `link_fake`), and keeps a hard link on other systems
and a copy as the fallback. The exec then resolves to the built file in
`target/`, which no test deletes. The first-run scan cost stays low: a
symlink also points at the one built file. `check_built_fakes` still
guards against a write through a linked name. No timeout changed.

### Rates

Load: 2 loops of `cargo build --workspace --all-targets` from clean, in
separate target dirs, plus 6 `yes`. Load average 17 to 40.

| Test | Before (hard links) | After (symlinks) |
|---|---|---|
| Micro: 16 workers run `fake-opencode` for 2 s each, then delete their dir | 2 SIGKILL in 2400 runs | 0 in 2400 runs |
| Full `fakes` + `lifecycle` test binaries | 2 failures in 40 binary runs | 1 failure in 200 binary runs |
| `syspolicyd` "Malware rejection" log lines during the runs | 3 (19:53:53, 19:58:18, 20:03:35) | 0 |

Each micro-experiment SIGKILL has a matching `syspolicyd` log line. 2 in 2400
is a low rate, so the micro numbers alone are weak. The log mechanism is the
main evidence.

The 1 failure after the fix is a different flake (next section).

## Item 1: operator skills in the ledger record

- `crates/horch/src/cmd/spawn.rs` (approved by the orchestrator): before
  `plan_launch`, the CLI extends the roster catalog with
  `SkillCatalog::with_operator_skills` for the teammate that launches. That
  is the requested teammate, or the `tier` of the record to resume. `~`
  expands from `ctx.inherited.home_var`, the same home the launch uses.
- `crates/horch-core/src/competition/coordinator.rs`: the other
  `plan_launch` caller. Dataset candidates launch through the same
  `launch.rs` path, so a candidate teammate with `operator_skills` gets them
  in its bundle. The coordinator now extends the catalog the same way, so
  the candidate record lists them too.
- `crates/horch-core/src/execution/plan.rs`: only the doc of
  `PlanInputs::catalog` changed. Planning stays pure (ARC-15: no `std::fs`).
- A failing operator dir now fails the spawn before a record or pane
  exists. Before, it failed later, in the launch.
- The routing fallback keeps the original teammate's `operator_skills`
  (`routing/decision.rs` `merge`), so the requested teammate's operator
  skills are the right ones.
- Test: `crates/horch-e2e/tests/skills_exposure.rs`
  `operator_skills_e2e_ledger_record_lists_them`. It failed before the
  change (`["tdd"]`) and passes after (`["tdd", "test-modernizer"]`,
  version `operator+…`).

## Item 2: teammates --matrix

`crates/horch/src/cmd/teammatescmd.rs`:
- `MatrixRow` (and so `--json`) has `available_skills`, `operator_skills`
  (`<dir>/<name>`), `offer_when` and `requires`.
- The table has a new "available skills" column. Operator skills are in
  "expected skills" as `operator:<dir>/<name>`. Notes gain
  `offer when <glob> or <glob>` and `requires <tool>`.
- The table is now built by `matrix_table`, so a test can check it.
- Test: `the_matrix_shows_skill_fields_and_the_offer_gate`.

## Decisions

- skill-creator lists all 18 files as sources, not only `SKILL.md`, because
  the conventions ask for every copied upstream file.
- Symlinks, not per-harness copies: copies bring back the slow first-run
  scan that a4a0640 fixed.
- Item 1 lives in the 2 shells (CLI and coordinator), not in `plan.rs`.

## Gotchas

- `log show` hides the killed path as `<private>`. Search for
  `process == "syspolicyd" AND eventMessage CONTAINS "Malware rejection"`.
- A cleanup `trap 'rm -rf /tmp/le-exp-*'` deleted my own scripts named
  `/tmp/le-exp*.sh`. Keep load scripts out of the pattern you delete.
- The ledger record names the teammate in `tier`, not `teammate`.

## Not fixed (outside scope)

- `arc_26_e2e_lifecycle_matrix_{codex,opencode}`: "timed out waiting for
  the session id" (30 s `wait_for`, `tests/lifecycle.rs:83`) at load 27 and
  load 40, with no SIGKILL and no `syspolicyd` line. Seen 1 time before and
  1 time after the fix. This is a timing flake, not the `syspolicyd` cause.
  I told opus-47 (D19 works on fake-herdr lock timing).
- Process leak: a `tel_session_recorded_when_agent_ends_fast` run from
  `mh-wt/followups` left `horch worker codex-sol-1` and its fake `codex`
  (scenario `stay`) alive for 13 h. D19 owns this.
- No coordinator test for item 1. A test needs a dataset round with a forced
  operator-skill candidate: about 130 lines copied from `arc_24` in
  `crates/horch-core/tests/coordinator.rs`, which this unit does not own.
  The CLI e2e test covers the shared mechanism.
