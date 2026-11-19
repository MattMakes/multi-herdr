# The 4 old pull-flow tests of taskledger.rs pass with the push flow, and the expected-failures list of Phase 4 is empty

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo test -p horch-e2e --test taskledger && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo fmt --all --check`
**Own:** `crates/horch-e2e/tests/taskledger.rs`, `crates/horch-e2e/expected-failures/taskledger.txt`

## Goal

The file `crates/horch-e2e/expected-failures/taskledger.txt` has no line, and each of its 4 tests
is green with the push flow of Phase 4.

## Context

T-352 made `horch spawn` bind its task as a push attempt before the pane starts. 4 tests of
`crates/horch-e2e/tests/taskledger.rs` tested the old pull flow (spawn, then `horch ledger claim`).
T-352 put them in the expected-failures list (design rule A22) with phase 4:

- `the_whole_flow_on_the_fake_herdr`
- `a_spawned_worker_records_its_pids_session_and_checkins`
- `stopping_one_of_three_workers_leaves_the_others_running`
- `a_worker_with_no_progress_is_stalled_by_the_ladder_and_stopped_by_its_pid`

Read the done summaries of T-352, T-353, T-354, T-355, T-356, T-389 and T-390 first
(`horch ledger show T-<n>`). They name the push flow: the bind, the launch, the move of an attempt
to live, `horch result`, the ingest, the verifier and the landing.

## Steps

1. Run the 4 tests with their list lines removed. Record which ones are red and why.
2. Rewrite each red test for the push flow. Keep what the test proves: the whole flow to a landed
   task, the recorded PIDs and session facts, a stop of 1 worker that leaves the others, and a
   stall that the ladder stops by PID. Do not make an assert weaker. Check: each test is green.
3. Remove each line from `expected-failures/taskledger.txt` and remove the
   `horch_e2e::expected::check` wrapper of each test. If the file is then empty, remove the file
   if the README of that folder says so.
4. If a test cannot be green because product code has a fault, do not hide it. Fix the fault if
   the fix is small (name the file in the done summary), or send QUESTION with the cause.

## Constraints

- Simplified Technical English. No em dash.
- No test depends on the speed of the host. Wait for an event, not for a time.
- Each test uses temporary folders. Never use the live ledger.
- Do not remove the task search of `horch ledger claim` here. Another ticket does that.

## Done when

- The Check passes with no Phase 4 line in the expected-failures list.
- The done summary names each test, what changed in it, and each product file that you changed.

## Report

Use `horch ledger done` with the summary. It is the only DONE path.
