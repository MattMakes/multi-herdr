# 21: 256000 is the context of each seat: context-windows.md and the compact_window of each teammate file

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --test context_launch --test context_roster --test compaction --test baseline_oracles --test p7_context_256k && cargo test -p horch --test context_cli && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `teammates/`, `crates/horch-core/tests/oracles/`, `crates/horch-core/tests/golden/`, `crates/horch-core/tests/context_launch.rs`, `crates/horch-core/tests/context_roster.rs`, `crates/horch-core/tests/compaction.rs`, `crates/horch/tests/context_cli.rs`, `crates/horch-core/tests/p7_context_256k.rs`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 9 "Phase 7: docs sync and conformance, then exec mode" ("set `context-windows.md` to 256000 for every seat"); FR-9; "The numbers of the design" (Context of each agent)).

## Goal

Each new attempt of each seat gets 256000 as its native compaction window.

## Requirements

- FR-9: each agent runs with a context limit of 256k tokens by default, set through the CLI setting where one exists.
- The release row of Phase 7: "`context-windows.md` gives 256000 to new attempts."

## Context

- Read `docs/tickets/fleetd-p7/MAP.md` first: the sections "Open questions" and "Decisions that hold".
- Self-hosting danger: this phase changes the worker mode that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p6 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it (the text says "from ticket NN of fleetd-pN").
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`. No test uses the network: give each page through a local fixture. No ticket sends project text, a name or an email address to an outside service.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).
- Today `teammates/_base/context-windows.md` has `claude/orchestrator: 300000`, `claude/opus: 200000`, `claude/sonnet: 150000`, `codex/gpt-5.6-sol: 200000`, `codex/gpt-5.6-terra: 150000`. Set each to 256000.
- A teammate `compact_window` wins over that file. Today 93 teammate files set it (`rg -c "^compact_window" teammates/`): 17 at 150000, 74 at 200000, 2 at 300000. Set each to 256000, and the example line of `teammates/_template.md` too.
- The threshold that horch watches is `min(300000, floor(0.8 x native trigger))` = 204800. `horch teammates --check` fails a window that leaves less than 20000 tokens: 256000 leaves 51200.
- The tests and oracles that pin a window: `rg -ln "AUTO_COMPACT_WINDOW|compact_window|context-windows" crates/*/tests`. Update each expected value to 256000, and nothing else.
- This ticket changes data, not code. If a code change is needed, send QUESTION.

## Steps

1. Write the test first in `crates/horch-core/tests/p7_context_256k.rs`: `launch_environment_sets_256k_context_limit` (each Claude and Codex worker persona of the repository roster gets 256000 in its launch environment or config), `every_window_of_context_windows_is_256000`. They fail before the change.
2. Change the values. Update the pinned tests and oracles.
3. Run the Check and the guard tests. Commit the exact files.

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
- `rg -n "^compact_window" teammates/` shows only the value 256000.
