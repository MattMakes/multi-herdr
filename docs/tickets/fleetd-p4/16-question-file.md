# 16: A QUESTION of a push attempt also writes its question file in outbox/questions/

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch --bin horch -- messaging && cargo test -p horch --test p4_question_file && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch/src/cmd/messaging.rs`, `crates/horch/tests/p4_question_file.rs`

Spec: `docs/specs/fleetd/design.md`, "Workspace layout and file protocol" (`outbox/questions/`, one file each), "The task state machine" (`running`: a valid question file -> `blocked_on_user`), RR-12 (Phase 2 part).

## Goal

When a worker of a push attempt sends `QUESTION:`, its question row is written as in Phase 2, and a question file `outbox/questions/<n>.json` of its attempt is written atomically, 1 file for each question. The Stop hook and the repair pass then accept it in place of `result.json`.

## Requirements

- The question file holds `task`, `attempt`, `question_id` (the row of Phase 2), `text` and `at`; its size is under 64 KiB.
- A claim attempt writes no file (old path).

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- The QUESTION path is in `crates/horch/src/cmd/messaging.rs` (Phase 2 ticket 12 added the `questions` row).
- Choice Q9: Phase 4 adds no new question command; the `fleet ask` helper is Phase 5.
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `question_of_a_push_attempt_writes_1_question_file`, `2_questions_write_2_files`, `question_of_a_claim_attempt_writes_no_file`.
2. Write the file in the QUESTION path.
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

The Check passes. Each QUESTION of a push attempt has 1 question file in its outbox.
