# 24: horch ideas and horch idea show: the board of every idea, its state and what it waits on, the open needs-you items, and the diffs of the features in flight

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch --test p6_ideas_board && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/ideas/application/queries.rs`, `crates/horch/src/cmd/idea/board.rs`, `crates/horch/src/cmd/idea/show.rs`, `crates/horch/tests/p6_ideas_board.rs`

Spec: `docs/specs/fleetd/design.md` (Ideas (`horch ideas`; Quick approval); Between features (Build on what is in flight); Requirements OR-3).

## Goal

You read every idea in 1 board, and `horch idea show` prints what you approve.

## Requirements

- OR-3 (`horch ideas` shows only open needs-you items)
- Build on what is in flight (the diffs for planning)

## Context

- Read `docs/tickets/fleetd-p6/MAP.md` first: the sections "Open questions", "Old flow and new" and "Decisions that hold".
- Read models `IdeaBoard`, `InterviewRecord`, `InFlightDiffs`, `ApprovedAssumptionsAndSpec` (`ddd/08-code/ideas/design.md` section 4). The command opens the ledger read-only (Phase 5).
- The board: 1 line per idea that is not `merged` and not `cancelled`: id, feature, state, what it waits on (you, a question, a task, the gate), and since when. Then "Needs you": each open needs-you item.
- `horch idea show <id>`: the idea text, the amendments, `assumptions.md` with its sha256, the state, the plan verdict. `--in-flight` prints the changed files and the diffs of each other feature in flight against the primary branch (MAP.md, Q13). The sub-orchestrator runs it at plan time.
- What the user reads keeps the words of today; a new state word names its entity (`idea awaiting_approval`).
- Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p5 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `ideas_board_lists_each_idea_and_what_it_waits_on`, `ideas_board_shows_only_open_needs_you_items`, `idea_show_prints_the_assumptions_and_their_sha256`, `idea_show_in_flight_lists_the_changed_files_of_other_features`. They fail because the commands are stubs.
2. Write the queries and the 2 commands.
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
