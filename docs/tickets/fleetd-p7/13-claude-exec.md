# 13: The Claude Code adapter in exec mode: the run-to-completion launch, no dialog, 256000 as the compaction window, and the usage-limit signal of the stream

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --lib harness::claude && cargo test -p horch-core --test p7_claude_exec && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/harness/claude.rs`, `crates/horch-core/tests/p7_claude_exec.rs`, `crates/horch-core/tests/fixtures/streams/claude-exec.jsonl`

Spec: `docs/specs/fleetd/design.md` ("Worker profiles and adapters" (How each adapter applies a profile, column Claude Code; Exec or interactive); "What herdr and the agent CLIs give us").

## Goal

The Claude adapter gives the exact launch of 1 exec attempt and reads its stream: each event is a liveness signal, and a usage-limit or outage event maps to the outcome `quota`.

## Requirements

- FR-9 (part): `CLAUDE_CODE_AUTO_COMPACT_WINDOW=256000` in the settings env.
- RR-11 in exec mode (the usage-limit row of the adapter table).
- FM-18 (part): `--permission-mode dontAsk` with explicit allow rules, and `--permission-prompts none` in exec.

## Context

- Read `docs/tickets/fleetd-p7/MAP.md` first: the sections "Open questions" and "Decisions that hold".
- Self-hosting danger: this phase changes the worker mode that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p6 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it (the text says "from ticket NN of fleetd-pN").
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`. No test uses the network: give each page through a local fixture. No ticket sends project text, a name or an email address to an outside service.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).
- The exec launch of the design: `claude -p --output-format stream-json --verbose --include-hook-events`. The prompt is the path of `inbox/START.md` (design, "Dispatch order matters"). Keep the `--settings` overlay and `--strict-mcp-config` of today (no `CLAUDE_CONFIG_DIR`).
- The stream forms come from the public docs of Claude Code (the `cli-reference` and `headless` pages of `docs-sources.toml`). Record each form that you parse, with its doc page, in the fixture `crates/horch-core/tests/fixtures/streams/claude-exec.jsonl`. The fake of ticket 16 emits the same lines. If the docs name no usage-limit event, send QUESTION with what the docs say.
- Give 2 functions to the wrapper (ticket 17): the exec launch spec of 1 attempt, and `fn read_event(line) -> Event` with the kinds `activity`, `usage_limited`, `result`, `context_tokens(n)` and `other`.
- The launch starts through the launcher (ticket 04). The interactive launch of today does not change.

## Steps

1. Write the tests first in `crates/horch-core/tests/p7_claude_exec.rs`: `claude_exec_launch_has_the_flags_of_the_design`, `claude_exec_never_waits_on_a_dialog`, `claude_exec_settings_set_the_256000_window`, `claude_stream_usage_limit_is_quota`, `claude_stream_tool_events_are_activity`. They fail before the code.
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
- The interactive launch oracles of Claude personas do not change.
