# Windows core: the command channel hangs (1 test, 82 minutes) and 6 tests get 'No process is on the other end of the pipe' (error 233)

**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `RUSTC=$HOME/.cargo/bin/rustc $HOME/.cargo/bin/cargo check -p horch-platform --all-targets --locked --target x86_64-pc-windows-msvc && scripts/dev-sandbox.sh cargo test -p horch-platform --locked && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo fmt --all --check`
**Own:** `crates/horch-platform/src/windows/channel.rs`

## Fault

CI run 5, Windows. (1) `windows::channel::tests::client_that_never_reads_the_reply_does_not_block_the_next_client` (`crates/horch-platform/src/windows/channel.rs:793-811`) never ends: the job limit of 120 minutes ended it, and 27 test binaries after it did not run. The test expects that the server ends a connection after `DRAIN_LIMIT` when the client never reads. (2) 6 tests of `crates/horch-core/tests/fleetd_channel.rs:157` fail with `Os { code: 233, "No process is on the other end of the pipe." }`. The command channel is the operator boundary of fleetd (SR-8): a client that can block the server blocks each operator command.

This host cannot run Windows tests and cannot compile `horch-core` for Windows. Evidence: the log of CI run 44890393381, job `test-windows` (`gh run view 44890393381 --log --job <id>`; find the id with `gh run view 44890393381 --json jobs`), and `docs/specs/fleetd/gates/ci-run-5.md` on the run branch, section Windows. Compile check that works here: `RUSTC=$HOME/.cargo/bin/rustc $HOME/.cargo/bin/cargo check -p horch-platform --all-targets --locked --target x86_64-pc-windows-msvc`. Reproduce each cause on macOS with a unit test that feeds the Windows form of the input (a path string, bytes) where that is possible. Say in the commit message which tests of the log each change should turn green; the next CI run proves it.

## Steps

1. Read the Windows channel server and client (named pipes): each blocking call (connect, read, write, flush, disconnect) and its time limit. Find the call with no limit that the test reaches: a synchronous pipe read or `FlushFileBuffers` waits for ever when the peer does not read; a `DisconnectNamedPipe` before the client read the reply gives error 233 to the client.
2. Fix: each server step for 1 client has a deadline (overlapped I/O with a wait and `CancelIoEx`, or a worker thread for each connection that the accept loop does not wait for); the reply is flushed or the client is given time to read before the disconnect; the accept loop always goes on to the next client. Keep the Unix behavior and the peer check as they are. Each SAFETY comment states why the handle and the buffer live long enough.
3. The test itself gets a deadline (a hang must fail, not hang).
4. Say what the next CI run must show: the 1 test and the 6 tests green, the binary ends.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`. Run each `cargo test` through `scripts/dev-sandbox.sh`. Never install a tool or the build.
2. Never read, set or pass `ANTHROPIC_API_KEY`. Never write or read `~/.horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
3. No OS `cfg` outside `horch-platform`. No flaky test, no `#[ignore]`, no weaker assert. A pass runs no SQL statement for each row and never waits for a job.
4. Never run pkill or killall. Never touch a process of another fleet. `gh` is read only: never rerun, cancel or change a run or a pull request.
5. Write 1 `horch ledger progress` line each 5 minutes inside each wait loop (the watchdog stops a silent worker after 30 minutes). Commit after each step.
6. `git fetch` and `git rebase fleet/fleetd-finish` before `horch ledger finish`. After the comment sweep, run finish again at once; restore no comment.
7. Before DONE: no process of the unit is alive, and the sandbox folder is removed.

## Done when

The Check passes and the commit message names each cause with its proof.
