# 01: The fleetd module folders, their fitness tests and their rows in the architecture doc

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --test arch_scan --test architecture_doc && cargo build --workspace --locked`
**Own:** `crates/horch-core/src/lib.rs`, `crates/horch-core/src/fleetd/`, `crates/horch-core/tests/arch_scan.rs`, `docs/architecture.md`

Spec: `docs/specs/fleetd/design.md`, "Architecture and roles" (Module owners inside the 1 binary) and "Migration plan" item 3; `ddd/08-code/implementation-plan.md`, Slice 0 and Slice 1; `ddd/08-code/fleet-runtime/design.md` section 1.

## Goal

The module folders of the 4 contexts that Phase 1 uses exist under `crates/horch-core/src/fleetd/`, each with `domain/`, `application/` and `adapters/`. Each file that a later ticket of this map fills exists with a module doc only. 2 fitness tests guard the layout, and `docs/architecture.md` has the rows.

## Requirements

- No requirement becomes green here. This ticket gives the layout of `ddd/08-code` (Phase 0 left it to the later phases: `docs/tickets/fleetd-p0/MAP.md`, Open questions 3).

## Context

- Read `docs/tickets/fleetd-p1/MAP.md`, section "Open questions", first. It records each choice of this phase (Q1 to Q21), and each ticket follows it.
- Phase P (`docs/tickets/fleetd-pp/`) builds `crates/horch-platform`. Its concern tickets propose function names. The module doc of each concern file in `crates/horch-platform/src/` gives the final name. Read that doc before you call a platform function. A name in this ticket is the proposed name.
- Today `crates/horch-core/src/lib.rs` has no `fleetd` module. The test `every_core_module_has_a_row_in_the_architecture_doc` in `crates/horch-core/tests/architecture_doc.rs` needs a row for each `pub mod` of `lib.rs`, so `docs/architecture.md` gets a `fleetd` row in the table "horch-core modules".
- Phase 1 uses 4 contexts: `fleet_runtime`, `task_execution`, `worker_profiles` and `ideas` (only the needs-you list). `worktree_custody` and `integration` come in Phases 3 and 4. Do not create them.
- The files to create, under `crates/horch-core/src/` (MAP.md, Q14: 1 file for each concern, so that tickets of 1 wave own different files):

```
fleetd/mod.rs
fleetd/fleet_runtime/mod.rs
fleetd/fleet_runtime/domain/{mod,config,ledger,generation,clock,jobs,budget,health,pass,heartbeat}.rs
fleetd/fleet_runtime/application/{mod,ledger,jobs,holds,pass}.rs
fleetd/fleet_runtime/adapters/{mod,sqlite,config_file,fleetd_row,clock_store,jobs_store,cursors_store,scans,effects,watchdog_duties}.rs
fleetd/task_execution/mod.rs, domain/{mod,task}.rs, application/{mod,commands}.rs, adapters/{mod,sqlite}.rs
fleetd/worker_profiles/mod.rs, domain/mod.rs, application/{mod,ports}.rs, adapters/{mod,launcher}.rs
fleetd/ideas/mod.rs, domain/{mod,needs_you}.rs, application/{mod,needs_you}.rs, adapters/{mod,sqlite}.rs
```

- Each `mod.rs` has only the module doc and the `pub mod` lines of its children. `arc_25_no_shim_modules` allows `pub mod` lines. Each other file has only a module doc that names its concern and the ticket of `docs/tickets/fleetd-p1/MAP.md` that fills it.
- The table owners for `fitness_no_context_touches_foreign_tables` (MAP.md, Q13): `fleet_runtime` owns `fleetd`, `jobs`, `pass_cursors` and `events`; `task_execution` owns `tasks`, `task_deps`, `workers` and `attempts`; `ideas` owns `needs_you`; `worker_profiles` owns no table in Phase 1. The 1 door, `fleetd/fleet_runtime/adapters/sqlite.rs` (ticket 11), writes the state of each entity and is the 1 named exception.
- `fitness_only_fleet_runtime_opens_the_ledger_writable` is not in Phase 1: the `horch` commands still write the ledger until Phase 5 (MAP.md, Q13).

## Steps

1. Write the 2 tests first, in `crates/horch-core/tests/arch_scan.rs`, with the helpers of that file (`crate_src`, `rust_files`, `code_lines`):
   - `fitness_domain_imports_nothing_outward`: no `use` or path in a file under `fleetd/<context>/domain/` names a crate other than `std`, or a module of `crate` other than `crate::fleetd::<the same context>::domain`. It fails before the folders exist, because it asserts that the 4 `domain/` folders exist.
   - `fitness_no_context_touches_foreign_tables`: each SQL table name in a string of a file under `fleetd/<context>/` is a table of that context, by the owner list above. The 1 door is exempt by path. It fails before the folders exist, for the same reason.
   - Add 1 test for each rule that shows the rule finds a breach in a sample text, as `arc_22_matcher_finds_error_text_checks` does.
2. Create the files of the list above. Add `pub mod fleetd;` to `crates/horch-core/src/lib.rs`.
3. Add to `docs/architecture.md`: the `fleetd` row in "horch-core modules", and 2 rows for the new rules in "The rules the tests enforce".
4. Run the Check.

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

The Check passes. The 2 fitness tests fail on a sample breach and pass on the tree. No file under `fleetd/` holds code.
