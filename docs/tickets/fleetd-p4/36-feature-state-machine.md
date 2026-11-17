# 36: The feature state machine of the 1 feature of a run: open, ready after the full gate, merged by content at the fetched primary tip

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd::integration && cargo test -p horch-core --test feature_states && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/integration/domain/feature.rs`, `crates/horch-core/src/fleetd/integration/application/features.rs`, `crates/horch-core/tests/feature_states.rs`

Spec: `docs/specs/fleetd/design.md`, "The feature state machine", "Between features" (The full gate; Merged), I8, "Migration plan" item 6 (In Phases 4 and 5, each run is 1 feature row).

## Goal

The feature of a run is `open`. It moves to `ready` when each task of the run is `done` or `cancelled`, each task branch is `landed` or `abandoned`, and the full gate passed on its tip over the fetched primary tip. It moves back to `open` when its tip or the fetched primary tip moves. It is `merged` when git shows its changes in the fetched primary tip by content (or the local primary branch with `remote = none`).

## Requirements

- I8: a feature is `merged` only when its changes are in the target by content; fleetd decides it 1 time and does not check it again.
- The full gate runs once as a job when the branches are landed; a pass on a tip that moved is stale.

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- Phase 4 has no ideas: the condition "each idea is `integrating`, `cancelled` or `parked`" is "each task of the run is `done` or `cancelled`" (Q20).
- The fetched tips come from `remote_tips` (ticket 37). The pull request is ticket 39; abandon is ticket 40.
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `feature_is_ready_after_its_branches_landed_and_the_full_gate_passed`, `moved_primary_tip_makes_the_full_gate_stale_and_the_feature_open`, `squash_merged_feature_is_merged`, `merged_is_decided_once`, `remote_none_reads_the_local_primary_branch`.
2. Write the rules and the application.
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

The Check passes. A run's feature reaches `ready` only after its full gate, and `merged` only by content.
