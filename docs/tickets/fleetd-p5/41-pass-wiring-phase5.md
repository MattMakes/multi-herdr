# 41: Wire Phase 5 into fleetd: the token, the channel server, the request files, the outbox and wrapper files, the job result files and the polite stop rows

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test fleetd_pass_p5 && cargo test -p horch-core --test fleetd_pass && cargo build -p horch && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/scans.rs`, `crates/horch/src/cmd/fleetd.rs`, `crates/horch/src/cmd/taskledger_watch.rs`, `crates/horch-core/tests/fleetd_pass_p5.rs`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 7 "Phase 5: one ledger writer"; Architecture and roles, "The command channel"; Workspace layout and file protocol, Protocol rules).

## Goal

A running fleetd serves its channel, reads the request files, ingests each worker and wrapper file, and ends jobs from their result files.

## Requirements

- RR-5
- RR-8
- SR-5
- SR-6
- FM-15
- FM-23
- Open question Q3 (fleetd makes the outbox of each live attempt)

## Context

- Read `docs/tickets/fleetd-p5/MAP.md` first: the sections "Open questions", "Request kinds" and "Decisions that hold".
- Self-hosting danger: this phase changes how each `horch` command reaches the ledger. The build fleet itself uses the installed `horch`. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- At start, after the lock and the generation (Phase 1): make the token (ticket 10), apply the request files that wait (ticket 14), then start the channel server (ticket 13). A token file that fails its checks opens 1 needs-you item and fleetd serves no operator request until you fix it.
- Part 1 of each pass (facts, with the scan cursors and budgets): the request files, the outboxes (ticket 29), the wrapper files (ticket 31), the job result files (ticket 33). Part 2 (1 transaction): their rows. Part 3: the effects (the polite stops of ticket 34).
- Q3: at its first pass after the update, fleetd makes the outbox folder of each live attempt that has none (mode 0700), so an old worker writes files from then.
- `horch ledger watch --once` runs 1 pass in the foreground when no fleetd holds the lock (Q11 of `docs/tickets/fleetd-p1/MAP.md`); it does the same scans.
- `effects.rs` is ticket 34 and is not in `Own`; call its functions.
- A file that a ticket of fleetd-p2, fleetd-p3 or fleetd-p4 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/fleetd_pass_p5.rs`: `fleetd_start_makes_the_token_and_serves_the_channel`, `request_file_written_while_fleetd_is_down_is_applied_at_start`, `pass_ingests_progress_result_and_heartbeat`, `pass_ends_a_job_from_its_result_file`, `first_pass_makes_the_outbox_of_each_live_attempt`, `bad_token_file_opens_1_needs_you_item_and_refuses_operator_requests`, `heartbeat_stays_under_its_limit_with_10000_outboxes` (the scan budget).
2. Wire the calls.

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
- The 7 tests and `fleetd_pass` pass.
