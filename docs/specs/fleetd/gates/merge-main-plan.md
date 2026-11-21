# E1 plan: merge origin/main into fleet/fleetd-finish

Task T-312 (E1-prep) wrote this plan for E1 (T-221). Each claim has a label:
**verified** (with the evidence) or **not verified**.

## Tips

| ref | commit | note |
|---|---|---|
| `fleet/fleetd-finish` at the trial | `2c6ef0b4` | the trial base |
| `fleet/fleetd-finish` at the end of T-312 | `0069a171` | 5 commits of T-254 after the trial base |
| `origin/main` | `e5170752` | Release v0.5.1, 10 commits ahead |
| merge base | `cb9655c6` | |
| trial merge | `52bca240` | local branch `keep/fleetd-finish-trial-merge-main`, parents `2c6ef0b4` and `e5170752`. Not pushed |

The 5 new commits of the run branch (`2c6ef0b4..0069a171`) change 7 files. None of
them is a conflicted file or a file that main changes. Verified:
`git merge-tree --write-tree fleet/fleetd-finish origin/main` at `0069a171`
gives the same 26 conflicts, and
`git merge-tree --write-tree fleet/fleetd-finish keep/fleetd-finish-trial-merge-main`
exits 0 (no conflict).

## How E1 uses the trial

1. Check that the run branch did not change a file of the trial since `0069a171`:
   `git merge-tree --write-tree fleet/fleetd-finish keep/fleetd-finish-trial-merge-main` exits 0.
2. On `fleet/fleetd-finish`: `git merge --no-ff keep/fleetd-finish-trial-merge-main`.
   The result has `origin/main` as an ancestor, so a later merge of main finds no conflict.
3. Apply the operator's answers to Q1 to Q3 (below) on top.
4. Run the full gate: `cargo test --workspace --locked && cargo clippy --workspace --bins --release --locked -- -D warnings`.

If step 1 fails, redo the merge with `git merge --no-ff --no-commit origin/main` and take
each conflicted file from the trial: `git checkout keep/fleetd-finish-trial-merge-main -- <file>`.
Also take the 7 files of "Compile result of the trial" and "Files outside the conflicts".

## Commits of main

| commit | PR | what it changes and why |
|---|---|---|
| `05ee2342` | #350 | Adds the seat `game-design-juicer` (Fable, Opus fallback) and the skill `game-juice`. Adds `SEATS_WITH_FALLBACKS`, so that 1 Fable seat can fall back to Opus. Adds ticket files for quiet orchestrator mail and video formats. |
| `65939082` | - | Release v0.4.9: version, `Cargo.lock`, CHANGELOG heading. |
| `0bcb3edb` | #356 (351-355) | Adds the finish step `comment-sweep` (schema v8, `finish/comments.rs`, seat `comment-remover`). Adds the seats `game-design-incremental-specialist`, `addictive-game-design-reviewer`, `root-cause-auditor` and their skills, and the skill `godot-library` for each godot seat. Sandbox fixes: a Claude fleet worker can write `.git` and `.worktrees`, a macOS sysctl fallback for process identity in `horch spawn`, tests honor `TMPDIR`, the e2e harness finds binaries in the Cargo 1.100 `out/` layout, a quota refusal is held no longer than its window, `horch note` exits 0 when the compaction follow-up refuses. |
| `09e74d0a` | - | Release v0.4.10. |
| `94b6db7d` | #358 | Adds the skill `make-videos` to `orchestrator` and `orchestrator-codex` by name, with its oracles. |
| `13d7f394` | - | Release v0.4.11. |
| `2ce05219` | #359 | Adds the crate `crates/ui-prop-tester` (property tests for web and terminal UIs) and its skill. `qa-engineer` and `frontend-developer` name the skill. |
| `e8b473ec` | - | Release v0.5.0. |
| `08a5b836` | #357 | Replaces the skill `incremental-game-design` with its design-first version. Skill files and the seat text only. |
| `e5170752` | - | Release v0.5.1. |

Verified: read with `git show --stat` and the commit messages of each commit.

## Conflicts

All 26 conflicts are content conflicts (`UU`). "ours" is the run branch, "theirs" is main.
State: **R** means resolved in the trial, and the trial compiles.

