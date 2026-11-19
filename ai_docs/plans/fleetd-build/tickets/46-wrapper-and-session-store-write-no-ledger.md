# The worker wrapper and the session store write no ledger row: the interim writes W6, W7, W8 are gone and session records go through fleetd

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** refactor
**Review:** none
**Check:** `cargo test -p horch-core --test fleetd_single_writer && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo fmt --all --check`
**Own:** `crates/horch-core/src/execution/lifecycle.rs`, `crates/horch-core/src/execution/store.rs`

## Goal

The worker wrapper (`execution/lifecycle.rs`, the interim writes W6, W7, W8 of T-479) and `ExecutionStore::update` in `execution/store.rs` write no ledger row. No process other than fleetd opens the ledger for a write for these paths, and the arch rule
of T-491 has no exception line for them.

## Context

- Phase 5 rule (design, RR-5 and the single writer): only fleetd writes the ledger. Each other
  process opens it read-only. A CLI command sends a request (the channel or a request file). A
  job process and a worker wrapper write files, and fleetd ingests them.
- T-491 (ticket 42 of `docs/tickets/fleetd-p5/`) made the writer capability, the seam, the arch
  rule and its tests. It listed each writer that it did not move as 1 exception with the name of
  this ticket. Read its done summary first (`horch ledger show T-491`). It names the exception
  lines that are yours.
- Read the done summaries of T-470 to T-477 (routed commands and the shared send function),
  T-479, T-480, T-482 and T-490 (wrapper files, job result files, ingest, pass wiring).
- Other tasks run beside this one. Merge the run branch often.

## Steps

1. Read the done summary of T-881 (the exit ingest ends the session record) and T-472 (the kinds `worker_launched` and `worker_launch_failed` for a role spawn with no attempt).
2. Remove W6 (session record running and exit), W7 (`workers.agent_pid`, `agent_started`) and W8 (`workers.session_id`, `transcript_path`, `last_checkin_at`) for a worker with no attempt. Give each fact a path through fleetd: a wrapper file that fleetd ingests, or a request of class operator for a role spawn that the operator starts. Do not make 2 paths for 1 fact.
3. Move each write of `ExecutionStore::update` behind fleetd in the same way, or show that fleetd is its only caller.
4. Keep these tests green: `arc_26_e2e_lifecycle_matrix`, the stop and stall tests of `crates/horch-e2e/tests/taskledger.rs`, `g52_concurrent_workers_get_their_own_session_and_transcript`. `horch ledger stop` must still stop a role worker by its PID.
5. Remove the exception lines of groups 6 and 7 from the arch rule. Run the Check.

## Constraints

- Simplified Technical English. No em dash.
- 1 door: each state change goes through `transition()`. Effects run after the commit.
- fleetd treats each file and each request payload as untrusted input: check form, size, owner
  and that each id exists and belongs to the run.
- A new request kind needs a payload check and 1 test. Name each new kind in the done summary.
- No schema step. If you think that you need one, send QUESTION and end your turn.
- Do not add an exception to the arch rule. Your task only removes exception lines.
- If a test outside your files goes red only because it needs a test fleetd, name it in the done
  summary (T-880 and T-882 move those tests). Do not make an assert weaker.
- Use a timeout on each test command. Each test uses temporary folders. Never use the live ledger.
- NEVER use `ANTHROPIC_API_KEY`.

## Done when

- The Check passes, and the arch rule has no exception line for the writers of this ticket.
- The done summary names each moved write, each new request kind or file field, and each red
  test that you leave for T-880 or T-882.

## Report

Use `horch ledger done` with the summary. It is the only DONE path.
