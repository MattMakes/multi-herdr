# Flaky or a real race: fsx::tests::dirlock_concurrent_breakers_keep_one_holder failed 1 time

**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --lib --locked fsx:: && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fsx.rs`

## Fault

The lib test `fsx::tests::dirlock_concurrent_breakers_keep_one_holder` (`crates/horch-core/src/fsx.rs:876`) failed 1 time in a full lib run of task T-319 while the host was under load. It passed 5 of 5 alone. The test says: when some processes or threads break a stale folder lock at the same time, exactly 1 becomes the holder. If the test is right and the code is wrong, 2 holders of 1 lock are possible in the product (the land lock, the git writer lock or another user of the folder lock: list them). That is a correctness fault, not a test detail. No flaky test is accepted.

## Steps

1. Read the folder lock (`dirlock`) and the test. Write down the steps of the break of a stale lock and each window between a check and an act (for example: read the owner, decide stale, remove, make). Say for each window what a second breaker can do in it.
2. Reproduce by design, not by load: put a hook or a scratch pause in the window that you suspect and see 2 holders, or prove from the code that 2 holders cannot occur and that the test itself has the race (then say exactly which assert can fail with a correct lock).
3. Fix the code or the test by the result. A product fix must make the take of the lock 1 atomic step of the file system (a rename or a create that fails when the target exists) with an owner check after it; say why it holds on macOS, Linux and Windows. Add a test that fails before the fix by the hook of step 2.
4. Run the lib tests `fsx::` 30 times, in parallel with the other lib tests 5 times.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`. Run each `cargo test` through `scripts/dev-sandbox.sh`.
2. Never read, set or pass `ANTHROPIC_API_KEY`. Never write or read `~/.horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`, or the ledger of another fleet. Never install the build.
3. A dev fleetd and a test never stop a process that they did not start. Never run pkill, killall or `rm -rf /tmp/horch-e2e-*`. Other fleets run on this host: never touch their processes. At most 4 test processes at the same time.
4. No flaky test, no `#[ignore]`, no weaker assert, no sleep as a fix, no larger time limit without a measured cause.
5. Write 1 `horch ledger progress` line each 5 minutes while a long test runs. `git fetch` and `git rebase fleet/fleetd-finish` before each `horch ledger finish`. After the comment sweep, read its commit and restore each comment that gives a safety rule, an invariant, a bound or a design id.
6. Before DONE: no process that the unit started is alive (`ps`), and the sandbox folder is removed.

## Done when

The Check passes, the cause is named with its proof in the commit message, and the 30 runs have 0 failures.
