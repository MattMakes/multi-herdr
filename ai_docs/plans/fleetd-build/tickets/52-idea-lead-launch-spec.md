# fleetd starts the idea-lead sub-orchestrator of an idea: the live launch spec for an idea-lead attempt

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets --locked && cargo test -p horch-core --lib fleetd && cargo test -p horch-core --test idea_lead_persona --test nfr --test arch_scan --test architecture_doc && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/adapters/watchdog_duties.rs`

## Goal

The live fleetd gives a launch spec for an idea-lead sub-orchestrator attempt, so that the
dispatcher of Phase 6 starts 1 `idea-lead` session for each idea that needs one, through the same
launch path as each other attempt.

## Context

T-553 (ticket `docs/tickets/fleetd-p6/51-*.md`) wired Phase 6 into the pass with an `IdeaPorts`
trait and registered `handlers_idea` in the live fleetd. Its worker found that
`Live::attempt_spec` in `crates/horch-core/src/fleetd/fleet_runtime/adapters/watchdog_duties.rs`
refuses each idea-lead attempt, and no ticket of Phase 6 owns this. The e2e tests T-556, T-558,
T-559, T-560 and T-562 run the binary with the fake idea-lead of T-555 and need a started
sub-orchestrator.

Read first:

- The done summary of T-553: `horch ledger show T-553`.
- `docs/tickets/fleetd-p6/27-idea-lead.md`, `teammates/idea-lead.md` and
  `teammates/_base/fleet-idea-lead.md` (the persona), and the oracle
  `crates/horch-core/tests/oracles/launch/idea-lead.json`.
- `docs/specs/fleetd/design.md`: the parts on the dispatcher and the sub-orchestrator (FR-2,
  FM-16, FM-29, FM-31) and invariant I7.
- The done summary of T-555 (the fake idea-lead and how a test selects it).

## Steps

1. Read how the dispatcher of Phase 6 records the attempt of a sub-orchestrator and what
   `attempt_spec` gets for it. Write in a progress note which row and which fields identify an
   idea-lead attempt. Check: the note names the file and the line.
2. Write 1 test that fails today: the live `attempt_spec` (or the smallest unit that it calls)
   returns a launch spec for an idea-lead attempt with the persona `idea-lead`, the working folder
   of the idea, and the brief of the idea. Check: the test is red before your change.
3. Build the launch spec. Use the persona resolution and the launch code that the worker attempts
   use; do not write a second launch path. The launch must agree with the oracle
   `oracles/launch/idea-lead.json`. Check: the test of step 2 is green.
4. Check that a test roster can replace the idea-lead with the fake of T-555 in the same way as it
   replaces a worker. If it cannot, make the smallest change that permits it. Check: 1 test starts
   the fake idea-lead through the test fleetd and sees its session start. Put this test in a new
   file `crates/horch-e2e/tests/p6_idea_lead_launch.rs`.
5. Run the Check.

## Constraints

- Simplified Technical English in comments and messages. No em dash.
- Never read, set, pass or depend on `ANTHROPIC_API_KEY`.
- No new schema step. If you think that you need one, send QUESTION and end your turn.
- `watchdog_duties.rs` and `crates/horch/src/cmd/fleetd.rs` are changed by other tasks at the same
  time. Put your code in its own functions, change as few shared lines as you can, merge the run
  branch each 30 minutes and before finish, and resolve a conflict by keeping both parts.
- Each test command has a time limit. Run only your own test files, not the whole workspace.
- Send `horch ledger progress` each 5 min in long test runs.

## Done when

- The tests of steps 2 and 4 pass, and the Check passes.
- The done summary names: the fields that identify an idea-lead attempt, how a test selects the
  fake idea-lead, and each file that you changed.

## Report

Use `horch ledger done` with the summary. It is the only DONE path.
