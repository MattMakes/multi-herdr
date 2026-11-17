# 31: fleetd ingests heartbeat.json, exit.json and session.json of each attempt

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test wrapper_ingest && cargo test -p horch-core --lib fleetd::task_execution && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/task_execution/adapters/attempt_files.rs`, `crates/horch-core/tests/wrapper_ingest.rs`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 7 "Phase 5: one ledger writer"; Architecture and roles, "The command channel"; Workspace layout and file protocol, Protocol rules).

## Goal

fleetd turns the facts of the wrapper files into the rows that the wrapper wrote before, so the leases, the stall ladder and the sessions keep working.

## Requirements

- RR-5
- RR-8 (a snapshot is used only when its `seq` is higher)
- SR-6 (the wrapper files are read with the checks, each inside its own attempt folder)
- `idempotent: agent-status-changed` (DDD Slice 5)

## Context

- Read `docs/tickets/fleetd-p5/MAP.md` first: the sections "Open questions", "Request kinds" and "Decisions that hold".
- Self-hosting danger: this phase changes how each `horch` command reaches the ledger. The build fleet itself uses the installed `horch`. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- `heartbeat.json`: snapshot, not ingested; fleetd reads the last one and uses it only when its `seq` is higher than the last `seq` it used for that attempt (Protocol rules, kind Snapshot). Its facts set the checkin and activity columns that the checkin thread wrote before (`workers.last_checkin_at`, `attempts.active_at`, Phase 2 Q6 and Q7), so the lease renewal of ticket 08 of fleetd-p2 (`crates/horch-core/src/fleetd/task_execution/application/lease.rs`) needs no change.
- `exit.json` and `session.json`: immutable messages, ingested 1 time by path and hash. `exit.json` gives the exit of the agent that `settle` wrote before. `session.json` gives the `session_records` row.
- Each file passes ticket 07 with the root of its own attempt folder and its size limit (`heartbeat_max_bytes`; 4096 for the 2 others), and its task id and attempt id must equal those of its path.
- A file that a ticket of fleetd-p2, fleetd-p3 or fleetd-p4 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/wrapper_ingest.rs`: `heartbeat_with_a_higher_seq_renews_the_checkin`, `heartbeat_with_an_old_seq_is_not_used`, `heartbeat_over_4_kib_is_rejected`, `exit_json_settles_the_attempt_once`, `session_json_writes_the_session_record_once`, `idempotent_agent_status_changed`.
2. Write the readers and the functions that the pass calls (ticket 41 wires them).

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
- The 6 tests pass.
