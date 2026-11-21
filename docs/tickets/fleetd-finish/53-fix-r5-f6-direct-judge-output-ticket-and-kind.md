# 53: Fix R5-F6: give direct-plan judge tasks a deterministic output, a generated verdict-format ticket, and effective kind `judge` in task.yaml (FR-4, RR-3, RR-12)

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** none
**Check:** `scripts/dev-sandbox.sh cargo test -p horch-core --test idea_direct && scripts/dev-sandbox.sh cargo test -p horch-core --test task_files_p6 && scripts/dev-sandbox.sh cargo test -p horch-core --test task_files && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/ideas/application/direct.rs`, `crates/horch-core/src/fleetd/task_execution/application/dispatch.rs`, `crates/horch-core/tests/idea_direct.rs`, `crates/horch-core/tests/task_files_p6.rs`

Spec: `docs/specs/fleetd/reviews/end-review-p6.md` (finding R5-F6) and `docs/specs/fleetd/design.md` (rules FR-4, RR-3, RR-12).

## Read first: overlap with landing unit C5 (T-561)

Ticket 08 (`08-c5-land-t561.md`, landing branch `origin/fleet/fleetd-build-T561-the-e2e-tests-of-the-judge-pair-rr-3-fm`) touches `crates/horch-core/src/fleetd/ideas/application/direct.rs` and `crates/horch-core/tests/idea_direct.rs` (adding `"outputs": [verdict_file(work)]` to the direct judge task in `plan_yaml`). It does not generate a ticket with verdict-format instructions for the direct judge task, and it does not fix `dispatch::facts` at `crates/horch-core/src/fleetd/task_execution/application/dispatch.rs:583`, where `task.yaml` still renders `kind: null` for any planned task whose kind is not in `WorkKind` (`judge`, `research`, `design`, `creative`). Check `git log` in your worktree first and build on T-561 if it has already landed.

## Goal

1. `add_direct_idea` in `crates/horch-core/src/fleetd/ideas/application/direct.rs` gives the paired judge task of a `--direct` judgment idea one deterministic verdict output path (`<first_own_stem>.review.md` or `<key>.review.md`) and one generated C1 ticket file (`tickets/02-<key>-judge.md`) that specifies the verdict path and the required Markdown verdict format (`## Verdict` with `pass` or `critical_open`, and `## Findings` with `[CRITICAL]`, `[HIGH]`, `[MEDIUM]`, or `[LOW]` items).
2. `dispatch::facts` in `crates/horch-core/src/fleetd/task_execution/application/dispatch.rs` populates `TaskFacts.kind` with the effective task kind (`task.work_kind` falling back to `json_extract(spec_json, '$.kind')`), so `task.yaml` renders `kind: judge` (and `research`, `design`, `creative`) instead of `kind: null`.

## Requirements

- **R5-F6 (Important — FR-4, RR-3, RR-12)**:
  - **Finding text and file:line**:
    A direct plan creates its judge with a title, kind, dependency, and profile, but no ticket or output at `crates/horch-core/src/fleetd/ideas/application/direct.rs:265`. Plan commit cannot store `judge` in the current `WorkKind` enum, so it stores no ledger work kind at `crates/horch-core/src/fleetd/ideas/application/plan_commit.rs:423`. Dispatch then renders `kind: null` in the judge's task file from `task.work_kind` at `crates/horch-core/src/fleetd/task_execution/application/dispatch.rs:583`. The judge receives no declared verdict path, no verdict-format instructions in a ticket, and no effective kind in `task.yaml`. It can validly finish without the one Markdown verdict that the structural checker expects, which leaves the judged task waiting or sends the judge through repair without actionable task instructions. The smallest fix is to make direct-plan generation declare one deterministic judge output and a small generated ticket that specifies the verdict format. Store the effective kind so dispatch renders `kind: judge`; keep the `spec_json` fallback only for migrated rows.
  - **Design rules**:
    - **FR-4** (`docs/specs/fleetd/design.md:140`): "The plan validator shall check that every task has a kind, a profile, an acceptance check and an `after` DAG with no cycles, that every requirement in `requirements.yaml` is covered by at least one task, and that every task of a judgment kind has a paired judge task."
    - **RR-3** (`docs/specs/fleetd/design.md:155`): "For a judgment kind (research, design, docs, creative or report), `done` requires a structural check of the output and a pass verdict from a separate judge task, with no open CRITICAL item."
    - **RR-12** (`docs/specs/fleetd/design.md:165`): each launched attempt receives complete, actionable task files (`task.yaml` and `inbox/START.md`).

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

