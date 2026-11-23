# 52: Fix R5-F4: write own_decision assumptions.md as a post-commit effect and recover unwritten answer files from the ledger (RR-8, FM-11)

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --test question_answers && scripts/dev-sandbox.sh cargo test -p horch-core --test fleetd_pass_p6a && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/ideas/application/answers.rs`, `crates/horch-core/tests/question_answers.rs`

Spec: `docs/specs/fleetd/reviews/end-review-p6.md` (finding R5-F4) and `docs/specs/fleetd/design.md` (rules RR-8 and FM-11).

## Goal

`answers::answer_question` never writes `assumptions.md` inside the open SQLite transaction. Instead, for `Case::OwnDecision`, it records the intended new `assumptions.md` text (or own-decision line) in the transaction's `EventKind::Question` event and returns it in `answers::Recorded` to be written as an idempotent post-commit effect by `Recorded::write`. Furthermore, if fleetd exits after the transaction commits and before `Recorded::write` finishes, a recovery helper in `answers.rs` (or `settle_revisions` / `pending_replans`) reconstructs and writes any missing post-commit answer files (`assumptions.md`, `approval.json`, and `features/F-NNNN/decisions.md`) from the committed ledger rows.

## Requirements

- **R5-F4 (Important — RR-8, FM-11)**:
  - **Finding text and file:line**:
    `crates/horch-core/src/fleetd/ideas/application/answers.rs:416` explicitly places the own-decision append before commit. The function records the followed approval at line 447, then writes `assumptions.md` inside the open transaction at lines 448-450. Other answer files are correctly returned as post-commit effects at lines 103-122. If fleetd exits after `write_assumptions` and before the transaction commits, the filesystem contains a decision that has no committed question answer, event, or followed approval. The operation is designed to be idempotent, but the effect still exists without an intended row and recovery depends on the original request being retried. The smallest fix is an intended, keyed file-write row in the answer transaction. After commit, an idempotent effect writes the assumptions file, and the next pass retries or observes it before completing the answer.
  - **Design rules**:
    - **RR-8** (`docs/specs/fleetd/design.md:161`): "Each effect outside the ledger (a process start or stop, a file write, a git ref move) shall be a committed row in an intended state before it occurs, shall be keyed by the id of that row, and shall be safe to repeat."
    - **FM-11** (`docs/specs/fleetd/design.md:1554`): crash between ledger commit and external effect leaves no unrecorded effect before commit and completes idempotently after restart.

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

1. Write the failing tests first in `crates/horch-core/tests/question_answers.rs`:
   - Add test function `own_decision_rollback_before_commit_leaves_assumptions_md_unchanged`:
     - Set up a `World` with approved `ASSUMPTIONS` and an open question `q = w.ask("a", 1)`.
     - Open a transaction on `w.store`, call `answer_question(&tx, &ctx, &note)` with `AnswerCase::OwnDecision`, verify it returns `Ok(Answered::Recorded(recorded))`, and before calling `recorded.write(...)`, assert that `w.read("assumptions.md")` on disk still equals the original `ASSUMPTIONS` (no file write occurred inside the transaction).
     - Roll back (drop) `tx` without committing, and assert that `w.read("assumptions.md")` is still unchanged and `w.i7_holds()` is true.
   - Add test function `own_decision_crash_after_commit_before_effect_recovers_assumptions_and_approval`:
     - Commit `answer_question(&tx, &ctx, &note)` for `AnswerCase::OwnDecision` without calling `recorded.write(...)` (simulating a crash right after transaction commit).
     - Assert that `assumptions.md` on disk does not yet have the new line.
     - Run the recovery function (for example `answers::recover_answer_files(w.store.conn(), w.dir.path(), &owner())` or `recorded.write` re-derived from the committed ledger events) and assert that `assumptions.md`, `approval.json`, and `features/F-0001/decisions.md` are all written idempotently and `w.i7_holds()` is true.
   - Check: run `scripts/dev-sandbox.sh cargo test -p horch-core --test question_answers -- own_decision_rollback_before_commit_leaves_assumptions_md_unchanged` and see it fail because `own()` at `crates/horch-core/src/fleetd/ideas/application/answers.rs:454` writes `assumptions.md` before the transaction commits.
2. In `crates/horch-core/src/fleetd/ideas/application/answers.rs`:
   - Add an `assumptions: Option<(String, String)>` field (holding `(idea_id, new_assumptions_text)`) to `Recorded` (line 110).
   - In `own` (line 424): remove `folder.write_assumptions(&new)?;` from inside the transaction. Instead, return both the optional `ApprovalFile` and the intended `assumptions` write `(idea.id.clone(), new)` (and store the intended `assumptions` text or `approval_json` in the `EventKind::Question` event `data_json` at line 237 so it is durably recorded in the ledger row).
   - In `Recorded::write` (line 122): write `assumptions.md` first (via `IdeaFolder::new(&root.join(IDEAS), idea, owner.clone())?.write_assumptions(text)?` when `assumptions` is `Some`), then write `approval.json`, then write `self.decision.write(root, owner)`.
   - Add a recovery function `pub fn pending_answer_files(conn: &Connection, root: &Path, owner: &User) -> Result<Vec<Recorded>>` (and call or expose it so a pass after a crash can replay unwritten `Recorded` effects from the committed `EventKind::Question` events when `assumptions.md`, `approval.json`, or `decisions.md` does not yet hold the committed decision).
3. Run the ticket Check and confirm all tests and formatting pass.

## Out of scope

- Modifying `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs` (kept untouched so this ticket is not blocked by the landing chain).

## Done when

`own_decision_rollback_before_commit_leaves_assumptions_md_unchanged` and `own_decision_crash_after_commit_before_effect_recovers_assumptions_and_approval` in `crates/horch-core/tests/question_answers.rs` pass, `answer_question` performs no filesystem writes inside the SQLite transaction, and the ticket Check passes.
