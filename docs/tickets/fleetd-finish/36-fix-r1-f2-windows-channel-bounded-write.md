# 36: Fix R1-F2: make Windows channel reply writes bounded and cancellable on shutdown (PL-3, RR-7)

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-platform --lib && scripts/dev-sandbox.sh cargo test -p horch-platform --test channel && scripts/dev-sandbox.sh cargo test -p horch-core --test fleetd_channel && cargo fmt --all --check`
**Own:** `crates/horch-platform/src/channel.rs`, `crates/horch-platform/src/windows/channel.rs`, `crates/horch-platform/tests/channel.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_server.rs`, `crates/horch-core/tests/fleetd_channel.rs`

Spec: `docs/specs/fleetd/reviews/end-review-p0-pp-p1.md` (finding R1-F2) and `docs/specs/fleetd/design.md` (rules PL-3 and RR-7).

## Goal

Channel reply writes on Windows respect the write timeout instead of blocking indefinitely in `write_all` when a client stops reading a reply larger than the pipe buffer, and `ChannelServer::stop` / `Drop` cancels any active blocking I/O and joins the server thread within a bounded time.

## Requirements

- **R1-F2 (Important — PL-3, RR-7)**:
  - **Finding text and file:line**:
    `crates/horch-platform/src/channel.rs:131` documents that Windows cannot set a pipe write timeout. `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_server.rs:373` treats `Unsupported` as permission to call blocking `write_all`, and `:167` joins that server thread without a bound during shutdown. A concrete case is a Windows client that submits a valid request and then stops reading a reply larger than the remaining pipe buffer (`BUFFER = 4096` in `crates/horch-platform/src/windows/channel.rs:100`). The only channel thread blocks in `write_all`; later clients cannot connect, and a fleetd restart can hang while `ChannelServer::drop` joins the thread. Core code therefore does not decide a safe response to the platform's explicit unsupported value.
  - **Design rules**:
    - **PL-3** (`docs/specs/fleetd/design.md:192`): "Where an OS has no equivalent, its module shall return an explicit 'not supported' value, and core code shall decide what to do with it. Nothing shall fail silently."
    - **RR-7** (`docs/specs/fleetd/design.md:160`): "The supervisor shall survive its own restart with no lost or duplicated task, by rebuilding state from the ledger and the files."

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test`, `cargo run` and each manual binary through `scripts/dev-sandbox.sh` (unit A1 makes it). Until A1 lands, use the env of `ai_docs/plans/fleetd-finish/audit/isolation.md` section "Sandbox env", with the sandbox folder inside the worktree.
3. Never install the build. No `horch self-update`, `horch install`, `cargo install`, `launchctl`. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
4. Never read, set or pass `ANTHROPIC_API_KEY`.
5. Each Check ends with `cargo fmt --all --check`.
6. No flaky test. A test that fails 1 time in 3 runs is a bug: find the cause.
7. A unit that removes an entry of `crates/horch-e2e/expected-failures/` runs that test 3 times.
8. Before DONE: no process that the unit started is alive (`ps`), and the sandbox folder is removed. The finish step `cleanup` removes the worktree and its `target/`.
9. A unit that lands a task branch merges it with `git merge --no-ff origin/<branch>` in its worktree, resolves conflicts, and keeps each commit of the branch.

## Steps

1. Write the failing tests first:
   - In `crates/horch-platform/src/windows/channel.rs` (unit test module) and `crates/horch-platform/tests/channel.rs`, add test function `large_reply_to_non_reading_client_times_out_and_does_not_block`:
     - Bind a `Listener`, connect a client that sends a request line and never reads from its stream while keeping its handle open.
     - Have the server call `stream.set_write_timeout(Duration::from_millis(300))` and write a reply payload larger than the Windows pipe buffer (for example 128 KiB, well above `BUFFER = 4096`).
     - Assert that the write fails with `ErrorKind::TimedOut` (or ends within the timeout bound), that a second client can connect and receive its full reply, and that listener/connection teardown completes promptly.
   - In `crates/horch-core/tests/fleetd_channel.rs`, add test function `non_reading_client_with_large_reply_does_not_block_next_client_or_server_stop`:
     - Register a handler that returns a large JSON payload (for example 128 KiB) and start `ChannelServer` with `Options { read_deadline: Duration::from_millis(300), ..Options::default() }`.
     - Connect a client that sends 1 valid request line and never reads the reply.
     - Assert that a second client can connect and complete a request, and that `server.stop()` returns `Ok(())` within 2 seconds.
2. Implement bounded and cancellable writes and shutdown in `crates/horch-platform/src/windows/channel.rs`, `crates/horch-platform/src/channel.rs`, and `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_server.rs`:
   - In `crates/horch-platform/src/windows/channel.rs`:
     - Store `write_timeout: Mutex<Option<Duration>>` on `Stream` and implement `set_write_timeout` (rejecting a zero duration with `ErrorKind::InvalidInput` and storing the timeout instead of returning `Unsupported`).
     - In `Stream::write` / `flush`, when `write_timeout` is set (or on server streams), bound the write wait (for example by running the write or cancelling synchronous pipe I/O via `CancelSynchronousIo` / `DisconnectNamedPipe` when the deadline expires, as `end_connection` already does at lines 255-282) and return `ErrorKind::TimedOut` if the write does not complete before the timeout.
     - Provide a listener/stream cancellation hook or ensure `ChannelServer::end` (`crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_server.rs:167`) cancels any active blocking accept/read/write before joining the `fleetd-channel` thread.
   - In `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_server.rs:373` (`write_reply`):
     - Do not ignore `set_write_timeout` errors or fall back to an unbounded blocking `write_all`; if `set_write_timeout` fails, abort the reply write and close the connection.
3. Run the ticket Check and confirm all tests and formatting pass.

## Out of scope

- Changing the Unix domain socket wire protocol in `crates/horch-platform/src/mac/channel.rs` or `crates/horch-platform/src/linux/channel.rs`.

## Done when

`large_reply_to_non_reading_client_times_out_and_does_not_block` and `non_reading_client_with_large_reply_does_not_block_next_client_or_server_stop` pass, `write_reply` never performs an unbounded blocking write, `ChannelServer::stop` is bounded, and the ticket Check passes.
