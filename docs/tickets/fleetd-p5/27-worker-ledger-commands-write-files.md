# 27: horch ledger progress, note, done, finish and handoff write the outbox; claim and worktree new are refused to a worker

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch --test p5_worker_files && cargo build -p horch && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch/src/cmd/taskledger.rs`, `crates/horch/src/cmd/taskledger_finish.rs`, `crates/horch/tests/p5_worker_files.rs`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 7 "Phase 5: one ledger writer"; Architecture and roles, "The command channel"; Workspace layout and file protocol, Protocol rules).

## Goal

Each worker command of `horch ledger` reports through a file of its outbox, so it works when its sandbox makes the ledger read-only.

## Requirements

- RR-5
- FR-7
- Open question Q3 and Q4
- REQUIRED item 5 of the ticket of this map (no "attempt to write a readonly database")

## Context

- Read `docs/tickets/fleetd-p5/MAP.md` first: the sections "Open questions", "Request kinds" and "Decisions that hold".
- Self-hosting danger: this phase changes how each `horch` command reaches the ledger. The build fleet itself uses the installed `horch`. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- The mapping (answers of the orchestrator to Q3 and Q4):
   - `horch ledger progress <text>` appends 1 line of kind `progress`; `horch ledger note <text>` 1 line of kind `note`.
   - `horch ledger done <summary>` and `horch ledger finish` write `result.json` with `status: completed` and the summary. `finish` prints that fleetd runs the finish steps (Phase 4) and that the result goes to `horch ledger done` of the orchestrator as today.
   - `horch ledger handoff <path>` writes `result.json` with `status: blocked`, `reason: handoff` and the path. The path must be absolute and inside the worktree or the task folder of the attempt; else the command refuses. fleetd checks it again as an untrusted path (ticket 29).
   - `horch ledger claim` and `horch ledger worktree new` and `scratch` from a worker print `fleetd binds each task and makes its worktree; this command is not for a worker` and exit 1. Since Phase 4, each task is pushed (ticket 08 of fleetd-p4).
- `taskledger.rs` is also the file of tickets 21 and 22, which come before.
- Today `codex-reviewer-light` gets "attempt to write a readonly database" on `horch ledger done` and `horch ledger progress`, because its sandbox makes the ledger read-only. After this ticket these commands do not open the ledger for a write.
- The words that you read stay the words of today: `progress` still prints what it prints today.
- A file that a ticket of fleetd-p2, fleetd-p3 or fleetd-p4 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch/tests/p5_worker_files.rs`, each under a stand-in wrapper process with temporary folders: `progress_appends_1_line_and_opens_no_writable_ledger`, `note_appends_a_note_line`, `done_writes_result_completed`, `finish_writes_result_completed`, `handoff_writes_blocked_with_reason_handoff`, `handoff_with_a_path_outside_the_attempt_is_refused`, `claim_from_a_worker_is_refused_with_its_line`, `worktree_new_from_a_worker_is_refused`, `progress_and_done_work_when_the_ledger_file_is_read_only` (mode 0444 on the ledger file and its folder for the test).
2. Change the arms of these commands to the writer of ticket 26.

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

- The Check passes, with the guard tests.
- The 9 tests pass.
- No arm of these commands opens the ledger for a write.
