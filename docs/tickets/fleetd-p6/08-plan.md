# 08: plan.yaml and the plan validator frame: the schema of a task, the tickets in format C1 that it names, and the DAG check

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --test plan_validator && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/ideas/domain/plan.rs`, `crates/horch-core/src/fleetd/ideas/domain/plan_validator.rs`, `crates/horch-core/src/fleetd/ideas/domain/plan_rules_dag.rs`, `crates/horch-core/src/fleetd/ideas/adapters/plan_yaml.rs`, `crates/horch-core/tests/plan_validator.rs`

Spec: `docs/specs/fleetd/design.md` (Workspace layout and file protocol (Plan and ticket formats; Example: plan.yaml); Requirements FR-3, FR-4; `ddd/08-code/ideas/aggregate-canvas-plan.md`).

## Goal

fleetd reads `plan.yaml` into a `Plan`, and `PlanValidator` runs a list of rules on it and returns each problem. This ticket gives the schema rule and the DAG rule. Tickets 09 to 12 add 1 rule file each.

## Requirements

- FR-3 (schema and DAG checks)
- FR-4 (each task names a profile, a kind, inputs, outputs and an acceptance check)
- FM-10 (a cycle is refused)

## Context

- Read `docs/tickets/fleetd-p6/MAP.md` first: the sections "Open questions", "Old flow and new" and "Decisions that hold".
- The fields of a task: `key`, `title`, `ticket` (a Markdown file in format C1, `docs/specs/ticket-format.md`), `kind`, `profile`, `covers`, `inputs`, `outputs`, `acceptance` (`run:` or `sections:`), `after`, `judges`, `budget`. The plan has `idea`, `repo`, `feature`, `base_branch`.
- Kinds: code kinds `feature`, `quality`, `refactor`, `security`; judgment kinds `research`, `design`, `docs`, `report`, `creative`; and `judge`.
- The ticket file of a task is read with the C1 checks of `crates/horch-core/src/taskledger/map.rs` (`ticket_problems`). Reuse that function; do not write a second parser.
- `PlanValidator` is pure (no file, no ledger): `validate(plan, facts) -> Vec<Problem>`. `facts` holds what the rules need: the requirements, the approved assumptions, the roster, the declared gates. Define the `PlanFacts` type here with each field, so that tickets 09, 10, 11 and 12 only fill their rule.
- A `Problem` names the task key and the rule, so that `plan.verdict.json` can list it.
- Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p5 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/plan_validator.rs`: `plan_with_a_dependency_cycle_is_refused`, `plan_with_an_unknown_after_key_is_refused`, `task_without_an_acceptance_check_is_refused`, `task_with_an_unknown_kind_is_refused`, `task_whose_ticket_is_not_c1_is_refused`, `the_example_plan_of_the_design_parses`. They fail because the types do not exist.
2. Write `plan.rs`, `plan_yaml.rs`, the validator frame and the DAG rule.
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
