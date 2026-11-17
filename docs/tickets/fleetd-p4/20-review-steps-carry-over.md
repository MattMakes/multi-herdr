# 20: The review steps carry over unchanged: code-sweep rounds, findings, the answers file, waivers, complexity-sweep, recheck and the verification profiles, run by the verifier

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd::task_execution::application::review && cargo test -p horch-core --test verifier_review && cargo test -p horch-core --lib taskledger::finish && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/task_execution/application/review.rs`, `crates/horch-core/src/fleetd/task_execution/application/verifier.rs`, `crates/horch-core/tests/verifier_review.rs`, `crates/horch-core/src/taskledger/finish/mod.rs`

Spec: `docs/specs/fleetd/design.md`, "Done for each kind of task" (Code kind), D6, RR-18 (a review requeue keeps its own counter), `docs/specs/fleetd/preservation-04-behavior.md` rows F1, F2, F5, F12, `preservation-05-contracts-and-values.md` row V-14.

## Goal

After the check passes, the verifier runs the steps of the run's verification profile that come before `land`, in the run's order, with today's code: `code-sweep` (reviewer tasks, rounds, `F<n>` ids, the answers file, waivers), `complexity-sweep` and `recheck`. When each step passed or is not applicable, the task moves to `done` (V2 of the verification canvas). A finding that needs work sends the task back for a fix attempt by a review requeue with its own counter.

## Requirements

- V-14 (fixed contract): code-sweep `MAX_REQUEUES` 2 (`crates/horch-core/src/taskledger/finish/codesweep.rs:280`); review rounds light 1, full 3.
- `code_kind_needs_review_pass_with_at_most_2_requeues`.
- F1: profiles full (7 steps), light (the default), minimal; the order of the run; `land`, `pr` and `cleanup` are not steps of the verifier (land is a step of landing).
- RR-18: a requeue by a review keeps its own counter and does not change the attempt count.
- Each step that runs a command (complexity-sweep, recheck) is a `task_check` job run keyed by the tip and the hash of the step and its command, so the pass never waits.

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- "Unchanged" means: call the functions of `crates/horch-core/src/taskledger/finish/` (`codesweep.rs`, `complexity.rs`, `land::check`), do not copy them. Make in `finish/mod.rs` only the change that lets the verifier call 1 step for 1 task with no worker in the foreground (for example a `Ctx` built by fleetd); keep `finish` of a claim attempt as it is.
- Reviewer tasks are tasks: push dispatch (tickets 08 and 10) dispatches them; the code-sweep step waits for them as today (`reviews_in_for`).
- A complexity-sweep that names functions: the outcome `rejected` with the list in `verdict.json`, and the next attempt gets the list in its `START.md` (ticket 07 extra section). Choice Q11.
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/verifier_review.rs`: `light_profile_runs_check_and_complexity_sweep_then_done`, `full_profile_runs_code_sweep_rounds`, `code_kind_needs_review_pass_with_at_most_2_requeues`, `findings_keep_their_ids_across_rounds`, `review_requeue_keeps_the_attempt_count`, `answered_important_finding_passes`, `land_pr_and_cleanup_are_not_verifier_steps`, `claim_attempt_finish_is_unchanged`.
2. Write `application/review.rs`, call it from `application/verifier.rs` after the check job passed, and make the small change in `finish/mod.rs`.
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

The Check passes. A code task of a push attempt reaches `done` only after its check and today's review steps passed, with the limits of V-14.
