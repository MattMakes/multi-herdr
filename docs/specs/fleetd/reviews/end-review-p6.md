# End review of Phase 6

## Summary

Critical: 1. Important: 4. Minor: 1.
The Critical finding lets an agent that outlives its attempt pass the operator-process guard.
The Important findings can stop idea planning, reject or under-specify judge work, or put a file effect before its ledger row.

## Findings

### R5-F1 — Critical — FR-10, SR-8, FM-32

**Claim: verified.** `crates/horch/src/cmd/idea/mod.rs:130` asks only for live attempts, and returns `Fact::No` when that set is empty at line 156. The server authorization rule likewise describes and checks only live attempts at `crates/horch-core/src/fleetd/fleet_runtime/domain/authorize.rs:110`.

If fleetd ends an attempt while its agent or child shell is still alive, that process is no longer compared with any recorded attempt. It can therefore pass the CLI guard for `horch idea approve`; the same missing process fact affects server authorization. This gives a fleet worker an operator action. The smallest fix is one fail-closed fleet-process fact, shared by CLI and server authorization, that includes live and recently ended attempt process ancestry, process groups, and sessions. Unknown process facts must refuse the request.

### R5-F2 — Important — FR-2, FM-29, RR-7

**Claim: verified.** `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs:3790` defines an asking sub-orchestrator solely as a live clarifying process whose herdr state is `idle` or `done`. `crates/horch-core/src/fleetd/ideas/application/waiting.rs:278` turns that observation into a wait, and lines 319-350 stop the attempt after the idle limit. `crates/horch-core/src/fleetd/ideas/application/sub_orchestrators.rs:274` then declines to start a replacement while the idea waits.

A clarifier that stalls or merely returns to its prompt without asking a question is recorded as waiting on the operator. After 30 fleet minutes, fleetd ends it and does not restart it. The visible item contains no durable question that the operator can answer, so planning can hang until an unrelated amendment. The smallest fix is a durable interview-question or wait row written before the wait. Pane idleness can corroborate that row, but must not create the wait by itself.

### R5-F3 — Important — RR-3, FM-9, FM-33

**Claim: verified.** The plan schema deliberately permits a judge task without acceptance at `crates/horch-core/src/fleetd/ideas/domain/plan_validator.rs:168`, and direct plans create exactly that task at `crates/horch-core/src/fleetd/ideas/application/direct.rs:265`. Verification facts contain no task kind at `crates/horch-core/src/fleetd/task_execution/domain/verification.rs:67`; `verification::plan` rejects a changed task with no command and no `check_none` at lines 100-117. The later structural routing at `crates/horch-core/src/fleetd/task_execution/application/verifier.rs:340` cannot repair an already failed command verdict.

A direct docs or report task creates a judge that commits its verdict file. Because the judge has changes but no command check, fleetd records `no check` and never reaches the structural judge-verdict check. The judged task then waits for its judge. The smallest fix is to pass the effective task kind into `verification::plan` and make `judge` skip the command-check decision and proceed directly to its structural verdict check.

### R5-F4 — Important — RR-8, FM-11

**Claim: verified.** `crates/horch-core/src/fleetd/ideas/application/answers.rs:416` explicitly places the own-decision append before commit. The function records the followed approval at line 447, then writes `assumptions.md` inside the open transaction at lines 448-450. Other answer files are correctly returned as post-commit effects at lines 103-122.

If fleetd exits after `write_assumptions` and before the transaction commits, the filesystem contains a decision that has no committed question answer, event, or followed approval. The operation is designed to be idempotent, but the effect still exists without an intended row and recovery depends on the original request being retried. The smallest fix is an intended, keyed file-write row in the answer transaction. After commit, an idempotent effect writes the assumptions file, and the next pass retries or observes it before completing the answer.

### R5-F5 — Minor — OR-1, RR-8

**Claim: verified.** An approval transition records the expected `approval.json` hash before the returned file is written (`crates/horch-core/src/fleetd/ideas/application/approval.rs:133-158`). During that interval, `check_i7` treats the missing file as an approval breach and opens a needs-you item at lines 273-304. The post-commit effect writes the file, and a later pass resolves the item.

An operator can briefly receive a false I7 alert for a release or approval that is still completing normally. The smallest fix is to keep an intended approval-file state in the ledger and suppress the I7 item while that retryable effect remains pending. A failed or expired effect should still open the item.

