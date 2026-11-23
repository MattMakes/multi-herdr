# E1 proof 3: the fix wave of the run branch in the trial merge

Task T-394 (E1-prep-4) brought the fix wave of `fleet/fleetd-finish` into the
trial merge of proof 2 (`merge-main-proof-2.md`, T-330). The method is the
method of ticket 215. Each claim has a label: **verified** (with the
evidence) or **not verified**.

## Tip of the trial branch

| ref | commit | parents | note |
|---|---|---|---|
| `keep/fleetd-finish-trial-merge-main` | `e3574c64` | `873460de` | the result of T-394. Local branch, not pushed |
| trial branch `trial-proof-3` | `e3574c64` | `873460de` | fix: the `hc_doctor` worlds have a git |
| merge 2 of the run branch | `873460de` | `e52a879b`, `3acb0212` | `fleet/fleetd-finish` at `3acb0212`: 7 commits after `8847bb7a` (T-389, T-386, T-396) |
| merge 1 of the run branch | `e52a879b` | `3fd534f7`, `8847bb7a` | `3fd534f7` is the trial of T-330. `8847bb7a` is `fleet/fleetd-finish` at the start of T-394: `git rev-list --count 3fd534f7..8847bb7a` gives 103 |
| `origin/main` | `5863c698` | - | Release v0.5.2. `git fetch origin main` at the start of T-394 gave no new commit |

Verified:
- `git log --format='%h %p'` gives the parents above.
- `git merge-base --is-ancestor origin/main keep/fleetd-finish-trial-merge-main` exits 0.
- `git merge-base --is-ancestor 3acb0212 keep/fleetd-finish-trial-merge-main` and the same for `8847bb7a` and `3fd534f7` exit 0.
- `git merge-tree --write-tree fleet/fleetd-finish keep/fleetd-finish-trial-merge-main` exits 0 for `fleet/fleetd-finish` at `688feefb`.

The run branch moved during T-394. The tip that this proof covers is
`3acb0212`. At the end of T-394 `fleet/fleetd-finish` is `688feefb`, 2
commits after `3acb0212`. They add only `docs/specs/fleetd/gates/ci-run-5.md`
and `docs/specs/fleetd/gates/full-gate-macos-early.md`. They merge with no
conflict (the merge-tree line above): verified. The last small refresh is the
job of E1.

## Conflicts and resolutions

"ours" is the trial, "theirs" is the run branch. Each rule comes from reading
both sides with `git checkout --conflict=diff3`: verified.

