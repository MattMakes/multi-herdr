# 15: The client of the channel: send 1 request, check that the peer is fleetd, ask for the result after a lost reply, or write a request file

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** security
**Review:** full
**Check:** `cargo test -p horch-core --test fleetd_channel_client && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_client.rs`, `crates/horch-core/tests/fleetd_channel_client.rs`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 7 "Phase 5: one ledger writer"; Architecture and roles, "The command channel"; Workspace layout and file protocol, Protocol rules).

## Goal

Each `horch` command that changes state calls 1 function that delivers its request to fleetd exactly 1 time, or leaves it in a request file, and never talks to a process that is not fleetd.

## Requirements

- RR-8 (a retry sends the same `request_id`; a command that got no reply asks for its result first)
- SR-5
- SR-8
- REQUIRED item 2 of the ticket of this map (the client checks the peer process)
- FM-15

## Context

- Read `docs/tickets/fleetd-p5/MAP.md` first: the sections "Open questions", "Request kinds" and "Decisions that hold".
- Self-hosting danger: this phase changes how each `horch` command reaches the ledger. The build fleet itself uses the installed `horch`. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- The function: `fn send(ctx, kind, payload) -> Result<Sent>` with `Sent::Replied(Reply)` or `Sent::Queued { request_id, path }`. It makes 1 `request_id` for the call and keeps it for each retry of that call.
- Before it sends, it reads the `fleetd` row read-only (`taskledger::open_existing_read_only`, `crates/horch-core/src/taskledger/mod.rs`) and checks the server: `Stream::server_pid()` of ticket 05 and the start time of that PID equal `pid` and `pid_start` of the row. A mismatch or an unknown fact: the client sends nothing, prints the reason, and exits with an error. A process of the same user can remove the socket file and bind its own; this check refuses it.
- A lost reply (the connection ends after the send): before any new send, the client reads the `requests` row of its `request_id` read-only. A row that is applied or refused gives the stored result. With no row or a `pending` row, the client sends again with the same `request_id`, and fleetd applies it at most 1 time (ticket 12).
- fleetd down (`NotFound` or connection refused): the client writes `<fleet root>/run/requests/<request_id>.json` atomically (`name.tmp`, fsync, rename) with mode 0600, and prints: `fleetd is down: request <id> waits in <path>; fleetd applies it at start`. On Windows, `ERROR_PIPE_BUSY` waits inside the deadline (ticket 04).
- The token is read with ticket 10 and sent with each `operator` request. The client sends no `worker` kind.
- The fleetd check of each command (`horch fleetd check` before each command, ticket 23 of fleetd-p1) already runs first.
- A file that a ticket of fleetd-p2, fleetd-p3 or fleetd-p4 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/fleetd_channel_client.rs`: `client_refuses_a_server_that_is_not_the_fleetd_of_the_ledger` (a rogue listener at the socket path in another process), `client_sends_once_and_gets_the_reply`, `lost_reply_asks_for_the_result_before_a_second_send` (a test server that drops the first reply; the handler counts 1 call), `fleetd_down_writes_a_request_file_with_mode_0600`, `request_file_is_written_atomically` (no reader ever sees a partial file), `missing_token_file_is_an_error_that_names_the_file`.
2. Write `send` and its types.

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
- The client has no path that sends to an unchecked server.
