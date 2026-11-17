# 40: FR-7: the only bytes that horch types into a pane are fixed lines from 1 list

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test fr7_typed_lines && cargo test -p horch-core --lib messaging::delivery && cargo test -p horch --test p5_restore_warning && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/messaging/delivery.rs`, `crates/horch-core/src/sweep.rs`, `crates/horch/src/cmd/mode.rs`, `crates/horch-core/tests/fr7_typed_lines.rs`, `crates/horch/tests/p5_restore_warning.rs`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 7 "Phase 5: one ledger writer"; Architecture and roles, "The command channel"; Workspace layout and file protocol, Protocol rules).

## Goal

No message text is typed into a pane. A pane gets only a constant line, and the content is in files.

## Requirements

- FR-7 (the only bytes ever sent into a terminal are a fixed doorbell line)
- `only_the_fixed_doorbell_line_is_typed` (DDD Slice 5)
- pres 06 S2 (`pres_06_s2_warns_on_herdr_resume_agents_on_restore`)
- Open question Q9

## Context

- Read `docs/tickets/fleetd-p5/MAP.md` first: the sections "Open questions", "Request kinds" and "Decisions that hold".
- Self-hosting danger: this phase changes how each `horch` command reaches the ledger. The build fleet itself uses the installed `horch`. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- The verification of FR-7 in the design: "Grep of the code for mux send calls; doorbell text is a constant." The sites of today: `crates/horch-core/src/messaging/delivery.rs` (`send_line_with`, line 60: `agent_prompt`, then `pane_send_text` of the message), `crates/horch-core/src/sweep.rs` (line 604) and `crates/horch/src/cmd/mode.rs`. The trait is `crates/horch-core/src/workspace/client.rs`.
- The doorbell of a task pane is the constant `DOORBELL` of `crates/horch-core/src/fleetd/task_execution/domain/stall.rs` (ticket 09 of fleetd-p2; Q8 of `docs/tickets/fleetd-p4/MAP.md`). Choice Q9: a message to a pane that is not a task attempt (the orchestrator, you) types 1 other constant, `You have new mail. Run horch inbox.`, and the message stays in its mail file (today's mail, `messaging/mail.rs`). The Claude channel (MCP notification) and `codex queue` are not a terminal, and they keep their content.
- A slash command of `horch mode` or of the sweep is typed only when it is 1 of a fixed list of constants. If a site needs a text that is not a constant, stop and send QUESTION.
- Put the list of typed constants in 1 place (`messaging/delivery.rs`, `pub const TYPED_LINES`).
- This changes what the orchestrator agent sees: a mail line instead of the message. The live fleet keeps today's delivery until a release.
- A file that a ticket of fleetd-p2, fleetd-p3 or fleetd-p4 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first: `crates/horch-core/tests/fr7_typed_lines.rs` with `only_the_fixed_doorbell_line_is_typed` (a scan of production code: each call of `pane_send_text` and `agent_prompt` has an argument from `TYPED_LINES`), and `message_to_the_orchestrator_types_the_mail_line_and_keeps_the_text_in_mail`; `crates/horch/tests/p5_restore_warning.rs` with `pres_06_s2_warns_on_herdr_resume_agents_on_restore` (the warning of `crates/horch/src/cmd/restore.rs`, line 62, stays).
2. Change the 3 sites.

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
- The 3 tests pass.
- The tests of `messaging::delivery` pass.
