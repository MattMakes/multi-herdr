# 92: Strengthen R2 e2e triage tests in p3_triage.rs with a multi-item triage_apply decision boundary

<!-- horch-ticket: 1 -->
**Persona:** primary `qa-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo build --workspace --bins --locked && scripts/dev-sandbox.sh cargo test -p horch-e2e --test p3_triage && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/p3_triage.rs`

Spec: `docs/specs/fleetd/reviews/end-review-p2-p3.md` (section `## Tests that do not prove their rule`, row 1: `FM-11, RR-16, RR-17`) and `docs/specs/fleetd/design.md` (rules FM-11, RR-16, RR-17).

## Read first: dependency on ticket 39 (R2-F1)

Ticket 39 (`39-fix-r2-f1-triage-apply-per-decision-savepoint.md` in `FIX-INDEX-1.md`, task T-41) updates `triage::apply::ingest` in `crates/horch-core/src/taskledger/triage/apply.rs` to settle each `(item, fact)` pair under its own SQLite savepoint (`SAVEPOINT triage_decision`) and adds the unit/integration test in `crates/horch-core/tests/triage_apply_job.rs`. Ticket 39 does not own `crates/horch-e2e/tests/p3_triage.rs`. This ticket adds the multi-item binary e2e test in `crates/horch-e2e/tests/p3_triage.rs` that end review R2 names.

## Goal

Add a multi-item `horch worktree triage apply` e2e test to `crates/horch-e2e/tests/p3_triage.rs` proving across the real CLI-to-daemon and `triage_apply` job boundary that when item 1 completes its rescue ref, archive/trash ref, and folder removal while item 2 in the same multi-item submission fails or has a refused fact, item 1's rescue ref, archive/trash ref, folder removal, and `removed` ledger state remain committed and item 2 fails only its own row.

## Requirements

- **R2 weak-test point — FM-11, RR-16, RR-17 (`The 7 named tests in crates/horch-e2e/tests/p3_triage.rs`)**:
  - **Existing tests and file:line**:
    - `pres_04_w9_triage_commits_untracked_before_delete` at `crates/horch-e2e/tests/p3_triage.rs:201`
    - `rr17_triage_refuses_a_unique_detached_head_and_the_tree_stays` at `crates/horch-e2e/tests/p3_triage.rs:243`
    - `rr17_triage_deletes_a_branch_only_when_the_trash_ref_holds_its_tip` at `crates/horch-e2e/tests/p3_triage.rs:264`
    - `pres_04_w8_refs_move_forward_and_are_read_back` at `crates/horch-e2e/tests/p3_triage.rs:294`
    - `read_back_that_fails_keeps_the_folder` at `crates/horch-e2e/tests/p3_triage.rs:335`
    - `rescue_ref_is_never_overwritten_and_each_removal_gets_a_new_one` at `crates/horch-e2e/tests/p3_triage.rs:366`
    - `a_folder_at_the_path_of_a_removed_row_is_a_new_orphaned_row_and_nothing_is_removed` at `crates/horch-e2e/tests/p3_triage.rs:485`
  - **Design rules**:
    - **FM-11** (`docs/specs/fleetd/design.md:1556`): "Each effect is a committed row in an intended state first, then the effect after the commit, keyed by its row id and safe to repeat, then the observation on the next pass."
    - **RR-16** (`docs/specs/fleetd/design.md:169`): "Archive, trash and rescue refs shall only move forward. Each ref write shall be read back before a folder is removed. Trash refs shall expire after 30 days (04 W8)."
    - **RR-17** (`docs/specs/fleetd/design.md:170`): "Triage shall commit modified and untracked files before ARCHIVE or DELETE, shall refuse a dirty or unique detached HEAD, and shall delete a branch only when the trash ref holds its tip (04 W9)."
  - **Why the existing tests do not prove the rule (from R2)**:
    "Each proves its decision in isolation. None submits a multi-item `triage_apply` result where a later refused fact must not erase an earlier completed effect."
  - **Stronger assert to write**:
    In `crates/horch-e2e/tests/p3_triage.rs`, add a helper `apply_many(h: &Harness, rows: &[(&str, &str, &str)]) -> String` (submitting multiple `| W<n> | <name> | | <decision> | |` lines in one `reply.md` to `horch worktree triage apply` and waiting with `wait_for` for the `triage_apply` job to end) and add test function `multi_item_triage_apply_preserves_completed_first_item_when_second_item_fails`:
    - Create `W1` (`feat-a`) with modified and untracked files, and `W2` (`feat-b`) detached at a unique commit (and/or a third item `W3` `feat-c` whose `refs/horch/archive/feat-c` points to an unrelated commit so `ARCHIVE` is refused by RR-16).
    - Submit all items in a single `horch worktree triage apply` call (`W1 feat-a DELETE`, `W2 feat-b DELETE`, `W3 feat-c ARCHIVE`).
    - Also test ingesting a multi-item `triage_apply` job result where item 1 completed its rescue ref and removal (`ok: true`, `removed: true`) and item 2 carries a malformed/refused fact (such as a malformed `archive_ref`), waiting with `wait_for` for `horch fleetd` to ingest the job result file.
    - Assert that in both cases, `W1` (`feat-a`) has its untracked and modified files saved on `refs/horch/trash/feat-a/...` and `refs/horch/rescue/...`, its folder `.worktrees/feat-a` is gone, its branch `refs/heads/feat-a` is deleted, and its `worktrees` row in `ledger.sqlite3` is recorded with `state = 'removed'`, `status = 'trashed'`, and its verified `rescue_ref`, while `W2` and `W3` remain on disk with `status = 'cleanup_failed'`.
  - **What to do if the stronger test is red**:
    If the test fails in `crates/horch-e2e/tests/p3_triage.rs`, fix it inside `**Own:**`. If it fails because `triage::apply::ingest` in `crates/horch-core/src/taskledger/triage/apply.rs` rolled back item 1 when item 2's fact was refused (because ticket 39 has not landed yet), do not edit `crates/horch-core/src/taskledger/triage/apply.rs` — stop and send `BLOCKED:` to the orchestrator naming ticket 39 (`39-fix-r2-f1-triage-apply-per-decision-savepoint.md`).

