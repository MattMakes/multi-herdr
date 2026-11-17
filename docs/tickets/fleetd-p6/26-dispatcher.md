# 26: The dispatcher loop: each pass moves each idea to its next state when its files validate, and the ideas of 1 feature are planned 1 at a time

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test idea_dispatcher && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/ideas/application/dispatcher.rs`, `crates/horch-core/tests/idea_dispatcher.rs`

Spec: `docs/specs/fleetd/design.md` (Ideas (The idea state machine; How many sub-orchestrators); Requirements FR-2; Failure mode FM-8).

## Goal

Deterministic code moves each idea through the state machine. No AI decides what happens next to an idea.

## Requirements

- FR-2 (property test over the idea state machine)
- FM-8
- A28 (the ideas of 1 feature are planned in order)

## Context

- Read `docs/tickets/fleetd-p6/MAP.md` first: the sections "Open questions", "Old flow and new" and "Decisions that hold".
- The moves: `captured` -> `clarifying` (or `ticketing` for `--direct`) unless another idea of its feature is in `clarifying`, `specifying` or `ticketing`; `clarifying` -> `awaiting_approval` when `assumptions.md` validates (and its feature has no branch: ask `NameFeatureBranch`, ticket 15); `specifying` -> `ticketing` when `spec.md` and `requirements.yaml` validate; `ticketing` -> `in_progress` through `ValidatePlan` (ticket 14). The approval move is ticket 18.
- Each move goes through the door with a cause. The loop reads the ideas in the work budget of the pass with a cursor (`pass_cursors`, Phase 1).
- The dispatcher never reads the content of an idea for a decision: it only checks that files exist, validate and carry your approval (design: Idea dispatcher, Never does).
- Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p5 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `second_idea_of_a_feature_waits_in_captured_until_the_first_leaves_ticketing`, `ideas_of_2_features_move_at_the_same_time`, `clarifying_moves_to_awaiting_approval_only_with_valid_assumptions`, `specifying_moves_to_ticketing_only_with_cited_requirements`, `property_no_sequence_of_passes_puts_2_ideas_of_1_feature_in_planning` (a property test over random file events). They fail because the loop is empty.
2. Write the loop.
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
