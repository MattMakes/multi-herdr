# 10: fleetd launches each attempt that it bound: the spawn after the commit, through the launcher, with stops before launches

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd::task_execution::application::launch && cargo test -p horch --bin horch -- recipes && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/task_execution/application/launch.rs`, `crates/horch/src/cmd/recipes.rs`

Spec: `docs/specs/fleetd/design.md`, "The reconcile pass" (the row first, then the effect, then the observation), FM-11, RR-8, SR-7, "The task state machine" (`leased`).

## Goal

Each `launching` attempt of push dispatch is launched 1 time after its commit: its pane opens with `horch spawn <persona>` for that attempt, through the launcher. A crash before or after the launch leaves exactly 1 process of the attempt.

## Requirements

- FM-11 and RR-8 for a launch: before a launch, fleetd looks for a live process of the attempt id; stops come before launches.
- SR-7: the spawn starts through the launcher (`crates/horch-core/src/fleetd/worker_profiles/adapters/launcher.rs`), which removes `ANTHROPIC_API_KEY`.
- An attempt whose worktree is `reserved` launches only after its `worktree_add` job passed; a failed job ends the attempt `crashed` (rung 6).

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- Follow the restart of Phase 2 ticket 11 (`docs/tickets/fleetd-p2/11-waiting-quota-and-fallback.md`): an attempt in `launching`, then the respawn path of `crates/horch/src/cmd/recipes.rs` (`respawn_orchestrator`, `respawn_pane`) in part 3 of the pass. Here the spawn names the bound attempt with the hidden flag of ticket 09: `horch spawn <persona> --attempt <id>`.
- The pane goes to the workspace of the run (`runs.workspace_id`).
- `application/launch.rs` decides which attempts to launch (pure on the rows) and records the effect; `recipes.rs` holds the herdr side.
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `launching_attempt_is_launched_once_after_the_commit`, `crash_after_commit_before_launch_launches_once_on_the_next_pass`, `crash_after_launch_does_not_launch_again`, `reserved_tree_waits_for_its_worktree_add_job`, `failed_worktree_add_ends_the_attempt_crashed`.
2. Write the decision and the effect.
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

The Check passes. fleetd launches each bound attempt 1 time, after the commit, through the launcher.
