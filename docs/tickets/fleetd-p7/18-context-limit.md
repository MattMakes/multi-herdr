# 18: The 256k context limit for a CLI with no compaction setting: the wrapper ends the attempt with context_limit, the task goes on with a handoff, and a loop with no new commit stops

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test p7_context_limit && cargo test -p horch-core --lib fleetd::task_execution && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/task_execution/domain/context_limit.rs`, `crates/horch-core/src/fleetd/task_execution/domain/retry.rs`, `crates/horch-core/src/fleetd/task_execution/application/retry.rs`, `crates/horch-core/src/execution/lifecycle.rs`, `crates/horch-core/tests/p7_context_limit.rs`

Spec: `docs/specs/fleetd/design.md` (FM-20; FR-9; "Supervision, liveness and reconciliation" (rung 6 and the handoff file); "What herdr and the agent CLIs give us" (Compact at 256k)).

## Goal

An Antigravity attempt that reaches 256000 context tokens ends with the outcome `context_limit`, and its task goes on at once in the same worktree with a handoff file, with no attempt burned and no endless loop.

## Requirements

- FM-20: the wrapper ends the attempt with a handoff at the 256k limit.
- FR-9 (part): the limit for a CLI with no compaction setting.
- The loop guard (answer of the orchestrator, MAP.md Q2).

## Context

- Read `docs/tickets/fleetd-p7/MAP.md` first: the sections "Open questions" and "Decisions that hold".
- Self-hosting danger: this phase changes the worker mode that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p6 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it (the text says "from ticket NN of fleetd-pN").
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`. No test uses the network: give each page through a local fixture. No ticket sends project text, a name or an email address to an outside service.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).
- The outcome (MAP.md Q2, option 1 of the orchestrator): `context_limit` burns no attempt. The task moves to `ready` at once, with no `not_before`, and the new attempt runs in the same worktree with the handoff file of rung 6 (built from `progress.jsonl`, `git diff` and the transcript tail; `crates/horch-core/src/fleetd/task_execution/application/retry.rs`).
- The loop guard: when 2 attempts in a row of 1 task end with `context_limit` and the task branch has no new commit between them, the task does not go to `ready` again. It opens 1 needs-you item that names the task and the 2 attempts. Use the progress facts that the lease rules of Phase 2 use (`crates/horch-core/src/fleetd/task_execution/domain/lease.rs`). If none of them holds the head of the worktree, record the head at the end of the attempt in the `data_json` of the end event of the attempt. Add no new column or counter.
- The wrapper counts `context_tokens(n)` events of the adapter (ticket 15). Only a CLI with no compaction setting uses this limit; for Claude and Codex the CLI setting compacts (tickets 13 and 14).
- The limit is 256000 (FR-9). Antigravity has no `compact_window`: `horch teammates --check` refuses a window key for a harness with no window setting. Keep the number in 1 constant in `context_limit.rs`, with a doc line that names FR-9.

## Steps

1. Write the tests first in `crates/horch-core/tests/p7_context_limit.rs`, with `fake-antigravity` and its `long_context` key: `agy_attempt_at_256000_tokens_ends_with_context_limit`, `context_limit_burns_no_attempt_and_moves_the_task_to_ready_at_once`, `next_attempt_gets_the_handoff_file_in_the_same_worktree`, `two_context_limits_with_no_new_commit_open_1_needs_you_item`, `two_context_limits_with_a_new_commit_go_on`, `claude_attempt_is_not_ended_by_the_wrapper_limit`. They fail before the code.
2. Write the code. Run the Check and the guard tests. Commit the exact files.

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
- No attempt of the tests is burned by `context_limit`.
