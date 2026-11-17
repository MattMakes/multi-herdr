# 39: The verdict file of a judge task: 1 verdict, each finding ranked CRITICAL, HIGH, MEDIUM or LOW, its structural check, and the outcome critical_open that burns no attempt

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-core --test judge_verdict && cargo test -p horch-core --test verifier_structural && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/task_execution/domain/judge_pair.rs`, `crates/horch-core/src/fleetd/task_execution/domain/structural.rs`, `crates/horch-core/tests/judge_verdict.rs`

Spec: `docs/specs/fleetd/design.md` (Architecture and roles (Done for each kind of task: Judge kind; the rank table)).

## Goal

The verifier can read a judge task's verdict, and an open CRITICAL item ends the judge attempt with an outcome that is not a failure.

## Requirements

- RR-3 (the judge reports open CRITICAL items)
- FM-33

## Context

- Read `docs/tickets/fleetd-p6/MAP.md` first: the sections "Open questions", "Old flow and new" and "Decisions that hold".
- The verdict file is the `outputs` file of the judge task (for example `docs/sync.review.md`). Its form: a `## Verdict` section with 1 word (`pass` or `critical_open`), and a `## Findings` list where each item starts with its rank in brackets: `[CRITICAL]`, `[HIGH]`, `[MEDIUM]`, `[LOW]` (MAP.md, Q18).
- The structural check of kind `judge`: the file exists, it has 1 verdict, each finding has a rank, and the verdict is `critical_open` when a CRITICAL item is open. Extend `structural.rs` of Phase 4 (ticket 21 of fleetd-p4).
- The outcome `critical_open` burns no attempt, as `asked` and `quota` (Ledger, `attempts.outcome`).
- Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p5 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `verdict_without_a_rank_fails_the_structural_check`, `verdict_with_2_verdict_words_fails`, `critical_item_gives_critical_open`, `critical_open_burns_no_attempt`. They fail because the module is empty.
2. Write the parse and the check.
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
