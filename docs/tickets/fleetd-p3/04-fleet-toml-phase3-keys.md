# 04: The Phase 3 keys of fleet.toml: the period of the worktree_list job, the worktree disk budget and the time limit of a rescue job

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --lib fleetd::fleet_runtime && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/domain/config.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/config_file.rs`

Spec: `docs/specs/fleetd/design.md`, "The numbers of the design" (the rows "Period of the `worktree_list` job", "Worktree disk budget" and "Time limit of a `worktree_add`, `worktree_remove`, `worktree_list` ... job"), "Worktrees: leased, reconciled, never lost" ("The reconciler compares three views"; "Disk").

## Goal

`fleet.toml` has the 2 keys that Phase 3 needs, with the defaults of the design. Each later ticket reads them from `FleetConfig`.

## Requirements

- **RR-10** (the period): `worktree_list_period_s`, default 60.
- **FM-28** (the budget): the worktree disk budget, default 20 GB.

## Context

- Read `docs/tickets/fleetd-p3/MAP.md`, section "Open questions", first (Q20 and Q25 apply here).
- `FleetConfig` and its defaults are in `crates/horch-core/src/fleetd/fleet_runtime/domain/config.rs`; the file reader and its refusal of unknown keys and bad values are in `fleet_runtime/adapters/config_file.rs` (Phase 1 ticket 04, Phase 2 ticket 05 of fleetd-p2). Follow their form: 1 field, 1 default, 1 key, 1 test.
- The keys:
  - `worktree_list_period_s` = 60 (the design's name).
  - `worktree_disk_budget_gb` = 20 (Q20: the design's `worktree_disk_budget` with its unit in the name, as each key of Phases 1 and 2). A value of 0 is refused.
- The job limits `job_limit_s.worktree_add`, `worktree_remove` and `worktree_list` exist from Phase 1 (600 s each, `docs/tickets/fleetd-p1/MAP.md` Q2, `JobLimits` in `config.rs`). The kind `rescue` has no limit today, so `time_limit_s` (`fleet_runtime/application/jobs.rs`) returns `None` and a rescue job cannot be queued. Add `job_limit_s.rescue` = 600 to `JobLimits` (Q25). Ticket 05 reads it in `time_limit_s`.

## Steps

1. Write the tests first. Each fails before the key exists: `worktree_list_period_s_defaults_to_60`, `worktree_disk_budget_gb_defaults_to_20`, `worktree_disk_budget_gb_of_0_is_refused`, `job_limit_s_rescue_defaults_to_600`, `fleet_toml_sets_the_phase_3_keys`.
2. Write the code. Run the Check. Commit.

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

Self-hosting: each test makes its own temporary git repository and temporary folders (build rule 1). No test and no worker runs the reconciler, a rescue, a cleanup or a `git worktree remove` on the repository `/Users/MASCOTT/projects/mh/multi-herdr` or on its `.worktrees/` folder.

## Done when

The Check passes, and `FleetConfig` gives `worktree_list_period_s`, `worktree_disk_budget_gb` and `job_limit_s.rescue` from `fleet.toml` or the defaults of the design.
