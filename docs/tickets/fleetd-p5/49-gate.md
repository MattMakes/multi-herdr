# 49: Gate of Phase 5: each rule of the gate green, the arch test, the run-in-flight test, and the gate file

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** full
**Check:** `cargo test --workspace --locked`
**Own:** `docs/specs/fleetd/gates/fleetd-p5.md`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 7 "Phase 5: one ledger writer"; Architecture and roles, "The command channel"; Workspace layout and file protocol, Protocol rules).

## Goal

The gate of Phase 5 is green, and `docs/specs/fleetd/gates/fleetd-p5.md` records each row with its test and its result.

## Requirements

- FR-7, RR-5, RR-8, SR-5, SR-6, SR-8
- FM-15, FM-17, FM-23, FM-24
- the arch test
- the release test of Phase 5

## Context

- Read `docs/tickets/fleetd-p5/MAP.md` first: the sections "Open questions", "Request kinds" and "Decisions that hold".
- Self-hosting danger: this phase changes how each `horch` command reaches the ledger. The build fleet itself uses the installed `horch`. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- The gate of the design (Migration plan, item 7): "green: FR-7, RR-5, RR-8, SR-5, SR-6, SR-8, FM-15, FM-17, FM-23, FM-24. Tests: the arch test passes; from inside a worker sandbox, for each `operator` kind, a raw request on the channel and a request file with the right owner are both refused (SR-8)."
- Expected failures: Phase 5 has no line in `crates/horch-e2e/expected-failures/` today (no Phase 0 red test names phase 5). Check with `grep -n "| 5 |" crates/horch-e2e/expected-failures/*.txt`. If a line exists, the ticket that made its test green removes it; if it is still there, remove it here and own both files (the orchestrator permits it).
- The format of a gate file: `docs/specs/fleetd/gates/fleetd-p1.md` (and the gate files of Phases 2 to 4). 1 row for each requirement and failure mode of the gate line, each REQUIRED item of this map (Q1, the unix socket check, the server-side operator check, Q12, the read-only worker), the arch test and the release test: the test name, the file, and the result. Name each test that only CI can run (Linux, Windows) and its status.
- Correct small faults. A large fault goes back to its ticket through the orchestrator.
- A file that a ticket of fleetd-p2, fleetd-p3 or fleetd-p4 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Run `cargo test --workspace --locked` and the guard tests. Record each result.
2. Run each test of the gate rows by name and record it: `p5_operator_boundary`, `p5_untrusted_files`, `p5_requests`, `p5_read_only_worker`, `p5_release`, `fleetd_single_writer`, `fr7_typed_lines`, `arch_scan`, `fleetd_lock` (the second fleetd of RR-5 and FM-24, Phase 1), and the unit tests of tickets 07 to 15.
3. Write `docs/specs/fleetd/gates/fleetd-p5.md`.

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

- The Check passes, with the guard tests.
- `cargo test --workspace --locked` passes.
- The gate file has a row with a passing test for each rule of the gate, and names each CI-only test.
