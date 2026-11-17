# 61: The health guards compare numbers of the same scope, and a fact that the platform does not support gives no hold

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-platform --test machine && cargo test -p horch-core --lib fleetd::fleet_runtime::domain::health && cargo test -p horch-core --lib fleetd::fleet_runtime::application::holds && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/domain/health.rs`, `crates/horch-core/src/fleetd/fleet_runtime/application/holds.rs`, `crates/horch-platform/src/machine.rs`, `crates/horch-platform/src/mac/machine.rs`, `crates/horch-platform/src/linux/machine.rs`, `crates/horch-platform/src/windows/machine.rs`, `crates/horch-platform/tests/machine.rs`

Spec: `docs/specs/fleetd/design.md`, OR-5 (the health guards), "Supervision, liveness and reconciliation" (Holds on dispatch are visible), `docs/specs/fleetd/preservation-06-wiring-and-platform.md` row S12 (a reading that is not known is neither a pass nor a hold).

## Goal

Each health guard compares a count with a limit of the same scope: a count of the system with a limit of the system, and a count of the user with a limit of the user. A fact that the platform does not support is reported as unknown with the reason "not supported", and it never holds dispatch.

## Requirements

- Required item 4 of the phase: the guards compare numbers of the same scope (system with system, user with user).
- Today the open file guard compares the open files of the user (`user_open_file_count`) with the open file limit of the system (`system_open_file_limit`, macOS `kern.maxfiles`, Linux `/proc/sys/fs/file-max`). Change it to compare the open files of the system with the limit of the system: add `system_open_file_count` to `horch-platform/src/machine.rs` (macOS `kern.num_files`, Linux the first field of `/proc/sys/fs/file-nr`, Windows `NotSupported`).
- The process guard already compares the processes of the user with `RLIMIT_NPROC`, a limit of the user. Keep it, and give it a test that names the scope.
- A `NotSupported` result of the platform is `Reading::Unknown` with the reason `not supported on this platform`, not the reason `the probe has no reading`. `Counts::read` in `holds.rs` keeps the 2 reasons apart (today `.ok().flatten()` joins them). An unknown reading gives `Finding::Unknown` and no hold.

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- `domain/health.rs` is pure: `health(&Readings, &Floors)` gives the findings, and `share` makes the hold of a count. Rename the fields of `Readings` so that each name says its scope (`system_open_files`, `system_open_file_limit`, `user_processes`, `user_process_limit`). Keep the config keys and the needs-you text of the holds.
- `application/holds.rs` (ticket 04 adds the hold `missing fast gate` to it first) reads the platform in `Counts::read` and builds the readings in `readings`.
- Change the doc comments at the top of `horch-platform/src/machine.rs` and `domain/health.rs` that say "the open files of the user with the open file limit of the system".
- The platform test `the_system_open_file_limit_is_at_least_the_count_of_the_user` becomes `the_system_open_file_limit_is_at_least_the_count_of_the_system`. A sandbox can deny the sysctl: `None` is a valid result, as today.
- The tests of `horch-platform/tests/machine.rs` read the real machine, as they do today. The tests of `horch-core` give their readings as values.
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: in `health.rs`, `open_file_guard_compares_system_count_with_system_limit`, `process_guard_compares_user_count_with_user_limit` and `not_supported_fact_gives_unknown_and_no_hold`; in `holds.rs`, `not_supported_count_keeps_its_reason`; in `horch-platform/tests/machine.rs`, the renamed limit test and `system_open_file_count_is_at_least_the_count_of_the_user` (unix).
2. Add `system_open_file_count` for each platform. Change `Counts`, `Readings` and `share`.
3. Run the Check. Commit the exact files.

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

Self-hosting: this phase changes `horch spawn`, the claim and the finish steps. The build fleet itself uses the installed `horch` for these. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).

## Done when

The Check passes. Each guard compares a count and a limit of the same scope, and a fact that the platform does not support shows as unknown and holds nothing.
