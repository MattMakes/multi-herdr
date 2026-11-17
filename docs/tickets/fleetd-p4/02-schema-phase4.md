# 02: The next schema version: features, branches, bundles, bundle_members, gate_runs, remote_tips and ingested, the push columns of attempts, and the release backfill

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test taskledger_schema_p4 && cargo test -p horch-core --lib taskledger::schema && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/taskledger/schema.rs`, `crates/horch-core/tests/taskledger_schema_p4.rs`

Spec: `docs/specs/fleetd/design.md`, "Ledger" (Schema: `features`, `branches`, `bundles`, `bundle_members`, `gate_runs`, `remote_tips`, `ingested`; I12), "Migration plan" (Schema migration is forward-only; What a release does to a run in flight, row Phase 4); `docs/specs/fleetd/preservation-05-contracts-and-values.md` rows L-3, L-4, L-5.

## Goal

The ledger has the 7 tables of Phase 4 and the columns that push dispatch needs. A ledger of Phase 3 migrates forward with 1 backup, and each run in flight gets its feature row and the branch rows of its tasks.

## Requirements

- Schema of the design for `features` (with `push_state`, `push_failures`, `push_not_before`, `pr_state`), `branches` (with `landing_state`, `owner`, `due_at`, `queue_seq`, `repair_attempts`, `rescue_ref`, `seen_tip`, `seen_remote_tip`, `orphan`), `bundles` (state list with `landed`, `candidate_ref`, `candidate_tip`, `parent_id`, `job_id`, `due_at`), `bundle_members`, `gate_runs`, `remote_tips` and `ingested`.
- I12 (second part): a task branch is a member of at most 1 bundle that is not ended. Enforce it in SQL where SQLite can (a trigger on `bundle_members` that refuses a second open bundle), so that no writer can break it.
- Release row of Phase 4: each run gets 1 feature row, and its run branch is the feature branch. Each task branch gets a `branches` row: `landed` when its task is landed, else `building`.
- The attempt row records how it began: `attempts.via` is `claim` or `push` (MAP.md, Q4). A backfilled attempt is `claim`.

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- Do not write a schema number. The schema is "the next schema version": read `VERSION` in `crates/horch-core/src/taskledger/schema.rs`, and keep the number in that 1 constant.
- Today's tables to read: `runs` (`run_branch`, `base_branch`), `tasks`, `attempts` (Phase 1 ticket 03, Phase 2 ticket 02), `worktrees` (Phase 3 ticket 02, from ticket 02 of fleetd-p3).
- Feature id and name (Q5): `features.id` = `F-<run seq>` with the run's `seq`, `name` = the run slug, `branch` = `runs.run_branch`, `primary_branch` = `runs.base_branch`, `state` = `open`, or `merged` for a run in `done` whose branch is merged, else `open`. A run with no run branch (a solo or shared-checkout run) gets no feature row; its tasks keep today's land path (Q5).
- Branch rows: `name` = the branch of the task's worktree row, `feature_id` = the run's feature, `task_id`, `owner` = `worker`, `due_at` = the fleet clock now plus the task limit, `landing_state` = `landed` when the worktree is `landed` or the task is `done` with a landed commit, else `building`. `seen_tip` is null; the first pass reads it. The feature branch itself gets a row with `task_id` null and no `landing_state`.
- `ingested.outcome` is `accepted` or `rejected`. `gate_runs.result` is null while its job runs.
- The migration reads no git and no process. The first pass of fleetd checks again (as Phase 3 Q19).
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/taskledger_schema_p4.rs`: `phase3_ledger_migrates_with_1_backup`, `each_run_with_a_run_branch_gets_1_open_feature_row`, `landed_task_gets_a_landed_branch_row_and_open_task_a_building_row`, `bundle_member_of_2_open_bundles_is_refused`, `newer_schema_is_refused`, `development_build_does_not_migrate_the_shared_ledger`, `attempt_backfill_is_via_claim`. Each fails before the code exists.
2. Write the migration step and the backfill in `schema.rs`.
3. Run the Check. Commit the exact files.

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

The Check passes. A Phase 3 ledger migrates forward to the next schema version with its backup, each run with a run branch has 1 feature row, and each task with a worktree has 1 branch row.
