# The run duties of part 2 of the pass have a work budget and a cursor

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd && cargo test -p horch-core --lib taskledger && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo test -p horch-e2e --test taskledger --test watchdog_liveness && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/adapters/watchdog_duties.rs`, `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/sqlite.rs`, the functions of `crates/horch-core/src/taskledger` that the run duties call (`workers.rs`, `tasks.rs`, `wakes.rs`, `watchdog.rs`)

## Goal

Each run duty of part 2 of the fleetd pass handles at most `pass_budget` rows in 1 pass and
continues at its cursor in `pass_cursors` on the next pass, as the design says (PL-5, A32, "A pass
has a work budget").

## Context

The worker of T-91 measured: with 10,000 live attempts, 1 pass of a debug build takes about 10 s.
The fact scans of part 1 stop at their budget. The run duties of part 2 do not: `workers::of_run`,
`tasks::startable` (through `startable_rewake`), `wakes::stuck_wake` and `watchdog::due_workers`
read each worker and task of the run. The heartbeat gap was 11 s, below the limit of 30 s, so the
gate test passes today. A larger fleet or a slower disk can pass the limit. The design rule is:
"The fact scans of part 1 and the rows of part 2 have a budget, and the whole pass has a time
budget below the hang limit" (`docs/specs/fleetd/design.md`, PL-5, and "Supervision, liveness and
reconciliation").

## Prior work

- T-85 (ticket 22 of Phase 1) moved the duties into the pass. Read the head comment of
  `watchdog_duties.rs` and the done summary of T-85 (`horch ledger show T-85`).
- T-91 added the e2e test with 10,000 folders and attempts. Read its done summary for the test
  name and the numbers.
- The fleetd schema is version 8 on this branch. Add no schema version if `pass_cursors` can hold
  the new cursors. If it cannot, send QUESTION, then end your turn.

## Steps

1. Write the failing test first: 10,000 workers and tasks in 1 run, 1 pass, and an assert that
   each duty reads at most `pass_budget` rows (count the rows, do not measure time alone). Add an
   assert that N passes reach each row 1 time (fair order, no row starved).
2. Give each duty that reads all rows a bounded query with a cursor (a kind name for each duty in
   `pass_cursors`). Keep the cursor write in the transaction of the pass.
3. Keep each "1 time only" wake exactly 1 time: a wake key must not depend on the page.
4. A duty that must see the whole run for a correct answer (for example "the run is settled")
   must use 1 indexed count query, not a row loop. Name each such duty in the done summary.
5. Check the whole-pass time budget: the pass stops new part 2 work when the budget is used, and
   the heartbeat is written.
6. Run the e2e test of T-91 and give the pass time before and after in the done summary.
7. Run the Check.

## Constraints

- Before the commit, run `git remote -v` and `git config user.email`. The remote must be
  `git@git-alt:MattMakes/multi-herdr.git` and the email `m4ttplu5@gmail.com`. If not, send BLOCKED.
- No co-author line. Never write the em dash character. NEVER use `ANTHROPIC_API_KEY`.
- Do not use a shared `CARGO_TARGET_DIR`. Never install a development build. Never run
  `horch self-update` or `cargo install`. Never register a service. Each test uses temporary
  folders (`HORCH_DATA_DIR`, `HORCH_STATE_DIR`, `HORCH_LEDGER_DB`), never the live ledger.
- The design is the source of truth. Change no wake text and no wake key.
- Each ledger write goes through `transition()`.
- Run `horch ledger progress` each 5 min during a long run.
- If the GitHub SSH connection times out at `horch ledger finish`, run it again, 3 times maximum.

## Done when

The Check passes, the new test fails without the change, and the done summary gives the pass time
with 10,000 attempts before and after.
