# 18: The reconciler acts: it queues the rescue of each orphan and the removal of each landed tree, after the tests of RR-13 to RR-18 are green

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-e2e --test p3_refusals --test p3_triage --test p3_unlanded && cargo test -p horch-core --lib fleetd::worktree_custody && cargo test -p horch-core --test worktree_reconcile --test worktree_reconcile_acting --test worktree_disk --test fleetd_pass_p3 && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/worktree_custody/application/reconcile.rs`, `crates/horch-core/src/fleetd/worktree_custody/application/disk.rs`, `crates/horch-core/src/fleetd/worktree_custody/domain/worktree.rs`, `crates/horch-core/tests/worktree_reconcile.rs`, `crates/horch-core/tests/worktree_reconcile_acting.rs`

Spec: `docs/specs/fleetd/design.md`, "Migration plan" item 5 ("The reconciler runs in report-only mode, and lists the orphans already on your disk, until the tests of RR-13 to RR-18 are green. Only then does it act."), "Preservation analysis" ("The worktree reconciler stays in report-only mode until these tests are green, so no cleanup of Phase 3 runs before they hold."), "Worktrees: leased, reconciled, never lost" (the rows `orphaned`, `landed` and `rescued`; "Rescue before any cleanup"), FM-25, FM-27, FM-28.

## Goal

The reconciler leaves report-only mode. In each pass it queues 1 `rescue` job for each `orphaned` tree with no verified rescue ref, and 1 `worktree_remove` job for each `landed` tree, and the disk budget queues its removals. The jobs run outside the pass; the guard of ticket 07 checks each tree again at its removal.

## Requirements

- **FM-25** (the act): a verified rescue ref is written for each orphan before anything else.
- **FM-27**: fleetd removes only `landed` trees or trees with a verified rescue ref.
- **RR-9**, **RR-10** (the act of the reconciler).

## Context

- Read `docs/tickets/fleetd-p3/MAP.md`, section "Open questions", first (Q8 and Q9 apply here).
- The rule of the design: first, run the Check of this ticket before any change. Its first command runs the tests of RR-13 to RR-18 (`p3_refusals.rs` of ticket 12, `p3_triage.rs` of ticket 13, `p3_unlanded.rs` of ticket 14). If 1 of them fails, stop and send BLOCKED with the failing test. Do not turn on the act while 1 is red.
- Today (after ticket 09): `reconcile_worktrees` reports `would_rescue` and `would_remove`, and `disk_budget` (ticket 16) reports its trees, and they queue nothing.
- Change: `would_rescue` -> `jobs::queue` of 1 `rescue` run (subject the worktree id, input key the path and HEAD; ticket 06); `would_remove` -> `queue_remove` (ticket 07); the disk decision -> `queue_remove` for each of its trees. A row with an active job of that kind is not queued again (the index `jobs_one_active`). A rescue run that was `abandoned` follows its backoff and its limit (the rule of the orchestrator, Q25). Remove the report-only branch and its events; `horch worktrees` (ticket 15) then shows the jobs of each tree instead of the marks.
- The 2 tests of tickets 09 and 16 that pin report-only mode (`report_only_queues_no_rescue_and_no_remove_job`, `report_only_mode_queues_no_removal_for_space`) change into tests of the act. Name the change in the commit message.

## Steps

1. Run the first command of the Check. It must pass before you change code.
2. Write the tests first, in `crates/horch-core/tests/worktree_reconcile_acting.rs`. Each fails before the code exists:
   - `orphan_gets_1_rescue_job_and_becomes_rescued_after_its_verification`.
   - `orphan_with_dirty_untracked_files_and_commits_keeps_each_one_after_the_pass`.
   - `landed_tree_gets_1_remove_job_and_becomes_removed`.
   - `rescued_tree_of_an_active_run_is_not_removed`.
   - `rescue_job_is_queued_once_while_it_runs`.
   - `failed_rescue_leaves_the_tree_orphaned_and_queues_no_removal`.
3. Write the code. Run the Check. Commit.

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

Self-hosting: each test makes its own temporary git repository and temporary folders (build rule 1). No test and no worker runs the reconciler, a rescue, a cleanup or a `git worktree remove` on the repository `/Users/MASCOTT/projects/mh/multi-herdr` or on its `.worktrees/` folder.

## Done when

The Check passes. The tests of RR-13 to RR-18 were green before the change, and each pass now queues the rescue of each orphan and the removal of each landed tree, and of nothing else.
