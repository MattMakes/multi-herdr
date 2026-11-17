# 25: The landing state machine of a task branch: it follows its task, each state has a time limit, a cancelled task's branch is rescued and abandoned, and a detached old finish blocks landing

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd::integration && cargo test -p horch-core --test landing_states && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/integration/domain/task_branch.rs`, `crates/horch-core/src/fleetd/integration/application/landing.rs`, `crates/horch-core/tests/landing_states.rs`

Spec: `docs/specs/fleetd/design.md`, "Landing a task branch on its feature branch" (the state table; F9, F7, F5, L6), "A cancelled task does not block its idea", "What a release does to a run in flight" (row Phase 4).

## Goal

Each task branch of a push attempt has a landing state that follows its task: `building` while the task works, `verifying` when the task is `verifying`, `ready` when the task is `done`. A cancelled task's branch gets a rescue ref, then `abandoned`. Each state has its due time on the fleet clock, and past it 1 needs-you item opens.

## Requirements

- The exits of `building` and `verifying` of the design table, through `transition()`.
- Time limits: `ready` 30 min (`landing_ready_limit_s`), `conflicted` and `gate_failed` 60 min (`landing_repair_limit_s`), `in_bundle` the limits of the build job and the fast gate job plus `landing_job_margin_s`; past it, 1 needs-you item for the row, keyed by the branch.
- F9: a task cancelled at any point before the ref move lands nothing (ticket 31 checks it again just before the compare-and-set).
- Release row: a detached finish of a claim attempt that runs ends by the old path, and fleetd starts no landing of that task until it ends (the `finish-run.json` of `crates/horch-core/src/taskledger/finish/detach.rs` is live). Such a task lands 1 time.

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- The branch row exists before the branch (ticket 08 writes it with the bind of the first attempt of a task, through the rows store of ticket 06; if ticket 08 did not, write it here when the first push attempt of the task is bound, and say so in the summary).
- The rescue ref of a cancel comes from ticket 24.
- The function that the pass calls: `fn follow(tx, clock, budget) -> Vec<Effect>`.
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/landing_states.rs`: `branch_follows_its_task_to_verifying_and_ready`, `failed_check_with_a_new_attempt_moves_verifying_back_to_building`, `cancelled_task_branch_gets_a_rescue_ref_then_abandoned`, `ready_branch_past_30_min_opens_1_needs_you_item`, `detached_old_finish_blocks_landing_until_it_ends`, `claim_attempt_task_lands_1_time`.
2. Write the state rules and `follow`.
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

The Check passes. Each task branch has 1 landing state, 1 owner and 1 time limit, and none waits silently.
