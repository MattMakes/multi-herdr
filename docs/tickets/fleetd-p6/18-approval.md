# 18: horch idea approve and revise: fleetd writes approval.json with the hash of the assumptions you saw, ignores any other, and I7 holds an idea whose assumptions changed

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --test idea_approval && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/ideas/application/approval.rs`, `crates/horch-core/src/fleetd/ideas/domain/approval.rs`, `crates/horch-core/tests/idea_approval.rs`

Spec: `docs/specs/fleetd/design.md` (Ideas (Quick approval stays with you, in code); Requirements FR-10; Failure mode FM-32; Ledger I7).

## Goal

Only your approval moves an idea past `awaiting_approval`, and only for the exact assumptions that you saw.

## Requirements

- FR-10
- FM-32 (a forged approval)
- I7 (the approval part)

## Context

- Read `docs/tickets/fleetd-p6/MAP.md` first: the sections "Open questions", "Old flow and new" and "Decisions that hold".
- `ApproveAssumptions`: the request carries the sha256 of `assumptions.md` that `horch idea show` printed to you (MAP.md, Q9). fleetd refuses it when the file now has another hash. Else fleetd writes `approval.json` (`idea`, `sha256`, `approved_at`, `by: operator`), sets `approved_sha256`, and moves the idea to `specifying`.
- fleetd reads only the `approval.json` that it wrote: it keeps the sha256 of its own file in the ledger, and an `approval.json` with another content is ignored and logged.
- `ReviseAssumptions`: the text goes to `amendments/` as 1 file, and the idea goes back to `clarifying`.
- I7 (approval part): an idea past `awaiting_approval` whose `assumptions.md` hash is not `approved_sha256` is held: no plan commit and no new dispatch for its tasks, and 1 needs-you item. An idea with the marker `release` or a `--direct` approval compares with the file that its `approval.json` names (MAP.md, Q6).
- Self-hosting danger: this phase retires the orchestrator flow that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p5 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/idea_approval.rs`: `fm32_forged_approval_is_ignored`, `approve_with_a_stale_hash_is_refused`, `approve_writes_approval_json_and_moves_to_specifying`, `revise_sends_the_idea_back_to_clarifying`, `i7_changed_assumptions_after_approval_hold_the_idea`. They fail because the use cases are empty.
2. Write the use cases and the I7 check function (`ideas::domain::approval::i7_approval_holds`).
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
