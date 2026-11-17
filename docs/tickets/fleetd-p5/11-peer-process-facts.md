# 11: The peer process fact: is the process of a channel peer in a live attempt

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** security
**Review:** full
**Check:** `cargo test -p horch-core --test peer_process && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/fleet_runtime/adapters/peer_process.rs`, `crates/horch-core/tests/peer_process.rs`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 7 "Phase 5: one ledger writer"; Architecture and roles, "The command channel"; Workspace layout and file protocol, Protocol rules).

## Goal

Given the PID of a peer and the live attempts, fleetd knows if the peer belongs to an attempt: yes, no or unknown.

## Requirements

- SR-8 (fleetd refuses an `operator` request on the channel from a process in the process group or job object of a live attempt, or that descends from its wrapper)

## Context

- Read `docs/tickets/fleetd-p5/MAP.md` first: the sections "Open questions", "Request kinds" and "Decisions that hold".
- Self-hosting danger: this phase changes how each `horch` command reaches the ledger. The build fleet itself uses the installed `horch`. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A live attempt has the PID and start time of its wrapper and its process group in the ledger (the `attempts` row of Phase 1, `crates/horch-core/src/fleetd/task_execution/adapters/sqlite.rs`, and the launch record of `effects.rs`). Until the fleet-run wrapper exists, `horch worker` is the wrapper (Phase 2 Q6).
- Yes: the process group of the peer is the group of a live attempt, or a parent in the chain of the peer is the wrapper of a live attempt (PID and start time both equal). No: the chain reaches PID 1 or a process that is not in any attempt, with each fact read. Unknown: a parent or a group fact is `NotSupported` or fails to read, or the chain is longer than 64.
- Use the process facts of `crates/horch-platform/src/process.rs` and `group.rs`. On Windows the job object fact is `NotSupported` until the platform gives it, so the answer is unknown, and the operator request on the channel is refused (a guard whose fact is unknown does not hold). The request file path still works for you on Windows.
- A PID can be used again. Each compare uses the PID and the start time.
- A file that a ticket of fleetd-p2, fleetd-p3 or fleetd-p4 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/peer_process.rs`, with real child processes: `child_of_the_wrapper_is_in_the_attempt` (a `sh` as wrapper starts `sleep`), `grandchild_in_another_group_is_in_the_attempt_by_its_parent_chain`, `process_outside_each_attempt_is_not`, `reused_pid_with_another_start_time_is_not_the_wrapper`, `unreadable_parent_is_unknown` (a fact source seam).
2. Write `fn peer_in_live_attempt(peer: u32, attempts: &[AttemptProcess], facts: &dyn ProcessFacts) -> Fact` and the adapter of the platform facts.

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
- The 5 tests pass on this Mac.
- The module doc names the Windows gap and its effect.
