# 71: Fix R4-F2: wake ChannelServer::stop and Drop without depending on the socket pathname (RR-8, FM-15)

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --test fleetd_channel && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_server.rs`, `crates/horch-core/tests/fleetd_channel.rs`

Spec: `docs/specs/fleetd/reviews/end-review-p5.md` (finding R4-F2 and section "Tests that do not prove their rule" row 2) and `docs/specs/fleetd/design.md` (rules RR-8 and FM-15).

## Goal

`ChannelServer::stop` and `Drop` terminate and join the `fleetd-channel` server thread within a bounded time even if the `<fleet root>/run` folder or `<fleet root>/run/fleetd.sock` file was removed while the listener remained open.

## Requirements

- **R4-F2 (Important — RR-8, FM-15)**:
  - **Finding text and file:line**:
    `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_server.rs:167`. `ChannelServer::end` sets its stop flag, reconnects to the socket path to wake `accept`, and then joins the server thread. If the socket folder or socket file was removed while the listener remained open, the reconnect fails and `accept` stays blocked. Drop or explicit stop then waits forever in `join`, so fleetd can hang during shutdown or generation replacement. The smallest fix is a wake mechanism that does not depend on the bound pathname, such as a retained wake handle or a nonblocking listener with a bounded stop poll. Add a test that removes the socket folder before stopping the server.
  - **Design rules**:
    - **RR-8** (`docs/specs/fleetd/design.md:161`): "Each write of a message or a snapshot in the protocol shall be atomic (write to a temp name, fsync, rename), a log shall be appended by whole lines, and ingestion shall be idempotent. Each effect outside the ledger (a process start or stop, a file write, a git ref move) shall be a committed row in an intended state before it occurs, shall be keyed by the id of that row, and shall be safe to repeat."
    - **FM-15** (`docs/specs/fleetd/design.md:1560`): "Each command request has a `request_id`: fleetd applies it and writes its `requests` row in 1 transaction, a repeated id gets the stored result with no second effect, and a request file is archived only after its row is committed."

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

1. Write the failing test first in `crates/horch-core/tests/fleetd_channel.rs`:
   - Add test function `stop_completes_when_socket_folder_is_removed_before_stop`:
     - Create a `World`, start the `ChannelServer` with `w.start(Options::default())`, and verify 1 request succeeds so the server loop is waiting in `listener.accept()`.
     - Remove the socket file and its parent directory `w.root().join("run")` with `std::fs::remove_dir_all(w.root().join("run")).unwrap()`.
     - Call `server.stop()` on a background thread (or with a timed channel) and assert that `server.stop()` returns `Ok(())` within 1 second instead of hanging in `thread.join()`.
     - Repeat the assertion for `drop(server)` after `std::fs::remove_dir_all(w.root().join("run"))`.
   - Check: run `scripts/dev-sandbox.sh cargo test -p horch-core --test fleetd_channel -- stop_completes_when_socket_folder_is_removed_before_stop` and see it hang or time out because `Stream::connect(&self.socket, ...)` fails with `NotFound` when the socket path is gone.
2. In `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_server.rs` (coordinating with ticket `36-fix-r1-f2-windows-channel-bounded-write.md` if landed, or keeping the change self-contained in `channel_server.rs`):
   - Make the accept loop wake without relying on `Stream::connect(&self.socket, ...)` succeeding at stop time. For example:
     - Open or pre-connect a wake handle at server start while the socket path exists, or poll `stop.load(Ordering::SeqCst)` with a nonblocking / bounded-wait accept loop (sleeping `ACCEPT_RETRY` when `WouldBlock` or poll timeout occurs and checking `stop.load(Ordering::SeqCst)` on every iteration), while still calling `Stream::connect` as an immediate best-effort wake when the path is intact.
   - Verify that when `self.stop.store(true, Ordering::SeqCst)` is set in `ChannelServer::end`, `Server::run` exits within at most a few `ACCEPT_RETRY` intervals even when `<fleet root>/run` has been deleted.
3. Run the ticket Check and confirm all tests and formatting pass.

## Out of scope

- Modifying `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`.

## Done when

`stop_completes_when_socket_folder_is_removed_before_stop` in `crates/horch-core/tests/fleetd_channel.rs` passes within 1 second, `ChannelServer::stop` and `Drop` never hang when the socket file or folder is removed, and the ticket Check passes.
