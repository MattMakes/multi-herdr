# 09: horch spawn binds its task before the pane, CLAIM_TASK goes away, and the claim and the finish stay only for an attempt that began with a claim

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch --bin horch -- taskledger_spawn spawn && cargo test -p horch-core --test fr5_push_only && cargo test -p horch-core --lib taskledger::tasks && cargo test -p horch --test w3_ledger_cli_pins && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch/src/cmd/taskledger_spawn.rs`, `crates/horch/src/cmd/spawn.rs`, `crates/horch/src/cmd/taskledger.rs`, `crates/horch-core/src/taskledger/tasks.rs`, `crates/horch/tests/w3_ledger_cli_pins.rs`, `crates/horch-core/tests/fr5_push_only.rs`

Spec: `docs/specs/fleetd/design.md`, "Migration plan" item 6 (`horch spawn` binds a task before the pane exists; `CLAIM_TASK` goes away), "What a release does to a run in flight" (row Phase 4), FR-5, FM-2.

## Goal

`horch spawn` of a worker in a ledger run calls `bind` of ticket 08 before it opens the pane. The first line typed into the pane is the fixed doorbell line, not `CLAIM_TASK`. `horch ledger claim` searches no task for a new attempt. An attempt that began with a claim before the update ends by the old path.

## Requirements

- FR-5: no code path where an agent queries the ledger for open tasks. The test `crates/horch-core/tests/fr5_push_only.rs` reads the source and fails when a caller other than `bind` calls the choice of a task (`pick_task` or its replacement).
- The release row: `horch ledger claim` and the old finish work for an old attempt only (`attempts.via = claim`). For a push attempt, `claim` prints its `task.yaml` and `START.md` paths and the ticket snapshot (no search), `finish` and `done` refuse with the line "run `horch result` when the work is committed; fleetd runs the finish steps".
- A spawn with no ready task binds nothing and opens no pane: it prints the cause (no ready task, or the hold with its cause) and exits 1.
- `horch spawn` gets the hidden flag `--attempt <attempt id>`: it opens the pane of an attempt that fleetd already bound (ticket 10 uses it) and binds nothing.

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- Today: `CLAIM_TASK` in `crates/horch/src/cmd/taskledger_spawn.rs:22`, used at `taskledger_spawn.rs:850` and `crates/horch/src/cmd/spawn.rs:191`; `CLAIM_TARGET_ENV` names a target task (TRF-24): keep it as the target of `bind`. `claim` is `tasks::claim` (`crates/horch-core/src/taskledger/tasks.rs:937`).
- The first line: the doorbell constant of Phase 2 (`DOORBELL`, Phase 2 Q19: "Read inbox/START.md in your task folder and continue."), with the absolute path of `START.md`, because a pane has no task folder as cwd. Choice Q8: `Read <path>/inbox/START.md and continue.` is 1 fixed format with 1 path.
- Reviewer spawns (`spawn_reviewers`, `taskledger_spawn.rs:442`) bind their review task as a target in the same way.
- `crates/horch/tests/w3_ledger_cli_pins.rs` pins the CLI text of the ledger commands; change the pins of `claim`, `finish` and `done` for a push attempt, and keep the pins of a claim attempt.
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: in `taskledger_spawn.rs`, `spawn_binds_the_task_before_the_pane` and `spawn_with_no_ready_task_opens_no_pane`; in `tasks.rs`, `claim_of_a_push_attempt_searches_nothing` and `claim_attempt_keeps_the_old_path`; in `w3_ledger_cli_pins.rs`, the pins of `finish` and `done` of a push attempt; `fr5_push_only.rs`.
2. Change `prepare` to call `bind`, remove `CLAIM_TASK`, and change `claim`, `finish` and `done` for a push attempt.
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

The Check passes. `CLAIM_TASK` is gone. A spawned worker has its task bound and its `task.yaml` written before its pane opens. A claim attempt of Phase 3 still finishes by the old path.
