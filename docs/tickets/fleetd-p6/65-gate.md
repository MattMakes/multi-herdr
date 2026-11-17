# 65: Gate of Phase 6: each rule of the gate green, the expected failures of Phase 6 gone, the run-in-flight test, and the gate file

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** no
**Kind:** quality
**Review:** full
**Check:** `cargo test --workspace --locked`
**Own:** `docs/specs/fleetd/gates/fleetd-p6.md`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 8 "Phase 6: the idea dispatcher and sub-orchestrators"; Where each rule becomes green, row Phase 6).

## Goal

The gate of Phase 6 is green, and `docs/specs/fleetd/gates/fleetd-p6.md` records each row with its test and its result.

## Requirements

- FR-1, FR-2, FR-3, FR-4, FR-10, FR-12, RR-3, RR-12, OR-3
- FM-1, FM-8, FM-9, FM-10, FM-16, FM-21, FM-29, FM-30, FM-31, FM-32, FM-33, FM-35, FM-39, FM-41, FM-42
- Gate tests: a merge of the pull request of an `open` feature moves the feature to `merged` and its ideas in flight to a new feature; the cancel of a judged task cancels its judge task, and the cancel of a judge task alone is refused
- The release test of Phase 6
- REQUIRED items 1 to 5 of the map

## Context

- Read `docs/tickets/fleetd-p6/MAP.md` first: the sections "Open questions", "Old flow and new" and "Decisions that hold".
- The gate of the design (Migration plan, item 8): "green: FR-1, FR-2, FR-3, FR-4, FR-10, FR-12, RR-3, RR-12, OR-3, FM-1, FM-8, FM-9, FM-10, FM-16, FM-21, FM-29 to FM-33, FM-35, FM-39, FM-41, FM-42."
- Expected failures: after this phase the lists in `crates/horch-e2e/expected-failures/` have only the lines of Phase 7. Check with `grep -n "| 6 |" crates/horch-e2e/expected-failures/*.txt`. A line that is still there: remove it here and own both files (the orchestrator permits it).
- The format of a gate file: `docs/specs/fleetd/gates/fleetd-p1.md` and the gate files of Phases 2 to 5. 1 row for each requirement and failure mode of the gate line, each gate test, each REQUIRED item of the map and the release test: the test name, the file, and the result. Name each test that only CI can run (Linux, Windows) and its status.
- Correct small faults. A large fault goes back to its ticket through the orchestrator.
- Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p5 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Run `cargo test --workspace --locked` and the guard tests. Record each result.
2. Run each test of the gate rows by name and record it: `p6_intake`, `p6_sub_orchestrators`, `p6_plans`, `p6_judge_pair`, `p6_questions`, `p6_features`, `p6_release`, `p6_no_orchestrator_seat`, `p0_ideas`, `p0_landing`, and the unit tests of tickets 08, 09, 10, 11 and 12.
3. Write `docs/specs/fleetd/gates/fleetd-p6.md`.

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

## Done when

- The Check passes.
- No line of phase 6 is in `crates/horch-e2e/expected-failures/`.
- The gate file has 1 row for each item of the gate line.
