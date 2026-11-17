# 13: horch ledger add-map --dry-run runs the plan validator on a MAP.md or a plan.yaml, with the requirement tables of check-req-coverage.sh

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --test add_map_validator && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo test -p horch-core --lib taskledger::map && bash scripts/check-req-coverage.sh --help`
**Own:** `crates/horch-core/src/taskledger/map.rs`, `crates/horch-core/tests/add_map_validator.rs`, `scripts/check-req-coverage.sh`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 8 "Phase 6: the idea dispatcher and sub-orchestrators" ("Extend `add-map --dry-run` and `scripts/check-req-coverage.sh` into the plan validator"); Preservation analysis, contract F-1 and F-2).

## Goal

`horch ledger add-map --dry-run` is the plan validator for a person: it reads a `MAP.md` (with its `Spec:` line) or a `plan.yaml`, runs the same `PlanValidator` that fleetd runs, and prints each problem.

## Requirements

- FR-3 (1 validator for fleetd and for the dry run)
- FM-1 (the red test `fm1_plan_with_an_uncovered_requirement_is_refused` uses this path; ticket 54 makes it green)

## Context

- Read `docs/tickets/fleetd-p6/MAP.md` first: the sections "Open questions", "Old flow and new" and "Decisions that hold".
- Today `check()` in `crates/horch-core/src/taskledger/map.rs` checks the map and each C1 ticket. Keep each check of today (`MAP.md` of a run in flight stays readable).
- A `MAP.md` becomes a `Plan`: each row is a task, `Blocked by` gives `after:`, the ticket gives the kind, the persona gives the profile. The requirements are the ids of the tables of the `Spec:` file.
- Which table defines a requirement id: the rule of `scripts/check-req-coverage.sh` (a row `| XXX-00 | ...` in a table whose header's second cell is `Requirement` or `Check`). Port that rule to Rust in `map.rs` (MAP.md, Q14). The script keeps its test check. Add 1 comment line in the script that names the Rust rule, so the 2 stay the same.
- A task covers the requirement ids that its ticket names in `## Requirements`.
- `crates/horch/src/cmd/taskledger.rs` prints the result of `map::check`. Do not change that file: ticket 47 owns it in this phase. If the print needs a change, send QUESTION.
- Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p5 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/add_map_validator.rs`: `map_with_an_uncovered_requirement_names_the_id`, `map_with_a_cycle_is_refused`, `plan_yaml_runs_the_same_validator`, `script_and_validator_find_the_same_ids_in_docs_specs` (it runs the Rust rule and the script on a fixture spec). They fail because the dry run does not read the spec.
2. Make `check()` build a `Plan` and run `PlanValidator`.
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
- `horch ledger add-map --dry-run docs/tickets/fleetd-p6/MAP.md` still passes.
