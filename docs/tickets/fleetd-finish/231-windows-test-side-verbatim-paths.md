# Windows: test code that starts its own git with a canonicalize() path (about 100 tests)

**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --locked --test w3_finish_steps --test bundles --test worktree_jobs --test worktree_create && scripts/dev-sandbox.sh cargo test -p horch-core --lib --locked && cargo fmt --all --check`
**Own:** `crates/horch-core/tests/w3_finish_steps.rs`, `crates/horch-core/tests/bundles.rs`, `crates/horch-core/tests/worktree_jobs.rs`, `crates/horch-core/tests/worktree_create.rs`

Source: the commit of task T-340 on the run branch (`git log --grep 'FIX-36' -1`), section "Still need a test-side change", and `docs/specs/fleetd/gates/ci-run-4.md`, section Windows.

## Fault

On Windows, `canonicalize()` gives a path with the verbatim prefix (`\\\\?\\`), and git refuses it. Task T-340 fixed the product: each child that `horch_platform::start::Command` starts gets a plain path (`horch_platform::path::for_child`). Test code that starts git by itself with `std::process::Command::new("git")` and a `canonicalize()` path still fails on Windows: `w3_finish_steps.rs` (38 tests), `bundles.rs` (14), `worktree_jobs.rs` (8), `worktree_create.rs` (7), and lib test modules (34): `trust.rs`, `harness/launch.rs`, `land.rs`, `pr.rs`, `lifecycle.rs`, the tests of `landing.rs`, `commands.rs:387`.

## Goal

Each of these test helpers gives git a plain path on each OS, through `horch_platform::path::for_child` (or `horch_platform::path::canonicalize` if T-340 made one: read its commit). Prefer 1 change in a shared test helper over a change at each call. No change of an assert. No OS `cfg` in the test files. The lib test modules live in product files that other tasks edit: they are shared files; change only the test module lines.

## Steps

1. Read the commit of T-340 and the helper. List each test-side `canonicalize()` whose result reaches a git child (`git grep -n 'canonicalize()' -- 'crates/*/tests' 'crates/*/src'`, then read each).
2. Change the helpers. Run the Check (macOS must stay green).
3. Proof for Windows: the Windows host does not exist here. Compile what this host can: `RUSTC=$HOME/.cargo/bin/rustc $HOME/.cargo/bin/cargo check -p horch-platform --all-targets --locked --target x86_64-pc-windows-msvc` (the check of `horch-core` for Windows needs the Windows SDK and cannot run here: do not try to install it). Say in the commit message which Windows tests the change should turn green; the next CI run proves it.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test` through `scripts/dev-sandbox.sh`. Never install the build or a tool. Never write or read `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
3. Never read, set or pass `ANTHROPIC_API_KEY`.
4. No flaky test. Do not weaken an assert. No `#[ignore]`. No larger time limit without a measured cause. No expected-failures entry without an answer of the orchestrator.
5. Never run `rm -rf /tmp/horch-e2e-*`, pkill or killall. Never touch a process of another fleet. At most 4 test processes at the same time. Write 1 `horch ledger progress` line each 5 minutes while a long test runs.
6. Before each finish: `scripts/dev-sandbox.sh cargo test -p horch-core --locked --test arch_scan` and `cargo clippy --workspace --all-targets --locked -- -D warnings` add no new failure (tasks T-327 and T-347 fix the ones of the base).
7. `git fetch` and `git rebase fleet/fleetd-finish` before each `horch ledger finish`. After the comment sweep, read its commit: restore each comment that gives a security rule, an invariant, a bound or a design id.
8. Before DONE: no process that the unit started is alive, and the sandbox folder is removed.

## Done when

The Check passes and the commit message lists each changed helper.
