# 21: The structural check of a judgment kind: the task passes on its structural check and today's finish steps until the judge pair of Phase 6

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --lib fleetd::task_execution::domain::structural && cargo test -p horch-core --test verifier_structural && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/task_execution/domain/structural.rs`, `crates/horch-core/src/fleetd/task_execution/application/verifier.rs`, `crates/horch-core/tests/verifier_structural.rs`

Spec: `docs/specs/fleetd/design.md`, FR-4 (the structural part), "Done for each kind of task" (Judgment kind; Judge kind), "Migration plan" item 6 (A task of a judgment kind gets its structural check in Phase 4), FM-9 (the part test for the structural check).

## Goal

A task of a judgment kind (`research`, `design`, `docs`, `report`, `creative`) gets a structural check from the verifier: each output file exists at the tip of its branch and its required sections are valid. With no judge before Phase 6, it passes on its structural check and today's finish steps.

## Requirements

- FR-4 (structural part): "For a judgment kind the check is structural (the file exists, the schema or the required sections are valid)."
- FM-9 part: a judgment task whose output is missing or has no required section stays not done.

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- Today's ticket `**Kind:**` values are `feature`, `quality`, `refactor`, `security`, `docs`, `report` (`docs/specs/ticket-format.md`; `tasks.work_kind`). Choice Q12: the outputs of a task are the files of its `Own` that its commits changed (a `.md` file must have at least 1 `## ` heading and no empty section; a `.json` file must parse); a judgment task whose commits change no file of `Own` fails. The kinds `research`, `design` and `creative` are read as judgment kinds when a ticket names them; C1 does not change.
- The pure check takes the file list and the file contents at the tip (read by the job) and returns a verdict with each failure named.
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `docs_task_with_its_file_and_sections_passes`, `missing_output_file_fails`, `markdown_with_no_section_fails`, `invalid_json_output_fails`, `judgment_task_that_changes_no_owned_file_fails`, `code_kind_gets_no_structural_check`.
2. Write the check, and call it from `application/verifier.rs` for a judgment kind after the check job passed.
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

The Check passes. A judgment task reaches `done` only with its structural check passed.
