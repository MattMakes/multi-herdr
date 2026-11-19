# A push attempt records its wrapper process, so its agent can send a result, and its exit is seen at once

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo check --workspace --all-targets --locked && cargo test -p horch-core --lib fleetd && cargo test -p horch-core --test fleetd_pass_p5 && cargo test -p horch-e2e --test p7_exec --test p0_dispatch && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/p5_push_attempt_process.rs`

## Goal

When fleetd launches an attempt by push, the attempt row gets the PID and the start time of its
wrapper process at the move `launching -> live`. Then `horch result`, `horch ask` and a QUESTION
tell from that agent are accepted, and fleetd sees the exit of that process in the next pass.

## Context

3 workers found the same fault from 3 sides:

- T-603 (opus-148), e2e `p7_exec`: the exec fake commits, then runs `horch result`. It exits 1
  with 'no live attempt of this process'. Cause that the worker found: `live_attempts` in
  `outbox_writer.rs` needs `attempts.pid` and `attempts.pid_start` of the wrapper. Only
  `commands::attempt_live` writes them, and only from `launched`. A push launch record has process
  `None` (`launch.rs`), and `start_running` in `pass.rs` calls `attempt_live` with `None`. Earlier
  e2e tests set the PID by hand (`testing::bind_live`, stand-in wrappers), so no test saw it.
- T-392 (opus-147): a fake that exits at once makes its worker `launch_failed`. Its attempt stays
  live until the lease expires and then ends as stalled.
- T-881 (opus-hardening-155): a push worker that is killed while it works ends as
  `launch_failed`, and its task is held with 'retry after 30 s'.

Read first: `horch ledger show T-603`, `T-392`, `T-881`, `T-490` (pass wiring of Phase 5),
`T-480` and `T-482` (wrapper files and ingest), and in `docs/specs/fleetd/design.md` the launch
states of an attempt (`launching`, `launched`, `live`), the wrapper files and FM-2.

## Steps

1. Reproduce through the binary. Write 1 e2e test in the new file
   `crates/horch-e2e/tests/p5_push_attempt_process.rs`: fleetd launches an exec fake by push with
   no stand-in and no `bind_live`; the fake runs `horch result`; the task reaches `verifying` or
   later. Check: the test is red, with the text 'no live attempt of this process'.
2. Find the 1 source of the wrapper identity that the design names (the wrapper file that fleetd
   ingests, or the launch record). Write it in a progress note with the place in the design. Do
   not make a second path for the same fact.
3. Record the PID and the start time on the attempt row at `launching -> live`, through the door
   (`transition()` or the existing command). Treat the file as untrusted input: check form, size
   and that the attempt belongs to the run. Check: the test of step 1 is green.
4. Add 1 test for each of these cases in the same file:
   - the agent sends `horch ask`; it is accepted.
   - the agent exits with no result before the lease expires; the next pass ends the attempt by
     the exit path of the design (not by the lease), and the outcome is not `launch_failed` for an
     agent that was live.
   - the agent is killed while it works; the outcome is the crash or stall path of the design,
     not `launch_failed`.
   If the design gives another outcome for a case, follow the design and say so in the summary.
5. Remove the lines with owner T-892 from `crates/horch-e2e/expected-failures/` (the file of
   `p7_exec` from T-603) and make those tests green. Remove each `testing::bind_live` or stand-in
   wrapper that a test of `p0_dispatch.rs` or `p7_exec.rs` no longer needs.
6. Run the Check.

## Constraints

- Simplified Technical English in comments and messages. No em dash.
- Never read, set, pass or depend on `ANTHROPIC_API_KEY`.
- 1 door: each state change goes through `transition()`. Effects run after the commit.
- No schema step. If you think that you need one, send QUESTION and end your turn.
- Do not add an arch exception and do not make an assert weaker.
- `pass.rs`, `launch.rs` and `cmd/fleetd.rs` are changed by other tasks at the same time. Put your
  code in small functions, change as few shared lines as you can, merge the run branch each 30
  minutes and before finish, and resolve a conflict by keeping both parts.
- Each test command has a time limit. Run only your own test files. Send
  `horch ledger progress` each 5 min in long test runs.
- Windows code stays small. Name each Windows risk in the done summary.

## Done when

- The tests of steps 1 and 4 pass, the listed tests of step 5 are green and on no list, and the
  Check passes.
- The done summary names: the source of the wrapper identity, the outcome of each case of step 4,
  and each file that you changed.

## Report

Use `horch ledger done` with the summary. It is the only DONE path.
