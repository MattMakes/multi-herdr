# 22: horch docs sync and the docs_sync job: the version through the launcher, the pages into the fleet root, and a job result with no repository

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch --test p7_docs_sync_cli && cargo test -p horch-core --lib fleetd::fleet_runtime::application::jobs && cargo test -p horch-core --test p7_docs_sync_job && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch/src/cmd/docscmd.rs`, `crates/horch/src/main.rs`, `crates/horch/src/cmd/mod.rs`, `crates/horch/Cargo.toml`, `Cargo.lock`, `crates/horch/src/cmd/jobcmd.rs`, `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs`, `crates/horch-core/src/fleetd/worker_profiles/application/docs_sync.rs`, `crates/horch-core/src/fleetd/worker_profiles/adapters/docs_dir.rs`, `docs/architecture.md`, `crates/horch/tests/p7_docs_sync_cli.rs`, `crates/horch-core/tests/p7_docs_sync_job.rs`

Spec: `docs/specs/fleetd/design.md` ("Docs sync and adapter conformance" (What one sync does, steps 1 and 2; "Docs sync is a `horch` subcommand"); "The kinds of job", row `docs_sync`; Migration plan, item 9 "Phase 7: docs sync and conformance, then exec mode").

## Goal

`horch docs sync` and the executor of the `docs_sync` job write the pages, the manifest and CHANGES.md of each CLI into `<fleet root>/docs/<cli>/<version>/`, and the job ends with a result and no repository.

## Requirements

- FR-8 (part): steps 1 and 2 of 1 sync, as a horch subcommand and a job.
- SR-7 (part): each version command starts through the launcher.
- The gate test (part): a `docs_sync` run with no repository is a valid row.

## Context

- Read `docs/tickets/fleetd-p7/MAP.md` first: the sections "Open questions" and "Decisions that hold".
- Self-hosting danger: this phase changes the worker mode that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p6 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it (the text says "from ticket NN of fleetd-pN").
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`. No test uses the network: give each page through a local fixture. No ticket sends project text, a name or an email address to an outside service.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).
- horch-core has no HTTP crate (NFR-06). So `application/docs_sync.rs` gives a port (`trait DocsSyncRunner`), and `crates/horch/src/cmd/jobcmd.rs` gives the implementation with `agent-docs-sync` (tickets 11 and 12). Add `agent-docs-sync` as a dependency of `crates/horch`.
- `run_job` (`crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs:564`) has an executor for `task_check` and gives `no_executor` for the other kinds. Earlier phases add arms; add the `docs_sync` arm. Pass the runner in a struct of runners, so that ticket 25 adds the `conformance` runner beside it.
- The subject and the version (MAP.md Q8): the subject is `<cli> <binary id>`, where the binary id is the hash of the identity of the CLI binary (`crates/horch-core/src/exeid.rs`, `exe_identity`), because a pass does not wait for a version command. The job runs `version_cmd` through the launcher, records the version text in its result, and uses it as the folder name. `repo` is null.
- The command: `horch docs sync [--cli <name>] [--out <dir>]`. With no `--cli` it syncs each source. With no `--out` it writes into `<fleet root>/docs/`. `--cli herdr --out herdr-docs` refreshes the repository folder `herdr-docs/` as the old binary did (MAP.md Q4). It is an operator command and needs no fleetd.
- `docs/architecture.md` needs a row for the new CLI file (`every_cli_file_has_a_row_in_the_architecture_doc`).
- The result of a run: `passed` when each page and CHANGES.md are written; `failed`, with each failed page, when not. Either way the run has a result.

## Steps

1. Write the tests first: in `crates/horch/tests/p7_docs_sync_cli.rs` `docs_sync_writes_each_cli_into_the_fleet_root` and `docs_sync_cli_herdr_out_refreshes_a_folder` (pages from a local fixture through the fetch port); in `crates/horch-core/tests/p7_docs_sync_job.rs` `docs_sync_job_with_no_repository_is_a_valid_row`, `docs_sync_job_records_the_version_text`, `version_command_has_no_forbidden_name`. They fail before the code.
2. Write the code. Run the Check and the guard tests. Commit the exact files.

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
- No test uses the network.
