# 60: Gate of Phase 4: each rule of the gate green, and the gate file

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** no
**Kind:** docs
**Review:** none
**Check:** `cargo test --workspace --locked && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `docs/specs/fleetd/gates/fleetd-p4.md`

Spec: `docs/specs/fleetd/design.md`, "Migration plan" item 6 (*Gate*), "Where each rule becomes green" (row Phase 4), "What each gate needs" (row Phase 4), "What a release does to a run in flight" (row Phase 4).

## Goal

Each rule of the Phase 4 gate is green: each requirement and failure mode of the gate line has a test that passes and is off the list of expected failures, the part test of FM-9 passes, the 5 no-loss tests pass, the named tests of the gate line pass, and the run-in-flight test passes. The file `docs/specs/fleetd/gates/fleetd-p4.md` lists each row, its test names and the result.

## Requirements

- Green: FR-5, SR-2, SR-3, SR-4, PL-4, PL-5, FM-2, FM-6, FM-36, FM-37, FM-38, FM-40, FM-43.
- Part test: FM-9 for a code kind and for the structural check (FM-9 is green in Phase 6).
- 1 test each: I8, I9, I10, I11, I12.
- Tests: a `passed` bundle whose compare-and-set fails goes to `dropped` and its members go back to `ready`; the candidate ref of each ended bundle is deleted and verified; a failed resolver attempt starts the next one, and at `max_attempts` it puts 1 item on the needs-you list; a transient push failure is followed by a new run of the push job; 2 fetch periods in a row make 2 fetch runs.
- The run-in-flight test of Phase 4 (ticket 59).
- The pending rows that the orchestrator allowed: the Linux check sandbox tests when CI has no `bwrap` (Q-A, rule 3).

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- The tests of each row are in the coverage table of `docs/tickets/fleetd-p4/MAP.md`. Follow the gate files of the earlier phases (`docs/specs/fleetd/gates/fleetd-p3.md`).
- Run `cargo test --workspace --locked` (the check of the run). Correct small faults (a flaky wait, a wrong path in a test). A fault in a mechanism: send QUESTION with the evidence.
- The required items 1 to 5 of the phase: check each one and name its ticket in the gate file.
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Run the check of the run and each test of the gate rows.
2. Write `docs/specs/fleetd/gates/fleetd-p4.md`: 1 row for each requirement, failure mode, invariant and named test, with its test names and its result (pass, or pending with the reason the orchestrator allowed).
3. Name each Windows risk of the phase and each Linux pending row.
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

`cargo test --workspace --locked` passes, and the gate file shows each row of the Phase 4 gate green or pending with an allowed reason.
