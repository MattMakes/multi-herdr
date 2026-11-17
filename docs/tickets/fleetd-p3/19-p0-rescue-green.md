# 19: Make the Phase 0 red tests of rescue green: a killed worker loses nothing, and after a herdr restart each tree is rescued and reused

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** light
**Check:** `cargo test -p horch-e2e --test p0_rescue --test expected_failures && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-e2e/tests/p0_rescue.rs`, `crates/horch-e2e/expected-failures/p0_rescue.txt`

Spec: `docs/specs/fleetd/design.md`, FM-13, FM-25, "Migration plan" item 5 (*Gate:* "a worker killed with dirty and untracked files loses nothing; after a herdr restart, each worktree is rescued and the next attempt uses it again (the full FM-13)"), item 1 (the list of expected failures: "A phase removes its tests from the list").

## Goal

The 2 Phase 0 tests that wait for Phase 3 pass and are off the list of expected failures: `fm25_killed_worker_with_dirty_and_untracked_files_loses_nothing` and `fm13_herdr_restart_rescues_each_worktree_and_the_next_attempt_reuses_it`.

## Requirements

- **FM-25**: a worker killed with dirty and untracked files loses nothing.
- **FM-13** (the full form): after a herdr restart, each worktree is rescued and the next attempt uses it again.

## Context

- Read `docs/tickets/fleetd-p3/MAP.md`, section "Open questions", first (Q10 and Q21 apply here).
- `crates/horch-e2e/tests/p0_rescue.rs` (Phase 0 ticket 05; Phase 1 ticket 26 of fleetd-p1 made `fm13_herdr_restart_ends_attempts_and_queues_tasks_again` green). The test plays the agent with a `sleep` process, makes the tree with `horch ledger worktree new feat` (ticket 08 of this map runs its job), runs 1 pass at a time with `horch ledger watch --once` (`reconcile`), and waits for a ref under `refs/horch/rescue/` (`the_rescue_ref`, at most `CAUGHT_S`).
- `crates/horch-e2e/expected-failures/p0_rescue.txt` lists the 2 tests with the phase `3`. Remove the 2 lines. Keep each line of another phase.
- Change the tests only where a Phase 0 assumption no longer holds, for example the name of the rescue ref (`refs/horch/rescue/<worktree id>/<n>`, Q5) or that the next attempt makes its tree with `horch ledger worktree new` (ticket 11 prints that line for an `orphaned` tree). Keep each assertion about the saved files and the reuse of the same path. Name each change in the commit message.
- If a test still fails because of code outside `Own`, send a QUESTION with the cause and a proposed fix task. Do not change that code here.

## Steps

1. Run the 2 tests by name and read why each fails now.
2. Remove the 2 lines from `p0_rescue.txt`. Make the smallest change to the tests that the design and the Open questions explain.
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

The Check passes. The 2 tests pass, and `p0_rescue.txt` has no line of Phase 3.
