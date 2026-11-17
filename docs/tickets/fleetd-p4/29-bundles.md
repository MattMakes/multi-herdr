# 29: Bundles: fleetd builds 1 candidate of the ready branches of 1 feature, gates it 1 time, and continues each bundle from its row after a restart

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd::integration && cargo test -p horch-core --test bundles && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/integration/domain/bundle.rs`, `crates/horch-core/src/fleetd/integration/application/bundles.rs`, `crates/horch-core/tests/bundles.rs`

Spec: `docs/specs/fleetd/design.md`, "The bundle", "The bundle state machine" (`building`, `gating`), "A moved tip drops a bundle", FM-36, I12 (second part).

## Goal

When task branches of 1 feature are `ready`, fleetd writes 1 bundle row in `building` with the base tip and 1 member row for each branch in queue order, in 1 transaction, and queues the `bundle_build` job. A passed build moves the bundle to `gating` and queues the `fast_gate` job. A conflict sends that member to `conflicted` and the bundle to `dropped`. A moved feature tip drops a bundle in `building` or `gating`. After a restart, each bundle goes on from its row.

## Requirements

- FM-36: no bundle is built 2 times, and no branch stays in `in_bundle` for ever.
- I12 (second part): a task branch is a member of at most 1 bundle that is not ended.
- The time limits of `building` and `gating`: the job limit plus `landing_job_margin_s`; past it, 1 needs-you item.

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- `passed`, `landed` and the ref move are ticket 31; `failed` and the bisect are ticket 30.
- The pass never waits for a job; it reads the job rows (Phase 1 `jobs::observe`).
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/bundles.rs`: `given_3_ready_branches_1_bundle_row_and_3_members_in_1_transaction`, `passed_build_queues_1_fast_gate`, `conflict_sends_the_member_to_conflicted_and_the_others_to_ready`, `moved_feature_tip_drops_a_building_bundle`, `fm36_restart_continues_the_bundle_from_its_row`, `branch_in_an_open_bundle_is_not_put_in_a_second`.
2. Write the bundle rules and the application.
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

The Check passes. Ready branches of 1 feature go into 1 bundle with 1 fast gate, and a restart builds no bundle 2 times.
