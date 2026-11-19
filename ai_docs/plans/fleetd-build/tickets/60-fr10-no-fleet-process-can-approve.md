# FR-10: no process of a fleet attempt can record an approval, also after its attempt ended, and no agent outlives its attempt

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** light
**Check:** `cargo check --workspace --all-targets --locked && cargo test -p horch-core --lib fleetd && cargo test -p horch-core --test p6_authorize && cargo test -p horch-e2e --test p6_fr10_approval && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/p6_fr10_approval.rs`

## Goal

FR-10 holds in each case: only a command that the operator runs outside any fleet process can
record an approval. A process that is, or was, part of a fleet attempt (the agent, a child shell
of the agent, a detached child) is refused by `horch idea approve` and by each other command of
class operator, and a file `approval.json` that such a process writes is not accepted. And an
agent does not live on after fleetd ended its attempt.

## Context

CRITICAL safety fault, found by T-558 (opus-156) with T-892 merged. Evidence of its test `fr10`
in `crates/horch-e2e/tests/p6_intake.rs` (its task branch, with branch
`fleet/fleetd-build-T892-a-push-attempt-records-its-wrapper-proce` merged):

- The fake idea-lead ran `horch idea approve I-0001` while I-0001 was in `awaiting_approval`. It
  printed 'I-0001: approved (specifying)' and exit 0.
- The request row: kind `idea_approve`, class operator, state applied, reason empty.
- The attempt row: I-0001/clarifying/1, wrapper pid 19719, state ended, outcome `no_result`,
  started 09:14:36Z, ended 09:14:39Z. The idea moved to `awaiting_approval` in the same second.
  So fleetd ended the attempt while the agent process in the pane still ran.
- How the command decides today: `refuse_inside_fleet_attempt` in
  `crates/horch/src/cmd/idea/mod.rs` reads the open attempts with a recorded wrapper pid and
  checks if the caller descends from a wrapper pid (`peer_in_live_attempt`). With no open attempt
  it returns No. The channel peer check of fleetd (`fleet_runtime/domain/authorize.rs`) uses the
  same live-attempt fact, so it also lets the request through.
- A child shell of the agent has the same gap, because the check is only ancestry under a live
  wrapper.

Read first: in `docs/specs/fleetd/design.md` FR-10, I7, SR-4 and the operator boundary (which
process can read the operator token and reach the command channel), the stop ladder (the process
group of an attempt is stopped before the attempt ends), and the classes of a request;
`crates/horch-core/tests/p6_authorize.rs`; the done summaries of T-892, T-363, T-365, T-486 and
T-893 (if landed).

## Steps

1. Reproduce through the binary in the new file `crates/horch-e2e/tests/p6_fr10_approval.rs`,
   with a served test fleetd and the fake idea-lead. 4 tests, each red today or proved green:
   a. the agent runs `horch idea approve` while its attempt is live and the idea is in
      `awaiting_approval`;
   b. the same after fleetd ended its attempt and the agent process still runs;
   c. a child shell of the agent, and a detached child (new session, new process group), runs it;
   d. the agent writes `approval.json` itself.
   In each case the idea stays in `awaiting_approval`, the request is refused with a reason that
   names the caller as a fleet process, and 1 needs-you item or event records the try (follow the
   design for which). Check: a progress note gives red or green for each of the 4.
2. Find the 2 causes and write them in a progress note:
   - Cause A: why the attempt ended while its agent still ran (the wrapper is gone and the pane
     agent lives). By the stop ladder, fleetd stops the process group of an attempt before the
     attempt ends; an agent must not outlive its attempt.
   - Cause B: the decision 'the caller is a fleet process' reads only live attempts and only
     ancestry.
3. Fix cause B so that it does not depend on a live attempt or on a fact that the agent controls
   (an environment variable is not enough; a process can unset it). Use facts of the operating
   system that fleetd recorded: each wrapper PID and agent PID with its start time, of live and
   of ended attempts, their process groups and sessions, and the pane of a fleet worker. A caller
   that descends from one of them, or is in one of those process groups or sessions, or runs in a
   fleet pane, is a fleet process. When the check cannot decide (a platform that gives no facts),
   it refuses: fail closed. Put the rule in 1 function that the CLI and the channel peer check of
   fleetd both use. The fleetd side is the one that counts; the CLI check is only an early
   message.
4. Fix cause A by the design: the attempt of a push worker does not end while a process of its
   process group lives, or fleetd stops the group first (polite, then hard) and then ends the
   attempt. Coordinate with T-893 (stop and stall of a push attempt); if T-893 owns this path
   and is not landed, do the part that FR-10 needs and name the rest for T-893.
5. Check the second barrier and say what it is in the done summary: can a sandboxed agent of
   each exec adapter read the operator token or connect to the command channel (the conformance
   check 'Operator boundary')? Do not make FR-10 depend on the sandbox alone.
6. Search each other request of class operator (answer, cancel, run close, rescue drop, idea add
   and the rest): the same rule must hold. Send 1 NOTE with the list and what each does for a
   fleet process. FM-8 says that a tell of the operator is an idea; a fleet process must not make
   an idea in the name of the operator.
7. Run the Check.

## Constraints

- Simplified Technical English in comments and messages. No em dash.
- Never read, set, pass or depend on `ANTHROPIC_API_KEY`. Do not print the operator token in a
  log, an error text or a test output.
- 1 door: each state change goes through `transition()`. No schema step; if the facts that you
  need are not in the ledger today, send QUESTION with 2 options and end your turn.
- Do not make an assert weaker, do not add an arch exception, and do not change a security
  setting of the host.
- Windows: when the platform layer cannot give the facts, the rule is fail closed; name each
  Windows risk in the done summary.
- An e2e test serves a test fleetd (`h.serve_fleetd()`), makes its workspace with
  `h.fleet_workspace()`, puts the ledger and the fleet root outside the state folder, and sets
  `HORCH_TEST_DOCS_FIXTURE` to an empty local folder. Each test command has a time limit. Run
  only your own test files. Send `horch ledger progress` each 5 min. Merge the run branch each
  30 minutes and before finish.

## Done when

- The 4 tests of step 1 pass, the test `fr10` of T-558 passes, and the Check passes.
- The done summary names: cause A and cause B, the rule of step 3 and its 1 function, the list of
  step 6, the second barrier of step 5, each Windows risk, and each file that you changed.

## Report

Use `horch ledger done` with the summary. It is the only DONE path.
