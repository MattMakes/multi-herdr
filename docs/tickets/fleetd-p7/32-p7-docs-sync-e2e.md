# 32: The e2e test of docs sync and conformance: no repository, a daily new run, a drift that marks the profiles unhealthy and becomes an idea, and the recovery

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-e2e --test p7_docs_sync && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-e2e/tests/p7_docs_sync.rs`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 9 "Phase 7: docs sync and conformance, then exec mode" (Gate tests); FR-8; FM-19).

## Goal

With fleetd and the fake agents, the whole loop of docs sync and conformance runs as the design says.

## Requirements

- The gate tests of Phase 7: a `docs_sync` run and a `conformance` run with no repository are valid rows, and a daily run starts a new run; the check "Operator boundary" passes for each adapter.
- FR-8, FM-19, FR-9, SR-1, FM-18 (end to end).

## Context

- Read `docs/tickets/fleetd-p7/MAP.md` first: the sections "Open questions" and "Decisions that hold".
- Self-hosting danger: this phase changes the worker mode that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p6 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it (the text says "from ticket NN of fleetd-pN").
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`. No test uses the network: give each page through a local fixture. No ticket sends project text, a name or an email address to an outside service.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).
- Use the harness of `crates/horch-e2e/src/fleetd.rs` and a temporary fleet root. The docs pages come from a local fixture through the fetch port (ticket 11); the fake agents of ticket 16 are the CLIs.
- The time: move the fleet clock past 86400 s through the test seam of the fleet clock. Do not sleep.

## Steps

1. Write the tests: `docs_sync_and_conformance_rows_have_no_repository`, `next_day_starts_a_new_docs_sync_run`, `new_fake_cli_version_fails_a_check_marks_its_profiles_unhealthy_and_adds_1_idea`, `unhealthy_profile_gets_no_dispatch_and_status_names_the_hold`, `passed_conformance_marks_the_profiles_healthy_and_dispatch_resumes`, `operator_boundary_passes_for_each_adapter`, `context_limit_check_reads_back_256000`.
2. Each test must pass. A failure is a fault of a ticket of this map: correct a small fault in that ticket's file only with a QUESTION first, else report it.
3. Run the Check and the guard tests. Commit the exact file.

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
