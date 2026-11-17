# 21: The release test of Phase 3: a live tree stays active, a dirty orphan is listed, and nothing is removed after the update

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** light
**Check:** `cargo test -p horch-e2e --test p3_release && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-e2e/tests/p3_release.rs`

Spec: `docs/specs/fleetd/design.md`, "Migration plan", the table "What a release does to a run in flight" (row Phase 3: "A live tree and a dirty orphan tree start on Phase 2. After the update, the live tree is `active`, the orphan is listed in report-only mode, and nothing is removed.").

## Goal

A run in flight survives the update to Phase 3. A live worker goes on in its tree, and a tree that a dead worker left with dirty files is listed and keeps each file.

## Requirements

- The row Phase 3 of "What a release does to a run in flight": the backfill, the old processes, the old commands, and the test of the gate.

## Context

- Read `docs/tickets/fleetd-p3/MAP.md`, section "Open questions", first (Q9 and Q19 apply here).
- The Phase 2 state: a ledger at the schema version before the version of ticket 02 (`VERSION` in `crates/horch-core/src/taskledger/schema.rs`, less 1). Make it the way the migration tests of ticket 02 do (`crates/horch-core/tests/taskledger_schema_p3.rs`) and the release test of Phase 2 does (`crates/horch-e2e/tests/p2_release.rs`, ticket 17 of fleetd-p2): run the migration steps up to the Phase 2 version only, then write the rows of a run in flight, in a temporary git repository:
  - task A, `running`, its `attempts` row `live`, its worker with a live stand-in agent (a `sleep` process), and its worktree row `open` with a real tree and 1 commit;
  - task B, its attempt `ended`, its worker dead, and its worktree row `open` with a real tree that has 1 modified file, 1 untracked file and 1 commit that is not landed.
  Use the helpers of those files if they exist, and do not change them. If no helper can stop at the Phase 2 version, send a QUESTION.
- Then the update: open the ledger with the binary of this branch (the migration runs) and run passes with `horch ledger watch --once` and `HORCH_NOW` steps over 3 periods of the `worktree_list` job.
- "Listed in report-only mode" (Q9): the orphan is in `horch worktrees`. After ticket 18, a pass also rescues it: assert that its rescue ref, if one exists, holds the 3 changes. "Nothing is removed": both folders exist, with each file and each commit, and no row is `removed`.

## Steps

1. Write the 1 test: `live_tree_and_dirty_orphan_on_phase_2_are_active_and_listed_and_nothing_is_removed_after_the_update`. Assert: after the migration, A's tree is `active` with A's attempt as owner, and B's tree is `orphaned`; after the passes, A's worker is not reaped, A's tree is still `active`, B's tree is in the output of `horch worktrees`, both folders exist with each file and each commit, and no row is `removed`.
2. If the test fails because of code, send a QUESTION with the cause and a proposed fix task.
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

The Check passes with the 1 test.
