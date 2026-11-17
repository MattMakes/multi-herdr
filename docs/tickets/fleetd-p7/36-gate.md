# 36: Gate of Phase 7 and of the whole build: each rule of the gate green, empty lists of expected failures, the run-in-flight test, the table of each id of the design, the pending CI rows and the deviations

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** no
**Kind:** quality
**Review:** full
**Check:** `cargo test --workspace --locked && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `docs/specs/fleetd/gates/fleetd-p7.md`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 9 "Phase 7: docs sync and conformance, then exec mode"; "Where each rule becomes green", row Phase 7; "Each gate names its rows").

## Goal

The gate of Phase 7 is green, and `docs/specs/fleetd/gates/fleetd-p7.md` records each row of the gate, the final check of the whole build, the pending CI rows and the deviations from the design.

## Requirements

- FR-6, FR-8, FR-9, SR-1, SR-7, OR-1
- FM-18, FM-19, FM-20, FM-45
- The gate tests: a `docs_sync` run and a `conformance` run with no repository are valid rows, and a daily run starts a new run; the check "Operator boundary" passes for each adapter.
- The release test of Phase 7.
- REQUIRED items 1 to 4 of the map.

## Context

- Read `docs/tickets/fleetd-p7/MAP.md` first: the sections "Open questions" and "Decisions that hold".
- Self-hosting danger: this phase changes the worker mode that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p6 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it (the text says "from ticket NN of fleetd-pN").
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`. No test uses the network: give each page through a local fixture. No ticket sends project text, a name or an email address to an outside service.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).
- The gate of the design (Migration plan, item 9): "green: FR-6, FR-8, FR-9, SR-1, SR-7, OR-1, FM-18 to FM-20, FM-45." A requirement or a failure mode is green when each of its tests passes and is off the list of expected failures.
- REQUIRED 1, expected failures: after this phase each list in `crates/horch-e2e/expected-failures/` is empty (comment lines only). Check with `grep -v "^#" crates/horch-e2e/expected-failures/*.txt | grep -v ":$"`. The design does not say to remove the mechanism, so it stays with empty lists (MAP.md Q13). A line that is still there is a red gate: report it.
- REQUIRED 2, the final check of the whole build: each requirement id and each failure mode id of the design (FR, RR, SR, PL, OR, FM, I) has a row in 1 of the gate files `docs/specs/fleetd/gates/fleetd-p0.md` to `fleetd-p7.md` (and `fleetd-pp.md`) with a test that passes. Get the ids from the tables of `docs/specs/fleetd/design.md` (Requirements, Failure modes and effects, the invariants I1 to I13). Write the table "id, phase, test, result" for each id, and name each id with no test. An id with no test is a red gate.
- REQUIRED 3: 1 table of each row that is "pending CI" (Windows, and Linux where Docker did not give it) in the 8 earlier gate files, for the CI run of the pull request.
- REQUIRED 4: 1 table of each deviation from the design that the gate files and the `MAP.md` Open questions of the 8 phases (`docs/tickets/fleetd-p0/` to `fleetd-p6/`, `fleetd-pp/`, and this map) name, for the operator.
- The format of a gate file: `docs/specs/fleetd/gates/fleetd-p1.md` and the gate files of Phases 2 to 6.
- Correct small faults. A large fault goes back to its ticket through the orchestrator.

## Steps

1. Run `cargo test --workspace --locked` and the guard tests. Record each result.
2. Run each test of the gate rows by name and record it: `p7_docs_sync`, `p7_exec`, `p7_release`, `p7_launcher`, `fleetd_pass_p7`, `p7_profile_health`, `p7_conformance`, `p7_operator_boundary`, `p7_conformance_job`, `p7_docs_jobs`, `p7_context_limit`, `p7_profile_complete`, `p7_context_256k`, `p7_status_cli`, and `arch_scan -- fitness_no_process_start_outside_the_launcher`.
3. Check REQUIRED 1. Build the tables of REQUIRED 2, 3 and 4.
4. Write `docs/specs/fleetd/gates/fleetd-p7.md`. Commit.

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
- Each list of expected failures is empty.
- The gate file has 1 row for each item of the gate line, and the table of REQUIRED 2 names no id with no test, or the gate is reported red.