1. Write the failing tests first:
   - In `crates/horch-core/tests/idea_direct.rs`, add test function `direct_judge_task_gets_verdict_output_and_generated_ticket`:
     - Add `DOCS_TICKET` (`Own: docs/sync.md`, `Kind: docs`) with `w.add(DOCS_TICKET, None)` and commit the plan with `w.start(&idea)`.
     - Assert that the committed judge task has `Own` equal to `["docs/sync.review.md"]`, `spec_json["outputs"]` equal to `["docs/sync.review.md"]`, and a non-empty `ticket_snapshot` (from a generated ticket file `tickets/02-document-the-retries-judge.md` in the idea folder) that names `docs/sync.review.md`, `## Verdict`, `pass`, `critical_open`, and `## Findings` with `[CRITICAL]`, `[HIGH]`, `[MEDIUM]`, `[LOW]`.
   - In `crates/horch-core/tests/task_files_p6.rs` (or `crates/horch-core/tests/idea_direct.rs`), add test function `dispatched_judge_task_renders_kind_judge_and_verdict_instructions_in_task_files`:
     - Dispatch or call `dispatch::write_task_files` on the committed judge task of a `--direct` `docs` idea.
     - Read the written `task.yaml` and `inbox/START.md`.
     - Assert that `task.yaml` has `kind: judge` (not `kind: null`), and `inbox/START.md` starts with the generated judge ticket snapshot naming `docs/sync.review.md` and the verdict format.
   - Check: run `scripts/dev-sandbox.sh cargo test -p horch-core --test idea_direct --test task_files_p6` and see the new tests fail because the judge task has no generated ticket and `dispatch::facts` renders `kind: null`.
2. In `crates/horch-core/src/fleetd/ideas/application/direct.rs`:
   - Ensure the judge task entry in `plan_yaml` declares `"outputs": [verdict_file(work)]` (where `verdict_file` derives `<first_own_stem>.review.md`, or `<work.key>.review.md` if `first_own` is `None`, matching T-561).
   - For a judgment kind, also generate a small valid C1 judge ticket `tickets/02-<work.key>-judge.md` (with `<!-- horch-ticket: 1 -->`, `**Persona:** primary \`<judge_profile>\``, `**Kind:** report` or `**Check:** none`, `**Own:** \`<verdict_file>\``, `## Goal`, `## Steps`, and `## Done when` explaining that the judge must write `<verdict_file>` with a `## Verdict` section containing `pass` or `critical_open` and a `## Findings` list where each item starts with `[CRITICAL]`, `[HIGH]`, `[MEDIUM]`, or `[LOW]`).
   - Reference `"ticket": judge_ticket_rel` on the judge task in `plan.yaml` and write the generated judge ticket file in `DirectAdded::write` (and in `write_idea_effects` if needed, or store the generated judge ticket alongside the work ticket in `DirectAdded`).
3. In `crates/horch-core/src/fleetd/task_execution/application/dispatch.rs`:
   - In `facts` (line 583), resolve `kind` as `task.work_kind.or_else(|| effective_kind(conn, &task.task_id))` (reading `SELECT COALESCE(work_kind, json_extract(spec_json, '$.kind')) FROM tasks WHERE task_id = ?1`, just as `verifier::next_of` does at `crates/horch-core/src/fleetd/task_execution/application/verifier.rs:341`), so `TaskFacts.kind` is `Some("judge".to_string())` and `task.yaml` renders `kind: judge`.
4. Run the ticket Check and confirm all tests and formatting pass.

## Out of scope

- Modifying `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs` unless required by `DirectAdded` serialization (prefer keeping the judge ticket generation inside `direct.rs` or `plan_commit.rs` so `pass.rs` is untouched).
- Adding a new SQLite schema migration for `WorkKind` (listed as an operator decision in R5).

## Done when

`direct_judge_task_gets_verdict_output_and_generated_ticket` and `dispatched_judge_task_renders_kind_judge_and_verdict_instructions_in_task_files` pass, a direct judge task receives its verdict path, format instructions, and `kind: judge` in `task.yaml`, and the ticket Check passes.
