# CI FIX-3: fix the 33 rustdoc errors in 19 files of horch-core

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --locked && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs`, `crates/horch-core/src/fleetd/ideas/application/park_cancel.rs`, `crates/horch-core/src/fleetd/integration/adapters/exec_push.rs`, `crates/horch-core/src/fleetd/integration/application/commands.rs`, `crates/horch-core/src/fleetd/integration/application/new_features.rs`, `crates/horch-core/src/fleetd/task_execution/application/lease.rs`, `crates/horch-core/src/fleetd/task_execution/application/outbox_ingest.rs`, `crates/horch-core/src/fleetd/task_execution/application/quota.rs`, `crates/horch-core/src/fleetd/task_execution/application/verifier.rs`, `crates/horch-core/src/fleetd/worker_profiles/adapters/launcher.rs`, `crates/horch-core/src/fleetd/worker_profiles/application/drift_idea.rs`, `crates/horch-core/src/fleetd/worktree_custody/adapters/git.rs`, `crates/horch-core/src/fleetd/worktree_custody/application/create.rs`, `crates/horch-core/src/harness/codex.rs`, `crates/horch-core/src/roster/validation.rs`, `crates/horch-core/src/taskledger/finish/mod.rs`, `crates/horch-core/src/taskledger/import.rs`, `crates/horch-core/src/taskledger/schema.rs`, `crates/horch-core/src/taskledger/tasks.rs`, `crates/horch-core/src/taskledger/wakes.rs`, `crates/horch-core/src/taskledger/workers.rs`, `crates/horch-e2e/src/probe.rs`

Spec: `docs/specs/fleetd/gates/ci-baseline-2026-11.md` (section `## Linux`, `### Job docs` and `## Fix units` row FIX-3).

## Goal

On CI, job `docs` (`RUSTDOCFLAGS=-D warnings cargo doc --workspace --no-deps --all-features --locked`) fails with 33 rustdoc errors in 19 files of `crates/horch-core/src/` (plus 1 private intra-doc link error in `crates/horch-e2e/src/probe.rs:11` introduced by T-898 if present). Edit doc comments only so that `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --locked` passes with 0 warnings.

## Read first

- `docs/specs/fleetd/gates/ci-baseline-2026-11.md`: section `### Job docs` and `## Fix units` row FIX-3.
- The 19 files of `crates/horch-core/src/` (and `crates/horch-e2e/src/probe.rs:11`) at the exact lines listed below.

## Errors covered (33 errors in `horch-core`, 1 in `horch-e2e`)

1. **Public doc links to a private item (`-D rustdoc::private-intra-doc-links`, 16 errors in `horch-core` + 1 in `horch-e2e`)** — replace `[\`item\`]` with plain code spans `` `item` `` in the doc comment:
   - `crates/horch-core/src/fleetd/integration/adapters/exec_push.rs:12`: `public documentation for exec_push links to private item SAFE_CONFIG` and `SAFE_ENV` (2 errors)
   - `crates/horch-core/src/fleetd/integration/application/new_features.rs:130`: `public documentation for name_from_title links to private item MAX_TITLE_NAME` (1 error)
   - `crates/horch-core/src/fleetd/task_execution/application/lease.rs:11`: `public documentation for lease links to private item end_repair_attempt` (1 error)
   - `crates/horch-core/src/fleetd/task_execution/application/quota.rs:337`: `public documentation for is_restart links to private item restart_attempt_id` (1 error)
   - `crates/horch-core/src/fleetd/worktree_custody/adapters/git.rs:509`: `public documentation for CliWorktreeGit links to private item SAFE_CONFIG` and `SAFE_ENV` (2 errors)
   - `crates/horch-core/src/fleetd/worktree_custody/adapters/git.rs:666`: `public documentation for checked links to private item SAFE_CONFIG` (1 error)
   - `crates/horch-core/src/fleetd/worktree_custody/application/create.rs:234`: `public documentation for attach links to private item refuse_while_removal_active` (1 error)
   - `crates/horch-core/src/harness/codex.rs:1906`: `public documentation for mcp_server_problems links to private item MCP_SERVER_KEYS` (1 error)
   - `crates/horch-core/src/roster/validation.rs:1842`: `public documentation for profile_problems links to private item missing_profile_fields` (1 error)
   - `crates/horch-core/src/taskledger/import.rs:1214`: `public documentation for unfinished links to private item work_landed` (1 error)
   - `crates/horch-core/src/taskledger/schema.rs:1447` and `:1454`: `public documentation for run_idea links to private item phase_4_backfill` and `RELEASE_APPROVAL` (2 errors)
   - `crates/horch-core/src/taskledger/tasks.rs:61`: `public documentation for status links to private item status_json` (1 error)
   - `crates/horch-core/src/taskledger/workers.rs:782`: `public documentation for attempt_mode links to private item max_wait_s` (1 error)
   - `crates/horch-e2e/src/probe.rs:11` (when present after T-4): `public documentation for probe links to private item operator_boundary` (1 error)
