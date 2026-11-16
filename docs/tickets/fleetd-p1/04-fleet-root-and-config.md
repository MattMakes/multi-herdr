# 04: The fleet root and the Phase 1 keys of fleet.toml

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --lib fleetd::fleet_runtime && cargo test -p horch-core --lib runtime::paths && cargo test -p horch-core --test arch_scan`
**Own:** `Cargo.toml`, `Cargo.lock`, `crates/horch-core/Cargo.toml`, `crates/horch-core/src/runtime/paths.rs`, `crates/horch-core/src/fleetd/fleet_runtime/domain/config.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/config_file.rs`

Spec: `docs/specs/fleetd/design.md`, "Workspace layout and file protocol" (the root and `fleet.toml`), "Migration plan" ("The numbers of the design").

## Goal

fleetd and the `horch` commands find 1 fleet root for 1 ledger, and read the Phase 1 keys of `<fleet root>/fleet.toml`, with the default or starting value of each key when the file or the key is absent.

## Requirements

- No requirement becomes green here. Each later ticket reads its numbers from this config (the table "The numbers of the design").

## Context

- Read `docs/tickets/fleetd-p1/MAP.md`, section "Open questions", first. It records each choice of this phase (Q1 to Q21), and each ticket follows it.
- Phase P (`docs/tickets/fleetd-pp/`) builds `crates/horch-platform`. Its concern tickets propose function names. The module doc of each concern file in `crates/horch-platform/src/` gives the final name. Read that doc before you call a platform function. A name in this ticket is the proposed name.
- The fleet root (MAP.md, Q4): `$FLEET_ROOT`, else the folder `fleet` next to the ledger file (`<folder of ctx.paths.ledger_db>/fleet`). So 1 ledger has 1 fleet root, 1 lock and 1 heartbeat. The live fleet then uses `~/.horch/fleet/`, and each test uses the folder of its own `HORCH_LEDGER_DB`. The design default `/fleet` needs root rights on macOS.
- `arc_05_no_ambient_env_in_core` keeps each env read in `crates/horch-core/src/runtime/`. Read `FLEET_ROOT` in `runtime/paths.rs`, into a new field `fleet_root: PathBuf` of `Paths` (with its doc, as the other fields have).
- The keys of Phase 1 and their values (MAP.md, Q1 and Q2). Each is a field of 1 struct `FleetConfig` in `domain/config.rs`:
  - `pass_period_s` 5; `pass_time_budget_s` 20; `scan_budget_ms` 2000; `pass_budget` 200 (rows of each kind).
  - `stop_grace_s` 20; `retention_days` 30.
  - `abandoned_run_limit` 3; `job_backoff_s` [30, 120, 480]; `job_limit_s` for each kind: `worktree_add`, `worktree_remove`, `worktree_list` 600; `docs_sync`, `conformance` 3600. `task_check` uses the check timeout of today (`CHECK_TIMEOUT` in `crates/horch-core/src/taskledger/finish/land.rs`, 20 min, preservation 05 V-10), not a key.
  - The health floors: `health_min_free_disk_gb` 10; `health_max_load_per_cpu` 1.0 (today: the load note of `horch spawn`, `crates/horch/src/cmd/spawn.rs:908`, `load >= cpus`); `health_min_free_memory_mb` 1024; `health_max_process_share` 0.9 of the process limit of the user; `health_max_open_file_share` 0.9 of the open file limit.
  - The fixed contract (not a key): the hang limit of 30 s of awake time (PL-5). Put it in `domain/config.rs` as a `const`.
- `domain/` holds no file I/O: `domain/config.rs` parses a `&str` (TOML) and refuses an unknown key and a value out of range (for example `pass_time_budget_s` >= 30), as `Policy::parse` in `crates/horch-core/src/routing/policy.rs` refuses an unknown field. `adapters/config_file.rs` reads the file.
- The workspace has no TOML crate today. Add `toml` (the current 0.8 or later release, with `serde`) to `[workspace.dependencies]` of the root `Cargo.toml` and to `crates/horch-core/Cargo.toml`, and update `Cargo.lock` with `cargo build` (MAP.md, Q4). The download is a package install that the task needs.

## Steps

1. Write the tests first. Each fails before the code exists:
   - In `domain/config.rs`: `defaults_are_the_numbers_of_the_design`, `unknown_key_is_refused_with_its_name`, `pass_time_budget_at_or_over_the_hang_limit_is_refused`, `a_partial_file_keeps_the_default_of_each_absent_key`.
   - In `adapters/config_file.rs`: `missing_file_gives_the_defaults`.
   - In `runtime/paths.rs`: `fleet_root_is_next_to_the_ledger_by_default` and `fleet_root_env_wins`.
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

The Check passes. A missing `fleet.toml` gives the numbers of the design and of MAP.md, Q1 and Q2.
