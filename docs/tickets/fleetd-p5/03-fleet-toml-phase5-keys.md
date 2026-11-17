# 03: The Phase 5 keys of fleet.toml: the size limits of worker and request files, and the list of orchestrator seats

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --test fleetd_config_p5 && cargo test -p horch-core --lib fleetd::fleet_runtime && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/domain/config.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/config_file.rs`, `crates/horch-core/tests/fleetd_config_p5.rs`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 7 "Phase 5: one ledger writer"; Architecture and roles, "The command channel"; Workspace layout and file protocol, Protocol rules).

## Goal

`fleet.toml` has a key for each size limit of the protocol rules and the list of orchestrator seats, each with the value of the design.

## Requirements

- SR-6 (the size limit of each type of file)
- FM-23
- Open question Q2 (the orchestrator seats come from fleet.toml, never from an environment variable)

## Context

- Read `docs/tickets/fleetd-p5/MAP.md` first: the sections "Open questions", "Request kinds" and "Decisions that hold".
- Self-hosting danger: this phase changes how each `horch` command reaches the ledger. The build fleet itself uses the installed `horch`. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- The size limits are in the design, Workspace layout and file protocol, Protocol rules, rule 7. They are starting values.
- `result_max_bytes` = 65536 exists from ticket 03 of fleetd-p4 (Open question Q7 of `docs/tickets/fleetd-p4/MAP.md`). Keep it.
- Each key has its unit in the name, as each key of Phases 1 to 4.
- A file that a ticket of fleetd-p2, fleetd-p3 or fleetd-p4 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/fleetd_config_p5.rs`: `phase5_keys_have_the_values_of_the_design` (an empty `fleet.toml` gives each default below), `phase5_key_from_the_file_wins`, `orchestrator_seats_default_to_fleet_orchestrators`, `negative_or_zero_limit_is_refused_with_the_key_name`.
2. Add the keys and their defaults:
   - `question_max_bytes` = 65536
   - `request_max_bytes` = 65536
   - `heartbeat_max_bytes` = 4096
   - `progress_max_bytes` = 8388608, `progress_max_lines` = 10000, `progress_line_max_bytes` = 4096
   - `transcript_tail_bytes` = 262144, `transcript_line_max_bytes` = 1048576
   - `job_result_max_bytes` = 65536 (Open question Q14)
   - `orchestrator_seats`: a list of persona names. The default is the list `FLEET_ORCHESTRATORS` of `crates/horch-core/src/roster/mod.rs`.

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

- The Check passes, with the guard tests.
- The 4 tests pass.
- Each key is read in 1 place (`config.rs`), and no other file has the numbers.
