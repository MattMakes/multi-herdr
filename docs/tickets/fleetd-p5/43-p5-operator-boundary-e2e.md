# 43: The SR-8 gate test: from inside a worker sandbox, each operator kind on the channel and as a request file is refused and recorded

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** security
**Review:** full
**Check:** `cargo test -p horch-e2e --test p5_operator_boundary && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-e2e/tests/p5_operator_boundary.rs`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 7 "Phase 5: one ledger writer"; Architecture and roles, "The command channel"; Workspace layout and file protocol, Protocol rules).

## Goal

The attack tests prove the operator boundary: a worker cannot send an operator request in any way.

## Requirements

- SR-8 (the Phase 5 test of the design)
- SR-5
- FM-32
- REQUIRED item 3 of the ticket of this map (each operator-only command, with `HORCH_ROLE` set or removed)
- Open question Q2 (the orchestrator seat cannot be forged)

## Context

- Read `docs/tickets/fleetd-p5/MAP.md` first: the sections "Open questions", "Request kinds" and "Decisions that hold".
- Self-hosting danger: this phase changes how each `horch` command reaches the ledger. The build fleet itself uses the installed `horch`. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- The e2e harness is `crates/horch-e2e` (the fake agents of `src/bin/`, the test fleetd of `src/fleetd.rs`, the fault points). Each test uses temporary `HORCH_DATA_DIR`, `HORCH_STATE_DIR`, `HORCH_LEDGER_DB` and `FLEET_ROOT`, and sets `HORCH_TEST_ASSERT_READ_ONLY=1` for each CLI process (ticket 42).
- A test waits for an event (a row, a file, an exit), never for a time.
- The worker sandbox of the test is the platform sandbox of ticket 06 with the 3 denials, around a fake agent that a test wrapper starts as a live attempt. So each attack meets the sandbox first and the server second.
- For each `operator` kind of the MAP table, 4 attacks: (1) inside the sandbox, a raw request line on the channel; (2) inside the sandbox, a request file with the right owner and mode; (3) with the sandbox off but the process in the live attempt, a raw request on the channel with a copy of a valid token (the test gives it), which the peer process check refuses; (4) with the sandbox off, a request file with no token. (1) and (2) fail at the sandbox (the act is refused by the OS); (3) and (4) reach fleetd and are refused, and the `requests` row records the refusal with its reason and a null `auth`.
- REQUIRED item 3: for each operator-only command (`horch job requeue`, `horch ledger requeue`, `horch ledger cancel`, `horch ledger waive`, `horch task requeue`, `horch task cancel`, `horch worktree restore`, `horch ledger worktree drop`, `horch feature abandon`, `horch branch abandon`, `horch rescue drop`), a worker process that sets `HORCH_ROLE=orchestrator`, and one that removes `HORCH_ROLE`, run the command and are refused by fleetd.
- Q2: a worker attempt whose environment names an orchestrator persona is refused; an attempt of a seat of `fleet.toml` with a null task is authorized.
- A file that a ticket of fleetd-p2, fleetd-p3 or fleetd-p4 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests in `crates/horch-e2e/tests/p5_operator_boundary.rs`: `operator_request_from_a_worker_sandbox_is_refused_for_each_operator_kind` (a loop over the kinds, with the 4 attacks), `operator_request_from_a_process_of_a_live_attempt_is_refused`, `operator_only_command_with_horch_role_set_or_removed_is_refused_for_each_command`, `forged_orchestrator_seat_is_refused`, `orchestrator_seat_of_fleet_toml_is_authorized`, `operator_on_the_command_line_is_authorized` (the control case).
2. Run them. A failure is a fault of an earlier ticket: fix it only when it is small and inside the files of this phase, else send QUESTION.

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
- Each test passes on this Mac.
- The done summary names the tests that CI runs on Linux and Windows and their expected result there.
