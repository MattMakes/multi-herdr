# The job processes write no ledger row: push, fetch and triage apply put their facts in the job result file, and fleetd ingests them

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** refactor
**Review:** none
**Check:** `cargo test -p horch-core --test fleetd_single_writer && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo fmt --all --check`
**Own:** `crates/horch/src/cmd/jobcmd.rs`, `crates/horch-core/src/fleetd/integration/application/exec_push.rs`, `crates/horch-core/src/fleetd/integration/application/exec_fetch.rs`, `crates/horch-core/src/taskledger/triage/apply.rs`

## Goal

The job process (`crates/horch/src/cmd/jobcmd.rs`, `exec_push.rs`, `exec_fetch.rs`, `taskledger/triage/apply.rs`) opens the ledger read-only. No process other than fleetd opens the ledger for a write for these paths, and the arch rule
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

1. List each ledger write that a job process does (group 8 of the T-491 question: `jobcmd.rs` opens `fleetd::ledger_conn`; `exec_push.rs`, `exec_fetch.rs` and `taskledger/triage/apply.rs` write). Find the real paths with `grep` if a path above is not exact.
2. Write 1 test for each job kind: the job process opens the ledger read-only. Check: red first.
3. For each write: the job puts the fact in its result file `run/jobs/<id>-<run>.json` (T-482), and the ingest of fleetd writes the row in 1 transaction with the job end. A second ingest of the same file changes nothing.
4. Remove the exception lines of group 8 from the arch rule. Run the Check.

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
