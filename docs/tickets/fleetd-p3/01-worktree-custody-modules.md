# 01: The module files of worktree_custody, its tables in the fitness rules, and its row in the architecture doc

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --lib fleetd && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/mod.rs`, `crates/horch-core/src/fleetd/worktree_custody/`, `crates/horch-core/tests/arch_scan.rs`, `docs/architecture.md`

Spec: `docs/specs/fleetd/design.md`, "Worktrees: leased, reconciled, never lost"; `ddd/08-code/worktree-custody/design.md` §1 (module layout) and §4 (ports); `ddd/08-code/implementation-plan.md`, Slice 3.

## Goal

The context `worktree_custody` exists under `crates/horch-core/src/fleetd/`, with 1 file for each concern of Phase 3. Each file has a module doc and no behavior yet. The fitness rules know that `worktree_custody` owns the tables `worktrees` and `worktree_modes`. Later tickets of this map each own different files of the context, so they can run at the same time.

## Requirements

- None of the design is green here. This ticket gives the files that FR-11, RR-9 and RR-10 are built in.

## Context

- Read `docs/tickets/fleetd-p3/MAP.md`, section "Open questions", first (Q24 applies here).
- `docs/tickets/fleetd-p1/MAP.md` Q14 and `docs/tickets/fleetd-p2/MAP.md` Q24: 1 file for each concern under `domain/`, `application/` and `adapters/`, more files than `ddd/08-code` lists. Do the same here.
- `crates/horch-core/src/fleetd/mod.rs` names the 4 contexts of Phases 1 and 2 (`fleet_runtime`, `task_execution`, `worker_profiles`, `ideas`). `docs/tickets/fleetd-p1/01-fleetd-modules.md` says that `worktree_custody` comes in Phase 3.
- The files to make, each with a module doc (`//!`) of 1 to 4 lines that names its concern and the ticket of this map that fills it:
  - `worktree_custody/mod.rs`, `domain/mod.rs`, `application/mod.rs`, `adapters/mod.rs`.
  - `domain/worktree.rs` (ticket 09: the decisions of the reconciler and I6), `domain/rescue.rs` (ticket 06: the rescue plan and its verification), `domain/removal.rs` (ticket 07: the guard of a removal, RR-9), `domain/disk.rs` (ticket 16: the disk budget).
  - `application/ports.rs` (ticket 05), `application/create.rs` (ticket 08: the reserve, the observation, and the attach of a tree to a new owner), `application/reconcile.rs` (ticket 09), `application/rescue.rs` (ticket 06), `application/remove.rs` (ticket 07), `application/disk.rs` (ticket 16), `application/queries.rs` (ticket 15).
  - `adapters/sqlite.rs` (ticket 03: the rows of `worktrees`), `adapters/git.rs` (ticket 05: the add and list executors, the only `git worktree add` and `list` calls), `adapters/run_now.rs` (ticket 05: queue, start and wait for a job from a command), `adapters/executors.rs` (ticket 05: the executor of each worktree job kind), `adapters/rescue_git.rs` (ticket 06), `adapters/remove_git.rs` (ticket 07), `adapters/memory.rs` (the in-memory fakes that tests share; each later ticket adds its fake here only when no other ticket of its wave owns the file, else in its own test module).
- The fitness rules in `crates/horch-core/tests/arch_scan.rs` (`docs/tickets/fleetd-p1/MAP.md` Q13):
  - `FLEETD_TABLE_OWNERS` gets the entry `("worktree_custody", &["worktrees", "worktree_modes"])`.
  - `fitness_domain_imports_nothing_outward` covers the new `domain/` files with no change, because it walks each context. Check that it does.
- `docs/architecture.md`: the row of `fleetd` (the module table, line about "1 folder for each context") names `worktree_custody`. `every_core_module_has_a_row_in_the_architecture_doc` (`crates/horch-core/tests/architecture_doc.rs`) must pass.

## Steps

1. Change the test first: add the entry to `FLEETD_TABLE_OWNERS`. Run `cargo test -p horch-core --test arch_scan` and write down the result in the commit message (the rule can fail on a context with no folder).
2. Make each file. Each `mod.rs` declares its children. No function has a body yet that a later ticket must keep.
3. Add the row in `docs/architecture.md`. Run the Check. Commit.

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

The Check passes. `crates/horch-core/src/fleetd/worktree_custody/` has each file of the list, `FLEETD_TABLE_OWNERS` names `worktree_custody`, and `docs/architecture.md` names the context.