| # | file | rule | state |
|---|---|---|---|
| 1 | `Cargo.toml` | Members: keep `crates/agent-docs-sync` (the run branch renamed `herdr-docs-sync`) and add `crates/ui-prop-tester`. The version `0.5.1` of main merges without a conflict. | R |
| 2 | `Cargo.lock` | Take main's file, then run `cargo metadata` once without `--locked` (it needs crates.io for `boa_engine` and others). The result is main's lock plus 5 packages of the run branch: `agent-docs-sync` (rename), `horch-platform`, `serde_spanned 1.1.3`, `toml 1.2.1`, `toml_writer 1.2.0`. No other version changes. | R |
| 3 | `README.md` | Ours wins in all 4 hunks: they describe the retired `horch ledger finish` and verify profiles. Main's other additions (seats, ui-prop-tester section) merge without a conflict. The comment-sweep text depends on Q1. | R |
| 4 | `crates/horch-core/src/harness/claude.rs` | Keep both tests. Main's test uses `std::process::Command` by full path, because the run branch imports `horch_platform::start::Command` in this file. | R |
| 5 | `crates/horch-core/src/messaging/report.rs` | Ours wins (test name `a_closed_socket_is_an_error`). Add main's `.or_else(\|_\| tempfile::tempdir())` fallback to its `tempdir_in("/tmp")`. | R |
| 6 | `crates/horch-core/src/procid.rs` | Ours wins (the OS calls are in `horch-platform`). Decision (b): main's `is_same_sandboxed` and `parent_sandboxed` stay in `procid.rs` as wrappers, because `crates/horch/src/cmd/spawn.rs` (`Procs::os`) uses them. The sysctl code goes to `horch-platform/src/mac/process.rs` (`sysctl_start_time`, `sysctl_parent`) and `horch-platform/src/process.rs` (`start_time_sandboxed`, `parent_sandboxed`, `None` on Linux and Windows). Main's 2 macOS tests call the new functions. | R |
| 7 | `crates/horch-core/src/roster/validation.rs` | Count of Claude teammates: 79 (ours) + 4 (main) = 83. | R, count not verified by a test run |
| 8 | `crates/horch-core/src/taskledger/schema.rs` | Both sides add version 8. Keep main's v8 as `COMMENT_SWEEP_VERSION = 8` (`comment_sweep_step()`). Move the fleetd steps up by 1: `PHASE_1_VERSION = COMMENT_SWEEP_VERSION + 1`, and the run branch's `v8()` becomes `phase_1()`. `VERSION = PHASE_7_VERSION` (17). Reason and risk: see S2 and Q2. | R |
| 9 | `crates/horch-core/tests/oracles/launch/frontend-developer.json` | Line 1 (settings) from ours: it has the run branch's sandbox and deny list. Line 2 (prompt) from main: it names `horch:ui-prop-tester` and 8 skills that are not in the bundle. | R, oracle not verified by a test run |
| 10 | `crates/horch-core/tests/oracles/launch/qa-engineer.json` | Same rule as row 9. | R, oracle not verified by a test run |
| 11 | `crates/horch-core/tests/taskledger_queue.rs` | Ours wins: the run branch builds the old file with `migrate_to`, so main's "version 8" assert is not needed. | R |
| 12 | `crates/horch-core/tests/trf_48_ledger_guard.rs` | Ours wins (`migrate_to(&mut conn, 6)`), same reason as row 11. | R |
| 13 | `crates/horch-e2e/src/lib.rs` | Ours wins: `harness::built_dir_of` already finds the Cargo 1.100 `out/` layout (verified: `crates/horch-e2e/src/harness.rs:1132`). Main's 2 tests stay and call `harness::built_dir_of` instead of `bin_dir_from`. | R |
| 14 | `crates/horch-e2e/tests/fleet_start.rs` | Ours wins: the run branch deleted the test `a_fleet_whose_orchestrator_skill_is_missing_creates_no_workspace` (`horch fleet` starts no orchestrator pane). Main's change to its skills line is not needed. | R |
| 15 | `crates/horch-e2e/tests/taskledger.rs` | Ours wins in 3 hunks: the run branch rewrote the finish tests for fleetd. Main's `comment_sweep` helper is for the retired `horch ledger finish`. | R |
| 16 | `crates/horch/src/cmd/recipes.rs` | Ours wins: the run branch deleted the verify-profile tests of `horch fleet`. | R |
| 17 | `crates/horch/src/cmd/taskledger.rs` | Ours wins. Main's change (`note_as` returns the exit code) gives the same behavior as ours: ours prints the follow-up error and returns `ExitCode::SUCCESS`. | R |
| 18 | `crates/horch/src/cmd/taskledger_finish.rs` | Ours wins: `horch ledger finish` only writes `result.json`. Main's "<step> of <task>" reviewer text is for the retired finish (decision (a): fleetd starts the reviewers). | R |
| 19 | `crates/horch/src/cmd/taskledger_spawn.rs` | Ours wins in 2 hunks: the run branch removed `spawn_reviewers` and `ReviewSpawn`. Decision (a): not needed in fleetd. | R |
| 20 | `crates/horch/tests/w3_ledger_cli_pins.rs` | Ours wins in 2 hunks: the run branch pins the event lines, not the steps of the retired finish. | R |
| 21 | `docs/command-flow.md` | Ours wins: the diagram shows fleetd jobs. Comment-sweep text depends on Q1. | R |
| 22 | `docs/fleet-workflow.md` | Ours wins: the table shows the fleetd flow. Comment-sweep text depends on Q1. | R |
| 23 | `skills/orchestrate/SKILL.md` | Ours wins: the skill describes ideas and the retired commands. Main's change names comment-sweep in the retired finish. | R |
| 24 | `teammates/README.md` | Ours wins in the orchestrator paragraph. Main's sections (new seats, `make-videos`, `comment-remover`) merge without a conflict. The `comment-remover` section says "horch spawns it for the finish step comment-sweep": see Q1. | R |
| 25 | `teammates/_base/fleet-orchestrator.md` | Ours wins: the briefing is the ideas briefing. Main's "== Comment sweep ==" section is about `horch ledger finish`, which the run branch retired. It merged into the old text that ours removed, so it is gone. | R |
| 26 | `teammates/_base/fleet-worker.md` | Ours wins: workers use `horch result`, and fleetd runs the finish steps. Main's comment-sweep lines are for the retired finish. | R |

