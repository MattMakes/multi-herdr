# 49: Make the Phase 0 red tests of dispatch green: FM-2 checks the binding at process entry, and FM-6 leaves the work to the next attempt

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-e2e --test p0_dispatch && cargo test -p horch-e2e --test expected_failures && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-e2e/tests/p0_dispatch.rs`, `crates/horch-e2e/expected-failures/p0_dispatch.txt`

Spec: `docs/specs/fleetd/design.md`, FM-2, FM-6, FR-5, "Migration plan" (Each gate names its rows).

## Goal

`fm2_agent_that_never_claims_has_its_task_bound_at_spawn` and `fm6_agent_that_exits_without_done_leaves_its_work_to_the_next_attempt` pass, and their 2 lines leave `crates/horch-e2e/expected-failures/p0_dispatch.txt`.

## Requirements

- Required item 1 of the phase: tighten the FM-2 test. The fake agent (`bound` scenario of ticket 48) records at process entry that its task is bound and that `task.yaml` exists; the test reads `entry.json` and fails when either fact is false at entry. Today the test reads the ledger only after `horch spawn` returned.
- FM-6: the next attempt (A2) is dispatched by push, its `task.yaml` names the same worktree, and the commit of A1 is in it. Keep the intent of the test; change its steps only where the claim of A1 must now be a push attempt.

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- Keep the names of the 2 tests. Remove their lines from the list only when they pass for the reason of the design.
- The test uses `horch ledger watch --once` (Phase 1 Q11) to run 1 pass.
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Change the FM-2 test first, and see it fail for the reason "not bound at entry" on a build without push dispatch (or reason about it, if no such build exists in your tree, and say so).
2. Make both tests pass with the code of the earlier tickets. Fix small faults in the test only; send QUESTION for a fault in the code of another ticket.
3. Remove the 2 lines from `p0_dispatch.txt`.
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

The Check passes with the 2 tests green and off the list.
