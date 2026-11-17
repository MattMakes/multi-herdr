# 03: The Phase 4 keys of fleet.toml, the time limits of the 5 job kinds that have none, and the route of the integration job kinds

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --lib fleetd::fleet_runtime && cargo test -p horch-core --test fleetd_jobs && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/domain/config.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/config_file.rs`, `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs`, `crates/horch-core/tests/fleetd_jobs.rs`

Spec: `docs/specs/fleetd/design.md`, "The numbers of the design", "The kinds of job", "Landing a task branch on its feature branch" (time limits), "Between features" (fetch period), "No work is lost" (`rescue_unique_budget`).

## Goal

Each number of Phase 4 has its key in `fleet.toml`, with its default. Each job kind has a time limit, so no job of Phase 4 runs without one. `run_job` sends the integration job kinds to the router of ticket 01.

## Requirements

- Required item 5 of the phase: `job_limit_s` gets the keys `fast_gate` = 1200, `full_gate` = 3600, `trial_merge` = 180, `bundle_build` = 600 and `rebase` = 600 (answer of the orchestrator, Q-B in MAP.md: starting values to tune). `time_limit_s` in `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs` reads them and returns `Some` for each kind. `task_check` keeps 1200 s, `fetch` 180 s and `push` 600 s (F11, fixed contract).
- Keys: `fetch_period_s` = 60, `push_failure_limit` = 3, `rescue_unique_budget_mb` = 5120 (starting value), `landing_ready_limit_s` = 1800, `landing_repair_limit_s` = 3600, `landing_job_margin_s` = 600, `bundle_passed_limit_s` = 600, `result_max_bytes` = 65536, `resolver_max_lines` = 100, `resolver_max_files` = 2 (Q6, Q7).
- Each starting value has a comment line in the file format and in the doc comment of its field: "a starting value, to tune from real runs".

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- Phase 1 ticket 04 made the file format, Phase 2 ticket 05 and Phase 3 ticket 04 added keys. Follow their pattern, also for the error that names a bad key.
- The route: in `run_job` of `jobs.rs`, the kinds `bundle_build`, `fast_gate`, `full_gate`, `fetch`, `push` and `rebase` call `crate::fleetd::integration::adapters::executors::run`. `trial_merge` keeps "no executor" (Phase 6).
- Tests read no real file of the machine: each test writes its own `fleet.toml` in a temporary folder.
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: in `config.rs`, `phase4_keys_have_the_design_defaults` and `a_bad_phase4_key_names_the_key`; in `crates/horch-core/tests/fleetd_jobs.rs`, `each_job_kind_has_a_time_limit` (it walks `KINDS` and fails today for 5 kinds) and `integration_kinds_route_to_the_integration_router`.
2. Write the keys, `time_limit_s` and the route.
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

The Check passes. `time_limit_s` returns a limit for each job kind, and each Phase 4 key reads from `fleet.toml` with its default.
