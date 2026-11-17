# 33: Resolver and fix attempts: fleetd starts an attempt of a task branch in conflicted or gate_failed, in its worktree, with the conflict or the gate output

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd::task_execution::application::repair && cargo test -p horch-core --test repair_attempts && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/task_execution/application/repair.rs`, `crates/horch-core/tests/repair_attempts.rs`

Spec: `docs/specs/fleetd/design.md`, "Landing a task branch" (`conflicted`, `gate_failed`; A resolver attempt and a fix attempt of a task that is `done`), I1, FM-38.

## Goal

fleetd owns `conflicted` and `gate_failed`. For a branch in `conflicted`, it binds a resolver attempt of the task (the task stays `done`) in its worktree; `START.md` gives the conflict in `zdiff3` form, its files, and the tests of both sides. For `gate_failed`, a fix attempt gets the gate output. The attempt has a lease and the stall ladder as a running attempt. Its result runs the check of the task; a pass moves the branch to `ready` (resolver) or `verifying -> ready` (fix).

## Requirements

- FM-38: a branch that conflicts or fails the gate after its worker exited still lands or reaches you.
- I1: a resolver or fix attempt holds the lease of its task while it is live.

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- The bind is ticket 08's `bind` with a kind of attempt (`work`, `resolver`, `fix`) for a `done` task; the launch is ticket 10. `START.md` gets an extra section (ticket 07).
- The conflict text: the member and files from the `bundle_build` result (ticket 27); the worker merges the feature tip into its branch in its tree (`git merge`, with `merge.conflictStyle=zdiff3` set by ticket 23), resolves, and commits.
- Failures and their count are ticket 34.
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `conflicted_branch_gets_1_resolver_attempt_and_the_task_stays_done`, `resolver_start_md_names_the_conflict_files`, `gate_failed_branch_gets_1_fix_attempt_with_the_gate_output`, `passed_resolver_result_moves_the_branch_to_ready`, `repair_attempt_has_a_lease`.
2. Write the repair start and the result path.
3. This ticket moves a ref, lands, pushes, deletes a branch or a ref, or changes who runs the finish steps. It has a crash test: a fault point before the commit of the intended row, and after the commit and before the effect. In each case the next pass completes or fails the effect 1 time, and no tip and no work is lost.
4. Run the Check. Commit the exact files.

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

Self-hosting: this phase changes `horch spawn`, the claim and the finish steps. The build fleet itself uses the installed `horch` for these. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).

## Done when

The Check passes. A conflicted or gate-failed branch gets a resolver or fix attempt in its tree, and a passed result makes it ready again.
