# The gate of Phase 3

The gate: FR-11, RR-9, RR-10, RR-13, RR-14, RR-15, RR-16, RR-17, RR-18, FM-13, FM-25, FM-26, FM-27
and FM-28 green (`docs/specs/fleetd/design.md`, Migration plan, item 5). The tests of the gate: a
worker killed with dirty and untracked files loses nothing; after a herdr restart, each worktree is
rescued and the next attempt uses it again (the full FM-13); a `git worktree add` of 60 s keeps the
heartbeat, and the attempt launches only after its `worktree_add` job passed. The release row of
Phase 3 is in "What a release does to a run in flight".

A rule is green when each of its tests passes and is off the list of expected failures.

## The commit

- Base: `193beacd`, the tip of `fleet/fleetd-build` when this ticket started. It holds the gate of
  Phase 2 (T-273) and T-691 (each git command of a fleetd custody job runs in the platform
  sandbox).
- Gate: `d84c9ddd`. It adds 2 commits of this ticket to the base:
  - `b6bffd23`: an imported worktree row writes its state (see "The state of a new row" below).
  - `d84c9ddd`: the doc comment of `CliRemoval` (`remove_git.rs`) names
    `<fleet root>/tmp/<worktree id>/` for the private index files, not `<fleet root>/run`.

## The runs

- macOS: macOS aarch64, this Mac. `cargo test --workspace --locked --no-fail-fast` at the base.
  Then, at `d84c9ddd`, `cargo build --workspace --bins` and 1 run of each test target of the gate:
  `horch-core --lib`; the `horch-core` targets `arch_scan`, `worktree_create`, `worktree_remove`,
  `worktree_rescue`, `worktree_reconcile`, `fleetd_pass_p3`, `worktree_disk`,
  `w3_taskledger_core_import`, `nfr`, `architecture_doc`; the `horch-e2e` targets `p3_worktrees`,
  `p3_refusals`, `p3_unlanded`, `p3_triage`, `p0_rescue`, `p3_release`, `expected_failures`.
- Linux: not run. Docker on this Mac created the container (`rust:1.96.1`, `--privileged`, with
  `apt-get install bubblewrap` in the script) and did not start it, 2 times on 2026-11-18.
  `docker inspect` of the container hung. Docker Desktop was not restarted, because other
  containers of the operator run in it. The script is the one of the Phase 2 gate, with the
  targets above and 3 runs of the `horch-e2e` targets.
- Windows: pending CI. No pull request exists for `fleet/fleetd-build`, and
  `gh run list --repo MattMakes/multi-herdr --branch fleet/fleetd-build` is empty on 2026-11-18.
  This Mac cannot compile or run the Windows code.

As in Phase P and Phase 2, a row is green on an OS only with a run of that OS.

## The whole workspace on macOS

| Record | ok | failed | ignored |
| --- | --- | --- | --- |
| base `193beacd` (the run compiled an early form of `b6bffd23`) | 3722 | 2 | 12 |

The 2 failures:

1. `no_status_literal_in_ledger_sql` (`w5_status_words_scan.rs`). The cause was the first form of
   the fix of `import.rs`, which wrote `'cleaned'` in SQL. `b6bffd23` builds the state from the
   enums. The test passes at `d84c9ddd`.
