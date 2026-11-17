# 03: The Phase 6 keys of fleet.toml: idle_stop_s, park_after_days, reminder_every_s and working_hours

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --test fleetd_config_p6 && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/domain/config.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/config_file.rs`, `crates/horch-core/tests/fleetd_config_p6.rs`

Spec: `docs/specs/fleetd/design.md` (The numbers of the design (Stop of an idle sub-orchestrator; Park of an idea that waits on you); Ideas (Waiting on you is not a stall)).

## Goal

`fleet.toml` has the keys that the idea dispatcher needs, with the values of the design and range checks.

## Requirements

- FM-29 (the timers of an idea that waits on you)
- PL-4 (the 30 min stop is on the fleet clock; the 7 day park and the reminders use calendar time)

## Context

- Read `docs/tickets/fleetd-p6/MAP.md` first: the sections "Open questions", "Old flow and new" and "Decisions that hold".
- Follow the keys of earlier phases in `config.rs` and their range errors (ticket 05 of fleetd-p2 has the table form).
- Keys: `idle_stop_s` = 1800 (Default, 60 to 86400, fleet clock); `park_after_days` = 7 (Default, 1 to 365, calendar time); `reminder_every_s` = 3600 (Default, 600 to 86400, calendar time); `working_hours` = `"Mon-Fri 09:00-18:00"` (Starting value, the local time zone of the machine; MAP.md, Q3).
- `question_limit_s` and `replan_limit_s` exist from Phase 2. Do not add them again.
- `working_hours` has 1 form: a day range and an hour range. A test gives the time through the fleet clock and a fixed time zone, never through the real clock.
- Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p5 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/fleetd_config_p6.rs`: `phase6_keys_have_the_values_of_the_design`, `each_phase6_key_out_of_range_is_refused_with_its_name`, `working_hours_parses_days_and_hours`, `working_hours_with_a_bad_form_is_refused`. They fail because the keys do not exist.
2. Add the keys and their parse.
3. Run the Check and the guard tests. Commit the exact files.

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

- The Check passes.
- An empty `fleet.toml` gives the 4 values above.
