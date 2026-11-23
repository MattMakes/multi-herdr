# The horch integration tests run the real binary in isolation

The integration tests in `crates/horch/tests/` run the built `horch` binary
as a child, with an environment that cannot reach the operator's real
state. `crates/horch/tests/builds.rs` must stay: having any integration
test in `crates/horch` makes `cargo test` build the `horch` binary, which
the end-to-end tests in `crates/horch-e2e` run.

## Consequences

- The `horch skills` and `horch sessions` oracle files in
  `crates/horch/tests/oracles/` are the A0 behavior baseline (ARC-01) and
  never change. `HORCH_BLESS=1` writes them, for the A0 baseline only. A
  diff in `tests/baseline_cli.rs` means user-visible output changed, and it
  is not fixed by re-blessing.
- CLI tests remove every `HERDR_*` variable from the child horch. A test
  run inside a herdr pane would otherwise see that pane's workspace, and
  `horch sessions` filters by workspace, which would hide the fixtures.
- Every git child horch runs goes through
  `horch_marketplace::git::scrub_repo_env`, because `git -C <dir>` does not
  win over an inherited `GIT_DIR`. Under `git rebase -x` a test helper once
  wrote to the real repository.
- Tests that run freshly written scripts warm them once, untimed
  (`test_support::write_warmed`). macOS scans a new executable at its first
  exec, for seconds, and the warm keeps that scan out of the call under
  test.
- On Linux a test thread that forks while another writes a script can hold
  it open for writing, so exec fails with "Text file busy" until that child
  execs. `write_warmed` retries until it does, and `warm()` in
  `tests/agent_list.rs` retries up to 100 times.
