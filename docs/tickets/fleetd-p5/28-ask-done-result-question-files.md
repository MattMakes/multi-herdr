# 28: horch ask, the QUESTION path, horch done and horch result write through the outbox writer

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch --test p5_ask_and_result && cargo test -p horch --bin horch -- messaging && cargo build -p horch && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch/src/cmd/askcmd.rs`, `crates/horch/src/cmd/messaging.rs`, `crates/horch/src/cmd/resultcmd.rs`, `crates/horch/src/cmd/mod.rs`, `crates/horch/src/main.rs`, `crates/horch/tests/p5_ask_and_result.rs`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 7 "Phase 5: one ledger writer"; Architecture and roles, "The command channel"; Workspace layout and file protocol, Protocol rules).

## Goal

The worker helpers `horch ledger progress`, `horch result` and the new `horch ask` are the `fleet` helper of the design: they validate and write the files, so a model never writes JSON by hand.

## Requirements

- FR-7
- RR-5
- Open question Q5 (the `fleet` helper)

## Context

- Read `docs/tickets/fleetd-p5/MAP.md` first: the sections "Open questions", "Request kinds" and "Decisions that hold".
- Self-hosting danger: this phase changes how each `horch` command reaches the ledger. The build fleet itself uses the installed `horch`. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- The design, Protocol rule 5: "Agents use the `fleet` helper. `fleet progress`, `fleet result` and `fleet ask` ... validate and write files for the agent." Choice Q5: the helper is the `horch` binary (1 binary, D-rule of the design), with the commands `horch ledger progress` (ticket 27), `horch result` (ticket 12 of fleetd-p4) and `horch ask "<question>"` (new here).
- `horch ask` writes `outbox/questions/<n>.json` through ticket 26; `n` is 1 more than the highest file there. The QUESTION path of `horch tell` (ticket 16 of fleetd-p4, `crates/horch/src/cmd/messaging.rs`) writes the same file through the same writer.
- `horch result` (`crates/horch/src/cmd/resultcmd.rs`) uses the writer of ticket 26 for its own attempt instead of its own path code.
- The top-level `horch done` of a worker writes `result.json` with `status: completed` (as `horch ledger done`). Find its arm in `crates/horch/src/main.rs`. If its code is in a file outside `Own`, stop and send QUESTION.
- `main.rs` and `cmd/mod.rs` are also the files of ticket 16, which comes before.
- A file that a ticket of fleetd-p2, fleetd-p3 or fleetd-p4 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch/tests/p5_ask_and_result.rs`: `ask_writes_question_1_then_2`, `question_line_of_tell_writes_the_same_question_file`, `result_writes_through_the_outbox_writer`, `horch_done_of_a_worker_writes_result_completed`, `ask_outside_an_attempt_is_refused`.
2. Add `horch ask`, and change the 3 other paths to the writer.

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
- The 5 tests pass.
- `horch ask --help` describes the command.
