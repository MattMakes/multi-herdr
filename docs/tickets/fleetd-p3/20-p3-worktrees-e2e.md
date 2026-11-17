# 20: The e2e tests of Phase 3: a hand-made tree is adopted, a vanished tree is escalated, the disk budget holds, and a slow worktree add keeps the heartbeat

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** quality
**Review:** light
**Check:** `cargo test -p horch-e2e --test p3_worktrees && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-e2e/tests/p3_worktrees.rs`

Spec: `docs/specs/fleetd/design.md`, FR-11, RR-10, FM-26, FM-27, FM-28, FM-12, "Migration plan" item 5 (Tests: "a `git worktree add` of 60 s keeps the heartbeat, and the attempt launches only after its `worktree_add` job passed"), "Failure modes and effects" (the rows FM-26 to FM-28, "Caught within").

## Goal

The rules of Phase 3 that the unit tests cannot show end to end have 1 e2e test each. Each test runs `horch` as you do, with a temporary repository and temporary ledger and state folders, and passes 1 fleetd pass at a time with `horch ledger watch --once`, or runs a fleetd of this build under `FLEET_ROOT` in a temporary folder.

## Requirements

- **FR-11**: a worker's own `git worktree add` is flagged on the next pass.
- **RR-10**: a worktree made by hand is adopted or rescued within 1 period of the `worktree_list` job.
- **FM-26**: a tree made outside fleetd is adopted as `orphaned` and rescued within 60 s.
- **FM-27**: a folder that vanishes without a removal by fleetd is escalated within 60 s; fleetd removes only `landed` and rescued trees.
- **FM-28**: over the budget, landed and rescued trees of finished runs are removed and dispatch is held before an active tree is touched.
- The gate test: a `git worktree add` of 60 s keeps the heartbeat, and the tree is used only after its `worktree_add` job passed (Q17).

## Context

- Read `docs/tickets/fleetd-p3/MAP.md`, section "Open questions", first (Q9, Q15, Q17 and Q20 apply here).
- The harness of `crates/horch-e2e` and the helpers of `crates/horch-e2e/tests/p0_rescue.rs` (a run in a temporary repository, a worker played by a `sleep` process, `reconcile` with `horch ledger watch --once`, `HORCH_NOW` steps for the fleet clock). Copy the helpers that you need into this file; do not change other test files.
- A slow `git worktree add`: a `git` script first on the `PATH` of the `horch` processes of the test, which sleeps 60 s before it calls the real `git` for `worktree add` and calls the real `git` at once for each other command. The heartbeat of a fleetd of this build (`<fleet root>/run/heartbeat.json`, `docs/tickets/fleetd-p1/MAP.md` Q20) must move within each `pass_time_budget_s` while the add runs. Start that fleetd only with temporary `FLEET_ROOT`, `HORCH_DATA_DIR`, `HORCH_STATE_DIR` and `HORCH_LEDGER_DB` (stand-in mode, `docs/tickets/fleetd-p1/MAP.md` Q8), and stop it in a `Drop` guard.
- The disk budget: write `fleet.toml` with `worktree_disk_budget_gb` set low for the test, or write `bytes_on_disk` through the `worktree_list` result; use the smallest form that shows the rule.

## Steps

1. Write the tests, in `crates/horch-e2e/tests/p3_worktrees.rs`:
   - `fr11_worker_own_git_worktree_add_is_flagged_on_the_next_pass`.
   - `rr10_hand_made_worktree_is_adopted_within_1_list_period`.
   - `fm26_tree_made_outside_fleetd_is_rescued_and_its_files_are_on_the_ref`.
   - `fm27_vanished_tree_opens_1_needs_you_item_within_60_s`.
   - `fm27_only_landed_or_rescued_trees_are_removed`.
   - `fm28_over_budget_removes_finished_trees_and_holds_dispatch_before_an_active_tree`.
   - `slow_worktree_add_of_60_s_keeps_the_heartbeat_and_the_tree_is_used_only_after_its_job_passed`.
   Each test checks, at its end, that each modified file, each untracked file and each commit that was not landed is still in a tree or on a rescue ref.
2. If a test fails because of code, send a QUESTION with the cause and a proposed fix task. The code is not in `Own`.
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

The Check passes, with each test of the list.
