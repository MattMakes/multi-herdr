# 40: horch feature abandon: a rescue ref for each branch that is not landed, each such branch abandoned, each task that is not terminal cancelled

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch --bin horch -- featurecmd && cargo test -p horch-core --test feature_abandon && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch/src/cmd/featurecmd.rs`, `crates/horch-core/src/fleetd/integration/application/commands.rs`, `crates/horch-core/tests/feature_abandon.rs`

Spec: `docs/specs/fleetd/design.md`, "The feature state machine" (`abandoned`), "A cancel", I10, FM-43 (the exit of a closed pull request).

## Goal

You run `horch feature abandon <feature>`. fleetd writes a verified rescue ref of each branch of the feature that is not landed, moves each such branch to `abandoned`, cancels each task of the run that is not terminal (the cancel of Phase 2: the stop of a live attempt first), and moves the feature to `abandoned`.

## Requirements

- No work is lost: each unlanded branch has its rescue ref before it moves.
- The order is crash safe: rescue refs, then branches, then tasks, then the feature; a crash at any point completes on the next pass.

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- Add `abandon_feature` to `integration/application/commands.rs` beside the branch abandon of ticket 35, and reuse its rescue-then-abandon step.
- The task cancel is Phase 2's (`crates/horch-core/src/fleetd/task_execution/application/cancel.rs`).
- Operator-only command: before Phase 5 there is no command channel. This command keeps the client-side role check of today: the caller is found as in `waive` (`crates/horch/src/cmd/taskledger.rs`, `worker_id_opt`, then `finish::codesweep::worker_in_pane`), and a caller that is a worker and not the orchestrator is refused. The server-side operator check (SR-8) is Phase 5.
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `abandon_rescues_each_unlanded_branch_first`, `abandon_cancels_each_open_task_after_its_stop`, `landed_branches_stay_landed`, `worker_caller_is_refused`, `crash_in_the_middle_completes_on_the_next_pass`.
2. Write the command.
3. This ticket moves a ref, lands, pushes, deletes a branch or a ref, or changes who runs the finish steps. It has a crash test: a fault point before the commit of the intended row, and after the commit and before the effect. In each case the next pass completes or fails the effect 1 time, and no tip and no work is lost.
4. Run the Check. Commit the exact files.

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

The Check passes. An abandoned feature loses no work: each unlanded branch has a rescue ref.