2. `workspace::herdr::tests::a_call_past_the_deadline_is_killed` ("herdr did not answer within
   299 ms"). It is not a test of this gate. It passed 5 of 5 times alone and in the `--lib` run at
   `d84c9ddd` (1660 passed, 0 failed). It is flaky under the load of the whole workspace.

The list of expected failures: `crates/horch-e2e/expected-failures/` has no line of Phase 3. The
lines that stay are of Phase 4 and Phase 6 (`p0_dispatch.txt`, `p0_ideas.txt`, `p0_landing.txt`).
`cargo test -p horch-e2e --test expected_failures` passes (1 of 1).

The guard tests pass on macOS at `d84c9ddd`: `cargo test -p horch-core --test nfr --test arch_scan
--test architecture_doc`.

## The rows

Each test passes on macOS in the run at `d84c9ddd`. Each Linux cell is "not run" (see The runs).
Each Windows cell is "pending CI".

| Row | Tests | Commit | macOS (this Mac) | Linux | Windows |
| --- | --- | --- | --- | --- | --- |
| FR-11 | `fr11_worker_own_git_worktree_add_is_flagged_on_the_next_pass` (`crates/horch-e2e/tests/p3_worktrees.rs`); `fitness_only_worktree_custody_runs_git_worktree_add_remove_or_prune` (`crates/horch-core/tests/arch_scan.rs`); `worktree_row_is_reserved_before_the_folder_exists` (`crates/horch-core/tests/worktree_create.rs`) | `d84c9ddd` | green: 3 of 3 pass | not run | pending CI |
| RR-9 | `rr9_rescue_ref_is_read_back_before_removal` (`worktree_remove.rs`); `fm_killed_worker_dirty_untracked_files_on_verified_rescue_ref`, `failed_rescue_verification_keeps_the_tree` (`worktree_rescue.rs`); `drop_of_a_clean_tree_writes_a_verified_rescue_ref_before_the_removal` (`p3_refusals.rs`) | `d84c9ddd` | green: 4 of 4 pass | not run | pending CI |
| RR-10 | `rr10_hand_made_worktree_is_adopted_within_1_list_period` (`p3_worktrees.rs`); `i6_worktree_on_disk_without_row_is_adopted_not_deleted` (`worktree_reconcile.rs`); `worktree_scan_continues_at_its_cursor_after_a_restart` (`fleetd_pass_p3.rs`) | `d84c9ddd` | green: 3 of 3 pass | not run | pending CI |
| RR-13 | `rr13_drop_of_a_dirty_tree_is_refused_and_the_tree_stays`, `rr13_drop_of_a_branch_with_a_stash_entry_is_refused_and_the_tree_stays`, `rr13_drop_of_unpublished_unique_commits_is_refused_and_the_tree_stays`, `rr13_drop_with_a_head_that_moved_during_the_checks_is_refused_and_the_tree_stays` (`crates/horch-e2e/tests/p3_refusals.rs`); `pres_04_w6_drop_refuses_dirty_stash_unique_or_moved_head` (`worktree_remove.rs`) | `d84c9ddd` | green: 5 of 5 pass | not run | pending CI |
| RR-14 | `pres_04_l5_unlanded_task_keeps_worktree_and_blocks_dependents`, `rr14_requeue_of_a_parked_task_runs_in_the_same_tree` (`crates/horch-e2e/tests/p3_unlanded.rs`) | `d84c9ddd` | green: 2 of 2 pass | not run | pending CI |
| RR-15 | `pres_04_l9_unlanded_work_is_listed_and_decided_once`, `rr15_orphaned_and_rescued_trees_are_listed_with_their_rescue_ref` (`p3_unlanded.rs`) | `d84c9ddd` | green: 2 of 2 pass | not run | pending CI |
| RR-16 | `rr16_second_rescue_of_1_tree_does_not_overwrite_the_first_ref` (`worktree_rescue.rs`); `rr16_failed_read_back_keeps_the_folder`, `pres_04_w8_refs_move_forward_and_are_read_back` (`crates/horch-e2e/tests/p3_triage.rs`) | `d84c9ddd` | green: 3 of 3 pass | not run | pending CI |
| RR-17 | `pres_04_w9_triage_commits_untracked_before_delete`, `rr17_triage_refuses_a_unique_detached_head_and_the_tree_stays` (`p3_triage.rs`) | `d84c9ddd` | green: 2 of 2 pass | not run | pending CI |
| RR-18 | `rr18_requeue_of_an_escalated_task_sets_its_attempt_count_to_0`, `rr18_review_requeue_keeps_the_attempt_count` (`p3_unlanded.rs`) | `d84c9ddd` | green: 2 of 2 pass | not run | pending CI |
| FM-13 | `fm13_herdr_restart_rescues_each_worktree_and_the_next_attempt_reuses_it`, `fm13_herdr_restart_ends_attempts_and_queues_tasks_again` (`crates/horch-e2e/tests/p0_rescue.rs`) | `d84c9ddd` | green: 2 of 2 pass | not run | pending CI |
| FM-25 | `fm25_killed_worker_with_dirty_and_untracked_files_loses_nothing` (`p0_rescue.rs`); `owner_death_marks_the_tree_orphaned_in_the_same_pass` (`worktree_reconcile.rs`) | `d84c9ddd` | green: 2 of 2 pass | not run | pending CI |
| FM-26 | `fm26_tree_made_outside_fleetd_is_rescued_and_its_files_are_on_the_ref` (`p3_worktrees.rs`); `worker_settings_deny_git_worktree_add_remove_and_prune` (`harness/claude.rs`); `worker_rules_forbid_git_worktree_add_remove_and_prune` (`harness/codex.rs`) | `d84c9ddd` | green: 3 of 3 pass | not run | pending CI. On Windows, codex workers get no forbidden rule (MAP Q16, ticket 17), so there the row rests on the reconciler test `fm26_tree_made_outside_fleetd_is_rescued_and_its_files_are_on_the_ref`. Windows also runs no custody git job (see The sandbox), so on Windows that tree is found and not rescued. |
| FM-27 | `fm27_vanished_tree_opens_1_needs_you_item_within_60_s`, `fm27_only_landed_or_rescued_trees_are_removed` (`p3_worktrees.rs`) | `d84c9ddd` | green: 2 of 2 pass | not run | pending CI |
| FM-28 | `fm28_over_budget_removes_finished_trees_and_holds_dispatch_before_an_active_tree` (`p3_worktrees.rs`); `over_budget_never_removes_an_active_tree_and_holds_dispatch` (`worktree_disk.rs` and `worktree_custody/domain/disk.rs`) | `d84c9ddd` | green: 3 of 3 pass | not run | pending CI |
| Test: killed worker | `fm25_killed_worker_with_dirty_and_untracked_files_loses_nothing` | `d84c9ddd` | green | not run | pending CI |
| Test: herdr restart | `fm13_herdr_restart_rescues_each_worktree_and_the_next_attempt_reuses_it` | `d84c9ddd` | green | not run | pending CI |
| Test: slow add | `slow_worktree_add_of_60_s_keeps_the_heartbeat_and_the_tree_is_used_only_after_its_job_passed` (`p3_worktrees.rs`); `slow_worktree_add_job_keeps_the_pass_under_its_time_budget` (`fleetd_pass_p3.rs`). In Phase 3 "the attempt launches only after its job passed" is: `horch ledger worktree new` gives the path only after the `worktree_add` job passed (MAP Q17). | `d84c9ddd` | green: 2 of 2 pass | not run | pending CI |
| Run in flight (release row of Phase 3) | `live_tree_and_dirty_orphan_on_phase_2_are_active_and_listed_and_nothing_is_removed_after_the_update` (`crates/horch-e2e/tests/p3_release.rs`) | `d84c9ddd` | green | not run | pending CI |
| The state of a new row (check of the orchestrator) | `fitness_each_worktree_insert_writes_its_state`, `worktree_insert_matcher_finds_an_insert_without_state` (`arch_scan.rs`); `an_imported_cleaned_tree_is_removed_and_a_new_tree_takes_its_path`, `a_tree_made_by_hand_at_the_path_of_an_imported_cleaned_tree_is_an_orphan` (`crates/horch-core/tests/w3_taskledger_core_import.rs`) | `b6bffd23`, review F1 | green: 4 of 4 pass; each failed before its fix | not run | pending CI |

The gate of Phase 3 is green on macOS. The gate is not complete until Linux and Windows have runs.

## The state of a new row

The column `worktrees.state` has the default `orphaned` as a bridge (T-282). A row that takes the
default lists a live tree as orphaned. The production writers of a `worktrees` row:

- `worktree_ops.rs`, `finish/mod.rs`, `triage/apply.rs` and `runs.rs` insert through worktree
  custody (`worktree_custody/adapters/sqlite.rs`, `insert_reserved` or `insert_orphan`), which
  writes the state.
- `taskledger/import.rs` (`insert_worktree`, the import of an old JSON claim) used the default.
  So an imported `cleaned` tree was `orphaned`, not `removed`. That row held the partial unique
  index of its path, and a new tree at that path failed. `b6bffd23` writes the state of the
  release backfill (MAP Q19): `cleaned` is `removed`, each other status is `orphaned`.
  The import also counts a tree as known only when its row is not `removed` (review F1), so a
  tree made by hand at the path of a `removed` row is reported as an orphan.

The new `arch_scan` rule `fitness_each_worktree_insert_writes_its_state` fails each production
`INSERT INTO worktrees` that does not name `state`. `docs/architecture.md` lists it.

The comment of `phase_3()` in `crates/horch-core/src/taskledger/schema.rs` still says "`import.rs`,
`insert_worktree` of an imported tree: no ticket". That file is not in `Own`, so this ticket does
not change it.

## The sandbox (T-691)

T-691 is in the base: each git command of a custody job (`worktree_add`, `worktree_list`,
`rescue`, `worktree_remove`) runs in the sandbox of the platform. macOS uses `sandbox-exec`.
Linux uses `bwrap`, and needs the package `bubblewrap` on the `PATH` and a kernel that lets it make
a user namespace. Facts of the hosts:

- A host with no sandbox (Windows, or Linux with no `bwrap`): each custody job runs no git and
  fails with the text `no sandbox`. A failed `worktree_add` opens 1 needs-you item
  `git_sandbox:worktree_add` for each job kind, not 1 for each job. So no tree is made, and no tree
  is rescued or removed: each tree stays on disk.
- Windows runs no custody git job.
- Git 2.41 or newer is needed. An older git has no `--attr-source`, and the rescue snapshot fails
  with a message that names 2.41.
- The job sandbox hides `HOME`, except the install folder of git (after each link). A git shim below
  `HOME` (for example a shim script of a version manager) fails in the job sandbox.
- CI: `.github/workflows/ci.yml` installs `bubblewrap` on Linux and sets
  `kernel.apparmor_restrict_unprivileged_userns=0`. No change was needed.
- Without `bwrap`, 2 tests fail on Linux: `worktree_add_attaches_an_existing_branch_that_is_at_the_base_tip_of_its_row`
  and `worktree_add_refuses_an_existing_branch_that_is_not_at_the_base_tip_of_its_row`
  ("bwrap is not on PATH"), as the gate of Phase 2 found.

## The stand-in agents of the e2e tests

The gate of Phase 2 found that `/bin/sh -c 'set -m; ...'` makes no process group on Linux (`dash`),
so a signal to the group reached nothing. No Phase 3 e2e file uses `set -m`. In `p3_worktrees.rs`,
`p0_rescue.rs` and `p3_release.rs`, each stand-in agent is a direct child `/bin/sleep 600`, and the
test stops it with `Child::kill`, which sends `SIGKILL` to its own pid on each Unix. The `/bin/sh`
scripts of `p3_refusals.rs`, `p3_triage.rs`, `p3_unlanded.rs` and `p3_worktrees.rs` (line 735) are
git or check shims that `exec` or exit; no test signals them. This ticket has no Linux run to show
it on Linux.

## Deviations from the design and facts of this phase

1. Design line 1055 (`docs/specs/fleetd/design.md`, "Rescue before any cleanup") puts the rescue
   index at `/fleet/run/rescue-T-0007-03.index`. The code puts the 2 private indexes under
   `<fleet root>/tmp/<worktree id>/` (`rescue_git.rs`). This ticket does not change
   `docs/specs/fleetd/`.
2. Paths, branch names and rescue ref names keep the forms of today (MAP Q4, Q5):
   `refs/horch/rescue/<worktree id>/<n>`, not `refs/horch/rescue/T-0007-03/1`.
3. `status` and `state` are 2 columns until Phase 4 (MAP Q3).
4. Phase 3 deletes no rescue ref and saves no ignored file (MAP Q6).
5. `vcs/landing.rs` (the claims of `horch worktree new` and `land`) keeps its git calls in Phase 3,
   the 1 exemption of the `arch_scan` rule of FR-11 (MAP Q15).
6. The attempt starts before its tree; `horch ledger worktree new` waits for the `worktree_add`
   job (MAP Q1, Q17).
7. On Windows, codex has no forbidden rule (MAP Q16), and no custody git job runs.

## The open questions of MAP.md

MAP.md says that the orchestrator has a NOTE of Q1, Q8 and Q9 for the operator. MAP.md records no
answer of the operator to them.

- Q1, who runs the `worktree_add` job before Phase 4: open. The code follows the choice of MAP.md
  (the worker commands queue and wait for the job).
- Q8, when fleetd removes a tree in Phase 3: open. The code follows the choice (a `landed` tree,
  and over the budget the `landed` and `rescued` trees of a `done` or `abandoned` run).
- Q9, report-only then acting: open. Ticket 18 turned the queues on; the release test checks that
  nothing is removed.
- Q25, the time limit of a rescue job: answered by the orchestrator (`job_limit_s.rescue` = 600 s).

## Flaky tests

- `workspace::herdr::tests::a_call_past_the_deadline_is_killed` (not a test of this gate): 1
  failure in the whole-workspace run, 6 passes after it (5 alone, 1 in the `--lib` run).
- Each test of the gate passed in its 1 run at `d84c9ddd` on macOS. No test of the gate was seen
  flaky.
