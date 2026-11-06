# 41: The outbox drain releases its lock explicitly (flaky trf_21 tests)

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets && cargo clippy --workspace --all-targets -- -D warnings && for i in 1 2 3 4 5 6 7 8 9 10; do cargo test -p horch-core --lib -q || exit 1; done`
**Own:** `crates/horch-core/src/taskledger/outbox.rs`

Spec: `docs/specs/trace-fixes.md` (C3; conventions in section 8). Added by the orchestrator after a report from ticket 22.
Waits for: nothing.

## Evidence

`trf_21_a_drain_takes_only_its_workspace_and_waits_for_a_running_drain` in `crates/horch-core/src/taskledger/outbox.rs`
fails 2 of 6 times in the full `horch-core` lib test run and 0 of 8 times alone.
`trf_21_a_failed_first_send_leaves_the_row_and_a_later_drain_delivers_it` also fails sometimes.

## Root cause (hypothesis to confirm)

`drain` opens `<ledger>.outbox.lock`, calls `try_lock`, and releases the lock only when the `File` drops. A flock
belongs to the open file description. When another thread forks a child process (another test, or in production a
send that runs `herdr` or `git`), the child holds a copy of the descriptor until its `exec` closes it (`O_CLOEXEC`).
In that window, the drop of the parent's `File` does not release the lock. The next `drain` sees `WouldBlock`,
returns `busy`, and delivers nothing. In production this delays a wake to the next 30 s pass; in tests it fails.

## Steps

1. Confirm the cause: write a test that forks a long-lived child while a drain holds the lock (for example
   `std::process::Command::new("sleep").arg("2").spawn()` from inside `send`), then drains again right after the
   first drain returns. It must fail before the fix.
2. Fix: release the lock explicitly before `drain` returns, on every path (`File::unlock`, or `libc::flock(fd, LOCK_UN)`).
   An explicit `LOCK_UN` releases the shared open file description even while a child holds a copy. Use a small
   guard type so an early `?` return also unlocks.
3. If the cause is something else, find it, fix it, and say so in the report. Never add a sleep or a retry to a test.
4. Name the test `trf_21_a_drain_lock_is_free_while_a_child_process_lives`.
5. Run the Check. It runs the lib tests 10 times; all 10 must pass.

## Done when

The check passes, and the report states the confirmed cause.

## Report

Write `/Users/mascott/projects/multi-herdr/ai_docs/reports/trace-fixes/41.md` (spec section 8). Then run `horch ledger finish`; when every step passed, run `horch ledger done "<summary>"`.
