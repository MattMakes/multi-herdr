# 08: horch ledger worktree new and scratch write the reserved row first and run the worktree_add job

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd::worktree_custody && cargo test -p horch-core --lib taskledger && cargo test -p horch-core --test worktree_create && cargo test -p horch-e2e --test fleet18_brief_worktree --test taskledger --test worktree --test done_budget && cargo test -p horch --test w3_ledger_cli_pins && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/worktree_custody/application/create.rs`, `crates/horch-core/src/taskledger/worktree_ops.rs`, `crates/horch-core/src/taskledger/finish/mod.rs`, `crates/horch-core/tests/worktree_create.rs`

Spec: `docs/specs/fleetd/design.md`, "Worktrees: leased, reconciled, never lost" (the rows `reserved` and `created`; "Only fleetd creates or removes worktrees"), "Supervision, liveness and reconciliation" ("A pass has 3 parts: the row first, then the effect, then the observation"; the row `worktree_add` of "The kinds of job"), "Migration plan" item 5, FR-11; `docs/specs/fleetd/preservation-04-behavior.md` rows W1, W2 and W4; `ddd/08-code/worktree-custody/design.md` §3 (`CreateWorktree`) and §6 (`worktree-created`, `task-leased`).

## Goal

A worktree is a ledger row before it exists. `horch ledger worktree new <name>` and `horch ledger worktree scratch <name>` write the row in `reserved` and queue 1 `worktree_add` job in 1 transaction. The `git worktree add` runs in that job, outside the transaction. The command returns only after the job passed and the row is `created` and then `active`, owned by the live attempt of the worker. The output of the command does not change.

## Requirements

- **FR-11**: each worktree is leased in the ledger, tied to 1 task and 1 owner, before it is created.
- **I6** (the first half): no folder of a worktree exists before its row.
- Row W1 changes as the design says: `git worktree add` leaves the transaction. Rows W2 and W4 stay: an existing tree of the task is reused and moves to the new owner, its base does not change without a drop; a scratch tree has no task.
- The test of the gate "the attempt launches only after its `worktree_add` job passed" in its Phase 3 form (Q17): the command does not return a path before the job passed.

## Context

- Read `docs/tickets/fleetd-p3/MAP.md`, section "Open questions", first (Q1, Q4, Q10, Q17 and Q23 apply here).
- Today: `finish::worktree_new` (`crates/horch-core/src/taskledger/finish/mod.rs`) with `create_worktree`, and `worktree_ops::create` (`crates/horch-core/src/taskledger/worktree_ops.rs`, for `--from` and scratch trees) run `git worktree add` inside `store.immediate(...)`, and remove the folder and the branch when the row write fails (W1). Names, paths and branch names of today stay (Q4): `<project>/.worktrees/T<seq>-<name>` on `<run branch>-T<seq>-<name>`; `_scratch/<name>` on `fleet-scratch-<run>-<name>`.
- The use case, in `application/create.rs`, `create_worktree(tx, new) -> CreateOutcome`:
  - The task has a row that is not `removed` (W2): no new row. A row in `created` or `rescued` moves to `active` with the new owner; a row in `active` or `landing` changes its owner (today's `UPDATE worktrees SET worker_id`), and `owner_attempt` follows. A row in `orphaned` is not taken: the command first runs its rescue (ticket 06, `run_now`) and then takes the `rescued` row. A row in `reserved` returns its active job. A request for another base is refused as today.
  - Else: 1 row in `reserved` (ticket 03 store, `insert_reserved`, with `worker_id`, `base`, `base_commit`, today's `status` `open` and the `worktree_modes` row), and 1 `worktree_add` job through `jobs::queue`, in the same transaction.
  - Write the move of an existing row to a new owner as 1 public function, for example `attach(tx, row, worker, attempt) -> Attach`, that returns `Attached` or `NeedsRescue`. Ticket 11 calls it from the claim and the handoff.
  - Handling the same request 2 times has the effect of 1 (`idempotent: task-leased`, `ddd/08-code/worktree-custody/design.md` §6): the second call finds the row and its job.
- The observation, in `application/create.rs`, a function that the pass (ticket 09) and the command both call in 1 transaction: a `passed` `worktree_add` run and a folder that exists -> `created`, then `active` with the owner of the request. Its event carries exactly `worktree_id`, `task_id`, `path` and `branch` (`contract: worktree-created`). A `failed` run -> the door drops the `reserved` row (ticket 03, `drop_reserved`, cause `worktree_add_failed`), and the command prints the reason of the job. An `abandoned` run -> the row stays `reserved` and the next run follows the rule of `abandoned` runs.
- The command (`worktree_new`, `worktree_ops::create`): 1 transaction for `create_worktree`, then `run_now` (ticket 05) with no transaction open, then 1 transaction for the observation. Then print today's lines. The CLI files do not change (`crates/horch/src/cmd/taskledger.rs` calls these functions).
- The owner: the `attempts` row in `live` of the worker (Phase 1). A worker with no live attempt (a test that plays the agent) gives a null owner and the row is `active` with its `worker_id`, as today.
- Before Phase 5, a worker command writes the ledger directly, through `transition()` ("Who writes the ledger before Phase 5").

## Steps

1. Write the tests first, in `crates/horch-core/tests/worktree_create.rs`, each with its own temporary repository and ledger. Each fails before the code exists:
   - `worktree_row_is_reserved_before_the_folder_exists`: a fake gateway records that the row was committed in `reserved` before `git worktree add` ran, and that no transaction was open while it ran.
   - `attempt_waits_for_worktree_add_job_and_lease_starts_at_launch`: the command does not return a path before the job passed (a slow fake add).
   - `failed_worktree_add_drops_the_reserved_row_and_abandoned_runs_again`.
   - `create_worktree_twice_has_the_effect_of_once` (`idempotent: task-leased`).
   - `published_worktree_created_carries_worktree_id_task_id_path_branch` (`contract: worktree-created`).
   - `retry_reuses_the_same_worktree` (W2).
   - `orphaned_tree_is_rescued_before_the_next_attempt_takes_it`.
   - `scratch_tree_has_no_task_and_goes_through_the_job`.
2. Write the code. Keep each test of `crates/horch-e2e/tests/fleet18_brief_worktree.rs`, `taskledger.rs` and `crates/horch/tests/w3_ledger_cli_pins.rs` green with no change.
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

The Check passes. `horch ledger worktree new` writes the `reserved` row and its job first, runs `git worktree add` with no transaction open, and prints the same lines as today.
