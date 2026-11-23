# Fix tickets index: end review R5 (Phase 6)

Review covered:
- R5: `docs/specs/fleetd/reviews/end-review-p6.md` (commit `127bc22d` on `fleet/fleetd-finish`)

## Finding R5-F1 — Already fixed by T-898 (landing unit T-4 / ticket `04-c1-land-t898.md`)

- **R5-F1 (Critical — FR-10, SR-8, FM-32)**: `crates/horch/src/cmd/idea/mod.rs:130` and `crates/horch-core/src/fleetd/fleet_runtime/domain/authorize.rs:110` on `fleet/fleetd-finish` (before T-4 lands) checked only live attempts (`open_attempts_with_persona` and `peer_in_live_attempt`).
- **Why no ticket is written**: Per the extra rule of `fix-tickets-brief-2.md`, landing unit T-4 (`04-c1-land-t898.md`, merging `origin/fleet/fleetd-build-T898-fr-10-no-process-of-a-fleet-attempt-can`) already fixes R5-F1:
  - In `crates/horch-core/src/fleetd/fleet_runtime/adapters/peer_process.rs`, T-898 replaces `peer_in_live_attempt` with `fleet_process` and `recorded_processes`, which include the wrapper and agent PIDs and start times of all live attempts and all attempts ended within `ENDED_COUNTS_S` (24 hours), walk the caller's parent chain up to `MAX_CHAIN` (64), check process groups (`group_of`) and sessions (`session_of`) via `still_names`, and fail closed (`Fact::Unknown`) whenever any process fact cannot be read.
  - In `crates/horch/src/cmd/idea/mod.rs:103-135`, T-898 updates `refuse_inside_fleet_attempt` and `attempt_fact` to call `recorded_processes` and `fleet_process`, refusing on both `Fact::Yes` and `Fact::Unknown`.
  - In `crates/horch-core/src/fleetd/fleet_runtime/domain/authorize.rs:110-124`, T-898 updates `check_peer` and `PeerProcess` to enforce the `fleet_process` rule on the command channel.
  - In `crates/horch-e2e/tests/p6_fr10_approval.rs`, T-898 adds 6 e2e tests covering live attempts, recently ended attempts with a surviving agent or child shell, orphan process groups, and double-fork orphans.

## Fix tickets

| File | Finding ID | Rank | Persona | Own paths | Blocked by |
|---|---|---|---|---|---|
| [50-fix-r5-f2-clarifying-wait-durable-question.md](50-fix-r5-f2-clarifying-wait-durable-question.md) | R5-F2 | Important | primary `opus`, fallback `codex-sol` | `crates/horch/src/cmd/idea/note.rs`, `crates/horch-core/src/fleetd/ideas/adapters/idea_notes_reader.rs`, `crates/horch-core/src/fleetd/ideas/application/notes_ingest.rs`, `crates/horch-core/src/fleetd/ideas/application/waiting.rs`, `crates/horch-core/tests/idea_waiting.rs`, `crates/horch-core/tests/idea_notes.rs`, `crates/horch-core/tests/fleetd_pass_p6a.rs` |  |
| [51-fix-r5-f3-judge-verification-plan-no-command-check.md](51-fix-r5-f3-judge-verification-plan-no-command-check.md) | R5-F3 | Important | primary `opus`, fallback `codex-sol` | `crates/horch-core/src/fleetd/task_execution/domain/verification.rs`, `crates/horch-core/src/fleetd/task_execution/application/verifier.rs`, `crates/horch-core/tests/judge_pair.rs` | T-8 (`08-c5-land-t561.md`) |
| [52-fix-r5-f4-own-decision-post-commit-effect.md](52-fix-r5-f4-own-decision-post-commit-effect.md) | R5-F4 | Important | primary `opus`, fallback `codex-sol` | `crates/horch-core/src/fleetd/ideas/application/answers.rs`, `crates/horch-core/tests/question_answers.rs` |  |
| [53-fix-r5-f6-direct-judge-output-ticket-and-kind.md](53-fix-r5-f6-direct-judge-output-ticket-and-kind.md) | R5-F6 | Important | primary `opus`, fallback `codex-sol` | `crates/horch-core/src/fleetd/ideas/application/direct.rs`, `crates/horch-core/src/fleetd/task_execution/application/dispatch.rs`, `crates/horch-core/tests/idea_direct.rs`, `crates/horch-core/tests/task_files_p6.rs` | T-8 (`08-c5-land-t561.md`) |

## File overlaps and `pass.rs` avoidance

