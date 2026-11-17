# 35: A merge with ideas in flight: fleetd records the feature as merged and moves each idea in flight to a new feature, with a rescue ref and a rebase of each task branch

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test merged_with_ideas && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/integration/application/merged_with_ideas.rs`, `crates/horch-core/tests/merged_with_ideas.rs`

Spec: `docs/specs/fleetd/design.md` (Features (A merge with ideas in flight); Ledger I7, I13; Failure mode FM-41).

## Goal

Use case `MoveIdeasToNewFeature`: no idea is left without a feature and an owner, and no branch lands on the branch of a `merged` feature.

## Requirements

- FM-41
- A merge with ideas in flight

## Context

- Read `docs/tickets/fleetd-p6/MAP.md` first: the sections "Open questions", "Old flow and new" and "Decisions that hold".
- Trigger: the feature is `open`, and git shows its changes in the primary branch by content at the fetched primary tip (I8, Phase 4).
- In 1 transaction: the feature -> `merged`; 1 new feature row with its internal run and its branch row (ticket 15), from the fetched primary tip; each idea of the old feature that is not `integrating`, `cancelled`, `parked` or `merged` -> the new feature; its open questions -> the new owner; the sub-orchestrator attempt and its pane stay with the idea.
- Then, as effects after the commit, keyed by their rows: the new branch ref; for each task branch of a moved idea that is not `landed` or `abandoned`, a rescue ref of its tip (Phase 4, I10), then a `rebase` job onto the new feature branch. A branch in `in_bundle` of the old feature: its bundle is `dropped` first, and the branch goes back to `ready` on the new feature.
- A bundle of the old feature never moves the old feature ref after the merge (I9 compare-and-set fails).
- Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p5 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `open_feature_merged_by_content_moves_ideas_in_flight_to_a_new_feature`, `moved_idea_keeps_its_sub_orchestrator_and_pane`, `each_task_branch_of_a_moved_idea_gets_a_rescue_ref_before_its_rebase`, `crash_between_commit_and_rebase_loses_no_tip`, `no_branch_lands_on_a_merged_feature`, `idempotent_pull_request_merged` (the same merge seen on 2 passes moves the ideas 1 time). They fail because the use case is empty.
2. Write the use case.
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