2. **Name is both a function and a module (`-D rustdoc::broken-intra-doc-links`, 9 errors)** — disambiguate with `[\`name()\`]` (or `[\`mod@name\`]` where it refers to the module):
   - `crates/horch-core/src/fleetd/ideas/application/park_cancel.rs:17`, `:70`, `:327`: `` `cancel` is both a function and a module `` (3 errors)
   - `crates/horch-core/src/fleetd/integration/application/commands.rs:9`, `:141`: `` `rescue` is both a function and a module `` (2 errors)
   - `crates/horch-core/src/fleetd/task_execution/application/outbox_ingest.rs:11`: `` `ingest` is both a function and a module `` (1 error)
   - `crates/horch-core/src/fleetd/task_execution/application/verifier.rs:25`: `` `review` is both a function and a module `` (1 error)
   - `crates/horch-core/src/fleetd/task_execution/application/verifier.rs:27`, `:466`: `` `structural` is both a function and a module `` (2 errors)
3. **Unclosed HTML tag (`-D rustdoc::invalid-html-tags`, 3 errors)** — wrap `<n>`, `<tool>`, and `<version>` in backticks:
   - `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs:673`: ``unclosed HTML tag `n` `` (1 error)
   - `crates/horch-core/src/fleetd/worker_profiles/application/drift_idea.rs:5`, `:6`: ``unclosed HTML tag `tool` `` and ``unclosed HTML tag `version` `` (2 errors)
4. **Redundant explicit link target (`-D rustdoc::redundant-explicit-links`, 3 errors)** — remove the redundant `(crate::...)` target:
   - `crates/horch-core/src/fleetd/task_execution/application/quota.rs:2`, `:3`: `redundant explicit link target` on `[\`exit\`](...)` and `[\`released\`](...)` (2 errors)
   - `crates/horch-core/src/fleetd/worker_profiles/adapters/launcher.rs:6`: `redundant explicit link target` on `[\`scrub_child_env\`](...)` (1 error)
5. **Unresolved link (`-D rustdoc::broken-intra-doc-links`, 2 errors)** — replace `[\`create_and_wait\`]` and `[\`held_state\`]` with plain code spans `` `create_and_wait` `` and `` `held_state` `` (or their valid in-scope path):
   - `crates/horch-core/src/taskledger/finish/mod.rs:546`: ``unresolved link to `create_and_wait` `` (1 error)
   - `crates/horch-core/src/taskledger/wakes.rs:535`: ``unresolved link to `held_state` `` (1 error)

## Steps

1. Edit only the doc comments at the lines listed above across the owned files. Do not change any item visibility, function signature, or executable code.
2. Run `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --locked && cargo fmt --all --check` and confirm 0 errors and 0 warnings.

## Out of scope

- Any non-comment code change.

## Done when

The Check `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --locked && cargo fmt --all --check` passes.

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
