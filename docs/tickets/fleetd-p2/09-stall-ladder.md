# 09: The stall ladder: the agent state of each attempt, and rungs 1 to 5

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd::task_execution && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/task_execution/domain/stall.rs`, `crates/horch-core/src/fleetd/task_execution/application/stall.rs`

Spec: `docs/specs/fleetd/design.md`, "Supervision, liveness and reconciliation" ("Liveness comes from outside the model": the 4 signals and the 7 agent states; "Stall ladder", rungs 1 to 5; "Finishing without reporting is closed at both ends"), "Migration plan" item 4 ("Replace '3 checks of 600 s' with the stall ladder"; "Rung 4 ... Before Phase 7"), FM-3, FM-5; `ddd/08-code/task-execution/aggregate-canvas-task.md` (`apply-stall-ladder`).

## Goal

fleetd derives 1 agent state for each live attempt from outside signals, and acts on it with fixed timers on the fleet clock: a doorbell, a second doorbell, the end of the attempt. A usage limit is never a stall. Each rung is code; none depends on a model reading anything. Ticket 10 does rung 6; ticket 11 acts on a usage limit; ticket 14 calls this in the pass.

## Requirements

- **RR-1**: liveness is judged from outside the model: the process state, the wrapper heartbeat, hook events and output activity. Fault injection: a worker stopped with `kill -STOP` is caught within 1 lease (`hung` after `stall_after_s` with no activity, then rung 3).
- **FM-3** (the ladder part): a worker that holds a task and does nothing is caught in about 7 min (`stall_after_s` 300 + `lease_s` 120).
- **FM-5** (the ladder part): an agent stuck at a login, trust or error prompt is caught (herdr `blocked` -> `waiting_approval`, rung 4). A usage limit is `usage_limited`, never `hung`.

## Context

- Read `docs/tickets/fleetd-p2/MAP.md`, section "Open questions", first (Q7, Q9, Q10, Q18 and Q19 apply here).
- Today's rule is "3 checks of 600 s" (`check_every_s` 600 x `stall_rounds` 3, `crates/horch-core/src/taskledger/watchdog.rs`: `check_run` at `:199`, `judge` at `:392` and `stall_round` at `:481`; Phase 1 ticket 22 moved the call into the pass, `crates/horch-core/src/fleetd/fleet_runtime/adapters/watchdog_duties.rs`). This ticket writes the ladder that replaces it. Ticket 14 removes the old rule from the pass.
- The facts of 1 live attempt, as values (part 1 of the pass reads them; ticket 14 builds them):
  - the process (`Seen`, Phase 1); its exit, when it is gone;
  - the heartbeat age (ticket 07) and `active_at` on the fleet clock (ticket 08);
  - the herdr status of its pane (`working`, `blocked`, `idle`, from the snapshot of part 1) and the screen excerpt (`Observation::screen`, `watchdog.rs:44`);
  - `pool_limited`: the pool of the attempt's persona is limited (ticket 11 gives the rule; here it is a bool input);
  - `open_question`: the worker has an open `questions` row (ticket 12);
  - `waiting_until`: the fleet clock at which a declared wait of the worker ends (`worker_waits.due_at`, ticket 12), or none;
  - `mode` and `stall_after_s` of the profile (ticket 06);
  - the doorbell events of the attempt (see rungs 1 and 2).
- The agent states, first match wins: `exited` (the process is gone); `usage_limited` (`pool_limited`); `waiting_approval` (herdr `blocked`); `working` (activity within `stall_after_s`, or a declared wait that has not reached `waiting_until`); `idle_no_result` (an interactive pane that herdr shows `idle`, and the task is still `running`); `hung` (the process is alive, no activity for `stall_after_s`); `starting` (no heartbeat and no activity yet, within `start_timeout_s` of the claim). Write each state as an enum variant with the design word.
- The rungs (all timers on the fleet clock):
  1. `idle_no_result` in an interactive pane: send the doorbell 1 time. Record an event of kind `doorbell` with the fleet clock in `data_json`.
  2. Still `idle_no_result` 120 s later: the doorbell again, and record it.
  3. Still `idle_no_result` 120 s after rung 2, or `hung` at any point: end the attempt with the outcome `stalled`. The effect is a `Stop` of the process group (Phase 1, `application/commands.rs`; `stop_grace_s` of `fleet.toml`). The task moves to `stalled`. Ticket 10 does rung 6 after the stop.
  4. `waiting_approval`: end the attempt with the outcome `stalled`, stop its process group, and open 1 needs-you item with the key `dialog:<attempt id>` and the screen excerpt (at most 2 KiB). Before Phase 7 there is no profile health (MAP.md, Q18).
  5. `exited`: an open question of the worker gives the outcome `asked`, and the task moves to `blocked_on_user` (no attempt burned; its worktree stays). `pool_limited` gives the outcome `quota`, and ticket 11 decides `ready` on the fallback or `waiting_quota` (no attempt burned). Else the outcome `no_result`, and rung 6 (ticket 10).
  - `usage_limited` while the process is alive: end the attempt with the outcome `quota` and stop its process group (MAP.md, Q15). It is never `hung` and never reaped as stuck.
- The doorbell: 1 constant line in `domain/stall.rs` (`DOORBELL`), with no task data in it (FR-7 makes it the only bytes that fleetd types, in Phase 5). The send is an effect: this ticket returns `Doorbell { pane }`, and ticket 14 sends it after the commit through today's pane send of herdr.
- The decision is pure in `domain/stall.rs`: facts in, 1 decision out (`None`, `Doorbell`, `End { outcome, stop }`, `NeedsYou`). `application/stall.rs` writes the rows through `transition()` and the needs-you list, and returns the effects. The transitions it uses are in the table of ticket 04.

## Steps

1. Write the tests first. Each fails before the code exists:
   - In `domain/stall.rs`: `each_agent_state_from_its_facts` (1 case for each state), `usage_limited_is_never_hung`, `declared_wait_counts_as_working_until_its_due_at`, `rung_1_rings_once_and_rung_2_rings_120_s_later`, `rung_3_ends_the_attempt_120_s_after_rung_2`, `hung_ends_the_attempt_at_once`, `rr1_stopped_worker_is_hung_after_stall_after_s_with_no_activity`, `fm3_idle_claim_is_caught_within_stall_after_plus_lease`, `sleep_past_a_rung_fires_it_only_after_the_rest_of_its_fleet_time`, `exit_with_an_open_question_is_asked_and_burns_no_attempt`, `exit_with_a_limited_pool_is_quota`, `exit_with_neither_is_no_result`.
   - In `application/stall.rs`: `waiting_approval_ends_the_attempt_and_opens_1_dialog_item_with_the_excerpt`, `asked_moves_the_task_to_blocked_on_user_and_keeps_the_attempt_count`, `stalled_attempt_returns_a_stop_of_its_process_group`.
2. Write the code. Run the Check and the guard tests. Commit.

## Constraints

@@RULES@@

## Done when

The Check passes. Each agent state follows from its facts, each rung fires on its fleet-clock timer, and a usage limit is never treated as a stall.