| # | merge | file | sides | resolution |
|---|---|---|---|---|
| 1 | 1 (`8847bb7a`) | `crates/horch-core/tests/context_prime.rs` (`World::new`) | ours: `.tempdir_in("/tmp")` with a fallback to the default temp dir (T-312). Theirs: `.tempdir_in(root)`, where `root` is `/tmp` on unix and `temp_dir()` on Windows (T-341, CI4 FIX-38) | both: `.tempdir_in(&root)` with the fallback of ours |
| 2 | 1 (`8847bb7a`) | `crates/horch-e2e/src/harness.rs` (`reap`, the group list) | ours: 1 loop over the herdr state and the `fake-hc` state (main, PR #363), with `is_same_group`. Theirs: `horch_platform::group::is_group_leader(pid, started).unwrap_or(false)` (T-369, D242-F1), herdr state only | both: the loop of ours over the 2 states, with the group check of theirs. `is_same_group` is no longer imported in `reap` |
| 3 | 2 (`3acb0212`) | `crates/horch/src/cmd/doctor.rs` (the test module) | ours: main's `hcb_13_unit_version_floor_checks_hc_and_app`. Theirs: `git_ctx`, `doctor_checks_the_git_version_of_the_rescue`, `doctor_fails_without_git` (T-389) | both test sets |

The harness changes of T-342, T-363 and T-365 merged with no text conflict
(`stop_jobs`, `job_groups`, the time limit of `Harness::run`): verified by
`cargo check` and the e2e rows below.

## Fixes on the trial

| commit | file | why |
|---|---|---|
| `e3574c64` | `crates/horch-e2e/tests/hc_doctor.rs` | T-389 adds a git line to `horch doctor`, and it fails with `git not found on PATH` when the world has no git. Main's `world()` used `Harness::new` without `with_git`, so 3 of its 4 tests failed only in the trial. `world()` now calls `with_git()`. verified: `hc_doctor` 4 / 0 |

Semantic checks with no text conflict (step 3 of the ticket):

| check | result |
|---|---|
| schema version numbers | The fix wave adds no schema step: `git diff 4e806d0b 3acb0212 -- crates/horch-core/src/taskledger/schema.rs` is empty. The new tests of the fix wave use `schema::VERSION`, not a literal. The numbers stay as proof 2 item 4: fleetd steps 10 to 18, `VERSION` 18. verified |
| pane host calls | No added line of the fix wave in `*.rs` names `Herdr`, `PaneHost`, `Backend` or a `pane_` call outside tests (grep of `git diff 4e806d0b 3acb0212 -U0`). `refuse_hc` stays at `cmd/fleetd.rs:128` and `cmd/recipes.rs:228`. `horch fleetd` calls `refuse_hc` before the new git check of T-389. verified (read, and `hc_fleet_start` 2 / 0) |
| `arch_scan` | The fix wave does not change `crates/horch-core/tests/arch_scan.rs`. The skip of `ui-prop-tester` and its guard test `no_crate_of_horch_depends_on_ui_prop_tester` pass. All `fitness_*` tests pass, also `fitness_no_context_touches_foreign_tables` and `fitness_workspace_production_code_has_no_os_cfg_outside_horch_platform`, which were red in proof 2. verified (horch-core suite) |
| `horch doctor` | T-389 adds 1 line, `rescue_git_line`, at the end of `run`. It is not in `check()`, so main's `check()` of the `hc` host is unchanged, and `hcb_13_unit_version_floor_checks_hc_and_app` passes. The line also runs on `hc`. Main's e2e doctor tests needed a git (fix `e3574c64`). verified |
| `cargo check --workspace --all-targets --locked` | exit 0 on `e52a879b` and on `873460de`. verified |

## Result of each suite

Each command ran through `scripts/dev-sandbox.sh`. "first" ran on `e52a879b`
(merge 1). "final" ran on `873460de` (merge 2), except the `hc_doctor` row
after its fix, which ran on `e3574c64`. Load is the 1-minute load average at
the start. A run with a load above 32 used `--test-threads=4`.

| command | first: passed / failed | load | final: passed / failed | load |
|---|---|---|---|---|
| `cargo check --workspace --all-targets --locked` | exit 0 | 20.8 | exit 0 | 26.3 |
| `cargo fmt --all --check` | exit 0 | 18.4 | exit 0 | 30.0 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | exit 0 | 17.8 | exit 0 | 29.8 |
| `cargo test -p horch-core --locked --no-fail-fast` (214 binaries, 23 ignored) | 3818 / 0 | 17.3 | 3818 / 0 (`--test-threads=4`) | 33.1 |
| `cargo test -p horch --locked --no-fail-fast` (45 binaries, 3 ignored) | 618 / 0 | 25.8 | 621 / 0 | 24.1 |
| e2e `fakes` | 61 / 0 | 23.8 | 61 / 0 | 20.8 |
| e2e `hc_courier` | 1 / 0 | 23.0 | 1 / 0 | 22.4 |
| e2e `hc_doctor` | 4 / 0 | 20.6 | 1 / 3, then 4 / 0 on `e3574c64` | 21.5 / 8.7 |
| e2e `hc_fleet_start` | 2 / 0 | 20.4 | 2 / 0 | 21.1 |
| e2e `pane_host_contract` (1 ignored: the real `hc`, opt-in) | 2 / 0 | 20.1 | 2 / 0 | 20.0 |
| e2e `taskledger` | 51 / 0 | 19.8 | 50 / 1 | 19.6 |
| e2e `p0_liveness` | 9 / 0 | 24.1 | 4 / 5 (red before the merge) | 15.1 |
| e2e `fleet_start` | 4 / 0 | 25.3 | 4 / 0 | 14.0 |
| e2e `p6_worker_question` (changed by T-386) | - | - | 9 / 0 | 15.8 |
| e2e `doctor_extras_reviewer` | - | - | 0 / 1 (red before the merge) | not recorded |
| e2e `doctor_extras_watchdog` | - | - | 0 / 2 (red before the merge) | not recorded |
| e2e `restored_pane` | - | - | 1 / 2 (red before the merge) | not recorded |
| e2e `fleet18_delivery_probe` | - | - | 0 / 3 (red before the merge) | not recorded |

Each row: verified (the run logs of T-394). The e2e harness reported no
`leaked N process(es)` line in these runs: verified (grep of the logs).

The 4 rows `doctor_extras_*`, `restored_pane` and `fleet18_delivery_probe`
are not in the list of the ticket. I ran them because they run `horch doctor`
without `with_git`, as `hc_doctor` did.

### `taskledger` `the_whole_flow_on_the_fake_herdr`

The red test of the final `taskledger` row. Each run is the 1 test alone,
unless the row says "file".

| commit | runs | passed | failed | load | time of a failed run |
|---|---|---|---|---|---|
| trial `e52a879b` | 1 (file) | 1 | 0 | 19.8 | - |
| trial `873460de` | 1 (file) | 0 | 1 | 19.6 | - |
| trial `e3574c64` | 1 + 1 (file) | 0 | 2 | 11.8, 23.8 | 691 s alone |
| trial `e3574c64` | 4 | 3 | 1 | 15.4, 5.8, 7.6, 9.7 | 665 s |
| run tip `3acb0212` | 4 | 4 | 0 | 15.2, 5.8, 7.2, 8.2 | - |

| trial `e3574c64`, `TMPDIR` of a kept world | 3 | 3 | 0 | 20.9, 20.9, 20.0 | - |
| trial `e3574c64`, 2 kept runs in parallel | 6 | 6 | 0 | 17.8 to 19.4 | - |

Totals: the trial fails 4 of 17 runs, and the run tip fails 0 of 4 runs.
verified. The orchestrator gave a measured rate for the run branch: about 1
failure in 14 runs, with the same message (ticket 258). Each review task of
T-3 there burns 2 attempts (attempt 1 rejected after 8 s, attempt 2
crashed), and attempt 3 comes after a backoff of about 3 min. That uses
240 s of the 480 s bound. not verified by T-394.

Each failure has the same message: `timed out waiting for T-3 done`
(`taskledger.rs:293`, the 480 s bound of `wait_pipeline`). The test is on
`crates/horch-e2e/expected-failures/taskledger.txt` with another reason
(`the gate tree gate-F-0001 stays open`), so `expected::check` fails it.
verified.

## Red before the merge

Each test below is red on the run tip `3acb0212` without the merge too.
Verified by a run in the scratch worktree `t394-run-tip-baseline` at
`3acb0212`: the same tests fail with the same message.

| test | file | cause |
|---|---|---|
| `an_unreadable_old_record_of_the_polite_stop_is_removed_and_the_pane_is_kept`, `a_reaped_worker_whose_pane_has_no_process_keeps_its_pane_with_1_item`, `a_fleetd_that_dies_after_the_polite_stop_leaves_the_pane_close_to_the_next_fleetd`, `a_reaped_worker_that_ends_on_the_polite_stop_has_its_pane_closed`, `a_pane_that_names_another_process_after_the_polite_stop_is_kept_with_1_item` | `p0_liveness.rs` | `pane_fleet()` makes a world without `with_git`. Since T-389 (`513b73e1`) fleetd refuses its start: `horch fleetd: refused: git not found on PATH`. The test waits 120 s for the fleetd row |
| `doctor_warns_when_the_codex_reviewer_has_no_codex` | `doctor_extras_reviewer.rs` | `horch doctor` fails on its T-389 git line: `git not found on PATH` |
| `doctor_is_quiet_when_no_offered_teammate_uses_such_a_harness`, `doctor_warns_for_a_harness_the_watchdog_cannot_see` | `doctor_extras_watchdog.rs` | the same |
| `a_restored_worker_pane_tells_as_its_role_peeks_its_inbox_and_finishes`, `doctor_and_fleet_warn_while_herdr_resumes_agents_on_restore` | `restored_pane.rs` | the same |
| `g61_doctor_typing_failure_warns_without_failing_or_leaking_workspace`, `g61_doctor_confirms_executed_echo_and_cleans_up`, `g61_doctor_call_bound_follows_the_fault` | `fleet18_delivery_probe.rs` | the same |

13 tests. T-394 told the orchestrator. T-394 did not fix them: the files are
not in its Own, and the fix belongs on the run branch.

T-378 (`full-gate-macos-early.md`, tip `05ff4081`, before T-389) lists 5
other red tests: `g56_later_spawn_receives_recorded_fact`,
`arc_14_provenance_on_spawn_substitute_resume`, 2 `the_hermetic_story` tests
and `spawn_fails_with_the_error_of_a_worker_whose_brief_is_missing`. Their
files are not in the suites of this ticket. T-394 did not run them.

The 5 red items of proof 2 are green in the trial now: the 2 `arch_scan`
tests, the clippy `type_complexity` at `disk.rs:190`,
`watch_once_runs_the_phase4_pass` and
`a_launch_of_an_unknown_teammate_leaves_no_live_row`. verified (the first
and final rows above).

## For the operator

1. **The keep branch** is `e3574c64`. It holds `origin/main` 5863c698 and the
   run branch at `3acb0212`. The 2 later doc commits of the run branch merge
   with no conflict. verified.
2. **T-389 made 13 e2e tests red on the run branch** (section above). The
   worlds of these tests have no git. The fix is on the run branch: each such
   world calls `with_git()`, as `hc_doctor` does on the trial now, or the
   harness gives every world a git. When E1 merges that fix, the trial fix
   `e3574c64` can conflict with it or be needed no more.
3. **`the_whole_flow_on_the_fake_herdr`** fails more often on the trial than
   on the run tip (section "Result of each suite"). See "Not verified".
4. **No new `hc` gap.** The fix wave adds no pane host call, and fleetd still
   refuses `hc` (proof 2, item 1).
5. **Schema numbers** are unchanged since proof 2 (item 4 there).

## Not verified

- The cause of the failures of `the_whole_flow_on_the_fake_herdr` on the
  trial. The 4 failures came in the first 8 runs on `873460de` and
  `e3574c64`. The next 9 runs passed. None of the 9 kept runs failed, so
  T-394 has no failed world to read. The orchestrator bound this work to 45
  minutes. Open questions:
  - Is the rate on the trial higher than on the run branch (4 of 17 against
    about 1 of 14)? 4 runs on the run tip are too few to decide.
  - Do the review tasks of T-3 need more attempts or more time on the trial?
    A failed trial world must give the `attempts` rows of each review task.
  - If yes, which change of main causes it? `git diff 3acb0212 trial-proof-3`
    changes 6 files of fleetd, with 2 to 6 changed lines each. It changes
    `workspace/exec.rs`, `workspace/herdr.rs`, `workspace/host.rs`,
    `messaging/delivery.rs`, `harness/codex.rs` and `harness/launch.rs`. Each
    is on the launch and typing path of a review attempt.
  - E1 must run the test 14 or more times on the trial and on the run tip,
    and read 1 failed world, before it lands the merge.
- `cargo test --workspace --locked` as 1 command, and the e2e files outside
  the table.
- `cargo clippy --workspace --bins --release --locked -- -D warnings` (the CI
  form). The ticket asks for the `--all-targets` form, which is green.
- The suites on `e3574c64` other than `hc_doctor` and the
  `the_whole_flow_on_the_fake_herdr` runs. `e3574c64` changes only
  `hc_doctor.rs` after `873460de`.
- The CI result of the trial.
- The real `hc` contract test `hcb_14_contract_on_a_real_hc_core` (it needs a
  real `hc`, ticket rule 2).