### R5-F6 — Important — FR-4, RR-3, RR-12

**Claim: verified.** A direct plan creates its judge with a title, kind, dependency, and profile, but no ticket or output at `crates/horch-core/src/fleetd/ideas/application/direct.rs:265`. Plan commit cannot store `judge` in the current `WorkKind` enum, so it stores no ledger work kind at `crates/horch-core/src/fleetd/ideas/application/plan_commit.rs:423`. Dispatch then renders `kind: null` in the judge's task file from `task.work_kind` at `crates/horch-core/src/fleetd/task_execution/application/dispatch.rs:583`.

The judge receives no declared verdict path, no verdict-format instructions in a ticket, and no effective kind in `task.yaml`. It can validly finish without the one Markdown verdict that the structural checker expects, which leaves the judged task waiting or sends the judge through repair without actionable task instructions. The smallest fix is to make direct-plan generation declare one deterministic judge output and a small generated ticket that specifies the verdict format. Store the effective kind so dispatch renders `kind: judge`; keep the `spec_json` fallback only for migrated rows.

## Watchlist lines covered

| Line | Point | Result |
|---:|---|---|
| 6 | T-504 | **Checked, no fault (verified):** task addition reads a parent idea or the youngest in-flight idea and refuses absence at `crates/horch-core/src/taskledger/tasks.rs:643`; the release backfill, not `tasks::add`, creates legacy ideas at `crates/horch-core/src/taskledger/schema.rs:1439`. |
| 14 | T-542 | Finding R5-F6. Verifier routing has a `spec_json` fallback at `crates/horch-core/src/fleetd/task_execution/application/verifier.rs:340`, but dispatched task files do not. |
| 22 | T-521 | **Checked, no code fault (verified):** the 3 amend exits are explicit at `crates/horch-core/src/fleetd/fleet_runtime/domain/ledger.rs:353`; the design state table needs the same amendment paths. |
| 26 | T-531 | Finding R5-F2. |
| 30 | T-543 | Finding R5-F6 for the task-file reader. Structural routing itself correctly falls back to `spec_json` at `crates/horch-core/src/fleetd/task_execution/application/verifier.rs:340`. |
| 33 | T-885 | **Checked, no worker-forge fault (verified):** `question_answer` is operator class at `crates/horch-core/src/fleetd/fleet_runtime/domain/requests.rs:166`. The missing CLI-level proof remains listed below. |
| 34 | T-390 | **Checked, no code fault (verified):** repair dispatch uses the task's selected persona at `crates/horch-core/src/fleetd/task_execution/application/repair.rs:530`. The source of that policy is a design decision. |
| 38 | T-472, T-550, T-885 | **Checked, no class fault (verified):** all 8 unlisted kinds are operator class at `crates/horch-core/src/fleetd/fleet_runtime/domain/requests.rs:166`. `spawn_adopt` is dead surface and should be retired; the stop-command gap is an operator decision. |
| 39 | T-600 | **Checked, no implementation fault (verified):** drift selects the first repository after sorting paths at `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs:4276`. The design and docs-only adapter gaps require decisions. |
| 40 | T-550 | **Checked, no contradiction in current code (verified):** the retired command says fleetd remains under the OS service manager at `crates/horch/tests/p6_retired_worker_commands.rs:60`. The missing product command requires a decision. |
| 43 | T-565 | **Checked, no fault (verified):** the fleet skill says fleetd launches attempts and retains the operator's status and stop commands at `skills/orchestrate/SKILL.md:8` and line 29. |
| 56 | T-603 | **Checked, no additional fault (verified):** task outcome derives `no_result` only when the worker supplies no result at `crates/horch-core/src/fleetd/task_execution/domain/task.rs:442`. The unlanded end-to-end case is not evidence against this worktree. |
| 60 | T-896 | **Checked, no fault (verified):** direct status comes from the ledger, while `files_written` only observes completion of its post-commit effect before the next transition at `crates/horch-core/src/fleetd/ideas/application/dispatcher.rs:224`. |
| 61 | T-561 | Finding R5-F3. |
| 62 | T-896 | Finding R5-F5. |
| 64 | T-561 | Finding R5-F6. The current direct plan declares no judge output at `crates/horch-core/src/fleetd/ideas/application/direct.rs:265`; the proposed repository path is an operator decision. |
| 68 | T-561 | **Checked, no hidden wait (verified):** direct selection is deterministic and cross-provider at `crates/horch-core/src/fleetd/ideas/application/direct.rs:284`; unhealthy profiles become visible task holds at `crates/horch-core/src/fleetd/fleet_runtime/application/holds.rs:346`. Judge designation remains an operator decision. |
| 69 | T-558 | Finding R5-F1. |
| 70 | T-898 | Finding R5-F1. Real Claude and Codex sandbox proof is not verified in this review. |
| 73 | T-895 | **Checked, no active client found (verified):** `spawn_adopt` remains accepted at `crates/horch-core/src/fleetd/fleet_runtime/domain/requests.rs:136` and handled at `crates/horch-core/src/fleetd/fleet_runtime/application/handlers_worker.rs:57`; retire it. |
| 76 | T-895 | **Checked, no Phase 6-specific fault established (verified):** task attempts without process evidence are governed by the task stall ladder, not the idea dispatcher, at `crates/horch-core/src/fleetd/task_execution/domain/stall.rs:140`. |

