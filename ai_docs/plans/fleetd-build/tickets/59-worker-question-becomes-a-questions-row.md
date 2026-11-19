# A question of a worker in a push attempt becomes 1 questions row with its owner, through the ingest of fleetd (RR-12, FM-35)

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets --locked && cargo test -p horch-core --lib fleetd && cargo test -p horch-e2e --test p6_worker_question && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/p6_worker_question.rs`

## Goal

When a worker of a push attempt asks a question (`horch ask`, or a QUESTION tell that the design
maps to a question), fleetd writes exactly 1 `questions` row with the owner that the design names
(the sub-orchestrator of the feature, or the operator), in the same transaction as the move of
the task to `blocked_on_user`. The answer path of Phase 6 then works from a real question.

## Context

Found by T-562 (opus-159) on the run branch:

- `questions::ask` has 1 caller, `workers::sync_questions`, and `watchdog_duties` calls it only
  for a run with a `workspace_id` (the old flow with an orchestrator seat).
- A question file in the outbox of an attempt only moves the task to `blocked_on_user`
  (`ingest.rs`, outcome `asked`) and writes no `questions` row.
- Probe of the worker: the fake worker ran `ask`; the task stayed `running` and the `questions`
  table stayed empty. (A second cause can be T-892: `horch ask` of a pushed agent was refused with
  'no live attempt of this process' before T-892.)
- Also: the fake idea-lead plays the key `ask` (role Any), and its tell made a new idea I-0002.

Read first: `horch ledger show T-562`, `T-892`, `T-553`, `T-894`; fleetd-p6 ticket 30
(`docs/tickets/fleetd-p6/30-questions.md`) and the tickets of the waits and the answers; in
`docs/specs/fleetd/design.md` RR-12, FM-35, OR-3, the question file, the `questions` table, the
owner of a question, and FM-8 (each tell of the operator is an idea).

## Steps

1. Reproduce through the binary in the new file `crates/horch-e2e/tests/p6_worker_question.rs`:
   a worker of a push attempt of a `--direct` feature runs `horch ask`. Assert: 1 `questions` row
   with the task, the attempt and the owner of the design; the task is `blocked_on_user`; the
   attempt has the outcome `asked`. Check: the test is red, and a progress note names which
   assert fails first, with T-892 on the run branch.
2. Fix it by the design: the ingest of the question file writes the `questions` row in the same
   transaction as the task move, through `transition()`. Treat the file as untrusted input (form,
   size, the attempt belongs to the run). 1 question file gives 1 row, also when the pass runs
   2 times (idempotent). Remove the old caller path (`workers::sync_questions` with a
   `workspace_id`) if it has no caller left after Phase 6; do not keep 2 paths for 1 fact.
3. Add 1 test for each: the answer of the owner reaches the worker and the task leaves
   `blocked_on_user`; a second question of the same attempt gives a second row; a question file
   of a bad form gives no row and 1 needs-you item.
4. The fake idea-lead: a fake plays `ask` only when its scenario asks for it by its role (as
   T-894 did for the judge and the worker). A tell of a sub-orchestrator must not make a new idea;
   if the product makes an idea from a tell of a fleet process, that is a product fault (FM-8 is
   for the operator): send QUESTION with the evidence.
5. Run the Check.

## Constraints

- Simplified Technical English in comments and messages. No em dash.
- Never read, set, pass or depend on `ANTHROPIC_API_KEY`.
- 1 door: each state change goes through `transition()`. Effects run after the commit. No schema
  step; if you need one, send QUESTION and end your turn.
- Do not make an assert weaker and do not add an arch exception.
- An e2e test serves a test fleetd (`h.serve_fleetd()`), makes its workspace with
  `h.fleet_workspace()`, puts the ledger and the fleet root outside the state folder (explicit
  `HORCH_LEDGER_DB` and `FLEET_ROOT`), and sets `HORCH_TEST_DOCS_FIXTURE` to an empty local
  folder. Each test command has a time limit. Run only your own test files. Send
  `horch ledger progress` each 5 min. Merge the run branch each 30 minutes and before finish.

## Done when

- The tests of steps 1 and 3 pass, and the Check passes.
- The done summary names: the owner rule of a question, the path that you removed, the fake
  change, and each file that you changed.

## Report

Use `horch ledger done` with the summary. It is the only DONE path.