Verified: each rule comes from reading both sides with `git checkout --conflict=diff3`,
and for code rows from reading the new code path that the row names.

## Changes of main that fleetd code must follow

| # | change of main | effect on the run branch | state |
|---|---|---|---|
| S1 | `VerifyProfile::steps()` (`taskledger/model.rs`) puts `CommentSweep` into every profile. | `verifier_steps` (`fleetd/task_execution/application/review.rs`) returns it, and `run` sends it to `job_step`. That runs the task's check command again under the name `comment-sweep`, and no comment is removed. The unit test `verifier_steps_are_the_steps_before_land_without_the_check` fails. Trial fix: `verifier_steps` drops `CommentSweep`, as it drops `Check`. `steps_through` uses `verifier_steps` too, so a `comment-sweep` row that stays pending does not hold a task. | verified (read; the unit test passes in the trial). The real choice is Q1. |
| S2 | Schema v8 (`comment-sweep` in the `CHECK` of `task_steps`). | The installed horch 0.5.1 already migrated the live ledger: `PRAGMA user_version` of `~/.horch/ledger.sqlite3` is 8. Without the renumber, a fleetd build reads version 8 as "Phase 1 done", so it skips the fleetd tables. With the renumber, a ledger file that a run-branch build wrote at version 8 to 16 is read 1 step too low. | verified (`pragma user_version` gives 8; `horch --version` gives 0.5.1). Q2 |
| S3 | `crates/horch/src/cmd/spawn.rs` calls `horch_core::procid::is_same_sandboxed` and `parent_sandboxed`. | Ported (conflict row 6). The file merged without a conflict and compiles. | verified (compiles; 2 sysctl tests pass on macOS) |
| S4 | `harness/claude.rs`: write access to `.git` and `.worktrees` for a sandboxed worker. | It applies only to `HorchAccess::Full`. A fleetd launch uses `HorchAccess::Report`, so it does not get this access. Whether a sandboxed fleetd worker can commit in its worktree is not checked here. | not verified |
| S5 | `routing/quota.rs`: a refusal is held no longer than its window, and a scoped refusal holds only its scope. | fleetd reads `routing::quota` in `fleet_runtime/application/holds.rs`. The change merged without a conflict and compiles. The behavior of the fleetd holds with it is not tested. | not verified |
| S6 | Counts in tests that merged without a conflict: `skills_catalog.rs`, `golden_prompts.rs`, `baseline_oracles.rs`, `godot_wave.rs`, `tests/harness.rs`, and the subagent-deny and pre-approval counts that commit `05ee2342` names (79 and 84 at that commit). | The run branch added its own teammates (for example `idea-lead`), so a count of main can be wrong in the merge. Only the full test suite shows it. | not verified |
| S7 | The code of `finish/comments.rs` and `cms_comment_sweep.rs`. | It merged into the old finish module. Only its test calls it: `horch ledger finish` no longer runs the steps. | not verified that no other caller exists |
| S8 | Docs that describe comment-sweep in `horch ledger finish`: `README.md` (3), `docs/specs/task-ledger.md` (6), `teammates/README.md` (2), `GLOSSARY.md` (1), `teammates/comment-remover.md`, `docs/specs/comment-sweep.md`. | They describe a step that fleetd does not run (trial state). Edit them after Q1. | verified (`git grep -c comment-sweep -- '*.md'` in the trial) |
| S9 | CHANGELOG: main adds 101 lines, all under released headings v0.4.9 to v0.5.1. | The run branch adds no CHANGELOG line, and `## Unreleased` is empty after the merge. A merge of the run-branch PR into main publishes a release only if a line is added under `## Unreleased`. | verified (`git diff cb9655c6 fleet/fleetd-finish -- CHANGELOG.md` is empty) |
| S10 | `.github/workflows/` | Main does not change it. The run branch's `ci.yml` change (bubblewrap, `--no-fail-fast`, the `test-windows` job) stays. The new crate `ui-prop-tester` runs in both jobs. `test-windows` has `continue-on-error`. | verified (diff stat); the result of the new crate in CI is not verified |
| S11 | The crate `ui-prop-tester` | It compiles in the trial. `cargo clippy --workspace --bins --release --locked -- -D warnings` and its tests are not run. | not verified |
| S12 | `make-videos` in `teammates/orchestrator.md` and `orchestrator-codex.md` | The run-branch orchestrator is a chat persona with the operator, so the skill fits. The orchestrator oracles merged without a conflict. | verified (read); oracles not verified by a test run |

