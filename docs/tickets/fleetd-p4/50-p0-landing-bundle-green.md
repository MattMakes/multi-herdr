# 50: Make the Phase 0 red test of the bundle green: 3 ready branches land with 1 fast gate run

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-e2e --test p0_landing && cargo test -p horch-e2e --test expected_failures && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-e2e/tests/p0_landing.rs`, `crates/horch-e2e/expected-failures/p0_landing.txt`

Spec: `docs/specs/fleetd/design.md`, "The bundle", FM-36, "Between features" (Two declared gates).

## Goal

`given_3_ready_branches_when_bundle_then_fast_gate_runs_once_and_none_is_forgotten` passes, and its line leaves `p0_landing.txt`. `two_conflicting_features_keep_their_tips_and_reach_a_decision` stays on the list (Phase 6).

## Requirements

- Required item 2 of the phase: `declare_gates` keeps the format of ticket 04 (`[gates]` with `fast` and `full` in the project `fleet.toml`); correct the helper only if ticket 04 changed it.
- The test today claims 3 tasks and runs 3 `horch ledger finish` at once: those are claim attempts, which land by the old path and never run the fast gate. Change the steps to push attempts (3 spawns that bind, 3 results, passes of fleetd) and keep the name and the 2 assertions: each file is on the run branch, and the fast gate ran 1 time.

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- The tasks must be ready at the same pass for 1 bundle: commit the 3 results before the pass that verifies them.
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Change the test steps first.
2. Make it pass. Remove its line from `p0_landing.txt`.
3. Run the Check. Commit the exact files.

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

The Check passes with the bundle test green and off the list.
