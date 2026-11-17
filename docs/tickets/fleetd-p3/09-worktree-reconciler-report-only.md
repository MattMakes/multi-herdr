# 09: The worktree reconciler in report-only mode: adopt, mark orphaned, observe the jobs, and check I6

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd::worktree_custody && cargo test -p horch-core --test worktree_reconcile && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/worktree_custody/domain/worktree.rs`, `crates/horch-core/src/fleetd/worktree_custody/application/reconcile.rs`, `crates/horch-core/tests/worktree_reconcile.rs`

Spec: `docs/specs/fleetd/design.md`, "Worktrees: leased, reconciled, never lost" (the state table; "The reconciler compares three views" and its 4 rules), "Ledger" (rule 3, I6), "Supervision, liveness and reconciliation" (the pseudo code of the pass: `reconcile_worktrees(tx, trees)`; "Stops come before launches"), "Migration plan" item 5 ("The reconciler runs in report-only mode, and lists the orphans already on your disk, until the tests of RR-13 to RR-18 are green. Only then does it act."), RR-10, FM-25, FM-26, FM-27; `ddd/08-code/worktree-custody/design.md` §2, §3 (`ReconcileWorktrees`) and §9.

## Goal

In part 2 of each pass, the reconciler compares the `worktrees` rows, the worktree folders that the scan of part 1 saw, and the last result of the `worktree_list` job. It writes rows: it adopts a folder with no row as `orphaned`, marks a row with a dead owner `orphaned` in the same pass, observes the worktree jobs, escalates a folder that vanished, and checks I6. In this ticket it is in report-only mode: it queues no rescue job and no remove job. Ticket 18 turns those on.

## Requirements

- **RR-10**: the reconciler compares the ledger with the work directory, within the scan budget of each pass, and with `git worktree list` through the `worktree_list` job, and adopts what the ledger does not own.
- **I6**: every worktree that `git worktree list` or the work directory shows has a row; every row that is not `removed` has its folder; no worktree is removed unless its branch landed or its rescue ref was verified.
- **FM-25** (the mark): owner death marks the tree `orphaned` in the same pass.
- **FM-26** (the adoption): a tree made outside fleetd is adopted as `orphaned`.
- **FM-27** (the escalation): a folder that vanishes without a removal by fleetd opens 1 needs-you item.

## Context

- Read `docs/tickets/fleetd-p3/MAP.md`, section "Open questions", first (Q4, Q8, Q9, Q10, Q11, Q15 and Q23 apply here).
- The pure decisions, in `domain/worktree.rs`: input values (the rows, the folder facts, the list result, the state of each owner attempt and task), output a list of decisions. 1 function for each rule:
  - A folder or a `git worktree list` entry with no row (not the main checkout, not a path of a row) -> adopt as `orphaned`: a new row through `insert_orphan` (ticket 03), `origin` `native`, its task from the branch name when it is `<run branch>-T<seq>-<name>` of a run of the ledger, else no task (Q15).
  - A row in `active` whose owner attempt is `ended` (Phase 1 `attempts.state`) -> by the outcome of that attempt (Q10): the task is parked by `done --unlanded` or is `ready` again after a verified exit of no work -> `created`; each other outcome (`crashed`, `killed`, `stalled`, `timed_out`, `no_result`, or an attempt with no outcome) -> `orphaned`. A row in `active` with no owner attempt and whose worker is not live (a test that plays the agent, Q23) -> `orphaned`. A cancelled task -> `orphaned`.
  - A row in `reserved` with a passed `worktree_add` run and its folder -> call the observation of ticket 08. A failed run -> the drop of ticket 08. A row in `reserved` with no job (a crash between the 2 writes cannot happen, because they are 1 transaction; check and assert it).
  - A row that is not `reserved` or `removed` and whose folder the scan saw as missing (and the list does not show) -> 1 needs-you item `worktree_vanished:<worktree id>` and the event; the row stays (FM-27).
  - A rescue run or a remove run that ended -> call the observations of tickets 06 and 07.
  - A `worktree_list` result -> `set_facts` for each row it names (HEAD, dirty, size, `seen_at`) (Q11).
  - Report-only (Q9): a row in `orphaned` with no rescue ref, and a row in `landed`, are reported, not queued. The decision returns them as `would_rescue` and `would_remove`, and `application/reconcile.rs` writes 1 event of each for each row 1 time (not on each pass). Ticket 15 shows them in `horch worktrees`.
- `application/reconcile.rs`: `reconcile_worktrees(tx, facts, clock) -> Report`, which reads the rows, calls the domain, and writes each decision through the store and `transition()`. It never runs git and never touches a folder. It is level-triggered: a second call with the same facts writes nothing more.
- I6, in `application/reconcile.rs`: `check_i6(tx, facts) -> Vec<Breach>` for the invariant check of the pass. A breach is reported, and the rule that repairs it acts on the next pass.
- The facts come from part 1 of the pass: the folder scan of Phase 1 (`scan_worktree_folders`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/scans.rs`, which reads each row path with a cursor and a time budget), the list of folders under `<project>/.worktrees/` of each run that is not `done` or `abandoned` (Q4), and the last `passed` `worktree_list` run of each repository. Ticket 10 wires them into the pass. Here, the tests give the facts as values.
- The owner and the outcome of an attempt: `attempts` (Phase 1 `fleetd/task_execution`). Read them; do not change `task_execution`.

## Steps

1. Write the tests first, in `crates/horch-core/tests/worktree_reconcile.rs` and in the module tests of `domain/worktree.rs`. Each fails before the code exists:
   - `i6_worktree_on_disk_without_row_is_adopted_not_deleted`.
   - `owner_death_marks_the_tree_orphaned_in_the_same_pass`.
   - `parked_attempt_leaves_the_tree_created_with_no_owner`.
   - `reserved_row_left_by_crash_is_created_or_dropped_next_pass`.
   - `vanished_folder_opens_1_needs_you_item_and_keeps_the_row`.
   - `adopted_tree_is_linked_to_the_task_its_branch_names`.
   - `report_only_queues_no_rescue_and_no_remove_job`.
   - `reconcile_twice_with_the_same_facts_writes_nothing_more`.
   - `worktree_list_result_refreshes_head_dirty_and_size`.
2. Write the code. Run the Check. Commit.

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

The Check passes. With the facts of a pass, the reconciler adopts each unknown tree as `orphaned`, marks each tree of a dead owner `orphaned`, escalates a vanished folder, reports I6, and queues no rescue and no removal.
