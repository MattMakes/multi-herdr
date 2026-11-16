# 19: The health guards and pacing: holds on new dispatch, with their events

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --lib fleetd::fleet_runtime && cargo test -p horch && cargo test -p horch-e2e --test taskledger`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/domain/health.rs`, `crates/horch-core/src/fleetd/fleet_runtime/application/holds.rs`, `crates/horch/src/cmd/spawn.rs`, `crates/horch/src/cmd/taskledger_spawn.rs`

Spec: `docs/specs/fleetd/design.md`, OR-4, OR-5, D8, "Worker profiles and adapters" (No budget, no cap, and the model guide: "Pacing"), "Supervision, liveness and reconciliation" (Holds on dispatch are visible), "Preservation analysis" rows 06 S12 and 06 S13.

## Goal

New dispatch pauses only while free disk, machine load, free memory, the count of processes or the count of open files of the user is past its floor, and resumes by itself. A pool whose usage runs ahead of its window is paced. A probe that returns unknown is reported, not read as healthy or unhealthy. fleetd records an event when a global hold starts and when it ends.

## Requirements

- **OR-4**: fleetd reads the usage window of each pool and dispatches so that the usage is consumed by the reset. Test: pacing with a fake pool window.
- **OR-5**: the 5 health guards, read from the machine probe of the platform layer, pause new dispatch and resume by themselves; an unknown probe is reported. Test: a low-disk probe pauses dispatch; a normal probe resumes it.

## Context

- Read `docs/tickets/fleetd-p1/MAP.md`, section "Open questions", first. It records each choice of this phase (Q1 to Q21), and each ticket follows it.
- Phase P (`docs/tickets/fleetd-pp/`) builds `crates/horch-platform`. Its concern tickets propose function names. The module doc of each concern file in `crates/horch-platform/src/` gives the final name. Read that doc before you call a platform function. A name in this ticket is the proposed name.
- Dispatch in Phase 1 (MAP.md, Q16): push dispatch comes in Phase 4. In Phase 1, a new worker for a task starts through `horch spawn` (`crates/horch/src/cmd/spawn.rs`) and `horch ledger spawn` (`crates/horch/src/cmd/taskledger_spawn.rs`). Both call `holds(ctx)` and refuse with each hold and its cause. The refusal is not a cap: it names the floor and the reading.
- The floors (MAP.md, Q1) come from ticket 04. The load floor keeps the value of today: the load note of `horch spawn` at `crates/horch/src/cmd/spawn.rs:908` (`load >= cpus`). Replace that note by the guard, with the same threshold.
- The probe: today `crates/horch-core/src/runtime/machine.rs` `probe(path, bins, fixture)` gives `MachineSnapshot`; the `fixture` (`ctx.settings.machine_file`) is the fault injection of the test. The 2 counts and the open file limit come from ticket 09 (`horch_platform::machine`). Add them to the input of `health()` without a change of `MachineSnapshot` (it has `deny_unknown_fields` and is a contract of today): read them beside it.
- An unknown reading (`Known::Unknown`, or `NotSupported` from the platform) is neither a pass nor a hold: `health()` returns it as `Unknown { fact }`, and the spawn prints 1 note for it (row 06 S12).
- Pacing: today `crates/horch-core/src/routing/` reads each pool window (`routing::snapshot::obtain`, `quota::Window::pace`, `Policy::pace_factor` 1.25 and `pace_min_used` 0.50 in `routing/policy.rs`; preservation 05 V-26). The pacing hold of a pool: its window pace is above `pace_factor` and its use is at least `pace_min_used`. The pool of a spawn comes from `quota::pool_for`. Keep the numbers of today.
- The events: `application/holds.rs` gives `record_hold_changes(tx, before, now)`: 1 event `hold_started` or `hold_ended` with the cause, when a global hold (a health floor) starts or ends. Ticket 21 calls it in each pass.
- `domain/health.rs` is pure: inputs are readings and floors, the output is a list of holds and unknowns.

## Steps

1. Write the tests first. Each fails before the code exists:
   - In `domain/health.rs`: `low_disk_probe_pauses_dispatch_and_normal_probe_resumes`, `each_floor_pauses_only_past_its_value`, `pres_06_s12_unknown_probe_neither_pauses_nor_passes` (Slice 1), `pacing_holds_a_pool_ahead_of_its_window_and_frees_one_on_pace`.
   - In `application/holds.rs`: `a_hold_records_1_event_when_it_starts_and_1_when_it_ends`.
   - In `taskledger_spawn.rs`: `ledger_spawn_refuses_with_the_hold_and_its_cause` (a machine fixture with low disk).
2. Write the code. Run the Check.

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

The Check passes. A low-disk fixture refuses a spawn with its cause; a normal fixture allows it; an unknown probe is a note, not a hold.
