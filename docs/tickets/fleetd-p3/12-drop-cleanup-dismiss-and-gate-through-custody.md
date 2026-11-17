# 12: Drop, cleanup, dismiss and the run gate keep their refusals and remove a tree only through the rescue and remove jobs

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib taskledger && cargo test -p horch-core --lib fleetd::worktree_custody && cargo test -p horch-e2e --test p3_refusals --test taskledger --test fleet18_brief_worktree --test done_budget --test i308_fleet_steward && cargo test -p horch --test w3_ledger_cli_pins && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/taskledger/worktree_ops.rs`, `crates/horch-core/src/taskledger/finish/land.rs`, `crates/horch-core/src/taskledger/import.rs`, `crates/horch-core/src/taskledger/runs.rs`, `crates/horch-core/src/fleetd/worktree_custody/adapters/git.rs`, `crates/horch-e2e/tests/p3_refusals.rs`

Spec: `docs/specs/fleetd/design.md`, "Worktrees: leased, reconciled, never lost" (the rows `active`, `landing`, `landed`; "Only fleetd creates or removes worktrees"; "Rescue before any cleanup"), RR-9, RR-13, FM-27, "Preservation analysis" ("Rows 04 W6, L5, L9, W8, W9 and L8 are requirements RR-13 to RR-18"); `docs/specs/fleetd/preservation-04-behavior.md` rows W6, W11 and L9.

## Goal

Each path of today that removes a worktree folder keeps each of its refusals, and then removes the tree only through custody: the tree is `landed`, or it is rescued first by a verified rescue job, and then 1 `worktree_remove` job removes it. The finish steps move the tree to `landing` and `landed`. No path of this ticket runs `git worktree add` or `remove` itself any more.

## Requirements

- **RR-13**: `horch ledger worktree drop` and each cleanup refuse a dirty tree, a branch that a `git stash` entry names, unpublished unique commits and a HEAD that moved during the checks. Test: a drop for each of the 4 conditions is refused, and the tree stays.
- **RR-9** (the paths of today): no worktree is removed until its work is landed or on a verified rescue ref.
- **FM-27**: inside the fleet, only `landed` trees or trees with a verified rescue ref are removed.

## Context

- Read `docs/tickets/fleetd-p3/MAP.md`, section "Open questions", first (Q7, Q10 and Q15 apply here).
- The paths, each with its refusals of today (keep each one, and keep its words):
  - `worktree_ops::drop_worktree` and `verified_head` (`horch ledger worktree drop`, row W6).
  - `finish/land.rs`: the land step and `cleanup`, `delete_worktree`, `cleanup_failed`, `reopen_landing`, `reopen_if_new_commits`.
  - `import.rs`: `decide` for `horch ledger dismiss --cleanup` (row L9: it removes only the recorded path, through the safe removal).
  - `runs.rs`: the run gate folder `.worktrees/_gate-<slug>` (row W11): `checkout_tip` adds it with `git worktree add --detach`, and `remove_gate_worktree` removes it at close.
- The custody calls (tickets 05, 06, 07 and 08): `run_now`, the rescue observation, `queue_remove` and its observation. A command never holds a transaction while a job runs.
- The flows (Q10):
  - The land step starts: `active` -> `landing` through `transition()`. A red land and new commits keep `landing` (today's `status` goes back to `open` as today). The land passed: `landing` -> `landed`, with today's `landed_commit`.
  - Cleanup after the land: `queue_remove` of the `landed` row, then `run_now`, then the observation. A refusal of the guard of ticket 07 keeps today's `cleanup_failed` status and its reason.
  - Drop: today's refusals first. Then: `active` -> `orphaned` (cause `dropped`), a rescue job (`run_now`), then `queue_remove` of the `rescued` row and `run_now`. With `--keep-branch`, the branch stays as today. A scratch tree follows the same flow.
  - `dismiss --cleanup`: the same flow as drop, on the recorded path only.
  - The run gate folder: add it through a `worktree_add` job in a detached form. Add the detached form to the executor in `adapters/git.rs` (`git worktree add --detach <path> <tip>`, for a row of the gate only). Remove it at close through the rescue and remove flow; `checkout_tip` of a folder that exists stays a `git checkout` (not a worktree call).
- The e2e tests run `horch` as the CLI does, with a temporary repository, `HORCH_LEDGER_DB`, `HORCH_DATA_DIR` and `HORCH_STATE_DIR` in temporary folders. Use the harness of `crates/horch-e2e` (for example the helpers of `crates/horch-e2e/tests/p0_rescue.rs`). No fleetd runs: the command runs its jobs with `run_now`.

## Steps

1. Write the tests first, in `crates/horch-e2e/tests/p3_refusals.rs`. Each fails before the code exists, or names the old path that it replaces:
   - `rr13_drop_of_a_dirty_tree_is_refused_and_the_tree_stays`.
   - `rr13_drop_of_a_branch_with_a_stash_entry_is_refused_and_the_tree_stays`.
   - `rr13_drop_of_unpublished_unique_commits_is_refused_and_the_tree_stays`.
   - `rr13_drop_with_a_head_that_moved_during_the_checks_is_refused_and_the_tree_stays`.
   - `drop_of_a_clean_tree_writes_a_verified_rescue_ref_before_the_removal`.
   - `cleanup_after_land_removes_a_landed_tree_through_the_remove_job`.
   - `cleanup_of_a_landed_tree_with_an_untracked_file_is_refused_and_marked_cleanup_failed`.
   - `dismiss_cleanup_rescues_then_removes_only_the_recorded_path`.
   - `run_gate_folder_is_added_and_removed_through_the_jobs`.
   For each refusal: each modified file, each untracked file, each commit and each stash entry is still there after it.
2. Write the code. Keep the tests of `crates/horch-e2e/tests/taskledger.rs`, `fleet18_brief_worktree.rs`, `done_budget.rs`, `i308_fleet_steward.rs` and `crates/horch/tests/w3_ledger_cli_pins.rs` green; change a test only where an output line changes, and name it in the commit message.
3. Run the Check. Commit.

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

The Check passes. Drop, cleanup, dismiss and the run gate keep each refusal of today, and each removal is a `worktree_remove` job of a `landed` tree or of a tree with a verified rescue ref.
