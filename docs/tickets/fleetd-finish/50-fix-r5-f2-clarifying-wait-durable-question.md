# 50: Fix R5-F2: require a durable open question or wait row before a clarifying sub-orchestrator enters waiting_on_you (FR-2, FM-29, RR-7)

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --test idea_waiting && scripts/dev-sandbox.sh cargo test -p horch-core --test idea_notes && scripts/dev-sandbox.sh cargo test -p horch-core --test sub_orchestrators && scripts/dev-sandbox.sh cargo test -p horch-core --test fleetd_pass_p6a && cargo fmt --all --check`
**Own:** `crates/horch/src/cmd/idea/note.rs`, `crates/horch-core/src/fleetd/ideas/adapters/idea_notes_reader.rs`, `crates/horch-core/src/fleetd/ideas/application/notes_ingest.rs`, `crates/horch-core/src/fleetd/ideas/application/waiting.rs`, `crates/horch-core/tests/idea_waiting.rs`, `crates/horch-core/tests/idea_notes.rs`, `crates/horch-core/tests/fleetd_pass_p6a.rs`

Spec: `docs/specs/fleetd/reviews/end-review-p6.md` (finding R5-F2) and `docs/specs/fleetd/design.md` (rules FR-2, FM-29, RR-7).

## Goal

A clarifying idea enters `waiting_on_you` only when the ledger has a durable open interview-question or wait row recorded for that idea before the wait begins. Pane idleness (`Wait::asking`) can corroborate that row, or `waits` in `waiting.rs` requires that row in addition to `ctx.asking`, so a clarifier that stalls or returns to its prompt without asking a durable question is never marked `waiting_on_you`, never idle-stopped as waiting on the operator, and never left without a replacement sub-orchestrator.

## Requirements

- **R5-F2 (Important — FR-2, FM-29, RR-7)**:
  - **Finding text and file:line**:
    `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs:3790` defines an asking sub-orchestrator solely as a live clarifying process whose herdr state is `idle` or `done`. `crates/horch-core/src/fleetd/ideas/application/waiting.rs:278` turns that observation into a wait, and lines 319-350 stop the attempt after the idle limit. `crates/horch-core/src/fleetd/ideas/application/sub_orchestrators.rs:274` then declines to start a replacement while the idea waits. A clarifier that stalls or merely returns to its prompt without asking a question is recorded as waiting on the operator. After 30 fleet minutes, fleetd ends it and does not restart it. The visible item contains no durable question that the operator can answer, so planning can hang until an unrelated amendment. The smallest fix is a durable interview-question or wait row written before the wait. Pane idleness can corroborate that row, but must not create the wait by itself.
  - **Design rules**:
    - **FR-2** (`docs/specs/fleetd/design.md:138`): "Deterministic code in fleetd shall move each idea through its states as its files validate, and shall start one fresh sub-orchestrator process for each phase of one idea."
    - **FM-29** (`docs/specs/fleetd/design.md:1574`): "While an idea is in `clarifying` with a question open, or in `awaiting_approval`, its lease is paused and fleetd sends one reminder an hour during your working hours. After 30 minutes on the fleet clock it stops the idle sub-orchestrator process to free memory; an open question or your approval starts it again from the folder."
    - **RR-7** (`docs/specs/fleetd/design.md:160`): "The supervisor shall survive its own restart with no lost or duplicated task, by rebuilding state from the ledger and the files."

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

1. Write the failing tests first in `crates/horch-core/tests/idea_waiting.rs`:
   - Add test function `idle_clarifier_without_durable_question_does_not_wait_on_you`:
     - Set up a `World` with `IDEA` in `"clarifying"` and a live sub-orchestrator attempt `P1`, with `IDEA` present in `Wait::asking` (its pane is `idle`), but with no durable open interview-question note/event recorded for `IDEA`.
     - Run `w.wait(1000, MONDAY_10, &[IDEA], &[P1])` and `w.wait(2800, "2026-11-16T10:30:00+01:00", &[IDEA], &[P1])`.
     - Assert that `waits.started` is empty, `waits.idle_stopped` is empty, `w.waiting_since()` is `None`, and no `waiting_on_you:I-0002` needs-you item is opened.
   - Add test function `clarifier_with_durable_open_question_waits_and_shows_question_text`:
     - Record a durable open interview question for `IDEA` (via an `idea note` interview-ask note or durable `events` row of `Entity::Idea` with the question text), then run `w.wait(1000, MONDAY_10, &[IDEA], &[P1])`.
     - Assert that `waits.started` is `[IDEA]`, `w.waiting_since()` is `Some(...)`, the open needs-you item `waiting_key(IDEA)` includes the durable question text so the operator can answer it, and when an answer note or amendment is ingested after the question, the open question is closed and `keep_waits` resolves the wait.
   - Check: run `scripts/dev-sandbox.sh cargo test -p horch-core --test idea_waiting` and see `idle_clarifier_without_durable_question_does_not_wait_on_you` fail because `waits` at `crates/horch-core/src/fleetd/ideas/application/waiting.rs:283-284` currently returns `true` from `ctx.asking.contains(&row.id)` alone.
2. Add durable interview-question recording and checking without touching `pass.rs`:
   - In `crates/horch/src/cmd/idea/note.rs`, `crates/horch-core/src/fleetd/ideas/adapters/idea_notes_reader.rs`, and `crates/horch-core/src/fleetd/ideas/application/notes_ingest.rs`:
     - Allow the clarifying sub-orchestrator to record an open interview question note before the operator answers (for example `horch idea note <id> --ask "<question>"` or an `interview` note whose answer is `None` / omitted when no `" - "` separator is present, while keeping `"<question> - <answer>"` for a recorded question-and-answer pair).
     - When `notes_ingest::ingest` accepts an open interview question note, record a durable `EventKind::Note` event on `Entity::Idea` (for example cause `"interview question asked"` with `data_json` `{ "attempt": note.attempt, "question": question, "amendments": ... }`). When `notes_ingest::ingest` accepts an interview answer note (`"<question> - <answer>"`), append to `interview.md` as today and record a durable `EventKind::Note` event (for example cause `"interview question answered"`) that closes the open interview question.
   - In `crates/horch-core/src/fleetd/ideas/application/waiting.rs`:
     - Add a helper `open_interview_question(conn: &Connection, ctx: &Wait<'_>, idea: &str) -> Result<Option<String>>` that checks whether `idea` has a durable open interview-question event (newer than any `"interview question answered"` event and with no new amendment file added in `amendments/` since the question was asked).
     - In `waits` (line 273): for `"clarifying"`, require `open_interview_question(tx, ctx, &row.id)?.is_some()` both when `live` is `Some(_)` (corroborated by `ctx.asking.contains(&row.id)`) and when `live` is `None` after `IDLE_STOP` (`!answered(tx, ctx, &row.id)?`).
     - In `waiting_text` (line 430): include the durable open question text in the `waiting_on_you` needs-you item when present.
   - Update existing unit tests in `crates/horch-core/tests/idea_waiting.rs`, `crates/horch-core/tests/idea_notes.rs`, and `crates/horch-core/tests/fleetd_pass_p6a.rs` (`pass_never_waits_for_a_sub_orchestrator`) so that clarifying wait tests record the durable open interview question before expecting `waiting_on_you_since` to be set.
3. Run the ticket Check and confirm all tests and formatting pass.

## Out of scope

- Modifying `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs` (kept untouched so this ticket is not blocked by the landing chain).
- Changing `awaiting_approval` wait behavior.

## Done when

`idle_clarifier_without_durable_question_does_not_wait_on_you` and `clarifier_with_durable_open_question_waits_and_shows_question_text` pass, an idle clarifying pane without a durable open question never enters `waiting_on_you`, and the ticket Check passes.
