# 13: The channel server of fleetd: bind, the peer user check, 1 request line with a deadline, the decision, the reply

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** security
**Review:** full
**Check:** `cargo test -p horch-core --test fleetd_channel && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_server.rs`, `crates/horch-core/tests/fleetd_channel.rs`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 7 "Phase 5: one ledger writer"; Architecture and roles, "The command channel"; Workspace layout and file protocol, Protocol rules).

## Goal

fleetd serves its command channel: each connection gives 1 request, and fleetd answers with the decision that it recorded.

## Requirements

- SR-5 (the channel accepts only the user that owns the ledger)
- SR-8 (the peer process check on the live channel)
- RR-8
- FM-15

## Context

- Read `docs/tickets/fleetd-p5/MAP.md` first: the sections "Open questions", "Request kinds" and "Decisions that hold".
- Self-hosting danger: this phase changes how each `horch` command reaches the ledger. The build fleet itself uses the installed `horch`. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- The transport is `horch_platform::channel` (`Listener::bind`, `accept` with its `Peer`). The name: `<fleet root>/run/fleetd.sock`. On Windows the platform turns that path into the pipe name (module doc of `crates/horch-platform/src/channel.rs`).
- `bind` refuses an existing file at the name. The caller holds the fleetd lock, so it removes an old socket file first, and only a socket file (never a regular file or a folder; refuse and open 1 needs-you item for another kind of file).
- Each connection: read 1 line with a deadline of 5 s (the connect time of today, `preservation-05-contracts-and-values.md`, row V-18) and at most `request_max_bytes` (ticket 03); parse it (ticket 08); compare `Peer::user` with `User::current()`; ask ticket 11 for the peer process fact; read the token (ticket 10); decide (ticket 09); handle (ticket 12); write the reply line; close. An error ends only that connection.
- The server runs in 1 thread of fleetd with its own connection to the ledger. SQLite gives 1 writer at a time inside the 1 fleetd process.
- A slow or silent client must not block the next one longer than its deadline (this matters on Windows, where ticket 04 gives 1 pipe instance).
- A file that a ticket of fleetd-p2, fleetd-p3 or fleetd-p4 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/fleetd_channel.rs` with a temporary fleet root: `request_from_this_user_is_applied_and_replied`, `peer_user_that_is_not_the_owner_is_refused_with_its_reason` (the peer facts come through a test seam), `line_over_the_limit_is_refused`, `silent_client_is_dropped_at_its_deadline_and_the_next_is_served`, `worker_kind_on_the_channel_is_refused_and_recorded`, `operator_request_from_a_process_of_a_live_attempt_is_refused` (a child of a recorded wrapper sends it), `old_socket_file_is_replaced_but_a_regular_file_is_not`.
2. Write `ChannelServer::start(fleet_root, ledger, handlers) -> Result<ChannelServer>` and its stop. Ticket 41 starts it in fleetd.

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
- The 7 tests pass.
- The server never reads more than the limit from a peer.
