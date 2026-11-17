# 14: A task that cannot land parks with its tree, a requeue resumes it in the same tree, and unlanded work stays on record

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib taskledger && cargo test -p horch-e2e --test p3_unlanded --test taskledger && cargo test -p horch --test w3_ledger_cli_pins && cargo test -p horch-core --test w3_taskledger_core_import && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/taskledger/finish/mod.rs`, `crates/horch-core/src/taskledger/tasks.rs`, `crates/horch-core/src/taskledger/import.rs`, `crates/horch-e2e/tests/p3_unlanded.rs`

Spec: `docs/specs/fleetd/design.md`, RR-14, RR-15, RR-18, "Worktrees: leased, reconciled, never lost" (the rows `active` and `created`: "retried -> back to `created`, so the next attempt continues in the same tree"), "Preservation analysis" (rows 04 L5, L8 and L9; "Failure mode analysis": a task that cannot land); `docs/specs/fleetd/preservation-04-behavior.md` rows L5, L8 and L9; `docs/specs/fleetd/preservation-05-contracts-and-values.md` row V-14.

## Goal

The safeties of today for work that does not land hold with the new tree states. `horch done --unlanded` parks the task with its tree: the tree is `created` with no owner, the dependents stay blocked. `horch ledger requeue` resumes the task in the same tree. `horch ledger unfinished` lists each tree with work that never landed, including `orphaned` and `rescued` trees, until you reopen or dismiss it, and a dismissed finding is not listed again.

## Requirements

- **RR-14**: a parked task keeps its tree, its dependent stays `pending`, and its requeue runs in the same tree.
- **RR-15**: an unlanded tree is listed until you reopen or dismiss it, and a dismissed finding is not listed again.
- **RR-18**: a task escalated after its last attempt runs again after your requeue (attempt count 0), and a review requeue does not change its attempt count.

## Context

- Read `docs/tickets/fleetd-p3/MAP.md`, section "Open questions", first (Q10 applies here).
- Today:
  - `finish::complete` (`crates/horch-core/src/taskledger/finish/mod.rs`): `done --unlanded "<reason>"` sets the worktree `status` `unlanded` (`set_worktree`), records `EventKind::Unlanded`, and parks the task (`held_reason = unlanded`).
  - `tasks.rs`: `blocked_by` (dependents wait with `BlockerParked`), `requeue_by_operator` and `requeue_by_operator_in` (the attempt floor, row L8, and the reset of Phase 2 ticket 13 of fleetd-p2), and `resume_parked_worktree` (the tree goes back to `open`).
  - `import.rs`: `unfinished` (the findings: an unlanded tree, an orphan, a cancelled task with an open tree, a reaped task that never finished), `decide` (reopen and dismiss), `reopened_from`.
- Change:
  - `complete` with `--unlanded`: in the same transaction, the tree `active` -> `created` through `transition()` (cause `unlanded`), `owner_attempt` null. The tree stays on disk.
  - `resume_parked_worktree`: today's `status` change stays; the state stays `created`. The next claim makes it `active` (ticket 11).
  - `unfinished`: also list each tree in `orphaned` or `rescued` whose task is not `done` and that has no decision yet, with its rescue ref when it has one. A reopen gives the tree to the new task (its `task_id`), and its claim attaches it (ticket 11). A dismiss records the decision 1 time. `dismiss --cleanup` is the flow of ticket 12; do not change it.
  - The requeue of a review (`crates/horch-core/src/taskledger/finish/codesweep.rs`, its own counter, V-14) does not change. Only test it.

## Steps

1. Write the tests first, in `crates/horch-e2e/tests/p3_unlanded.rs`, each with its own temporary repository, ledger and state folders. Each fails before the code exists, or names the behavior that it pins:
   - `pres_04_l5_unlanded_task_keeps_worktree_and_blocks_dependents`: after `done --unlanded`, the tree and each file stay, the tree is `created` with no owner, and the dependent is `pending`.
   - `rr14_requeue_of_a_parked_task_runs_in_the_same_tree`: the next claim prints the same path, and the tree is `active` with the new attempt.
   - `pres_04_l9_unlanded_work_is_listed_and_decided_once`: the tree is listed by `horch ledger unfinished` until `reopen` or `dismiss`; after `dismiss` it is not listed again.
   - `rr15_orphaned_and_rescued_trees_are_listed_with_their_rescue_ref`.
   - `rr18_requeue_of_an_escalated_task_sets_its_attempt_count_to_0`.
   - `rr18_review_requeue_keeps_the_attempt_count`.
2. Write the code. Keep each test of `crates/horch-e2e/tests/taskledger.rs` and `crates/horch/tests/w3_ledger_cli_pins.rs` green; change a test only where an output line changes, and name it in the commit message.
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

The Check passes. A parked task keeps its tree and blocks its dependents, its requeue works in the same tree, each tree with work that did not land is listed until it is decided 1 time, and the attempt count follows RR-18.
