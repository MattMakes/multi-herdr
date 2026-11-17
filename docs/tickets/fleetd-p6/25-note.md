# 25: horch idea note: the sub-orchestrator records each question and answer as a file of its idea folder, and fleetd ingests it 1 time

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test idea_notes && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo test -p horch --test p6_idea_cli_parse`
**Own:** `crates/horch/src/cmd/idea/note.rs`, `crates/horch-core/src/fleetd/ideas/adapters/idea_notes_reader.rs`, `crates/horch-core/src/fleetd/ideas/application/notes_ingest.rs`, `crates/horch-core/tests/idea_notes.rs`

Spec: `docs/specs/fleetd/design.md` (Ideas (The interview; How a worker's question is resolved, step 4); Workspace layout (Protocol rule 5: `horch idea note` for sub-orchestrators); A35).

## Goal

`horch idea note` writes 1 file in `ideas/I-NNNN/notes/`, and fleetd appends each interview note to `interview.md` and passes each answer note to the question flow, each 1 time.

## Requirements

- FM-31 (each answer is recorded as it is given)
- RR-12 (the answer arrives as a file)

## Context

- Read `docs/tickets/fleetd-p6/MAP.md` first: the sections "Open questions", "Old flow and new" and "Decisions that hold".
- `idea note` is a worker kind (ticket 06): a file, never the channel. The command finds its idea folder from its own attempt (the process chain of ticket 26 of fleetd-p5), and refuses a caller that is not the sub-orchestrator attempt of that idea.
- A note file: `notes/<n>.json` with `idea`, `attempt`, `kind` (`interview`, `answer`, `dismiss`), `question`, `case`, `cite`, `text`. Written atomically.
- fleetd reads it with the SR-6 checks (the ids of the path equal the ids of the payload) and the `ingested` table of Phase 4 (1 ingestion per path and sha256).
- An `interview` note appends `Q<n>` and its answer to `interview.md`. An `answer` note goes to ticket 32. A `dismiss` note goes to the HIGH findings of ticket 41.
- Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p5 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `sub_orchestrator_answer_arrives_as_an_idea_note_file`, `interview_note_appends_to_interview_md_once`, `note_of_another_idea_is_rejected`, `note_that_is_a_symlink_is_rejected`, `idea_note_outside_a_sub_orchestrator_attempt_is_refused`. They fail because the files are empty.
2. Write the command, the reader and the ingest.
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
