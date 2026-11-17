# 13: Triage keeps its commits, refusals and forward-only refs, and removes and restores a tree only through the custody jobs

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib taskledger::triage && cargo test -p horch-core --test i308_triage_apply --test i308_triage_facts --test i308_triage_reply && cargo test -p horch-e2e --test p3_triage --test i308_triage --test i308_fleet_steward && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/taskledger/triage/apply.rs`, `crates/horch-core/src/taskledger/triage/refs.rs`, `crates/horch-e2e/tests/p3_triage.rs`

Spec: `docs/specs/fleetd/design.md`, "Worktrees: leased, reconciled, never lost" ("Built on what horch has": triage reads the new columns, `refs/horch/rescue/` sits beside the archive and trash refs), RR-16, RR-17, RR-9; `docs/specs/fleetd/preservation-04-behavior.md` rows W7, W8 and W9.

## Goal

`horch worktree triage apply` keeps each safety of today: ARCHIVE and DELETE first commit modified and untracked files, refuse a dirty or unique detached HEAD, write their archive or trash ref forward only and read it back, and delete a branch only when the trash ref holds its tip. Then the folder is removed only through custody: a verified rescue job, then a `worktree_remove` job. `horch worktree restore` of an archive or trash ref makes its tree through a `worktree_add` job.

## Requirements

- **RR-16**: archive, trash and rescue refs only move forward. Each ref write is read back before a folder is removed. Trash refs expire after 30 days. Test: a second rescue of 1 tree does not overwrite the first ref (ticket 06 has the unit test; this ticket has the e2e form through triage), and a failed read-back keeps the folder.
- **RR-17**: triage commits modified and untracked files before ARCHIVE or DELETE, refuses a dirty or unique detached HEAD, and deletes a branch only when the trash ref holds its tip. Test: triage of a tree with modified and untracked files saves each one, and a unique detached HEAD is refused.
- **RR-9** (the triage path).

## Context

- Read `docs/tickets/fleetd-p3/MAP.md`, section "Open questions", first (Q7 and Q15 apply here).
- Today: `crates/horch-core/src/taskledger/triage/apply.rs` (`check_identity`, the commit of modified and untracked files, `remove_folder` with `git worktree remove`, the branch delete, `restore` with `git worktree add`); `triage/refs.rs` (`ARCHIVE`, `TRASH`, `write` that moves only forward and reads back, `restorable`, `gc` of trash refs after `TRASH_DAYS` = 30). Keep each check and each word of today's output.
- Change:
  - `remove_folder`: after today's commit and the archive or trash ref write and read-back, move the row to `orphaned` if it is not `orphaned` or `rescued` (cause `triage`), run the rescue job (`run_now`, ticket 05; the rescue of ticket 06), then `queue_remove` and `run_now` (ticket 07). A failed read-back of the archive or trash ref, a failed rescue, or a refusal of the remove guard keeps the folder and today's `cleanup_failed` mark.
  - A folder that triage imports with no row (row W7) gets its row in `orphaned` through the custody store (`insert_orphan`, ticket 03), not today's direct insert of a row with no state.
  - `restore`: make the tree through a `worktree_add` job (`create_worktree` of ticket 08 with the branch and tip of the ref) instead of `git worktree add`.
- Rescue refs are listed by triage beside the archive and trash refs where triage lists refs. Rescue refs are not deleted by `gc` (Q6).

## Steps

1. Write the tests first, in `crates/horch-e2e/tests/p3_triage.rs`, each with its own temporary repository, ledger and state folders. Each fails before the code exists, or names the old path that it replaces:
   - `pres_04_w9_triage_commits_untracked_before_delete`: each modified and each untracked file is in the trash ref and in the rescue ref after DELETE.
   - `rr17_triage_refuses_a_unique_detached_head_and_the_tree_stays`.
   - `rr17_triage_deletes_a_branch_only_when_the_trash_ref_holds_its_tip`.
   - `pres_04_w8_refs_move_forward_and_are_read_back`: an archive ref that would move backward is refused.
   - `rr16_failed_read_back_keeps_the_folder` (a fault point makes the read-back fail).
   - `rr16_second_rescue_through_triage_does_not_overwrite_the_first_ref`.
   - `trash_ref_older_than_30_days_is_collected_and_a_rescue_ref_is_not`.
   - `restore_makes_its_tree_through_a_worktree_add_job`.
2. Write the code. Keep each test of `crates/horch-e2e/tests/i308_triage.rs`, `i308_fleet_steward.rs` and `crates/horch-core/tests/i308_triage_*.rs` green.
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

The Check passes. Triage keeps each of its safeties, and each folder that it removes is first on a verified rescue ref and then removed by a `worktree_remove` job.
