# 07: task.yaml and START.md: fleetd writes them once for each attempt, atomically, with no secret in them

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --lib fleetd::task_execution && cargo test -p horch-core --test task_files && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/task_execution/domain/task_files.rs`, `crates/horch-core/src/fleetd/task_execution/adapters/task_files.rs`, `crates/horch-core/tests/task_files.rs`

Spec: `docs/specs/fleetd/design.md`, "Workspace layout and file protocol" (Directory layout; Protocol rules 1, 2, 3; Example: task.yaml), SR-2.

## Goal

fleetd renders `task.yaml` and `inbox/START.md` of an attempt from the ledger, and writes each one 1 time with a temp name, an fsync and a rename. A file that exists is never rewritten. Neither file holds a secret.

## Requirements

- SR-2: secrets reach agents only through env vars named in the profile, never through task files. The lint refuses a file whose text holds the value of an env var of the profile, the value of a variable named in `FORBIDDEN_ENV` (`crates/horch-core/src/harness/`), or a line that matches `*_KEY=`, `*_TOKEN=`, `*_SECRET=`. A refused render writes no file and returns an error that names the rule, not the value.
- Protocol rule 1 (atomic write) and rule 2 (an immutable message is never rewritten).

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- Paths (Phase 1 Q4): `<fleet root>/tasks/<task id>/task.yaml` is per task in the design, but an attempt has its own. Choice (MAP.md, Q3): `<fleet root>/tasks/<task id>/attempts/<attempt>/task.yaml` and `.../inbox/START.md`, so each attempt has its own immutable file. `crates/horch-e2e/tests/p0_dispatch.rs` finds `task.yaml` anywhere under `tasks/`.
- `task.yaml` keys (design example, with today's ids): `task` (`T<seq>`), `task_id` (the UUID), `run`, `attempt` (`A<n>`), `profile` (the persona), `worktree` (the absolute path), `branch`, `read_first` (the ticket path), `goal` (the ticket title), `check`, `kind` (`work_kind`), `lease: {renew_every_s: 10}`, `report_to` (the outbox path). `crates/horch-e2e/tests/p0_dispatch.rs` reads the lines `attempt: A2` and `worktree: <path>`: keep 1 key per line, with no quotes on these 2 values.
- `START.md`: the ticket snapshot of the task (`ticket_snapshot`), then a short fixed part: the worktree, the branch, the outbox, and "When the work is committed, run `horch result`. To ask a question, send `QUESTION:` (ticket 16)." For a resolver or fix attempt, ticket 33 adds its section; the renderer takes an optional extra section.
- Use the atomic write helper of today (`crates/horch-core/src/fsx.rs`). No YAML crate is needed if the renderer writes the fixed keys; a test parses the output with `serde_yaml` only if the workspace has it, else with a line reader.
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `task_yaml_has_each_key_and_1_value_per_line`, `start_md_holds_the_ticket_and_the_result_line`, `existing_file_is_never_rewritten`, `half_written_file_is_never_read` (a `.tmp` stays invisible), `sr2_profile_env_value_in_a_task_file_is_refused`, `sr2_forbidden_env_name_in_a_task_file_is_refused`.
2. Write the pure renderer and lint in `domain/task_files.rs`, and the writer in `adapters/task_files.rs`.
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

The Check passes. The 2 files of an attempt are written 1 time, atomically, and a file with a secret is never written.
