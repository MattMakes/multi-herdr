# The ingest of exit.json ends the session record of the attempt, and g52 is green

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** none
**Check:** `cargo test -p horch-core --test wrapper_ingest && cargo test -p horch-e2e --test fleet18_observability && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo fmt --all --check`
**Own:** `crates/horch-core/src/taskledger/supervisor.rs`, `crates/horch-core/src/execution/`, `crates/horch-core/tests/wrapper_ingest.rs`, `crates/horch-e2e/tests/fleet18_observability.rs`, `crates/horch-e2e/expected-failures/fleet18_observability.txt`

## Goal

When the agent of an attempt exits, fleetd ends its session record: the record leaves `working`
and holds the exit state. The test `g52_concurrent_workers_get_their_own_session_and_transcript`
is green and has no expected-failures line.

## Context

- Found by T-490 (opus-hardening-142). For a worker with an attempt,
  `execution/lifecycle.rs` `agent_exited` writes only `exit.json`. The ingest of `exit.json`
  (`supervisor::settle`) does not end the session record. So `horch sessions` shows `working`
  for ever, and g52 fails at the step "old launches exited".
- Read the done summaries of T-479, T-480 and T-490 first (`horch ledger show T-<n>`). They name
  the wrapper files, the ingest functions and the pass wiring.
- The rule of Phase 5: only fleetd writes the ledger. The wrapper writes files; fleetd ingests.
- If the name of the test file of g52 is not `fleet18_observability.rs`, find it with `grep` and
  correct the Check in a QUESTION to the orchestrator before `horch ledger finish`.

## Steps

1. Write 1 test in `crates/horch-core/tests/wrapper_ingest.rs`: an attempt has a session record
   in `working`; `exit.json` is ingested; the record has the exit state. Check: red first.
2. Make the ingest of `exit.json` end the session record, in the same transaction as the other
   exit facts. A second ingest of the same file changes nothing. Check: the test is green.
3. Check the case of an exit with no session record, and the case of a crash with no `exit.json`
   (the pass sees that the process is gone). Name the rule for each and add 1 test each if no
   test proves it.
4. Remove the g52 line from the expected-failures list and its `expected::check` wrapper. Check:
   g52 is green.

## Constraints

- Simplified Technical English. No em dash.
- No schema step. If you think that you need one, send QUESTION and end your turn.
- No test depends on the speed of the host. Each test uses temporary folders.
- Use a timeout on each test command. A test run that does not end in 15 minutes hangs: stop it
  and find the cause.
- NEVER use `ANTHROPIC_API_KEY`.

## Done when

- The Check passes, and g52 has no expected-failures line.
- The done summary names the rule for each case of step 3 and each file that you changed.

## Report

Use `horch ledger done` with the summary. It is the only DONE path.
