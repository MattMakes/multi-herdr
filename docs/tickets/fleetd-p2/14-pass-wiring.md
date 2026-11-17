# 14: The pass of Phase 2: leases, the stall ladder, rung 6, quota, cancels and promotion in their order; the end of "3 checks of 600 s"

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** no
**Kind:** feature
**Review:** full
**Check:** `cargo test --workspace --locked`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/effects.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/scans.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/watchdog_duties.rs`, `crates/horch-core/src/taskledger/watchdog.rs`, `crates/horch/src/cmd/taskledger_watch.rs`, `crates/horch/src/cmd/fleetd.rs`, `crates/horch-core/tests/fleetd_pass_p2.rs`

Spec: `docs/specs/fleetd/design.md`, "Supervision, liveness and reconciliation" ("The reconcile pass": the pseudo-code; "A pass has 3 parts"; "Order inside 1 pass"; "Stall ladder"), "Migration plan" item 4 ("Replace '3 checks of 600 s' with the stall ladder"), RR-1, RR-2, RR-6, RR-11, FM-3, FM-5, FM-14.

## Goal

Each pass of fleetd runs the rules of Phase 2 in the order of the design. Part 1 reads the new facts. Part 2 calls the functions of tickets 04, 08 to 13 in 1 transaction. Part 3 does their effects: stops first, then doorbells and spawns. The stall rule of today ("3 checks of 600 s") no longer judges a worker.

## Requirements

- **RR-1**, **RR-2**, **RR-6**, **RR-11**, **FM-3**, **FM-5**, **FM-14**: this ticket connects their mechanisms to the pass. Tickets 15 to 17 prove each end to end.
- "Order inside 1 pass": results are ingested and acted on before a lease is expired; a result present in a pass wins over a lease expiry in the same pass.
- "Stops come before launches": a new attempt never starts while a process of an older attempt of the task is alive.

## Context

- Read `docs/tickets/fleetd-p2/MAP.md`, section "Open questions", first (Q6, Q7, Q9, Q14, Q19 and Q23 apply here).
- The pass of Phase 1: `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs` (ticket 21 of `docs/tickets/fleetd-p1/`), the effects in `adapters/effects.rs`, the fact scans in `adapters/scans.rs` (ticket 18), the duties of today's watchdog in `adapters/watchdog_duties.rs` (ticket 22), the composition root in `crates/horch/src/cmd/fleetd.rs` (ticket 23) and `horch ledger watch --once` (`crates/horch/src/cmd/taskledger_watch.rs`, ticket 25). Read those files first. The names below follow the design; use the names of the code where they differ.
- Part 1, new facts for each live attempt, each within the scan budget: the heartbeat age (`workers.last_checkin_at`, ticket 07); activity (today's `Observation`, `crates/horch-core/src/taskledger/watchdog.rs:44`, and its `Observer` in `taskledger_watch.rs`); the herdr status and the screen excerpt of its pane; whether a member of the agent's process group other than the agent is alive (`group_members`, `crates/horch-platform/src/process.rs:57`, for the background wait of ticket 12); the pool reading of its persona (ticket 11).
- Part 2, in this order, after the steps of Phase 1 that come first (`check_generation`, `tick_fleet_clock`, `observe_intents`, `mark_orphans`):
  1. `finish_exited`: Phase 1 plus rung 5 (ticket 09: `asked`, `quota`, `no_result`), then ticket 11 for `quota`, then rung 6 (ticket 10) for each burned attempt.
  2. `complete_cancels` (ticket 13).
  3. `promote_pending` (ticket 04, I2).
  4. For each live attempt, within `pass_budget`: `renew_or_expire_lease` (ticket 08), then the ladder (ticket 09: doorbells, rung 3, rung 4, `usage_limited`), then rung 6 for each attempt that the ladder ended.
  5. `release_reset_pools` (ticket 11) and `escalate_overdue` (ticket 10).
  6. The I1 check of leases (ticket 08), with the other invariants of Phase 1.
- Part 3: each `Stop` first, then each `Doorbell` (today's pane send of herdr, the constant `DOORBELL` of ticket 09), then each restart on a fallback (ticket 11, the spawn of `crates/horch/src/cmd/recipes.rs`). Each effect is keyed by its row and is safe to repeat (RR-8 form of Phase 1).
- The end of "3 checks of 600 s" (MAP.md, Q14): the pass no longer calls the stall rounds of `watchdog.rs` (`check_run` at `:199`, `judge` at `:392`, `stall_round` at `:481`) to reap a worker. Keep the parts of `check_run` that other duties of Phase 1 still need (the `checks` rows that `horch ledger show` prints, the unclaimed flag, the rewake). Remove the code that only the stall rounds used, and its tests. The run flags `--check-every` and `--stall-rounds` of `horch ledger run new` stay accepted and stored, and no longer decide a reap.
- The tests use the fleet harness of Phase 1 (a temporary ledger and fleet root, `horch ledger watch --once` or the pass function, `HORCH_NOW` for the wall clock). A large `pass_period_s` in the test's `fleet.toml` lets few passes move the fleet clock far (a pass adds at most 2 pass periods).
- The run check is the Check of this ticket. Run `horch ledger progress` each 5 min while it runs.

## Steps

1. Write the tests first, in the new file `crates/horch-core/tests/fleetd_pass_p2.rs`. Each fails before the code exists:
   - `result_in_the_pass_wins_over_the_lease_expiry`.
   - `idle_worker_is_stalled_and_queued_after_its_backoff` (FM-3 at pass level).
   - `usage_limited_worker_restarts_on_the_fallback_and_burns_no_attempt` (FM-5 and RR-11 at pass level).
   - `stop_of_the_old_attempt_comes_before_the_spawn_on_the_fallback` (FM-14).
   - `stall_rounds_of_today_no_longer_reap_a_worker`.
   - `background_wait_holds_while_the_agent_child_runs_and_ends_when_it_exits`.
   - `cancel_requested_task_is_cancelled_by_the_pass_after_its_process_is_gone`.
2. Write the code. Run the Check and the guard tests. Commit.

## Constraints

@@RULES@@

## Done when

The Check passes. A pass runs each rule of Phase 2 in the order of the design, and the stall rounds of today no longer reap a worker.
