# 24: Rescue refs of branches and the 2 classes of rescue refs: unique is kept until you decide, superseded expires, and a budget opens 1 needs-you item

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd::worktree_custody && cargo test -p horch-core --test rescue_ref_classes && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/worktree_custody/domain/rescue_class.rs`, `crates/horch-core/src/fleetd/worktree_custody/application/rescue_refs.rs`, `crates/horch-core/src/fleetd/worktree_custody/adapters/rescue_refs_git.rs`, `crates/horch-core/src/fleetd/worktree_custody/adapters/executors.rs`, `crates/horch-core/tests/rescue_ref_classes.rs`

Spec: `docs/specs/fleetd/design.md`, "No work is lost" (Rescue refs have 2 classes, and fleetd owns both; A cancelled task does not block its idea), I10, FM-40, RR-16, "The kinds of job" (`worktree_list`).

## Goal

fleetd writes a rescue ref of a branch tip before each rebase, forced update, cancel or abandon, create-only and read back. fleetd decides the class of each rescue ref by content, each time it reads it: `unique` is kept until you run `horch worktree restore` or `horch rescue drop`; `superseded` expires 30 days after its feature is `merged` or `abandoned`. Over `rescue_unique_budget_mb`, 1 needs-you item opens and nothing is deleted.

## Requirements

- I10: before each rebase or forced update of a branch, a rescue ref of the old tip exists. A rescue ref of class `unique` is deleted only by you.
- FM-40: unique work is never deleted by a collection, and the refs do not grow with no bound.
- RR-16: a rescue ref only moves forward; a second rescue does not overwrite the first; each write is read back.

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- Phase 3 (ticket 06 of fleetd-p3) writes worktree rescue refs `refs/horch/rescue/<worktree id>/<n>` with a manifest. Branch rescue refs (Q16): `refs/horch/rescue/branch/<branch name with / as ->/<n>`, the same create-only write (`git update-ref <ref> <tip> ""`) and read back, and `branches.rescue_ref` holds the last one.
- The class (pure, `domain/rescue_class.rs`): `superseded` when the by-content check of ticket 23 says the ref's changes are in a newer tip of its branch or in a landed branch; else `unique`. The facts come from git, read in the `worktree_list` job of each period (Phase 3 ticket 05; add the rescue refs to its result in `worktree_custody/adapters/executors.rs`), so the pass reads no git (Q17).
- The expiry: `retention_days` (30) after the feature of the ref left `open`; the deletion is `git update-ref -d <ref> <old>`, a short effect after the commit, verified on the next pass. The budget key is `rescue_unique_budget_mb` (ticket 03). The needs-you key is `rescue_unique_budget`.
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/rescue_ref_classes.rs`: `branch_rescue_ref_is_create_only_and_read_back`, `second_rescue_of_1_branch_does_not_overwrite_the_first`, `ref_with_work_in_no_landed_branch_is_unique`, `ref_whose_work_landed_is_superseded`, `superseded_ref_expires_30_days_after_its_feature_ended`, `unique_ref_is_never_deleted_by_the_expiry`, `over_the_unique_budget_opens_1_item_and_deletes_nothing`, `failed_deletion_is_done_again_on_the_next_pass`.
2. Write the class, the writer and the expiry.
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

The Check passes. Each branch rescue ref is written before its branch moves, and no `unique` ref is deleted by fleetd.
