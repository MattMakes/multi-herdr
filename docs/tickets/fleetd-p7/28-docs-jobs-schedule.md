# 28: fleetd queues the docs_sync and conformance jobs: each day, on a new horch binary, on a new CLI binary, and a conformance run after each docs_sync run

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --test p7_docs_jobs && cargo test -p horch-core --lib fleetd::worker_profiles && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/worker_profiles/domain/docs_jobs.rs`, `crates/horch-core/src/fleetd/worker_profiles/application/docs_jobs.rs`, `crates/horch-core/tests/p7_docs_jobs.rs`

Spec: `docs/specs/fleetd/design.md` ("The kinds of job", rows `docs_sync` and `conformance`; "Docs sync and adapter conformance" ("Run it daily, on `horch self-update`, and whenever fleetd sees a CLI binary whose version differs"); Migration plan, item 9 "Phase 7: docs sync and conformance, then exec mode").

## Goal

Each pass decides, as a pure rule, which `docs_sync` and `conformance` runs to queue, and queues them with no repository through `jobs::queue`.

## Requirements

- FR-8 (part): the schedule.
- FM-19 (part): a new version of a CLI starts docs sync and conformance before dispatch.
- The gate test: a daily run starts a new run.

## Context

- Read `docs/tickets/fleetd-p7/MAP.md` first: the sections "Open questions" and "Decisions that hold".
- Self-hosting danger: this phase changes the worker mode that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p6 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it (the text says "from ticket NN of fleetd-pN").
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`. No test uses the network: give each page through a local fixture. No ticket sends project text, a name or an email address to an outside service.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).
- The rules: (1) each CLI of `docs-sources.toml`: a `docs_sync` run when no run of the CLI started in the last 86400 s on the fleet clock (MAP.md Q11: a fixed number, not a key); (2) a `docs_sync` run of each CLI when the identity of the horch binary differs from the one in the result of the last run (that is "on `horch self-update`": `horch self-update` restarts fleetd on the new binary, OR-6); (3) a `docs_sync` and a `conformance` run when the binary id of a CLI (ticket 22) differs from the one of its last conformance run; (4) a `conformance` run after each `docs_sync` run ends.
- `jobs::queue` (`crates/horch-core/src/fleetd/fleet_runtime/application/jobs.rs`) returns `Active` when a run of the same kind and subject is queued or running, and an effect job is never `Reused`, so a daily run is a new row with the next `run` number. `repo` is `None`.
- The scan of the CLI binaries reads `exe_identity` (`crates/horch-core/src/exeid.rs`). It is a fact scan with a budget (`pass_cursors`), like the other scans of part 1.
- The domain rule is pure: it takes the clock, the last runs and the identities, and gives the runs to queue. A test gives each fact.
- The cooldown of CLI updates stays (preservation row 06 S7): docs sync and conformance read the installed binary and never update it.

## Steps

1. Write the tests first in `crates/horch-core/tests/p7_docs_jobs.rs`: `daily_run_starts_a_new_run`, `docs_sync_and_conformance_with_no_repository_are_valid_rows`, `new_horch_binary_queues_docs_sync`, `new_cli_binary_queues_docs_sync_and_conformance`, `conformance_follows_each_docs_sync_run`, `queued_run_is_not_queued_twice`, `pres_06_s7_cli_release_younger_than_cooldown_is_held` (Slice 7: docs sync starts no CLI update). They fail before the code.
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
