# The last CLI direct writers send requests: run close, run gate, gc, rescue drop, the answer record, triage apply and the worker progress

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** refactor
**Review:** none
**Check:** `cargo test -p horch-core --test fleetd_single_writer && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo fmt --all --check`
**Own:** `crates/horch/src/cmd/rescuecmd.rs`, `crates/horch/src/cmd/messaging.rs`, `crates/horch/src/cmd/worktree.rs`

## Goal

The CLI commands of groups 1, 2, 3, 4 and 9 of the T-491 question write no ledger row. No process other than fleetd opens the ledger for a write for these paths, and the arch rule
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

1. List the writers: `crates/horch/src/cmd/taskledger.rs` (the writable open for run close, run gate, gc, watch; `is_ledger_worker`; `progress_from_worker`), `rescuecmd.rs` (`drop_ref`), `messaging.rs` (`record_answer` writes the questions row), `worktree.rs` (`triage_rows` bytes cache and `triage_apply_direct`), and `taskledger_watch.rs` (`LiveEffects` of the fleetd pass).
2. For each writer choose 1 path and name it: an existing request kind, a new request kind with a handler, an outbox file that fleetd ingests (worker commands), or no write (a cache that can be a read). T-549 retires the run commands in Phase 6: if a command is retired there, do not route it; make it read-only or refused and name it.
3. `taskledger_watch.rs`: if `LiveEffects` runs inside the fleetd process, it is fleetd and keeps the writer capability; say so in the arch rule as the fleetd entry, not as an exception. If another process runs it, move it.
4. Write 1 test for each moved writer: the command with a test fleetd gives the same rows as before, and the command opens the ledger read-only.
5. Remove the exception lines of groups 1, 2, 3, 4 and 9 from the arch rule. Run the Check.

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
