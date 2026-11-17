# 11: A usage limit: restart at once on the fallback of another provider, else waiting_quota until the pool resets

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd::task_execution && cargo test -p horch-core --lib taskledger && cargo test -p horch --lib cmd::recipes && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/task_execution/domain/quota.rs`, `crates/horch-core/src/fleetd/task_execution/application/quota.rs`, `crates/horch-core/src/fleetd/task_execution/application/commands.rs`, `crates/horch/src/cmd/recipes.rs`

Spec: `docs/specs/fleetd/design.md`, RR-11, FM-5, "Supervision, liveness and reconciliation" ("Stall ladder", the paragraph before rung 1; "The task state machine": the rows `running` and `waiting_quota`), "Worker profiles and adapters" ("Every worker persona names a fallback persona"; the row "Usage-limit signal"), D2 of "Amendments"; `docs/specs/fleetd/preservation-04-behavior.md` rows S6 and L10; `ddd/08-code/task-execution/aggregate-canvas-task.md` (`move-to-fallback`).

## Goal

A worker that shows a usage limit is not stuck and burns no attempt. When the fallback of its persona is on another provider, fleetd restarts the task at once on a new worker of the fallback persona, on the same worktree. Else the task waits in `waiting_quota`, with its lease paused, until the pool resets: the calendar time passes `reset_at`, or, with no `reset_at`, the pool probe reports ok. When both pools are limited, the task waits.

## Requirements

- **RR-11**: a usage-limited worker is not treated as stuck and burns no attempt; the restart on a fallback of another provider; else `waiting_quota` with the lease paused until the pool resets; `reset_at` is optional; when both pools are limited, the task waits.
- **FM-5** (the quota part): no attempt is burned; the task restarts at once on the fallback persona of another provider, else waits in `waiting_quota`.
- Preservation 04 S6: today only the orchestrator briefing says "never reap a usage-limited worker". fleetd enforces it in code.

## Context

- Read `docs/tickets/fleetd-p2/MAP.md`, section "Open questions", first (Q1, Q8, Q9 and Q15 apply here).
- The usage-limit signal of Phase 2 (MAP.md, Q9) is the pool reading of the attempt's persona: `QuotaView` and `pool_for(agent, model)` in `crates/horch-core/src/routing/quota.rs` (`:133`, `:404`), the same source as the pacing hold of Phase 1 (`crates/horch-core/src/fleetd/fleet_runtime/application/holds.rs`). The pool is limited when the used share of a window is at or above `pool_full_use` (ticket 05), or when its state blocks (`State::blocks`, `quota.rs`). `reset_at` is the `resets_at` of that window, when it has one. The exact signal of each CLI is confirmed by the conformance suite in Phase 7.
- "Another provider" means another pool: `pool_for` of the fallback differs from `pool_for` of the persona.
- The fallback (MAP.md, Q8): the task's `fallback_persona` when it is set (today's `--fallback` of `horch ledger add`), else the first entry of `fallbacks` of the persona (ticket 06, `worker_profiles/domain/profile.rs`).
- No attempt burned (MAP.md, Q15): ticket 09 ends the attempt with the outcome `quota` and stops its process group. This ticket then adds 1 to `attempt_floor` of the task, so the limit of rung 6 counts only burned attempts, and `tasks.attempts` does not change.
- The 2 exits, pure in `domain/quota.rs`, written through `transition()` in `application/quota.rs`:
  - Fallback of another provider and its pool is not limited: `running -> ready`, `claimable_by` = the fallback (today's `tasks::fallback`, row L10, extended to the task that this rule moves), cause `usage_limit_fallback`. Then the restart: the row first, the spawn after the commit. Follow the respawn of the orchestrator of Phase 1 (ticket 22 of `docs/tickets/fleetd-p1/`: an attempt in `launching`, then the respawn function of `crates/horch/src/cmd/recipes.rs` in part 3). The spawn is today's `horch spawn <fallback>` with the claim target of the task (`CLAIM_TARGET_ENV`, `crates/horch/src/cmd/taskledger_spawn.rs:26`), through the launcher. The worktree of the task stays and moves to the new worker at its claim (preservation 04 W3). Before the spawn, the effect checks that no live process of an older attempt of the task remains (stops come before launches).
  - Else: `running -> waiting_quota`, cause `usage_limit`. The lease stays and is paused (ticket 08). On each pass, a task in `waiting_quota` whose pool is reset (`reset_at` passed in calendar time, or with no `reset_at` the pool is not limited) moves to `ready`, cause `pool_reset`, and its lease row goes.
- The pass (ticket 14) calls the function for each attempt with the outcome `quota` in the pass, and the function for each task in `waiting_quota`, for example `on_quota(tx, ...)` and `release_reset_pools(tx, ...)`.
- `application/commands.rs` changes only where the claim must take an attempt in `launching` for its task to `live` instead of a new attempt.

## Steps

1. Write the tests first. Each fails before the code exists:
   - In `domain/quota.rs`: `pool_at_full_use_is_limited`, `pool_state_that_blocks_is_limited`, `fallback_on_another_pool_restarts`, `fallback_on_the_same_pool_waits`, `both_pools_limited_waits` (no third persona), `usage_limit_without_reset_at_waits_for_pool_probe_ok` (`ddd/08-code/implementation-plan.md`, Slice 2), `reset_at_in_the_past_releases_the_task`.
   - In `application/quota.rs`: `fm5_usage_limit_burns_no_attempt_and_moves_to_fallback`, `transition_running_to_waiting_quota_pauses_the_lease`, `restart_writes_1_launching_attempt_and_spawns_once_after_the_commit`, `restart_waits_for_the_old_process_to_be_gone`, `pres_06_s13_quota_probe_holds_no_credential` (the pool reading reads only the quota file or the probe result).
2. Write the code. Run the Check and the guard tests. Commit.

## Constraints

1. This fleet runs on the installed `horch` binary and on the live ledger
   `/Users/MASCOTT/.horch/ledger.sqlite3`. The code that you change is the code of that tool.
   Do not break the fleet that builds it:
   - Each test, and each `horch` or `fleetd` process that you build and start, uses temporary
     folders for `HORCH_DATA_DIR`, `HORCH_STATE_DIR` and `HORCH_LEDGER_DB`. Never point a
     development build at the live ledger or at the live state folders.
   - Never install a development build. Never run `horch self-update`, `cargo install` or a
     release script from a worktree.
   - Never start a development `fleetd`, watchdog or service unit on this machine outside a test
     that uses temporary folders. Never register a launchd job, a systemd unit or a scheduled task
     on this machine.
   - The only `horch` commands that you run on the live fleet are the worker commands of your own
     task (`horch ledger ...`, `horch tell orchestrator ...`).
2. NEVER use `ANTHROPIC_API_KEY`. Never read, print, set, export, pass or depend on it. Never
   write code, config, docs or teammate files that use it. Each `claude` run uses the claude.ai
   subscription login. Remove the variable from each `claude` command:
   `env -u ANTHROPIC_API_KEY claude ...`. If a task seems to need an API key, stop and ask.
3. Do not use a shared `CARGO_TARGET_DIR`. Build in the `target/` folder of your worktree. Run
   `cargo clean` in your worktree before `horch ledger finish` only if the disk has less than
   20 GB free (`df -h /`).
4. Before the first commit, run `git remote -v` and `git config user.email`. The remote must be
   `git@git-alt:MattMakes/multi-herdr.git` and the email must be `m4ttplu5@gmail.com`. If not,
   stop and send BLOCKED.
5. Commit messages have no co-author line. Never write the em dash character; use `-`. Never edit
   `CHANGELOG.md` or a generated file by hand.
6. Write the tests first and see them fail for the correct reason. Then write the code.
7. The source of truth is `docs/specs/fleetd/design.md` (FINAL). If the code of today and the
   design do not agree on a point that the ticket does not settle, stop and send QUESTION. Do not
   change `docs/specs/fleetd/` or `ddd/`.
8. Change only the files of `Own`. If the work needs another file, send QUESTION.
9. Prefer quality, simplicity and robustness to speed. No feature that the design does not name.
10. If the GitHub SSH connection times out at `horch ledger finish`, run it again, 3 times maximum.

## Done when

The Check passes. A usage-limited attempt burns no attempt, and its task restarts on the fallback of another provider or waits in `waiting_quota` until its pool resets.
