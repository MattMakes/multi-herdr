# R8-F3 (Important): SR-8: fleetd still accepts the dead request kind `spawn_adopt`

**Persona:** primary `opus-hardening`, fallback `opus` · **Parallel:** yes
**Kind:** security
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch --locked --test p6_retired_worker_commands && scripts/dev-sandbox.sh cargo test -p horch-core --locked --test fleetd_handlers_worker --test p6_authorize && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/domain/requests.rs`, `crates/horch/tests/p6_retired_worker_commands.rs`

Source: the end review `docs/specs/fleetd/reviews/end-review-a.md` (task T-297, reviewed tip 08c6d4ff). Read its finding first; it has the file and line of each fact.

## Fault

The request catalog of the design (`docs/specs/fleetd/design.md:342`) has no `spawn_adopt`, and fleetd refuses unknown kinds. But `requests.rs:53`, `:95`, `:136` keep it as an accepted operator kind, `handlers_worker.rs:57` registers its handler (it changes state), and `p6_retired_worker_commands.rs:108` calls it a kept internal record. No client sends it after T-895.

## Goal

`spawn_adopt` is in `RETIRED`: fleetd refuses it with the retired reason and records the refusal; its payload validator and its handler are gone; the catalog tests say so. First prove that no code path sends it: search each crate, the wrapper, the launch helper and the scripts; put the result in the commit message. If a live path still sends it, stop and send `QUESTION:`.

## Steps

1. Write the failing test: a raw `spawn_adopt` request with a valid operator token is refused as retired and changes no row.
2. Remove the kind, the validator and the handler (`handlers_worker.rs` is a shared file: remove only that handler and its registration). Remove code that the removal leaves unused.
3. Update `p6_retired_worker_commands.rs` and each catalog test.

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test` and each manual binary through `scripts/dev-sandbox.sh`.
3. Never install the build or a tool. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
4. Never read, set or pass `ANTHROPIC_API_KEY`. Each `claude` command of a test or of the product removes it from the environment.
5. No flaky test. Do not weaken an assert. No `#[ignore]`. No expected-failures entry without an answer of the orchestrator.
6. Never run `rm -rf /tmp/horch-e2e-*`, pkill or killall. Never run `cargo test -p horch --bin horch` with a filter that matches `watch_once` before task T-287 has landed (it starts a process chain).
7. The host has a high load from another fleet of the operator: never touch its processes. Write 1 `horch ledger progress` line each 5 minutes while a long test runs.
8. Other tasks edit the same large files (`pass.rs`, `handlers_worker.rs`, `cmd/fleetd.rs`): change only the functions of your ticket there; `git fetch` and `git rebase fleet/fleetd-finish` before `horch ledger finish` and again when the reviews are in; resolve a conflict in favor of both changes.
9. Before DONE: no process that the unit started is alive, and the sandbox folder is removed.

## Done when

The Check passes and the test of step 1 is green 3 times.
