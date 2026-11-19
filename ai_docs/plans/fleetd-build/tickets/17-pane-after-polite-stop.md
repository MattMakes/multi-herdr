# The pane of a reaped worker that ends on the polite stop is closed, or goes to you

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo test -p horch-e2e --test p0_liveness --test p0_supervision --test taskledger --test watchdog_liveness && cargo fmt --all --check`
**Own:** `crates/horch-core/src/fleetd/task_execution/domain/task.rs`, `crates/horch-core/src/fleetd/task_execution/application/commands.rs`, `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/effects.rs`, `crates/horch/src/cmd/fleetd.rs`, `crates/horch-e2e/tests/p0_liveness.rs`

## Goal

When fleetd reaps a worker and the agent ends on the polite stop (SIGTERM), the pane of that
worker is closed if fleetd has the proving fact, and if not, 1 needs-you item `pane_kept:<pane>`
is opened. No pane of a reaped worker stays open with no record.

## Context

The old watchdog closed the pane of each reaped worker. T-88 stopped the old watchdog. T-89 made
FM-22 green for a worker that ignores SIGTERM: the kill pass closes the pane. The worker of T-89
found this gap and did not fix it (outside its Own):

- The pane is looked at only in a pass that has a process stop (`PaneCheck::AfterProcessStop`).
- When the agent ends on the polite stop, the next pass sees a dead process. Then no proof is
  left, the pane is not closed, and no needs-you item is opened.

Rule that holds (decision I5 of the build): fleetd closes a pane only with a fact that proves
that the pane is its own (the process of the pane by PID and start time, read through
`Herdr::pane_shell_pid` from T-89, or the same herdr server start). Without the fact, fleetd keeps
the pane and opens the needs-you item. fleetd never closes a pane that can be foreign.

## Prior work

- Read the done summaries of T-83, T-88 and T-89 (`horch ledger show T-89`), and the test
  `a_sigterm_to_fleetd_during_a_reap_leaves_the_reap_to_the_next_fleetd`.
- T-322 gave the run duties of part 2 a work budget and a cursor. Keep them.

## Steps

1. Write the failing e2e test first: a worker whose agent ends on SIGTERM is reaped; assert that
   its pane is closed at the end. Write a second test: the same, but herdr gives no process for
   the pane; assert that the pane is kept and that 1 `pane_kept` item is open (not 2 after 2
   passes).
2. Find the correct place for the proof. A good option: read and record the proving fact of the
   pane (shell PID and start time) in the same pass that writes the polite stop, as a committed
   row or column of the stop, before the process ends. The later pass closes the pane from that
   committed fact. An effect comes from a committed row, never from memory.
3. If the recorded fact cannot be stored without a schema change, send QUESTION with 2 options,
   then end your turn.
4. Keep the crash rule: a fleetd that dies between the polite stop and the pane close leaves the
   row, and the next fleetd closes the pane. Add this case to the crash test.
5. Run the Check.

## Constraints

- Before the commit, run `git remote -v` and `git config user.email`. The remote must be
  `git@git-alt:MattMakes/multi-herdr.git` and the email `m4ttplu5@gmail.com`. If not, send BLOCKED.
- No co-author line. Never write the em dash character. NEVER use `ANTHROPIC_API_KEY`.
- Do not use a shared `CARGO_TARGET_DIR`. Never install a development build. Never run
  `horch self-update` or `cargo install`. Never register a service. Each test uses temporary
  folders (`HORCH_DATA_DIR`, `HORCH_STATE_DIR`, `HORCH_LEDGER_DB`) and the fake herdr, never the
  live ledger and never the live herdr.
- Each ledger write goes through `transition()`. OS-specific code is only in
  `crates/horch-platform`.
- Run `horch ledger progress` each 5 min during a long run.
- If the GitHub SSH connection times out at `horch ledger finish`, run it again, 3 times maximum.

## Done when

The Check passes, both new tests fail without the change, and the done summary says where the
proving fact is recorded.
