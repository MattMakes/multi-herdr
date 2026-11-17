# 28: Sub-orchestrators: 1 for each feature that is not merged or abandoned, a fresh process for each phase, started when missing and stopped when extra

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test sub_orchestrators && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/ideas/application/sub_orchestrators.rs`, `crates/horch-core/tests/sub_orchestrators.rs`

Spec: `docs/specs/fleetd/design.md` (Ideas (How many sub-orchestrators; A fresh process for every phase); Ledger I7, I13; Supervision (fleetd supervises each sub-orchestrator pane); Failure modes FM-16, FM-31).

## Goal

Use case `StartSubOrchestrator` and the I7 and I13 checks: each feature has 1 owner process when it has work, never 2, and 1 process never plans 2 ideas.

## Requirements

- FR-2 (1 sub-orchestrator for each feature; 1 idea for each sub-orchestrator attempt)
- I7 (the live part)
- I13
- FM-16
- FM-31

## Context

- Read `docs/tickets/fleetd-p6/MAP.md` first: the sections "Open questions", "Old flow and new" and "Decisions that hold".
- An attempt of a sub-orchestrator: `task_id` null, `idea_id` the idea of its phase, `worker` idea-lead, pane `idea-NNNN` of the feature. It starts through the launcher of Phase 1 after its row is committed in `launching` (SR-7, decision "a job starts only through the launcher").
- Its first instruction is the argument of its launch, not typed text (FR-7): the idea id, the phase and the idea folder.
- A fresh process for each phase: when the idea of the attempt changes state, fleetd stops the old process (polite stop of Phase 5, then the stop of its process group) and starts a new one for the next phase in the same pane.
- Owner process (MAP.md, Q11): the feature needs a live process when an idea of it is in `clarifying`, `specifying` or `ticketing`, or has an open question, or a failed task waits for its replan. Else the process may stop (ticket 29). I13 holds when the feature has its owner, live or stopped by rule.
- Extra process: a live sub-orchestrator attempt of a feature that has another live one, or of a `merged` or `abandoned` feature, is stopped. fleetd stops a process and closes a pane only with a fact that proves it is its own (the attempt row, PID and start time); else 1 needs-you item.
- A dead process mid-phase: the next pass starts a fresh one in the same phase, which resumes from the folder.
- Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p5 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `i7_missing_sub_orchestrator_is_started_extra_is_stopped`, `i7_two_ideas_of_1_feature_share_1_live_sub_orchestrator`, `i13_open_feature_without_sub_orchestrator_gets_one`, `each_phase_gets_a_fresh_process`, `dead_sub_orchestrator_restarts_in_the_same_phase`, `sub_orchestrator_of_a_merged_feature_is_stopped`, `an_unproven_pane_is_not_closed`. They fail because the use case is empty.
2. Write the use case and the 2 checks.
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
