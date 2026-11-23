# CI FIX-4: fix the clippy 1.100 chunks_exact lint in worktree_custody/adapters/git.rs

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `cargo clippy --workspace --bins --release --locked -- -D warnings && cargo clippy --workspace --all-targets --locked -- -D warnings && scripts/dev-sandbox.sh cargo test -p horch-core --lib fleetd::worktree_custody::adapters::git --locked && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/worktree_custody/adapters/git.rs`

Spec: `docs/specs/fleetd/gates/ci-baseline-2026-11.md` (section `## Linux`, `### Job release-lint` and `## Fix units` row FIX-4).

## Goal

On CI (rustc 1.100.0), job `release-lint` (`cargo clippy --workspace --bins --release --locked -- -D warnings`) fails with 1 error at `crates/horch-core/src/fleetd/worktree_custody/adapters/git.rs:823`. Fix that lint by the text of the CI log, and run both clippy commands locally with `-D warnings`.

## Read first

- `docs/specs/fleetd/gates/ci-baseline-2026-11.md`: section `### Job release-lint` and `## Fix units` row FIX-4.
- `crates/horch-core/src/fleetd/worktree_custody/adapters/git.rs`: `project_program` at lines 817 to 829.

## Error covered (1 row of `release-lint`)

- `cargo clippy` (`crates/horch-core/src/fleetd/worktree_custody/adapters/git.rs:823`): ``error: using `chunks_exact` with a constant chunk size`` (`-D clippy::chunks-exact-to-as-chunks`, on `parts.chunks_exact(3)` in `project_program`).

## Cause

The local toolchain is rustc 1.99.1 and CI runs rustc 1.100.0 (`workspace.package.rust-version = "1.89"`, `as_chunks` stabilized in Rust 1.88). In rustc 1.100.0, `clippy::chunks_exact_to_as_chunks` flags `parts.chunks_exact(3)` when the chunk size is a constant, recommending `parts.as_chunks::<3>().0.iter().find_map(|&[scope, origin, pair]| ...)`.

## Steps

1. In `crates/horch-core/src/fleetd/worktree_custody/adapters/git.rs:821-829` (`project_program`), replace `parts.chunks_exact(3).find_map(|rec| { let (scope, origin, pair) = (rec[0], rec[1], rec[2]); ... })` with `parts.as_chunks::<3>().0.iter().find_map(|&[scope, origin, pair]| { ... })` as required by the CI log text of `clippy::chunks-exact-to-as-chunks`.
2. Run both clippy commands locally:
   - `cargo clippy --workspace --bins --release --locked -- -D warnings`
   - `cargo clippy --workspace --all-targets --locked -- -D warnings`
   Fix each warning that they print inside `**Own:**` (if any warning is outside `**Own:**`, report it to the orchestrator).
3. Run the unit tests of `git.rs` and `cargo fmt --all --check`:
   `scripts/dev-sandbox.sh cargo test -p horch-core --lib fleetd::worktree_custody::adapters::git --locked && cargo fmt --all --check`
4. Do not run `rustup update` or install a second toolchain.

## Out of scope

- Any file outside `crates/horch-core/src/fleetd/worktree_custody/adapters/git.rs`.

## Done when

`project_program` no longer calls `chunks_exact(3)` with a constant chunk size, both clippy commands pass with `-D warnings`, and the Check passes.

## Rules


1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test`, `cargo run` and each manual binary through `scripts/dev-sandbox.sh` (unit A1 makes it). Until A1 lands, use the env of `/Users/matthewscott/repo/multi-herdr/ai_docs/plans/fleetd-finish/audit/isolation.md` section "Sandbox env", with the sandbox folder inside the worktree.
3. Never install the build. No `horch self-update`, `horch install`, `cargo install`, `launchctl`. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
4. Never read, set or pass `ANTHROPIC_API_KEY`.
5. Each Check ends with `cargo fmt --all --check`.
6. No flaky test. A test that fails 1 time in 3 runs is a bug: find the cause.
7. A unit that removes an entry of `crates/horch-e2e/expected-failures/` runs that test 3 times.
8. Before DONE: no process that the unit started is alive (`ps`), and the sandbox folder is removed. The finish step `cleanup` removes the worktree and its `target/`.
9. A unit that lands a task branch merges it with `git merge --no-ff origin/<branch>` in its worktree, resolves conflicts, and keeps each commit of the branch.
