# 11: The claim and the handoff give the tree of the task to the next attempt, and the rescue ref is named

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib taskledger::tasks && cargo test -p horch-core --test worktree_reuse && cargo test -p horch-e2e --test taskledger --test fleet18_brief_worktree && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/taskledger/tasks.rs`, `crates/horch-core/tests/worktree_reuse.rs`

Spec: `docs/specs/fleetd/design.md`, "Worktrees: leased, reconciled, never lost" ("Reuse, don't redo"; the rows `active`, `orphaned` and `rescued`), FM-13 ("rescues their worktrees and retries in them"), FM-25 ("the next attempt reuses the tree"), FM-14 ("a worktree has one owner"); `docs/specs/fleetd/preservation-04-behavior.md` rows W2 and W3.

## Goal

The next attempt of a task works in the same tree as the last one. When a worker claims a task, or takes a task by handoff, the tree of the task gets that worker's live attempt as its owner. A tree that is `orphaned` is rescued first. The claim prints the rescue ref of the tree when it has one, so the worker knows the safety copy.

## Requirements

- **FM-25** and **FM-13** (the reuse): the next attempt reuses the tree, with the rescue ref as a safety copy.
- **FM-14** (the tree part): a worktree has 1 owner. The owner moves to the new attempt only after the old attempt has ended.
- Rows W2 and W3 stay: claim and handoff move the held tree to the next worker, and a reap leaves the tree with its files.

## Context

- Read `docs/tickets/fleetd-p3/MAP.md`, section "Open questions", first (Q10 and Q17 apply here).
- Today: `take_task` (`crates/horch-core/src/taskledger/tasks.rs`, called by `claim`) and `handoff` / `handoff_task` move the held worktree (`open`, `landing`, `landed`, `cleanup_failed`) to the new worker by `UPDATE worktrees SET worker_id`. The claim prints the worktree path of the task.
- Change: in the same transaction, call `attach` of ticket 08 (`fleetd/worktree_custody/application/create.rs`) with the row of the task and the live attempt of the worker:
  - `created`, `rescued` -> `active` with the new owner. `active` or `landing` whose owner attempt has ended -> the new owner. `active` whose owner attempt is still live -> no move: the claim of today refuses a task held by a live worker, and this stays (FM-14).
  - `orphaned` -> `NeedsRescue`: the claim does not wait (it is 1 transaction). It prints 1 line that tells the worker to run `horch ledger worktree new <name>` (ticket 08 runs the rescue, then takes the tree). The words of the line keep today's style.
  - When the row has a `rescue_ref`, the claim prints 1 more line with the ref ("Its work of the last attempt is also saved at <ref>.").
  - `START.md` with the rescue commit and the progress notes of the last attempt is Phase 4 (push dispatch). Phase 3 prints the line in the claim output only (Q17).
- The words that a user reads keep today's form. A new line has a word that says what happened.
- `crates/horch-core/src/taskledger/tasks.rs` is also changed by ticket 14 (the requeue of a parked task). Ticket 14 runs after this ticket.

## Steps

1. Write the tests first, in `crates/horch-core/tests/worktree_reuse.rs`, each with its own temporary repository and ledger. Each fails before the code exists:
   - `claim_of_a_retried_task_makes_its_tree_active_with_the_new_attempt`.
   - `claim_of_a_task_with_a_rescued_tree_names_the_rescue_ref`.
   - `claim_of_a_task_with_an_orphaned_tree_does_not_take_it_before_the_rescue`.
   - `handoff_moves_the_owner_of_the_tree`.
   - `owner_does_not_move_while_the_old_attempt_is_live`.
   - `reap_leaves_the_tree_and_each_file` (W3).
2. Write the code. Keep each test of `crates/horch-e2e/tests/taskledger.rs` and `fleet18_brief_worktree.rs` green; change a test only where the new line is printed, and name it in the commit message.
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

The Check passes. A claim or a handoff gives the tree of the task to the new attempt, a tree that is `orphaned` is taken only after its rescue, and the claim names the rescue ref.
