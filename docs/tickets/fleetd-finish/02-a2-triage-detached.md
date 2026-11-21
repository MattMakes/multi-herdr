# 02: A2 triage bug: fix detached-HEAD worktree handling in horch worktree triage apply

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --lib taskledger::triage && scripts/dev-sandbox.sh cargo test -p horch-core --test worktree_triage && cargo fmt --all --check`
**Own:** `crates/horch-core/src/taskledger/triage/apply.rs`, `crates/horch-core/src/taskledger/triage/mod.rs`, `crates/horch-core/tests/worktree_triage.rs`

Spec: `/Users/matthewscott/repo/multi-herdr/ai_docs/plans/fleetd-finish/plan.md` (section 1 item L14; section 3 unit A2) and `/Users/matthewscott/repo/multi-herdr/ai_docs/plans/fleetd-finish/audit/worktree-cleanup.md` (section "Bug found").

## Goal

`horch worktree triage apply` succeeds with `ARCHIVE` and `DELETE` on a detached-HEAD worktree whose recorded branch is empty or `(detached)`. It passes the commit id `tip` to `git rev-list` instead of the literal text `(detached)`.

## Requirements

- **L14**: When a worktree has a detached HEAD (its recorded `branch` is empty or equals `custody_git::DETACHED`, `"(detached)"`), `triage::apply` uses the commit SHA `tip` for safety checks and never passes `(detached)` as a branch ref to git commands.

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

## Steps

1. In `crates/horch-core/tests/worktree_triage.rs` (or the triage test module in `crates/horch-core/src/taskledger/triage/apply.rs` if `tests/worktree_triage.rs` does not exist yet), write 2 tests with a detached-HEAD worktree (including a row whose `branch` is `"(detached)"` as set by `custody_git::DETACHED`):
   - `triage_archive_detached_worktree_uses_commit_id`: `ARCHIVE` writes `refs/horch/archive/<name>` at the detached commit id, passes `must_be_safe`, removes the worktree folder through custody, and records `WorktreeStatus::Archived`.
   - `triage_delete_detached_worktree_uses_commit_id`: `DELETE` writes `refs/horch/trash/<date>/<name>` at the detached commit id, passes `must_be_safe`, removes the worktree folder through custody, skips branch deletion (`"no branch to delete"`), and records `WorktreeStatus::Trashed`.
   - Check: run the new tests and see them fail with `git rev-list ... ..(detached)`.
2. In `crates/horch-core/src/taskledger/triage/apply.rs` and `crates/horch-core/src/taskledger/triage/mod.rs`, treat `wt.branch == custody_git::DETACHED` (`"(detached)"`) as having no branch (`has_no_branch(branch: &str) -> bool` returning `branch.is_empty() || branch == custody_git::DETACHED`):
   - In `check_identity` (line 347): allow `Some(_)` when `has_no_branch(&wt.branch)`, and when `git.current_branch(dir)` is `None`, check dirty files and reachable refs as RR-17 requires.
   - In `must_be_safe` (line 428): when `has_no_branch(&w.branch)`, set `w.branch = tip.to_string()` so `land::unsafe_to_remove` runs `git rev-list <holder>..<tip>`.
   - In `settle_gone` (line 488), `archive_at` (line 675), and `delete_branch` (line 725): treat `has_no_branch(&wt.branch)` the same as `wt.branch.is_empty()`.
   - In `ledger_row_of` in `crates/horch-core/src/taskledger/triage/mod.rs` (line 349): set `row.branch = (!l.branch.is_empty() && l.branch != custody_git::DETACHED).then(|| l.branch.clone())`.
   - Check: run the triage unit and integration tests through the sandbox env.
3. Run the ticket Check and format check.

## Out of scope

- Changing `custody_git::DETACHED` in `crates/horch-core/src/fleetd/worktree_custody/adapters/git.rs`.
- Changing `RR-17` refusal for a detached HEAD at a unique unreferenced commit.

## Done when

The Check passes, and `horch worktree triage apply` with `ARCHIVE` and `DELETE` succeeds on a detached-HEAD worktree without passing `(detached)` to `git rev-list`.
