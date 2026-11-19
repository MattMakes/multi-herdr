# The e2e harness never runs a stale horch binary

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** light
**Check:** `cargo test -p horch-e2e --lib && cargo test -p horch-e2e --test taskledger a_worker_with_no_progress_is_stalled_by_the_ladder_and_stopped_by_its_pid && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo fmt --all --check`
**Own:** `crates/horch-e2e/src/harness.rs`, `crates/horch-e2e/build.rs`, `crates/horch-e2e/Cargo.toml`

## Goal

A test of `horch-e2e` cannot pass or fail on a `horch` binary (or a fake binary) that is older than
the source of the workspace. The harness builds what it needs, or it stops with a clear message.

## Context

`Harness::horch_bin()` in `crates/horch-e2e/src/harness.rs` (about line 295) uses
`target/debug/horch` as it is. It only checks that the file exists. `cargo test -p horch-e2e` does
not build that binary. 2 workers lost time on this in 1 day:

- T-291: after `cargo clean`, the Check failed because the binary was not there.
- T-271 and T-294: after a merge of T-816, 2 tests in `crates/horch-e2e/tests/taskledger.rs`
  failed with a binary from before T-816 (the task was `ready`, the test wanted `stalled`). The
  failure looked like a flaky test under load. It was a stale binary.

`cargo test --workspace --locked` builds each binary, so the run check of the fleet is correct.
The trap is each run with `-p horch-e2e`, which each ticket Check and each worker uses.

## Steps

1. Read how the harness finds the binary folder (`bin_dir()`), the fakes, and which binaries the
   tests start (`horch`, the fake herdr and each other fake).
2. Choose 1 way and name why. Both are acceptable:
   - Way A: the harness builds the binaries 1 time for each test process, before the first use
     (`cargo build` of the needed packages and bins, with `--locked`, same profile and same
     target folder as the test), behind a `OnceLock`. Take care: many test binaries run at the
     same time, so cargo's own build lock must be the only lock; do not add a second lock file.
   - Way B: a `build.rs` or a check in the harness that compares the binary with the source and
     stops with the exact command to run when the binary is older.
   Way A is preferred, because no worker can forget it. Use way B only if way A makes a test run
   slower by more than a few seconds when nothing changed, or breaks the NFR-01 rule (the sealed
   environment allows only the fakes folder and 1 real git): the build must run outside the
   sealed environment of a test, as the test process itself.
3. Add 1 test in the harness lib that proves the rule you chose (for way B: an old file gives the
   message).
4. Prove it by hand and write the result in the done summary: touch a source file of `horch`,
   run 1 e2e test with `-p horch-e2e`, and show that the new binary is used.
5. Run `cargo test --workspace --locked --no-fail-fast`. No red test.

## Constraints

- Simplified Technical English in comments and messages. No em dash.
- No new dependency if the standard library and cargo can do it.
- Do not change a test file under `crates/horch-e2e/tests/`.
- No use of a shared CARGO_TARGET_DIR.
- Windows: use `exe()` for names; name each Windows risk in the done summary.

## Done when

- The Check passes, and the proof of step 4 is in the done summary.
- The full workspace suite has no red test, and its time is not longer by more than about 10 s.

## Report

Use `horch ledger done` with the summary. It is the only DONE path.
