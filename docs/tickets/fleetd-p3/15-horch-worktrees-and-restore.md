# 15: horch worktrees lists each tree, horch worktree restore turns a rescue ref into a branch and a tree, and the status shows the total size

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --lib fleetd::worktree_custody && cargo test -p horch --test p3_worktrees_cli --test w3_ledger_cli_pins --test w4_145_worktree_list_pins && cargo test -p horch-e2e --test i308_triage && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/worktree_custody/application/queries.rs`, `crates/horch/src/cmd/worktree.rs`, `crates/horch/src/main.rs`, `crates/horch-core/src/taskledger/status.rs`, `crates/horch/src/cmd/taskledger.rs`, `crates/horch/tests/p3_worktrees_cli.rs`

Spec: `docs/specs/fleetd/design.md`, "Worktrees: leased, reconciled, never lost" ("Ideas finish clean": `horch worktrees` and `horch worktree restore T-0007-03`; "Disk": `horch status` shows the total size of all trees), "Migration plan" item 5; `ddd/08-code/worktree-custody/design.md` §7 (`WorktreeRows`).

## Goal

You can see each tree of the fleet and get back the work of a rescue ref. `horch worktrees` lists every tree with its state, task, owner, age and size, and the trees that the reconciler would rescue or remove in report-only mode. `horch worktree restore <task>` turns the newest rescue ref of the tree of that task into a branch `rescue/T<seq>-<n>` and a tree. `horch ledger status` shows the total size of all trees.

## Requirements

- `horch worktrees` and `horch worktree restore` (the row Phase 3 of "What each gate needs").
- **FM-28** (the view): the total size of all trees is shown.
- **RR-10** (the report of report-only mode): each orphan that is on disk is listed.

## Context

- Read `docs/tickets/fleetd-p3/MAP.md`, section "Open questions", first (Q9, Q13 and Q14 apply here).
- The read model, in `application/queries.rs`: `worktree_rows(conn) -> Vec<WorktreeView>` (path, state, today's `status`, task as `T<seq>` and its run, the owner attempt and its worker, the age from `created_at`, `bytes_on_disk`, `dirty`, `rescue_ref`, and the "would rescue" or "would remove" mark of ticket 09) and `total_bytes(conn)`. Read only.
- The CLI (`crates/horch/src/cmd/worktree.rs`, `crates/horch/src/main.rs`):
  - `horch worktrees [--json]`: a new top-level command. 1 line for each tree that is not `removed`, then the total. It opens the ledger read-only (`taskledger::open_existing_read_only`, as `horch worktree triage` does).
  - `horch worktree restore <name>` (today: `WorktreeSubcommand::Restore`, which calls `triage::apply::restore` for an archive or trash ref). Q14: when `<name>` is a task (`T<seq>` of the active run) or a worktree id whose row has a rescue ref, make the branch `rescue/T<seq>-<n>` at the rescue commit and its tree at `<project>/.worktrees/rescue-T<seq>-<n>` through `create_worktree` and `run_now` (tickets 08 and 05). Else keep today's restore. Refuse when the branch or the folder exists; change nothing then.
  - `horch ledger status` (`status_cmd` and `print_status` in `crates/horch/src/cmd/taskledger.rs`; `status::Report` in `crates/horch-core/src/taskledger/status.rs`): 1 more line with the total size of all trees, and the field in `--json`. The other lines do not change.
- `docs/architecture.md` needs no new row: no new command file. `every_cli_file_has_a_row_in_the_architecture_doc` must pass.
- What you read keeps today's words for today's lines. A new line has a word that says what it is.

## Steps

1. Write the tests first, in `crates/horch/tests/p3_worktrees_cli.rs`, each with its own temporary repository, ledger and state folders. Each fails before the code exists:
   - `worktrees_lists_each_tree_with_state_task_owner_age_and_size`.
   - `worktrees_marks_an_orphan_that_report_only_mode_would_rescue`.
   - `worktrees_json_has_each_field`.
   - `restore_of_a_task_makes_branch_rescue_t_seq_n_and_its_tree_with_each_rescued_file`.
   - `restore_refuses_when_the_branch_or_the_folder_exists_and_changes_nothing`.
   - `restore_of_an_archive_name_keeps_today_s_behavior`.
   - `ledger_status_shows_the_total_size_of_all_trees`.
2. Write the code. Keep each test of `crates/horch/tests/w3_ledger_cli_pins.rs` and `w4_145_worktree_list_pins.rs` green.
3. Run the Check. Commit.

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

The Check passes. `horch worktrees` lists each tree and the total, `horch worktree restore T<seq>` gives back the rescued work on a new branch and tree, and `horch ledger status` shows the total size.
