# 25: The conformance job: the executor runs the suite, writes conformance.json into the fleet root, and its input key holds the hash of the adapter

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --test p7_conformance_job && cargo test -p horch-core --lib fleetd::fleet_runtime::application::jobs && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch/src/cmd/jobcmd.rs`, `crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs`, `crates/horch-core/src/fleetd/worker_profiles/adapters/docs_dir.rs`, `crates/horch-core/tests/p7_conformance_job.rs`

Spec: `docs/specs/fleetd/design.md` ("The kinds of job", row `conformance`; "Docs sync and adapter conformance" (step 3)).

## Goal

A `conformance` job runs the suite of tickets 23 and 24 for 1 CLI and writes `<fleet root>/docs/<cli>/<version>/conformance.json`, with no repository.

## Requirements

- FR-8 (part): step 3 of 1 sync as a job.
- The gate test (part): a `conformance` run with no repository is a valid row.

## Context

- Read `docs/tickets/fleetd-p7/MAP.md` first: the sections "Open questions" and "Decisions that hold".
- Self-hosting danger: this phase changes the worker mode that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p6 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it (the text says "from ticket NN of fleetd-pN").
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`. No test uses the network: give each page through a local fixture. No ticket sends project text, a name or an email address to an outside service.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).
- The kind row of `conformance` (`crates/horch-core/src/fleetd/fleet_runtime/domain/jobs.rs`): effect, subject "the CLI and its version", input key "the CLI, its version and the hash of its adapter", `executor_phase: 7`.
- The hash of the adapter (MAP.md Q9): the sha256 of the rendered conformance launch of the adapter for 1 fixed reference profile (the argv and the config text that the adapter writes). It changes when the adapter changes what it renders.
- Add the `conformance` arm to `run_job` beside the `docs_sync` arm of ticket 22, in the same struct of runners.
- The verdict of the job is `passed` when each check passes, else `failed` with the failed checks in its result. A check that is `not supported` does not fail the run.

## Steps

1. Write the tests first in `crates/horch-core/tests/p7_conformance_job.rs`: `conformance_job_with_no_repository_is_a_valid_row`, `conformance_job_writes_conformance_json`, `adapter_hash_changes_when_the_rendered_launch_changes`, `failed_check_fails_the_job_with_its_name`. They fail before the code.
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
