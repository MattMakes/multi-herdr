# 04: Fix 3 clippy errors in the git network helper

<!-- horch-ticket: 1 -->
**Persona:** primary `sonnet-bugfix`, fallback `opus` · **Parallel:** no
**Kind:** feature
**Review:** none
**Check:** `cargo clippy --workspace --bins --release --locked -- -D warnings && cargo test -p horch-core`
**Own:** `crates/horch-core/src/taskledger/git.rs`, `crates/horch-core/src/vcs/network.rs`

## Goal

`cargo clippy --workspace --bins --release --locked -- -D warnings` exits 0 on the run branch.

## Context

CI job `release-lint` on PR #347 fails with 3 clippy errors from T-4 (commit 0c563898):

- `crates/horch-core/src/taskledger/git.rs:63`: "this `match` expression can be replaced with `?`" (`Err(e) => match e.downcast::<network::Timeout>()`).
- `crates/horch-core/src/vcs/network.rs:77`: `ms % 60_000 == 0` must be `ms.is_multiple_of(60_000)`.
- `crates/horch-core/src/vcs/network.rs:79`: `ms % 1000 == 0` must be `ms.is_multiple_of(1000)`.

## Steps

1. Run the clippy command. Check: it shows the same 3 errors.
2. Fix each error as clippy suggests. Keep the behavior the same.
3. Run the check command. Check: it exits 0.
4. Run `cargo test -p horch-core --test git_network_bound`. Check: it passes.

## Constraints

- Do not add `#[allow(...)]`.
- Do not use `ANTHROPIC_API_KEY`.

## Done when

- The check command exits 0.

## Report

Send the DONE summary with `horch ledger done`.
