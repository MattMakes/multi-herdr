# 61: The e2e tests of features: FM-39, FM-41 and FM-42, and the gate test of a merge with ideas in flight

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** light
**Check:** `cargo test -p horch-e2e --test p6_features && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-e2e/tests/p6_features.rs`

Spec: `docs/specs/fleetd/design.md` (Failure modes FM-39, FM-41, FM-42; Migration plan, item 8 "Phase 6: the idea dispatcher and sub-orchestrators" (gate tests)).

## Goal

Each row has 1 e2e test through the binary, in a temporary repository with a bare remote.

## Requirements

- FM-39
- FM-41
- FM-42
- Gate test: a merge of the pull request of an `open` feature moves the feature to `merged` and its ideas in flight to a new feature

## Context

- Read `docs/tickets/fleetd-p6/MAP.md` first: the sections "Open questions", "Old flow and new" and "Decisions that hold".
- FM-41 is the gate test: merge the feature branch into the primary branch of the bare remote by hand while 1 idea is in `in_progress`. After 1 fetch period: the feature is `merged`, the idea is in a new feature with the same pane, each of its task branches has a rescue ref and lands on the new feature.
- FM-42: a stack of 2 features; merge the lower one; the upper one is rebased on the primary tip and its gates run again. Then the same with an abandon.
- FM-39: a cancelled task does not block its idea; the idea reaches `merged` and its rescued trees are removed.
- Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p5 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests: `fm41_merge_with_an_idea_in_flight_moves_it_to_a_new_feature`, `fm42_lower_feature_merged_rebases_the_upper`, `fm42_lower_feature_abandoned_rebases_the_upper`, `fm39_cancelled_task_does_not_block_its_idea`, `several_features_run_at_the_same_time`.
2. Run them. A failure is a fault of its ticket: send QUESTION with the name of the ticket.
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