## Rules

1. Work only in the worktree that `horch ledger claim` gives. The worktree keeps its own `target/`.
2. Run each `cargo test`, `cargo run` and each manual binary through `scripts/dev-sandbox.sh` (unit A1 makes it). Until A1 lands, use the env of `ai_docs/plans/fleetd-finish/audit/isolation.md` section "Sandbox env", with the sandbox folder inside the worktree.
3. Never install the build. No `horch self-update`, `horch install`, `cargo install`, `launchctl`. Never write `~/.horch`, `~/.local/state/horch`, `~/.local/bin`, `~/Library/LaunchAgents`, `/fleet`.
4. Never read, set or pass `ANTHROPIC_API_KEY`.
5. Each Check ends with `cargo fmt --all --check`.
6. No flaky test. A test that fails 1 time in 3 runs is a bug: find the cause.
7. A unit that removes an entry of `crates/horch-e2e/expected-failures/` runs that test 3 times.
8. Before DONE: no process that the unit started is alive (`ps`), and the sandbox folder is removed. The finish step `cleanup` removes the worktree and its `target/`.
9. A unit that lands a task branch merges it with `git merge --no-ff origin/<branch>` in its worktree, resolves conflicts, and keeps each commit of the branch.

## Steps

1. Inspect `crates/horch-e2e/tests/p3_triage.rs` (lines 80-520) and check `git log` to verify whether ticket 39 (`39-fix-r2-f1-triage-apply-per-decision-savepoint.md`) has landed.
2. In `crates/horch-e2e/tests/p3_triage.rs`, add helper `apply_many(h: &Harness, items: &[(&str, &str, &str)]) -> String` and test function `multi_item_triage_apply_preserves_completed_first_item_when_second_item_fails`:
   - Set up `h = world("p3-triage-multi")`.
   - Create `path_a = folder(&h, "feat-a")` with modified `feat-a.txt` and untracked `new-a.txt`.
   - Create `path_b = folder(&h, "feat-b")`, detach `HEAD`, and commit `only-b.txt` so `feat-b` is at a unique detached HEAD (refused by RR-17).
   - Create `path_c = folder(&h, "feat-c")` and point `refs/horch/archive/feat-c` at `feat-b`'s commit so `ARCHIVE` on `feat-c` is refused by RR-16 / W8.
   - Run `apply_many(&h, &[("W1", "feat-a", "DELETE"), ("W2", "feat-b", "DELETE"), ("W3", "feat-c", "ARCHIVE")])`.
   - Assert that `W1 feat-a` succeeded (`!path_a.exists()`, `states(&h, &path_a) == ["removed"]`, `refs(&h, "refs/horch/trash/feat-a/").len() == 1`, `refs(&h, "refs/horch/rescue/").len() == 1`, and `show` on both refs returns the modified and untracked content), while `path_b.exists()` and `path_c.exists()` both hold and their rows record `cleanup_failed`.
3. Build the workspace binaries and run the ticket Check:
   `scripts/dev-sandbox.sh cargo build --workspace --bins --locked && scripts/dev-sandbox.sh cargo test -p horch-e2e --test p3_triage && cargo fmt --all --check`

## Out of scope

- Modifying `crates/horch-core/src/taskledger/triage/apply.rs` or `crates/horch-core/tests/triage_apply_job.rs` (owned by ticket `39-fix-r2-f1-triage-apply-per-decision-savepoint.md`).

## Done when

`multi_item_triage_apply_preserves_completed_first_item_when_second_item_fails` in `crates/horch-e2e/tests/p3_triage.rs` passes, all existing tests in `p3_triage.rs` pass, and the ticket Check passes.
