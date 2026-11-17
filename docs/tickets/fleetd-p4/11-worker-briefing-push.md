# 11: The worker briefing of a push attempt: read START.md, work in the named tree, commit, run horch result

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --test golden_prompts --test trf_46_briefings && cargo test -p horch-core --lib harness::launch && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `teammates/_base/fleet-worker.md`, `crates/horch-core/tests/golden_prompts.rs`, `crates/horch-core/tests/trf_46_briefings.rs`, `crates/horch-core/src/harness/launch.rs`, `crates/horch-core/tests/golden/`

Spec: `docs/specs/fleetd/design.md`, "Workspace layout and file protocol" (`inbox/START.md`, `horch result`), "Migration plan" item 6.

## Goal

A worker of a push attempt is told the new protocol: its task is bound, `START.md` names the ticket and the tree, the worker commits in that tree and runs `horch result`, and fleetd runs the finish steps. No briefing tells a new worker to claim, to make a worktree or to run `horch ledger finish`.

## Requirements

- FR-5 (the text side): the briefing never tells a worker to search for work.
- FM-6 (the text side): the briefing names `horch result` as the end of the work, and a `QUESTION:` line as the other end.

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- Today: `teammates/_base/fleet-worker.md` ("When your task is \"Run horch ledger claim.\" ...", line 89 onward) and the text at `crates/horch-core/src/harness/launch.rs:1161`. The golden tests pin the rendered briefings.
- Self-hosting: `teammates/` of this branch is not read by the live fleet until the run branch merges to `main`. Keep the old section for an attempt that began with a claim, marked as such, until Phase 5.
- Keep the rules of today that still hold: progress, handoff, questions, the scope rules.
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Change the golden files first, and see the golden tests fail.
2. Change `fleet-worker.md` and `launch.rs`.
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

The Check passes. The briefing of a push attempt says: read `START.md`, commit, run `horch result`.
