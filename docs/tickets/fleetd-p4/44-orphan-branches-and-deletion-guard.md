# 44: I11 and I12: a fleet branch with no row is adopted as an orphan and never deleted; a branch is deleted only when landed, merged or rescued and no stash names it

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd::integration::domain::invariants && cargo test -p horch-core --test branch_invariants && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/integration/domain/invariants.rs`, `crates/horch-core/src/fleetd/integration/application/branches.rs`, `crates/horch-core/tests/branch_invariants.rs`

Spec: `docs/specs/fleetd/design.md`, I11, I12, "No work is lost" (items 4 and 5), `docs/specs/fleetd/preservation-04-behavior.md` row W6.

## Goal

Each pass that has the list of fleet branches (from the `worktree_list` job result) adopts each fleet branch with no `branches` row as an orphan row (`orphan = 1`) and reports it. fleetd deletes a task branch only when it is `landed`, its feature is `merged`, or it has a verified rescue ref, and no `git stash` entry names it.

## Requirements

- I11: an orphan fleet branch is adopted and not deleted (a test of the gate).
- I12: a `git stash` entry blocks the deletion of its branch (a test of the gate).

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- The fleet branches are the run branch prefix of today (`<run branch>-T<seq>-<name>`, Phase 3 Q4) and `fleet/`. The list and the stash names come from the gateway of ticket 23, read in the `worktree_list` job (Q17).
- The deletion is `git branch -D` only after the guard passes, as a short effect after the commit, keyed by the row; today's cleanup uses `git branch -d` (F13), keep `-d` where it applies.
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `orphan_fleet_branch_is_adopted_and_reported_and_not_deleted`, `stash_entry_blocks_the_deletion_of_its_branch`, `unlanded_unrescued_branch_is_never_deleted`, `landed_branch_with_no_stash_is_deleted`.
2. Write the invariant checks and the guard.
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

The Check passes. No fleet branch is deleted while it holds work that is not landed or rescued.
