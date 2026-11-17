# 05: The Phase 2 keys of fleet.toml

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --lib fleetd::fleet_runtime && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/domain/config.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/config_file.rs`

Spec: `docs/specs/fleetd/design.md`, "The numbers of the design" (the rows "Wrapper heartbeat", "Heartbeat age for a lease renewal", "Time limit of a question", "Replan limit"), "Supervision, liveness and reconciliation" ("Lease renewal is earned, not claimed"; "Stall ladder").

## Goal

`fleet.toml` has the keys that Phase 2 reads, each with its default or starting value and its range, in the same form as the keys of Phase 1.

## Requirements

- **RR-1** (the numbers): the wrapper heartbeat every 10 s; a lease renews only when the heartbeat is under 30 s old.
- **RR-11** (the number): the usage of a pool that counts as full use (MAP.md, Q1).
- **RR-6** (the number): the replan limit of a `failed` task.

## Context

- Read `docs/tickets/fleetd-p2/MAP.md`, section "Open questions", first (Q1 applies here).
- Phase 1 (ticket 04 of `docs/tickets/fleetd-p1/`) wrote `FleetConfig` in `crates/horch-core/src/fleetd/fleet_runtime/domain/config.rs` and the parser in `adapters/config_file.rs` (`load`, `parse`; an unknown key is refused with its name; an absent key keeps its default). Follow that form: 1 field and 1 doc comment for each key, with its class (default or starting value) and its source.
- The keys:

| Key | Value | Class | Range |
| --- | --- | --- | --- |
| `heartbeat_every_s` | 10 | Starting value | 1 to 60 |
| `heartbeat_max_age_s` | 30 | Starting value | greater than `heartbeat_every_s`, at most 300 |
| `question_limit_s` | 900 (15 min) | Starting value | 60 to 86400 |
| `replan_limit_s` | 1800 (30 min) | Starting value | 60 to 86400 |
| `pool_full_use` | 0.98 | Starting value | greater than 0.5, at most 1.0 |

- `pool_full_use`: the used share of the binding window of a pool at which the pool counts as limited (a usage limit, RR-11), and below which the pool probe "reports ok" for the exit of `waiting_quota`. The starting value 0.98 is the value of today for the same fact: `exhausted_used` in `crates/horch-core/src/routing/policy.rs:75`, the threshold at which today's routing calls a pool `exhausted`. Name that source in the doc comment. MAP.md Q1 asks the operator to confirm it.
- The design names the replan limit `replan_limit`. Use the key `replan_limit_s`, so the unit is in the name as in each key of Phase 1 (MAP.md, Q1).
- The per-profile numbers (`lease_s`, `stall_after_s`, `start_timeout_s`, `max_wait_s`, `backoff_s`, `max_attempts`) are not keys of `fleet.toml`. Ticket 06 adds them to the profile.

## Steps

1. Write the tests first, next to the tests of Phase 1 in `config_file.rs` and `config.rs`. Each fails before the code exists:
   - `phase2_keys_have_their_defaults` (a missing file gives each value of the table).
   - `heartbeat_max_age_must_be_above_heartbeat_every` (refused with both key names).
   - `pool_full_use_out_of_range_is_refused`.
   - `a_file_sets_each_phase2_key`.
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

The Check passes. Each key of the table parses, keeps its default when absent, and is refused out of its range with its name.
