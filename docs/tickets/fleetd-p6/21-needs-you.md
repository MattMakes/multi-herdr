# 21: The needs-you list in /fleet/needs-you/ and as a herdr notification: 1 file for each open item, removed when the item ends

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --test needs_you_folder && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/ideas/adapters/needs_you_folder.rs`, `crates/horch-core/src/fleetd/ideas/adapters/notify.rs`, `crates/horch-core/tests/needs_you_folder.rs`

Spec: `docs/specs/fleetd/design.md` (Ideas (The needs-you list has a life cycle); Requirements OR-3; Failure modes FM-29, FM-35).

## Goal

Each open item of the `needs_you` table has 1 file in `<fleet root>/needs-you/`, and a new open item gives 1 herdr notification.

## Requirements

- OR-3 (the folder and the notification; `horch ideas` is ticket 24)

## Context

- Read `docs/tickets/fleetd-p6/MAP.md` first: the sections "Open questions", "Old flow and new" and "Decisions that hold".
- The table and its life cycle exist from Phase 1 (`crates/horch-core/src/fleetd/ideas/application/needs_you.rs`). Do not change them.
- The folder is a mirror: 1 file `<kind>-<row>.md` for each open item, written atomically; a file whose item is not open is removed. The mirror runs at each pass with a cursor.
- The herdr notification (MAP.md, Q12): fleetd reports the agent state `blocked` on the pane `fleet-needs-you` through `report_agent` of `crates/horch-core/src/workspace/herdr.rs`, with the count of open items. herdr shows its toast for a pane that needs input. A herdr that is not running gives no error, only 1 log line.
- Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p5 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `open_item_has_1_file`, `resolved_item_file_is_removed`, `same_escalation_on_2_passes_gives_1_file`, `new_open_item_reports_1_notification`, `no_herdr_is_not_an_error`. They fail because the adapters are empty.
2. Write the 2 adapters.
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
