# 12: horch result writes result.json of the attempt, with its schema, and horch result --check tells a Stop hook whether the turn may end

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --lib fleetd::task_execution::domain::result && cargo test -p horch --test p4_result_cli && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch/src/cmd/resultcmd.rs`, `crates/horch-core/src/fleetd/task_execution/domain/result.rs`, `crates/horch/tests/p4_result_cli.rs`

Spec: `docs/specs/fleetd/design.md`, "Workspace layout and file protocol" (Example: result.json; Protocol rules 1, 2, 5), RR-3, FM-6.

## Goal

A worker runs `horch result` and gets a valid `outbox/result.json` of its own attempt, written 1 time, atomically. `horch result --check` exits 0 when `result.json` or a question file of the attempt exists, else exits 2 with the line "run `horch result` first".

## Requirements

- `status` is 1 of the closed list `completed`, `blocked`, `failed`; the schema refuses any other value (`result_json_with_another_status_is_refused`).
- The payload names `task` and `attempt` equal to those of its path (SR-6, the side of the writer).
- A second `horch result` of 1 attempt refuses and prints the path of the first: an immutable message is never rewritten.
- `commits` defaults to the commits of the branch since its base; `files_changed` to the files of those commits (read in the worktree of the attempt).

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- The attempt comes from the environment of the pane: the bound attempt id (ticket 09) or the outbox path of `task.yaml`. Outside a push attempt, `horch result` refuses with the cause.
- Size limit: `result_max_bytes` (ticket 03), 64 KiB.
- The schema lives in `domain/result.rs` as a pure validator (`fn validate(bytes, task, attempt) -> Result<ResultFile, Refusal>`), used by this command and by the ingest of ticket 13.
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `result_writes_a_valid_file_once`, `result_json_with_another_status_is_refused`, `second_result_of_1_attempt_is_refused`, `check_exits_2_with_neither_file_and_0_with_one`, `result_outside_a_push_attempt_is_refused`, `oversized_result_is_refused`.
2. Write the validator and the command.
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

The Check passes. `horch result` writes 1 valid `result.json`, and `horch result --check` gives the exit codes of the Stop hook.
