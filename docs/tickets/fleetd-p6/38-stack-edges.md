# 38: A stack edge ends with its lower feature: fleetd clears stacked_on, writes a rescue ref, rebases the upper feature, runs both gates again, and moves the base of its pull request

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test stack_edges && cargo test -p horch-core --test rebase_job && cargo test -p horch-core --test push_job && cargo test -p horch-core --test pr_follows_state && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/integration/application/stack_edges.rs`, `crates/horch-core/src/fleetd/integration/application/rebase.rs`, `crates/horch-core/src/fleetd/integration/adapters/exec_rebase.rs`, `crates/horch-core/src/fleetd/integration/adapters/exec_push.rs`, `crates/horch-core/tests/stack_edges.rs`

Spec: `docs/specs/fleetd/design.md` (Between features (A stack edge ends with its lower feature; Continuous rebase); Ledger I10; Failure mode FM-42).

## Goal

An upper feature never keeps a base that is gone.

## Requirements

- FM-42
- I10 for the rebase of an upper feature

## Context

- Read `docs/tickets/fleetd-p6/MAP.md` first: the sections "Open questions", "Old flow and new" and "Decisions that hold".
- Lower feature `merged`: clear `stacked_on` of the upper one, write a rescue ref of its tip, queue a `rebase` job onto the fetched primary tip, and after it the fast gate and the full gate jobs. With `pr = gh` the push job sets the base of the pull request to the primary branch; with `pr = manual` fleetd changes the needs-you item of the feature.
- Lower feature `abandoned`: clear, rescue ref, and rebase the upper one on the primary tip without the commits of the lower one (`git rebase --onto <primary> <lower tip> <upper>`).
- A conflict opens 1 resolver task (ticket 37 path).
- Continuous rebase (Phase 4, ticket 41 of fleetd-p4) orders the rebases of a stack: the bottom first. Add that order in `rebase.rs`.
- The rules of Phase 4 stay: no rebase of a feature while 1 of its bundles is `gating` or `passed`; 1 git writer for each repository.
- Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p5 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `lower_feature_merged_rebases_the_upper_on_the_primary_tip`, `lower_feature_abandoned_rebases_the_upper_without_its_commits`, `rescue_ref_exists_before_the_rebase`, `both_gates_run_again_after_the_rebase`, `pr_base_moves_to_the_primary_branch`, `stack_is_rebased_bottom_first`, `idempotent_pull_request_closed` (a closed pull request of the lower feature gives 1 needs-you item, not a rebase). They fail because the use case is empty.
2. Write the use case and the 3 changes of Phase 4 files.
3. Run the Check and the guard tests. Commit the exact files.

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

- The Check passes.
