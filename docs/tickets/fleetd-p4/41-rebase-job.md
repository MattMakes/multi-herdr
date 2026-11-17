# 41: The rebase job: when the fetched primary tip moves, the feature branch is rebased on it after a rescue ref of its old tip, never while a bundle is gating or passed

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd::integration::adapters::exec_rebase fleetd::integration::application::rebase && cargo test -p horch-core --test rebase_job && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/integration/adapters/exec_rebase.rs`, `crates/horch-core/src/fleetd/integration/application/rebase.rs`, `crates/horch-core/tests/rebase_job.rs`

Spec: `docs/specs/fleetd/design.md`, "Between features" (Continuous rebase), "The kinds of job" (`rebase`: effect; runs again when it ended `abandoned` or the tips moved), I9, I10, FM-37.

## Goal

When the fetched primary tip moves and the feature does not contain it, fleetd writes a rescue ref of the feature tip (I10), then queues a `rebase` job. The job rebases the feature branch on the primary tip in the gate tree of the feature (ticket 28) and moves the local ref by compare-and-set. A clean rebase drops each bundle in `building`, makes the fast gate and the full gate stale, and queues a push. fleetd queues no rebase while a bundle of the feature is in `gating` or `passed`.

## Requirements

- I10: the rescue ref of the old tip exists before the rebase; a crash before and after the rescue of a forced update loses no tip (a test of the gate).
- FM-37: no rebase is queued while a bundle is in `gating` or `passed`.
- A conflict ends the job with the conflict files and leaves the branch as it was (ticket 42 opens the resolver task).

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- The rebase needs a working tree: the gate tree of the feature (Q18), never a worker's tree.
- The forced push after the rebase is the push job with `--force-with-lease` (ticket 38).
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `moved_primary_tip_queues_1_rebase_after_a_rescue_ref`, `no_rebase_while_a_bundle_is_gating_or_passed`, `clean_rebase_moves_the_ref_by_compare_and_set_and_drops_a_building_bundle`, `crash_before_and_after_the_rescue_loses_no_tip`, `conflict_leaves_the_branch_at_its_old_tip`.
2. Write the rule and the executor.
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

The Check passes. A rebase of a feature branch always has the rescue ref of its old tip first.
