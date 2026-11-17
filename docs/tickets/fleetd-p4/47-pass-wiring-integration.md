# 47: The pass of Phase 4, part 2: landing, bundles, the ref move, candidate refs, the feature, fetch, push, rebase, rescue classes and the checks of I8 to I12 in the reconcile pass

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test fleetd_pass_p4b && cargo test -p horch-core --lib fleetd && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/application/pass.rs`, `crates/horch-core/src/fleetd/fleet_runtime/adapters/effects.rs`, `crates/horch-core/src/fleetd/fleet_runtime/domain/budget.rs`, `crates/horch-core/tests/fleetd_pass_p4b.rs`

Spec: `docs/specs/fleetd/design.md`, "The reconcile pass", "Long work is a job. A pass never waits for it", "A pass has a work budget", "No work is lost" (fleetd checks these on every pass against git), I8 to I12, "1 git writer for each repository".

## Goal

Each pass runs the integration work: the branch states follow their tasks; due bundles, gates, fetches, pushes and rebases are queued and observed; passed bundles move the ref after the commit; ended bundles lose their candidate refs; the feature state moves; the rescue classes and the expiry run from the `worktree_list` result; and the invariants I8 to I12 are checked, each breach repaired or escalated with 1 event. At most 1 git writer job runs for 1 repository at a time.

## Requirements

- I8 to I12 checked each pass.
- 1 git writer for each repository: a bundle build, a rebase, a rescue, a fetch, a push, a `worktree_add` or a `worktree_remove`.
- Each new row kind has its cursor in `pass_cursors` (`branches`, `bundles`, `features`).

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- The fetch period and the push backoff are deadlines on the fleet clock (PL-4).
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/fleetd_pass_p4b.rs`: `ready_branches_of_1_feature_land_by_passes_alone`, `only_1_git_writer_job_runs_for_1_repository`, `each_invariant_breach_records_1_event`, `branch_rows_continue_at_their_cursor`.
2. Wire the pass.
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

The Check passes. Ready branches land on their feature branch by passes alone, and each invariant is checked each pass.
