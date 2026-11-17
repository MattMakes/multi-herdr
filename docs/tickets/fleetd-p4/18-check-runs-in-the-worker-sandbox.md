# 18: The check of a task runs only in the sandbox profile of its worker; a persona with no check sandbox gets no dispatch, 1 needs-you item and 1 status line

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd::worker_profiles fleetd::fleet_runtime::application::jobs && cargo test -p horch-core --test check_sandbox && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/worker_profiles/adapters/check_sandbox.rs`, `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs`, `crates/horch-core/tests/check_sandbox.rs`

Spec: `docs/specs/fleetd/design.md`, SR-4, "Architecture and roles" (Verifier: the acceptance commands run in the sandbox profile of the task's worker, never with the rights of fleetd; A worker never runs without its sandbox), `docs/specs/fleetd/preservation-04-behavior.md` row F10.

## Goal

The `task_check` job runs the check in the sandbox of the task's worker: Codex keeps `codex sandbox` with its `fleet_write` profile (F10); each other CLI gets the check sandbox of ticket 17 with the writable paths of the profile (the worktree, the attempt folder, the temp folder and the declared `writable_paths`). A persona whose check sandbox is NotSupported is not dispatched, gets 1 needs-you item, and `horch status` shows why.

## Requirements

- Required item 3 of the phase: the `task_check` executor runs only in the sandbox profile of the worker. The Phase 1 executor is fail-closed without a profile (`task_check` in `jobs.rs`, the "SR-4: ... has no sandbox profile" errors). No sandbox means no worker and no check.
- Answer of the orchestrator (Q-A, rule 2): NotSupported means no dispatch of that profile, 1 needs-you item for each profile (key `no_check_sandbox:<persona>`), not for each task, and a clear line in `horch status` (ticket 45 prints it from the needs-you item or the hold). Never a silent run with the rights of fleetd.
- `check_runs_in_the_worker_sandbox_not_with_fleetd_rights`: a check that writes outside the allowed paths fails, and the file outside does not exist.
- SR-7: the check job starts through the launcher, so it never has `ANTHROPIC_API_KEY`.

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- Today: `ProfileSandboxes` and the trait `SandboxProfiles` in `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs` (from line 672): only a Codex worker has a profile. Move the implementation to `worker_profiles/adapters/check_sandbox.rs`; `jobs.rs` keeps the trait and the call.
- The port that ticket 08 calls: `CheckSandboxes::supported(persona) -> Result<(), NotSupported>`. Implement it here.
- Shared-checkout tasks (no worktree): the writable path is the project root, as today's `check_process` (`crates/horch-core/src/taskledger/finish/land.rs:227`).
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/check_sandbox.rs`: `check_runs_in_the_worker_sandbox_not_with_fleetd_rights`, `codex_worker_check_keeps_codex_sandbox`, `claude_worker_check_runs_in_the_platform_sandbox`, `not_supported_persona_is_not_dispatched_and_opens_1_item_per_persona`, `check_job_has_no_forbidden_env`.
2. Write the adapter and change `jobs.rs`.
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

The Check passes. No check runs outside the sandbox of its worker, and a persona with no check sandbox is visible and is not dispatched.
