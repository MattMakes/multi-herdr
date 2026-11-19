# Correct 6 old faults of the OS-specific code that Phase P moved without a change

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** full
**Check:** `cargo test -p horch-platform && cargo clippy -p horch-platform --all-targets -- -D warnings && cargo test -p horch-core --lib`
**Own:** `crates/horch-platform/src/`, `crates/horch-platform/tests/`, `crates/horch-core/src/workspace/herdr.rs`

## Goal

6 faults that the reviewers of Phase P found in the code of today are corrected, each with a test
that fails before the fix. Phase P kept them, because its rule (PL-6 of
`docs/specs/fleetd/design.md`) forbids a change of behavior. Its gate has passed, so the fixes are
permitted now.

## Context

Each fault is in `crates/horch-platform`, in code that Phase P moved from `crates/horch-core`. The
module doc or a doc comment of each function states the fault ("kept from the code of today" or
similar words). Find each one by its function name.

## Steps

For each fault: first write a test that shows it and see it fail. Then correct the code. Then
correct the doc comment that describes the fault.

1. `ipc`: `connect_by` (Unix) handles only `EINPROGRESS` after a non-blocking `connect`. On Linux a
   Unix socket can return `EAGAIN`. Treat `EAGAIN` as "not ready yet" and wait until the deadline.
2. `ipc`: `wait_ready` can return `TimedOut` up to 1 ms before the deadline, because `poll` takes
   whole milliseconds and the code truncates. Round up. Set the test
   `read_line_by_times_out_at_the_deadline` back to a strict limit.
3. `ipc`: `read_line_by` reads 256-byte chunks, so it can return bytes after the first newline and
   up to 255 bytes over `limit`. Make it return at most `limit` bytes and stop at the first
   newline. Bytes after the newline must not be lost for the next call: read 1 byte at a time, or
   keep a buffer in `LocalStream`. Choose the simpler one that passes the tests.
4. `group`: `kill_group_id` casts the group id from `u32` to `pid_t` with no check. A value of 0
   signals the group of the caller, and a value above `i32::MAX` becomes negative. Refuse 0 and
   each value that does not fit, with no signal sent.
5. `process` (Linux): each error of `pidfd_open` except `ENOSYS` is read as "no such process".
   Only `ESRCH` means that. For `EPERM`, `EMFILE`, `ENFILE` and each other error, fall back to the
   path that the code uses when `pidfd_open` is not available, so that a live process is never
   reported dead. This matters: the fleet stops a worker that it believes dead.
6. `group` (Windows): `Contained::start` reports `could not resume herdr`. The platform layer must
   not name its caller. Return a neutral error, and make the caller in
   `crates/horch-core/src/workspace/herdr.rs` add the word herdr, so that the user sees the same
   text as today.

This Mac compiles and runs only the macOS code. For a Linux or Windows fault, write the test with
the correct `cfg`, read the code with care, and say in your done summary that CI gives the result.

## Constraints

1. This fleet runs on the installed `horch` binary and on the live ledger
   `/Users/MASCOTT/.horch/ledger.sqlite3`. Each test and each process that you start uses
   temporary folders for `HORCH_DATA_DIR`, `HORCH_STATE_DIR` and `HORCH_LEDGER_DB`. Never install a
   development build. Never run `horch self-update` or `cargo install`. Never register a service.
2. NEVER use `ANTHROPIC_API_KEY`. Never read, print, set, export, pass or depend on it.
3. Do not use a shared `CARGO_TARGET_DIR`.
4. Before the first commit, run `git remote -v` and `git config user.email`. The remote must be
   `git@git-alt:MattMakes/multi-herdr.git` and the email `m4ttplu5@gmail.com`. If not, send BLOCKED.
5. No co-author line. Never write the em dash character. Never edit `CHANGELOG.md` by hand.
6. Change only the 6 faults. If a fix needs a file outside `Own`, send QUESTION.
7. While you wait for a long test run, run `horch ledger progress` each 5 minutes.
8. If the GitHub SSH connection times out at `horch ledger finish`, run it again, 3 times maximum.

## Done when

The Check passes, each of the 6 faults has a test, and no doc comment says that a fault is kept.
