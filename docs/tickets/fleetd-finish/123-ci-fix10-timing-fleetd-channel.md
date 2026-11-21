# CI FIX-10: fix the Linux ECONNRESET failure in horch-core tests/fleetd_channel.rs

<!-- horch-ticket: 1 -->
**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `for i in 1 2 3; do scripts/dev-sandbox.sh cargo test -p horch-core --test fleetd_channel --locked || exit 1; done && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_server.rs`, `crates/horch-core/tests/fleetd_channel.rs`

Spec: `docs/specs/fleetd/gates/ci-baseline-2026-11.md` (section `## macOS`, `### Failures that are not caused by the build` and `## Fix units` row FIX-10).

## Goal

Fix the unexplained class `f?` failure of `line_over_the_limit_is_refused` in `crates/horch-core/tests/fleetd_channel.rs:157` (`Connection reset by peer` on Linux) and prove that it passes 3 times in a row through `scripts/dev-sandbox.sh`.

## Read first

- `docs/specs/fleetd/gates/ci-baseline-2026-11.md`: section `### Failures that are not caused by the build` and `## Fix units` row FIX-10.
- `crates/horch-core/tests/fleetd_channel.rs` (lines 140 to 250) and `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_server.rs`.

## Test covered (1 named timing test of FIX-10)

- `line_over_the_limit_is_refused` (`crates/horch-core/tests/fleetd_channel.rs:157`, Linux, class `f?`): `Err: Os { code: 104, kind: ConnectionReset, message: "Connection reset by peer" }`

## Steps

1. Run `line_over_the_limit_is_refused` 3 times locally through `scripts/dev-sandbox.sh`:
   `for i in 1 2 3; do scripts/dev-sandbox.sh cargo test -p horch-core --test fleetd_channel -- line_over_the_limit_is_refused --locked || exit 1; done`
2. Inspect `crates/horch-core/tests/fleetd_channel.rs:140-250` (`ask` at line 157 and `line_over_the_limit_is_refused` at line 223) and `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_server.rs`:
   - When the client writes more than `MAX_BYTES` (`line.len() > MAX_BYTES + 1`), the server stops reading after `MAX_BYTES + 1` bytes, writes the refusal reply line, and closes its stream while unread client bytes remain in the kernel socket buffer. On Linux `AF_UNIX` stream sockets, closing a socket with unread data in the receive buffer or writing to a peer that has already closed causes `write_all` (or `read_line`) in `ask` to fail with `ECONNRESET` (`Os { code: 104, kind: ConnectionReset }`) before the client reads the refusal line.
3. Fix the cause in `crates/horch-core/src/fleetd/fleet_runtime/adapters/channel_server.rs` and/or `crates/horch-core/tests/fleetd_channel.rs` so the client reliably reads the refusal response on both Linux and macOS without `ConnectionReset`. Never weaken an assert.
4. Run the Check (which runs `fleetd_channel` 3 times in a row and checks formatting).

## Out of scope

- Any file outside `**Own:**`.

## Done when

`line_over_the_limit_is_refused` and the full `fleetd_channel` test binary pass 3 times in a row, and the Check passes.

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