## Tests that do not prove their rule

- **FR-10 / SR-8 / FM-32 — `fm32_forged_approval_is_ignored`: verified gap.** `crates/horch-core/tests/idea_approval.rs:218` proves that an untrusted file hash is ignored. It does not run `horch idea approve` from an agent process after fleetd ends its attempt. No current binary test proves the recently-ended process boundary.
- **FR-2 / FM-29 — `idle_sub_orchestrator_stops_after_30_min_of_fleet_clock`: verified gap.** `crates/horch-core/tests/idea_waiting.rs:313` supplies the `asking` set as a test fact. It never proves that an idle clarifier has a durable unanswered question, so the false-wait path passes.
- **RR-3 / FM-33 — `a_judgment_kind_gets_the_structural_check`: verified gap.** `crates/horch-core/src/fleetd/task_execution/domain/verification.rs:334` tests only routing after a successful command verdict. It does not pass a changed judge with no command through `verification::plan`.
- **FR-4 / RR-12 — direct judge task: verified gap.** Plan-validator tests prove judge pairing, but no current binary test proves that a direct judge receives a verdict path, format instructions, and `kind: judge` in its launched task files.
- **RR-8 / FM-11 — `own_decision_is_appended_as_an_assumption`: verified gap.** `crates/horch-core/tests/question_answers.rs:632` tests the successful write and idempotent content. It has no crash point between the assumptions write and transaction commit.
- **FR-1, FR-2, RR-3, RR-12 — Phase 6 binary coverage: verified gap.** The required binary suites `p6_intake.rs`, `p6_sub_orchestrators.rs`, `p6_judge_pair.rs`, and `p6_questions.rs` are absent in this worktree. Lower-level tests do not prove the CLI-to-daemon boundary, a real push attempt, or restart recovery.
- **FR-10 real sandbox: not verified.** No test in this review runs real Claude or Codex to prove that approval, operator-token read, and detached-process requests all fail.

## Operator decisions

- **Idea amendment exits (verified design gap).** Add `ticketing`, `in_progress`, and `integrating` to `awaiting_approval` to the design state table. Keep the current code because an amendment after approval must invalidate the old approval.
- **Repair persona (verified design gap).** Specify the primary persona of the conflicted task as the resolver persona. It is deterministic, preserves task context, and matches current repair dispatch.
- **Drift project (verified design gap).** Replace the implicit first-path rule with an explicit project in fleet configuration. Until that exists, document the sorted-first repository rule. Also define conformance for a docs-only CLI with no exec adapter.
- **Fleet stop (verified product gap).** Add `horch fleetd stop` through the platform service layer. It should stop by recorded PID and start time, wait for the lock, and disable automatic restart until an explicit start.
- **Unlisted request kinds (verified design gap).** Add retained internal kinds to the request map and retire `spawn_adopt` now. Dead accepted request kinds increase privileged surface without a client.
- **Judge profile (verified product choice).** Add an optional roster field for designated judge profiles. Preserve deterministic cross-provider selection as the fallback.
- **Judge verdict artifact (not verified as product intent).** Keep verdicts as review artifacts, but place them under a declared review-output path rather than deriving a repository filename from the judged task's first owned path. State explicitly whether that path lands.
- **Work-kind schema (verified debt).** Add `research`, `design`, `creative`, and `judge` to the ledger enum in the next schema migration. Keep the current `spec_json` fallback during migration.
