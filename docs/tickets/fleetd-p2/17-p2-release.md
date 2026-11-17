# 17: The release test of Phase 2: a held task with a live worker goes on after the update, with 1 lease

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** light
**Check:** `cargo test -p horch-e2e --test p2_release && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-e2e/tests/p2_release.rs`

Spec: `docs/specs/fleetd/design.md`, "Migration plan", the table "What a release does to a run in flight" (row Phase 2: "A held task with a live worker starts on Phase 1. After the update, the worker is not reaped, and the task has 1 lease.").

## Goal

The new file `crates/horch-e2e/tests/p2_release.rs` proves the release row of Phase 2: a run in flight on the Phase 1 schema goes on after the update.

## Requirements

- The release row of Phase 2: each held task gets a lease with a full time, an open `worker_waits` row of an interactive persona gets `max_wait_s`, a live worker goes on, and its lease renews on earned activity.

## Context

- Read `docs/tickets/fleetd-p2/MAP.md`, section "Open questions", first (Q6, Q7 and Q21 apply here).
- The Phase 1 state: a ledger at the schema version before the version of ticket 02 (`VERSION` in `crates/horch-core/src/taskledger/schema.rs`, less 1). Make it the way the migration tests of ticket 02 do (`crates/horch-core/tests/taskledger_schema_p2.rs`): run the migration steps up to the Phase 1 version only, then write the rows of a run in flight: 1 task in `running`, its `attempts` row in `live`, its worker with a live stand-in agent (a `sleep` process) and an open background wait. Use the helper of that test file or of `schema.rs` if one exists, and do not change it (it is not in `Own`). If no helper can stop at the Phase 1 version, send a QUESTION.
- The old supervisor of a Phase 1 binary checks in every 30 s (`runs.heartbeat_every_s`, ticket 07). Write `workers.last_checkin_at` at a 30 s period with `HORCH_NOW` steps to stand for it.
- Then the update: open the ledger with the binary of this branch (the migration runs), and run passes with `horch ledger watch --once` and `HORCH_NOW` steps over 10 min of the fleet clock.
- The 1 test: `held_task_with_a_live_worker_on_phase_1_is_not_reaped_after_the_update_and_has_1_lease`. Assert: after the migration the task has exactly 1 `leases` row with a full time (`expires_at` = the fleet clock + 120); the open wait has a `due_at`; after the passes the task is still `running`, the worker is not reaped, and the lease renewed (its `expires_at` moved forward).

## Steps

1. Write the test. Run it on the base of this ticket. It passes. A fault in the code of another ticket goes to the orchestrator as a QUESTION.
2. Run the Check and the guard tests. Commit.

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

## Done when

The Check passes. The release row of Phase 2 has a test that passes.
