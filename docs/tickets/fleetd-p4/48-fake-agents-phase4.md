# 48: The fake agents of Phase 4: scenario keys that record the binding at process entry, commit a file, run horch result, write a question, or exit with no result

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo build -p horch-e2e --bins && cargo test -p horch-e2e --test fakes && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-e2e/src/bin/fake-claude.rs`, `crates/horch-e2e/src/bin/fake-codex.rs`, `crates/horch-e2e/src/bin/fake-antigravity.rs`, `crates/horch-e2e/tests/fakes.rs`

Spec: `docs/specs/fleetd/design.md`, FM-2, FM-6, "Migration plan" item 1 (Phase 0: the fake harness and the fault points).

## Goal

The fake Claude, Codex and Antigravity agents get scenario keys for the Phase 4 tests: `bound` writes, at process entry, a file `entry.json` in its attempt folder with the task id from the ledger and whether `task.yaml` exists; `commit` commits 1 file in its worktree; `result` runs `horch result`; `ask` sends 1 `QUESTION:`; `noresult` exits with no result.

## Requirements

- Required item 1 of the phase (the fake side): the fake agent records at process entry that its task is bound and that `task.yaml` exists.

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- Scenario keys are read with `scenario_has` (`crates/horch-e2e/src/bin/fake-herdr.rs:382`, `fake-codex.rs:62`). Keep each key of today.
- The fakes read the attempt from the env or from the first line typed into the pane (the doorbell with the path of `START.md`, ticket 09).
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-e2e/tests/fakes.rs`: `bound_scenario_writes_entry_json_at_entry`, `result_scenario_writes_result_json`, `noresult_scenario_writes_nothing`.
2. Write the keys in the 3 fakes.
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

The Check passes. The Phase 4 e2e tests can play each end of an attempt with a fake agent.
