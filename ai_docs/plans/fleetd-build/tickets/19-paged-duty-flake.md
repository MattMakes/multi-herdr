# Find why a paged duty read each row 3 times in 1 lap, and make the test stable

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo test -p horch-core --test fleetd_pass && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/adapters/watchdog_duties.rs`, `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`, `crates/horch-core/src/taskledger/watchdog.rs`

## Goal

The test `each_paged_duty_reads_at_most_the_pass_budget_and_a_lap_reaches_each_row_once`
(`crates/horch-core/src/fleetd/fleet_runtime/adapters/watchdog_duties.rs`, about line 2074) passes
each time at each machine load, and the cause of its failure is known. If the product reads a row
more than 1 time in a lap, the product is corrected.

## Context

Task T-322 gave the run duties of part 2 of the fleetd pass a work budget and a cursor in
`pass_cursors` (`horch ledger show T-322`). The worker of T-258 saw this test fail 1 time in a
full `--lib` run under machine load: each row was read 3 times in 1 lap. Alone it passed 3 of 3.
Its guess: the time budget of a pass ends early under load. This is a guess.

2 cases are possible:

- Test fault: the test uses the real clock, so a slow host ends a pass early and the test counts
  wrong. Then the test must use the fleet clock or a fixed time budget.
- Product fault: when the time budget ends a pass in the middle of a page, the cursor is not saved
  through the last row that was done, so the next pass reads the same rows again. Then a slow
  machine does the same work again and again, and a duty can starve. This is a fault of PL-5.

## Steps

1. Reproduce first. Run the test in a loop under load (for example 14 parallel copies of the lib
   test binary) until it fails. Write down the failure rate. If it does not fail in 300 runs,
   make the time budget very small through the test seam to force the early end of a pass.
2. Find the cause in the code. Read where the pass stops at `time_up` and where it saves the
   cursor.
3. Product fault: write a test that fails for the exact case (the budget ends after N rows of a
   page; the next pass must start at row N+1, and 1 lap reaches each row 1 time). Then correct
   the product. Keep each "1 time only" wake exactly 1 time.
4. Test fault: make the test independent of the real clock and of the host speed. Do not make an
   assert weaker.
5. Run the loop of step 1 again: 0 failures in at least 300 runs under load.
6. Run the Check.

## Constraints

- Before the commit, run `git remote -v` and `git config user.email`. The remote must be
  `git@git-alt:MattMakes/multi-herdr.git` and the email `m4ttplu5@gmail.com`. If not, send BLOCKED.
- No co-author line. Never write the em dash character. NEVER use `ANTHROPIC_API_KEY`.
- Do not use a shared `CARGO_TARGET_DIR`. Never install a development build. Never run
  `horch self-update` or `cargo install`. Never register a service. Each test uses temporary
  folders, never the live ledger.
- Each ledger write goes through `transition()`. The cursor write stays in the transaction of
  the pass.
- No schema change. If you need one, send QUESTION, then end your turn.
- Read VERSION in `crates/horch-core/src/taskledger/schema.rs`; do not write a schema number.
- Run `horch ledger progress` each 5 min during a long run.
- If the GitHub SSH connection times out at `horch ledger finish`, run it again, 3 times maximum.

## Done when

The Check passes, the loop of step 5 has 0 failures, and the done summary gives the cause, says
"test fault" or "product fault", and gives the failure rate before and after.
