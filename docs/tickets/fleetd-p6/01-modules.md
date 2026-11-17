# 01: The module files of Phase 6 in ideas, integration, task_execution and fleet_runtime, and their rows in the fitness rules and the architecture doc

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** refactor
**Review:** light
**Check:** `cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo build -p horch-core`
**Own:** `crates/horch-core/src/fleetd/ideas/domain/mod.rs`, `crates/horch-core/src/fleetd/ideas/application/mod.rs`, `crates/horch-core/src/fleetd/ideas/adapters/mod.rs`, `crates/horch-core/src/fleetd/integration/domain/mod.rs`, `crates/horch-core/src/fleetd/integration/application/mod.rs`, `crates/horch-core/src/fleetd/integration/adapters/mod.rs`, `crates/horch-core/src/fleetd/task_execution/domain/mod.rs`, `crates/horch-core/src/fleetd/task_execution/application/mod.rs`, `crates/horch-core/src/fleetd/task_execution/adapters/mod.rs`, `crates/horch-core/src/fleetd/fleet_runtime/application/mod.rs`, `crates/horch-core/src/fleetd/ideas/domain/idea.rs`, `crates/horch-core/src/fleetd/ideas/domain/idea_folder.rs`, `crates/horch-core/src/fleetd/ideas/domain/approval.rs`, `crates/horch-core/src/fleetd/ideas/domain/plan.rs`, `crates/horch-core/src/fleetd/ideas/domain/plan_validator.rs`, `crates/horch-core/src/fleetd/ideas/domain/plan_rules_dag.rs`, `crates/horch-core/src/fleetd/ideas/domain/plan_rules_coverage.rs`, `crates/horch-core/src/fleetd/ideas/domain/plan_rules_profiles.rs`, `crates/horch-core/src/fleetd/ideas/domain/plan_rules_gates.rs`, `crates/horch-core/src/fleetd/ideas/domain/plan_rules_judges.rs`, `crates/horch-core/src/fleetd/ideas/domain/plan_rules_model_guide.rs`, `crates/horch-core/src/fleetd/ideas/domain/answer.rs`, `crates/horch-core/src/fleetd/ideas/application/intake.rs`, `crates/horch-core/src/fleetd/ideas/application/direct.rs`, `crates/horch-core/src/fleetd/ideas/application/approval.rs`, `crates/horch-core/src/fleetd/ideas/application/amend.rs`, `crates/horch-core/src/fleetd/ideas/application/park_cancel.rs`, `crates/horch-core/src/fleetd/ideas/application/dispatcher.rs`, `crates/horch-core/src/fleetd/ideas/application/sub_orchestrators.rs`, `crates/horch-core/src/fleetd/ideas/application/waiting.rs`, `crates/horch-core/src/fleetd/ideas/application/plan_commit.rs`, `crates/horch-core/src/fleetd/ideas/application/replan.rs`, `crates/horch-core/src/fleetd/ideas/application/rollup.rs`, `crates/horch-core/src/fleetd/ideas/application/question_flow.rs`, `crates/horch-core/src/fleetd/ideas/application/answers.rs`, `crates/horch-core/src/fleetd/ideas/application/notes_ingest.rs`, `crates/horch-core/src/fleetd/ideas/application/queries.rs`, `crates/horch-core/src/fleetd/ideas/application/release.rs`, `crates/horch-core/src/fleetd/ideas/adapters/ideas_store.rs`, `crates/horch-core/src/fleetd/ideas/adapters/idea_folder_files.rs`, `crates/horch-core/src/fleetd/ideas/adapters/plan_yaml.rs`, `crates/horch-core/src/fleetd/ideas/adapters/idea_notes_reader.rs`, `crates/horch-core/src/fleetd/ideas/adapters/needs_you_folder.rs`, `crates/horch-core/src/fleetd/ideas/adapters/notify.rs`, `crates/horch-core/src/fleetd/integration/domain/trial_merge.rs`, `crates/horch-core/src/fleetd/integration/domain/stack.rs`, `crates/horch-core/src/fleetd/integration/application/new_features.rs`, `crates/horch-core/src/fleetd/integration/application/trial_merges.rs`, `crates/horch-core/src/fleetd/integration/application/stacks.rs`, `crates/horch-core/src/fleetd/integration/application/stack_edges.rs`, `crates/horch-core/src/fleetd/integration/application/merged_with_ideas.rs`, `crates/horch-core/src/fleetd/integration/application/judge_branch.rs`, `crates/horch-core/src/fleetd/integration/adapters/exec_trial_merge.rs`, `crates/horch-core/src/fleetd/integration/adapters/trial_merges_store.rs`, `crates/horch-core/src/fleetd/task_execution/domain/judge_pair.rs`, `crates/horch-core/src/fleetd/task_execution/application/judge_pair.rs`, `crates/horch-core/src/fleetd/task_execution/application/planned_tasks.rs`, `crates/horch-core/src/fleetd/fleet_runtime/application/handlers_idea.rs`, `crates/horch-core/tests/arch_scan.rs`, `docs/architecture.md`

Spec: `docs/specs/fleetd/design.md` (Architecture and roles (Module owners inside the 1 binary); Migration plan, item 8 "Phase 6: the idea dispatcher and sub-orchestrators"; `ddd/08-code/ideas/design.md` section 1; `ddd/08-code/integration/design.md` section 1).

## Goal

Each file that a Phase 6 ticket owns in the 4 contexts exists before wave 2 starts, so that 2 tickets of 1 wave never own 1 file. Each new file holds only its module doc comment (1 to 3 lines that name the ticket that fills it) and compiles.

## Requirements

- No requirement id becomes green here. This ticket gives the files of the later tickets (MAP.md, Q16).

## Context

- Read `docs/tickets/fleetd-p6/MAP.md` first: the sections "Open questions", "Old flow and new" and "Decisions that hold".
- Follow ticket 01 of `docs/tickets/fleetd-p5/` and of `docs/tickets/fleetd-p4/`: the same shape and the same doc comment style.
- The context `ideas` owns the new tables `ideas`, `requirements` and `task_covers`. The context `integration` owns the new table `trial_merges`. Add them to `FLEETD_TABLE_OWNERS` in `crates/horch-core/tests/arch_scan.rs` (the rule `fitness_no_context_touches_foreign_tables`).
- Add 1 row for each new module in `docs/architecture.md`, in the table that `crates/horch-core/tests/architecture_doc.rs` reads (`every_core_module_has_a_row_in_the_architecture_doc`).
- The files of `crates/horch/src/cmd/idea/` are not in this ticket: ticket 05 makes them.
- Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p5 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Add the table owners in `arch_scan.rs` first, and see `cargo test -p horch-core --test arch_scan` fail because the tables have no owner module file yet, or see that it passes because no code names them yet. Record which.
2. Make each file of `Own` that does not exist, with its `pub mod` line in the parent `mod.rs`. A new file has its doc comment and nothing else.
3. Run the Check. Commit the exact files.
4. Run the Check and the guard tests. Commit the exact files.

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
- Each file of `Own` exists and is in the module tree.
- `arch_scan` knows the owners of `ideas`, `requirements`, `task_covers` and `trial_merges`.