Searched: each function, flag, file and count that the 10 commits touch, in the run
branch: `VerifyProfile`, `StepName`, `verifier_steps`, `steps_through`, `schema::VERSION`,
`migrate_to`, `spawn_reviewers`, `note_as`, `is_same_sandboxed`, `parent_sandboxed`,
`HorchAccess`, `routing::quota`, `SEATS_WITH_FALLBACKS`, `bin_dir`, `built_dir_of`,
`skills: [orchestrate`, `comment-sweep` in `*.md`, `CHANGELOG.md`, `.github/`.

## Compile result of the trial

Command: `scripts/dev-sandbox.sh cargo check --workspace --all-targets --locked`.

| run | errors | fix |
|---|---|---|
| 1 | 3 x E0599 in `harness/claude.rs` (main's test calls `Command::new`, which is `horch_platform::start::Command` here) | call `std::process::Command::new` in that test |
| 1 | E0063 in `tests/cms_comment_sweep.rs`: `Tools` has no field `jobs` | add `jobs: None` |
| 1 | E0425 in `tests/cms_comment_sweep.rs`: `tasks::claim` does not exist (T-871 removed it) | bind the worker with `taskledger::testing::bind_live`, as `tests/trf_18_shared_diff_base.rs` does |
| 2 | 2 x E0425 in `horch-e2e/src/lib.rs`: `bin_dir_from` does not exist | call `harness::built_dir_of` |
| 3 | none | exit 0 |

Then `cargo fmt --all` (2 files), and `cargo fmt --all --check` exits 0.

Proof run (1 test binary): `scripts/dev-sandbox.sh cargo test -p horch-core --lib --locked -- taskledger::schema review::tests procid`.
Result: 30 passed, 0 failed. It includes `trf_48_every_old_version_migrates_to_current`,
`cms_07_migration_keeps_the_step_rows_and_takes_the_comment_sweep_name`,
`verifier_steps_are_the_steps_before_land_without_the_check` and the 2 sysctl tests.
The full suites and clippy are not run (load average 43 to 54).

### Files outside the conflicts that the trial changes

- `crates/horch-platform/src/process.rs`, `crates/horch-platform/src/mac/process.rs` (row 6)
- `crates/horch-core/src/fleetd/task_execution/application/review.rs` (S1)
- `crates/horch-core/tests/cms_comment_sweep.rs` (compile fix)

## Open questions for the operator

1. **Q1: comment-sweep in fleetd.** Main runs comment-sweep in every profile, through
   the retired `horch ledger finish`. Option a (the trial): fleetd does not run it, and
   E1 changes the docs of S8 to say so. Option b: port it as a fleetd verifier step
   that starts 1 `comment-remover`, as `code_sweep` in `review.rs` starts the reviewers.
   That is new work after E1, with its own ticket. I recommend option a for E1 and a
   ticket for option b, because the merge then keeps the run branch's tested finish
   steps.
2. **Q2: schema numbers.** The trial keeps main's version 8 and moves the fleetd steps
   to versions 9 to 17. The live ledger is at version 8 of main, so it migrates
   correctly. Does any ledger file exist that a run-branch build migrated past version 7
   (not a scratch `HORCH_LEDGER_DB` file)? Such a file must be built again. I recommend
   the renumber.
3. **Q3: release.** The merge adds no CHANGELOG line. Do you want a line under
   `## Unreleased` for the fleetd release when the run-branch PR merges into main? This
   decides whether that merge publishes a release.
