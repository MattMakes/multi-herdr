# 22: Gate of Phase 3: each rule of the gate green, and the gate file

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** no
**Kind:** quality
**Review:** light
**Check:** `cargo test -p horch-e2e --test expected_failures && test -s docs/specs/fleetd/gates/fleetd-p3.md && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `docs/specs/fleetd/gates/fleetd-p3.md`

Spec: `docs/specs/fleetd/design.md`, "Migration plan" item 5 (*Gate:* and Tests), the tables "What each gate needs", "Where each rule becomes green", "What a release does to a run in flight" and "New work for each phase" (rows Phase 3).

## Goal

Show that the gate of Phase 3 is green: each requirement and failure mode of the gate line has a test that passes and is off the list of expected failures, each test of the gate passes, and the "run in flight" test passes. Write `docs/specs/fleetd/gates/fleetd-p3.md`.

## Requirements

- The gate line: FR-11, RR-9, RR-10, RR-13, RR-14, RR-15, RR-16, RR-17, RR-18, FM-13, FM-25, FM-26, FM-27, FM-28.
- The tests of the gate: a worker killed with dirty and untracked files loses nothing; after a herdr restart, each worktree is rescued and the next attempt uses it again (the full FM-13); a `git worktree add` of 60 s keeps the heartbeat, and the attempt launches only after its `worktree_add` job passed. The release row of Phase 3.
- A rule is green when each of its tests passes and is off the list of expected failures.

## Context

- Read `docs/tickets/fleetd-p3/MAP.md`, section "Open questions", first. It records each choice of this phase, and each ticket follows it.
- The rows and their tests (the ticket that wrote each; a worker may have changed a name: find it in the ticket and the file):

| Row | Test | Ticket |
| --- | --- | --- |
| FR-11 | `fr11_worker_own_git_worktree_add_is_flagged_on_the_next_pass` (`crates/horch-e2e/tests/p3_worktrees.rs`), `fitness_only_worktree_custody_runs_git_worktree_add_or_remove` (`crates/horch-core/tests/arch_scan.rs`), `worktree_row_is_reserved_before_the_folder_exists` | 20, 17, 08 |
| RR-9 | `rr9_rescue_ref_is_read_back_before_removal`, `fm_killed_worker_dirty_untracked_files_on_verified_rescue_ref`, `failed_rescue_verification_keeps_the_tree`, `drop_of_a_clean_tree_writes_a_verified_rescue_ref_before_the_removal` | 07, 06, 06, 12 |
| RR-10 | `rr10_hand_made_worktree_is_adopted_within_1_list_period` (`p3_worktrees.rs`), `i6_worktree_on_disk_without_row_is_adopted_not_deleted`, `worktree_scan_continues_at_its_cursor_after_a_restart` | 20, 09, 10 |
| RR-13 | the 4 tests `rr13_drop_..._is_refused_and_the_tree_stays` (`crates/horch-e2e/tests/p3_refusals.rs`), `pres_04_w6_drop_refuses_dirty_stash_unique_or_moved_head` | 12, 07 |
| RR-14 | `pres_04_l5_unlanded_task_keeps_worktree_and_blocks_dependents`, `rr14_requeue_of_a_parked_task_runs_in_the_same_tree` (`crates/horch-e2e/tests/p3_unlanded.rs`) | 14 |
| RR-15 | `pres_04_l9_unlanded_work_is_listed_and_decided_once`, `rr15_orphaned_and_rescued_trees_are_listed_with_their_rescue_ref` (`p3_unlanded.rs`) | 14 |
| RR-16 | `rr16_second_rescue_of_1_tree_does_not_overwrite_the_first_ref`, `rr16_failed_read_back_keeps_the_folder`, `pres_04_w8_refs_move_forward_and_are_read_back` (`crates/horch-e2e/tests/p3_triage.rs`) | 06, 13 |
| RR-17 | `pres_04_w9_triage_commits_untracked_before_delete`, `rr17_triage_refuses_a_unique_detached_head_and_the_tree_stays` (`p3_triage.rs`) | 13 |
| RR-18 | `rr18_requeue_of_an_escalated_task_sets_its_attempt_count_to_0`, `rr18_review_requeue_keeps_the_attempt_count` (`p3_unlanded.rs`) | 14 |
| FM-13 | `fm13_herdr_restart_rescues_each_worktree_and_the_next_attempt_reuses_it` (`crates/horch-e2e/tests/p0_rescue.rs`), `fm13_herdr_restart_ends_attempts_and_queues_tasks_again` (Phase 1) | 19 |
| FM-25 | `fm25_killed_worker_with_dirty_and_untracked_files_loses_nothing` (`p0_rescue.rs`), `owner_death_marks_the_tree_orphaned_in_the_same_pass` | 19, 09 |
| FM-26 | `fm26_tree_made_outside_fleetd_is_rescued_and_its_files_are_on_the_ref` (`p3_worktrees.rs`), `worker_settings_deny_git_worktree_add_remove_and_prune`, `worker_rules_forbid_git_worktree_add_remove_and_prune` | 20, 17 |
| FM-27 | `fm27_vanished_tree_opens_1_needs_you_item_within_60_s`, `fm27_only_landed_or_rescued_trees_are_removed` (`p3_worktrees.rs`) | 20 |
| FM-28 | `fm28_over_budget_removes_finished_trees_and_holds_dispatch_before_an_active_tree` (`p3_worktrees.rs`), `over_budget_never_removes_an_active_tree_and_holds_dispatch` | 20, 16 |
| Test: killed worker | `fm25_killed_worker_with_dirty_and_untracked_files_loses_nothing` | 19 |
| Test: herdr restart | `fm13_herdr_restart_rescues_each_worktree_and_the_next_attempt_reuses_it` | 19 |
| Test: slow add | `slow_worktree_add_of_60_s_keeps_the_heartbeat_and_the_tree_is_used_only_after_its_job_passed` (`p3_worktrees.rs`), `slow_worktree_add_job_keeps_the_pass_under_its_time_budget` | 20, 10 |
| Run in flight (release row) | `live_tree_and_dirty_orphan_on_phase_2_are_active_and_listed_and_nothing_is_removed_after_the_update` (`crates/horch-e2e/tests/p3_release.rs`) | 21 |

- The lines of Phase 3 in `crates/horch-e2e/expected-failures/p0_rescue.txt` are gone after ticket 19. Each line of a later phase stays.
- The runs, as in Phase P (`docs/specs/fleetd/gates/fleetd-pp.md`, a decision of the orchestrator): macOS runs on this Mac; Linux runs in Docker on this Mac (the image and the command of that file), and it runs only the tests of the table, never the whole workspace; remove the Docker volume after it. Windows is "pending CI" unless a pull request of `fleet/fleetd-build` has a CI run. You can read CI and not start it: `gh run list --repo MattMakes/multi-herdr --branch fleet/fleetd-build`, `gh run view <id> --repo MattMakes/multi-herdr --log-failed`. A row is green on an OS only with a run of that OS. On Windows, codex workers get no forbidden rule (ticket 17, Q16): there the FM-26 row rests on the reconciler test. Say so in the row.
- `docs/specs/fleetd/gates/fleetd-pp.md` shows the form of a gate file.
- Self-hosting: run no reconciler, rescue, cleanup or `git worktree remove` on this repository or its `.worktrees/` folder, and no development `horch` on the live ledger.

## Steps

1. Run `cargo test --workspace --locked` on this Mac. Run each test of the table by name and see it pass. Check that no line of Phase 3 is left in `crates/horch-e2e/expected-failures/`.
2. Run the tests of the table in Docker for Linux. Find a CI run of the tip of `fleet/fleetd-build`, if one exists, and read the result of each test on Windows. Do not guess a result.
3. Correct each small fault. A fault in a file outside `Own`, or a fault that is not small (more than about 50 lines, or a change of behavior), goes to the orchestrator as a QUESTION with a proposed fix task.
4. Write `docs/specs/fleetd/gates/fleetd-p3.md`: 1 row for each row of the table, with its test names, the commit, the run (this Mac, Docker, or the CI run id) and the result on macOS, Linux and Windows. Add each flaky test that you saw, and the state of each open question of `MAP.md` that is for the operator.
5. Run the Check. Commit.

## Constraints

1. This fleet runs on the installed `horch` binary and on the live ledger
   `/Users/MASCOTT/.horch/ledger.sqlite3`. The code that you change is the code of that tool.
   Do not break the fleet that builds it:
   - Each test, and each `horch` or `fleetd` process that you build and start, uses temporary
     folders for `HORCH_DATA_DIR`, `HORCH_STATE_DIR` and `HORCH_LEDGER_DB`. Never point a
     development build at the live ledger or at the live state folders.
   - Never install a development build. Never run `horch self-update`, `cargo install` or a
     release script from a worktree.
   - Never start a development `fleetd`, watchdog or service unit on this machine outside a test
     that uses temporary folders. Never register a launchd job, a systemd unit or a scheduled task
     on this machine.
   - The only `horch` commands that you run on the live fleet are the worker commands of your own
     task (`horch ledger ...`, `horch tell orchestrator ...`).
2. NEVER use `ANTHROPIC_API_KEY`. Never read, print, set, export, pass or depend on it. Never
   write code, config, docs or teammate files that use it. Each `claude` run uses the claude.ai
   subscription login. Remove the variable from each `claude` command:
   `env -u ANTHROPIC_API_KEY claude ...`. If a task seems to need an API key, stop and ask.
3. Do not use a shared `CARGO_TARGET_DIR`. Build in the `target/` folder of your worktree. Run
   `cargo clean` in your worktree before `horch ledger finish` only if the disk has less than
   20 GB free (`df -h /`).
4. Before the first commit, run `git remote -v` and `git config user.email`. The remote must be
   `git@git-alt:MattMakes/multi-herdr.git` and the email must be `m4ttplu5@gmail.com`. If not,
   stop and send BLOCKED.
5. Commit messages have no co-author line. Never write the em dash character; use `-`. Never edit
   `CHANGELOG.md` or a generated file by hand.
6. Write the tests first and see them fail for the correct reason. Then write the code.
7. The source of truth is `docs/specs/fleetd/design.md` (FINAL). If the code of today and the
   design do not agree on a point that the ticket does not settle, stop and send QUESTION. Do not
   change `docs/specs/fleetd/` or `ddd/`.
8. Change only the files of `Own`. If the work needs another file, send QUESTION.
9. Prefer quality, simplicity and robustness to speed. No feature that the design does not name.
10. If the GitHub SSH connection times out at `horch ledger finish`, run it again, 3 times maximum.

Self-hosting: each test makes its own temporary git repository and temporary folders (build rule 1). No test and no worker runs the reconciler, a rescue, a cleanup or a `git worktree remove` on the repository `/Users/MASCOTT/projects/mh/multi-herdr` or on its `.worktrees/` folder.

## Done when

The Check passes. `docs/specs/fleetd/gates/fleetd-p3.md` has 1 row for each row of the gate with its tests and their results, and each rule of the gate line is green on macOS.