- **`pass.rs` avoidance**: None of tickets 50 through 53 includes `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs` in `**Own:**`.
  - In ticket 50 (R5-F2), the durable open interview-question check is enforced inside `crates/horch-core/src/fleetd/ideas/application/waiting.rs` (`waits`), while `pass.rs:3790` (`asking`) continues to supply the corroborating pane-idleness set `Wait::asking`.
  - In ticket 52 (R5-F4), moving `write_assumptions` out of the transaction into `answers::Recorded::write` works seamlessly with `pass.rs:3490`, which already collects `answers::Answered::Recorded` and calls `retry_answers` (`r.write(root, owner)`) after the transaction commits.
- **No shared `Own` paths among tickets 50 to 53**: Each of the 4 tickets owns a disjoint set of files, and none overlaps with tickets 35 to 39 in `FIX-INDEX-1.md`.
- **Overlap with landing unit T-8 (`08-c5-land-t561.md`)**:
  - Ticket 51 (R5-F3) shares `crates/horch-core/src/fleetd/task_execution/domain/verification.rs`, `crates/horch-core/src/fleetd/task_execution/application/verifier.rs`, and `crates/horch-core/tests/judge_pair.rs` with landing unit T-8 (`08-c5-land-t561.md`). Note: T-561 (commits `0eafdc37` and `8a207cb6`) already adds `plan_judge` and `JUDGE_NO_CHECK` to `verification.rs` and `verifier.rs`; ticket 51 folds the effective task kind into `verification::Facts` / `verification::plan` and adds the direct unit test on `verification::plan`.
  - Ticket 53 (R5-F6) shares `crates/horch-core/src/fleetd/ideas/application/direct.rs` and `crates/horch-core/tests/idea_direct.rs` with landing unit T-8 (`08-c5-land-t561.md`). Note: T-561 already adds `"outputs": [verdict_file(work)]` in `direct.rs`; ticket 53 adds the generated judge ticket with verdict-format instructions and fixes `dispatch.rs:583` so `task.yaml` renders `kind: judge` instead of `kind: null`.

## Minor findings

- **R5-F5 (Minor — OR-1, RR-8)**:
  - Location: `crates/horch-core/src/fleetd/ideas/application/approval.rs:133-158` and `check_i7` at lines `273-304`.
  - Summary: An approval transition records the expected `approval.json` hash in `ideas.approval_sha256` before the returned post-commit `ApprovalFile` is written to disk. During that interval, `check_i7` treats the missing `approval.json` file as an approval breach and briefly opens an I7 needs-you item (`i7_approval_mismatch`), which a later pass resolves once the post-commit effect writes `approval.json`.

## Open questions

1. **R5-F3 and R5-F6 overlap with T-561 (landing unit T-8 / `08-c5-land-t561.md`)**: T-561 was not yet landed on `fleet/fleetd-finish` when R5 ran at `127bc22d`. Once T-8 lands T-561, R5-F3 is largely fixed by `plan_judge` (`0eafdc37` / `8a207cb6`) and the `"outputs": [verdict_file(work)]` part of R5-F6 is fixed (`609c8bf2`), leaving the `verification::plan` unit test (ticket 51) and the generated judge ticket + `dispatch.rs` `kind: judge` fix (ticket 53). Both tickets 51 and 53 are marked `Blocked by: T-8 (08-c5-land-t561.md)` so they do not conflict with the T-561 merge.
2. **Retiring `spawn_adopt` (R5 watchlist lines 38 and 73, and Operator decisions)**: R5 verified that `spawn_adopt` has no active client (`crates/horch-core/src/fleetd/fleet_runtime/domain/requests.rs:136` and `crates/horch-core/src/fleetd/fleet_runtime/application/handlers_worker.rs:57`) and recommends retiring it and documenting the remaining 7 kept internal request kinds in the design request map.
3. **Design and product decisions listed in R5 ("Operator decisions" section of `end-review-p6.md`)**:
   - *Idea amendment exits*: Add `ticketing`, `in_progress`, and `integrating` -> `awaiting_approval` to the state table in `docs/specs/fleetd/design.md` to match `crates/horch-core/src/fleetd/fleet_runtime/domain/ledger.rs:353`.
   - *Repair persona*: Document in `design.md` that repair dispatch uses the primary persona of the conflicted task (`crates/horch-core/src/fleetd/task_execution/application/repair.rs:530`).
   - *Drift project*: Document the sorted-first repository selection rule (`crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs:4276`) until an explicit config key is added.
   - *Fleet stop*: Operator decision on whether to add a `horch fleetd stop` CLI subcommand through the platform service layer.
   - *Judge profile & judge verdict artifact path*: Operator decisions on whether to add an optional roster field for designated judge profiles and whether `<first_own_stem>.review.md` should stay the declared verdict output path.
   - *Work-kind schema*: Operator decision on when to add `research`, `design`, `creative`, and `judge` to `WorkKind` in a future schema migration (keeping the `spec_json` fallback in the meantime).
