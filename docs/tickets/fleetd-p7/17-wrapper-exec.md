# 17: The wrapper runs an exec attempt: it starts the CLI with START.md, renders the stream into the pane, turns events into heartbeat activity, and sees the exit at once

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test p7_wrapper_exec && cargo test -p horch-core --test wrapper_files && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/execution/lifecycle.rs`, `crates/horch/src/cmd/worker.rs`, `crates/horch-core/tests/p7_wrapper_exec.rs`

Spec: `docs/specs/fleetd/design.md` ("Architecture and roles" (fleet-run wrapper); "Worker profiles and adapters" (Exec or interactive); "Supervision, liveness and reconciliation" (the 4 signals, the agent states, rung 5)).

## Goal

An attempt in exec mode runs to completion under the wrapper: each stream event renews its activity, a usage-limit event ends it with `quota`, and its exit is seen at once.

## Requirements

- FR-6 (part): the `mode` of the profile decides the launch.
- RR-11 in exec mode: a usage-limit event gives `exit.json` with the outcome hint `quota`.
- FM-18 (part): an exec attempt never waits on a dialog.

## Context

- Read `docs/tickets/fleetd-p7/MAP.md` first: the sections "Open questions" and "Decisions that hold".
- Self-hosting danger: this phase changes the worker mode that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p6 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it (the text says "from ticket NN of fleetd-pN").
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`. No test uses the network: give each page through a local fixture. No ticket sends project text, a name or an email address to an outside service.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).
- Today `execution::lifecycle::run_worker` (`crates/horch-core/src/execution/lifecycle.rs`) launches the agent CLI in the pane and records how it ended; `crates/horch/src/cmd/worker.rs` calls it. Phase 5 (fleetd-p5 ticket 30) makes the wrapper write `heartbeat.json`, `exit.json` and `session.json` and no ledger row. Keep that.
- In exec mode the wrapper: starts the exec launch of the adapter (tickets 13 to 15) through the launcher; writes each stream line to `transcript.jsonl` of the attempt; renders a short text of each event into the pane, so you watch it live; writes a heartbeat with activity for each `activity` event; on `usage_limited` it ends the CLI and writes `exit.json` with the outcome hint `quota`; on exit it writes `exit.json` at once.
- In interactive mode nothing changes. The mode comes from the profile (`Mode` in `crates/horch-core/src/fleetd/worker_profiles/domain/profile.rs`, from fleetd-p2 ticket 06). Ticket 19 changes the default.
- The stop gate and the repair pass of the wrapper (fleetd-p4 tickets 14 and 15) apply in exec mode too: an exit with no `result.json` and no question file starts the repair pass.

## Steps

1. Write the tests first in `crates/horch-core/tests/p7_wrapper_exec.rs`, with the fake agents: `exec_attempt_runs_to_completion_and_writes_exit_json`, `exec_stream_event_writes_heartbeat_activity`, `exec_usage_limit_writes_quota`, `exec_exit_without_result_runs_the_repair_pass`, `exec_stream_is_rendered_into_the_pane_text`, `interactive_attempt_is_unchanged`. They fail before the code.
2. Write the code. Run the Check and the guard tests. Commit the exact files.

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
- `wrapper_files` of Phase 5 stays green.
