# 39: Fix R2-F1: settle each triage_apply decision under its own savepoint (FM-11, RR-7, RR-16, RR-17)

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --test triage_apply_job && scripts/dev-sandbox.sh cargo test -p horch-e2e --test p3_triage && cargo fmt --all --check`
**Own:** `crates/horch-core/src/taskledger/triage/apply.rs`, `crates/horch-core/tests/triage_apply_job.rs`

Spec: `docs/specs/fleetd/reviews/end-review-p2-p3.md` (finding R2-F1) and `docs/specs/fleetd/design.md` (rules FM-11, RR-7, RR-16, RR-17).

## Goal

When fleetd ingests the result file of a multi-item `triage_apply` job in `triage::apply::ingest`, it settles each decision under its own SQLite savepoint so that a refused fact on one item fails only that item's decision and never rolls back completed rescue refs, folder removals, or state transitions of other items in the same job run.

## Requirements

- **R2-F1 (Important — FM-11, RR-7, RR-16, RR-17)**:
  - **Finding text and file:line**:
    `triage_apply` performs every `ARCHIVE`, `DELETE`, `LAND`, or `PR` effect independently and appends one fact per item (`crates/horch-core/src/taskledger/triage/apply.rs:1482`). Ingestion then checks and records those facts in order (`crates/horch-core/src/taskledger/triage/apply.rs:1673`). If a later fact is refused, `record_facts` rolls back every earlier ledger write and changes the whole job to `failed` (`crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs:881`). Concrete case: item 1 writes a verified rescue ref and removes its folder. Item 2 returns a malformed ref, commit, text, or worktree id. Item 1's effect has occurred, but item 2 makes fleetd roll back item 1's `rescued`/`removed` rows. The result is archived and the job ends failed, so the ledger does not record item 1's completed effect. This violates FM-11's rule that an effect is represented by an intended row and then observed idempotently. It also defeats the T-887 preservation fix when another item poisons the same result.
  - **Design rules**:
    - **FM-11** (`docs/specs/fleetd/design.md:1556`): "Each effect is a committed row in an intended state first, then the effect after the commit, keyed by its row id and safe to repeat, then the observation on the next pass."
    - **RR-7** (`docs/specs/fleetd/design.md:160`): "The supervisor shall survive its own restart with no lost or duplicated task, by rebuilding state from the ledger and the files."
    - **RR-16** (`docs/specs/fleetd/design.md:169`): "Archive, trash and rescue refs shall only move forward. Each ref write shall be read back before a folder is removed. Trash refs shall expire after 30 days (04 W8)."
    - **RR-17** (`docs/specs/fleetd/design.md:170`): "Triage shall commit modified and untracked files before ARCHIVE or DELETE, shall refuse a dirty or unique detached HEAD, and shall delete a branch only when the trash ref holds its tip (04 W9)."

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

1. Write the failing test first in `crates/horch-core/tests/triage_apply_job.rs`:
   - Add test function `refused_second_item_fact_does_not_roll_back_first_item_completed_rescue_and_removal`:
     - Create 2 worktrees (`tree1` and `tree2`) in `World` and queue a single multi-item `triage_apply` job covering both items (for example `ARCHIVE` or `DELETE` on `tree1` and `ARCHIVE` on `tree2`).
     - Run the job or write a `triage_apply` result file where item 1 has completed its rescue ref and folder removal (`ok: true`, `removed: true`, valid `rescue_ref` and `archive_ref`) and item 2 has a refused fact (for example a malformed `archive_ref`, an invalid `landed_commit`, or a mismatched `worktree_id`).
     - Ingest the result file through `jobs::end_from_files(&ctx, &mut w.conn)`.
     - Assert that item 1's worktree row remains recorded with `state = "removed"`, its verified `rescue_ref`, `status = "archived"` (or `"trashed"`), and `decision = "ARCHIVE"` (or `"DELETE"`), while item 2's worktree row is recorded as failed (`status = "cleanup_failed"`, `decision = None`, with the refusal reason in `reason`).
   - Check: run `scripts/dev-sandbox.sh cargo test -p horch-core --test triage_apply_job -- refused_second_item_fact_does_not_roll_back_first_item_completed_rescue_and_removal` and see it fail because `ingest` returns `Err` on item 2 and `record_facts` rolls back item 1.
2. In `crates/horch-core/src/taskledger/triage/apply.rs`:
   - In `ingest` (lines 1661-1710), process each `(item, fact)` pair inside its own savepoint (`SAVEPOINT triage_decision`).
   - Implement a helper `ingest_one(ctx, tx, item, fact) -> Result<()>` that parses `TriageDecision::parse(&item.decision)`, runs `check_fact(item, decision, fact)`, and settles the worktree row.
   - If `ingest_one` succeeds, `RELEASE triage_decision`.
   - If `ingest_one` returns `Err(why)`, `ROLLBACK TO triage_decision; RELEASE triage_decision`, and record the refusal on that item's worktree row via `failed(tx, &item.row, &wt, decision, &format!("fleetd refused the facts of this decision: {why:#}"))` (when the item's `wt` and `decision` exist in the ledger) so only that decision fails and earlier/later valid decisions in the same job remain committed.
3. Build workspace binaries if needed for `p3_triage` (`scripts/dev-sandbox.sh cargo build --workspace --bins`) and run the ticket Check.

## Out of scope

- Modifying `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`.
- Modifying `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs` (kept untouched so this ticket does not overlap with ticket 35).

## Done when

`refused_second_item_fact_does_not_roll_back_first_item_completed_rescue_and_removal` in `crates/horch-core/tests/triage_apply_job.rs` passes, a refused fact in a multi-item `triage_apply` result fails only its own decision without rolling back other decisions, and the ticket Check passes.
