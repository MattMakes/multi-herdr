# 31: The arch_scan rule fitness_no_process_start_outside_the_launcher, and no child of a worker, a job, a check, docs sync or conformance has the forbidden name

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** security
**Review:** full
**Check:** `cargo test -p horch-core --test arch_scan -- fitness_no_process_start_outside_the_launcher && cargo test -p horch-e2e --test p7_launcher && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/tests/arch_scan.rs`, `crates/horch-e2e/tests/p7_launcher.rs`

Spec: `docs/specs/fleetd/design.md` (SR-7; "Worker profiles and adapters" (1 launcher for each process); Migration plan, item 9 "Phase 7: docs sync and conformance, then exec mode").

## Goal

A scan proves that production code starts each process through the launcher or the platform start function, and an e2e test proves that no child of fleetd has `ANTHROPIC_API_KEY`.

## Requirements

- SR-7: an `arch_scan` rule finds no direct process start in production code outside the launcher. With the forbidden name set in the environment of fleetd, no child of a worker, a job, a verifier check, docs sync or a conformance launch has it.

## Context

- Read `docs/tickets/fleetd-p7/MAP.md` first: the sections "Open questions" and "Decisions that hold".
- Self-hosting danger: this phase changes the worker mode that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p6 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it (the text says "from ticket NN of fleetd-pN").
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`. No test uses the network: give each page through a local fixture. No ticket sends project text, a name or an email address to an outside service.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).
- The rule (MAP.md Q1): it reads each `.rs` file under `crates/*/src`, skips test code (`#[cfg(test)]` modules, `src/bin/` of `crates/horch-e2e`, `test_support.rs` under `cfg(test)`), and fails on `Command::new` or `process::Command` outside `crates/horch-platform/src/start.rs` and `crates/horch-core/src/fleetd/worker_profiles/adapters/launcher.rs`. Its allow-list has only these 2 files. Each failure names the file and the line.
- Write a self-test of the rule, as `fitness_domain_rule_finds_outward_paths` does for its rule: the rule finds a planted start in a temporary file.
- The e2e test runs fleetd (a temporary fleet root, `crates/horch-e2e/src/fleetd.rs`) with `ANTHROPIC_API_KEY` set in its environment, and starts: 1 worker attempt in exec mode, 1 `task_check` job, 1 `docs_sync` job (pages from a fixture), 1 `conformance` job. Each fake records its environment; none has the name.
- Name: `no_child_of_a_worker_job_check_docs_sync_or_conformance_has_the_forbidden_key` (Slice 7).

## Steps

1. Write the rule and its self-test first. Run it: it must pass now, because tickets 03 to 10 moved each start. If it fails, the failure names a start that a move ticket missed: move it here only if the file is in no open ticket, else send QUESTION.
2. Write the e2e test. Run the Check and the guard tests. Commit the exact files.

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
- The allow-list of the rule has exactly 2 files.
