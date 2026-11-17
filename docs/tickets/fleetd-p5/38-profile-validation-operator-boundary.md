# 38: Profile validation refuses a worker profile whose sandbox allows an operator act or does not exist on this OS; the no-sandbox mark

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** security
**Review:** full
**Check:** `cargo test -p horch-core --test p5_profile_validation && cargo test -p horch-core --lib roster && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/roster/validation.rs`, `crates/horch-core/src/fleetd/worker_profiles/adapters/sandbox_rules.rs`, `crates/horch-core/src/fleetd/worker_profiles/adapters/check_sandbox.rs`, `crates/horch-core/src/taskledger/status.rs`, `crates/horch-core/tests/p5_profile_validation.rs`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 7 "Phase 5: one ledger writer"; Architecture and roles, "The command channel"; Workspace layout and file protocol, Protocol rules).

## Goal

No worker profile runs without a sandbox that denies the 3 operator acts. A refused profile is visible, gets no dispatch, and opens 1 needs-you item.

## Requirements

- SR-8 (profile validation refuses a profile whose sandbox allows 1 of the 3, and a profile on an OS where its sandbox is not supported)
- A37
- 06 S9

## Context

- Read `docs/tickets/fleetd-p5/MAP.md` first: the sections "Open questions", "Request kinds" and "Decisions that hold".
- Self-hosting danger: this phase changes how each `horch` command reaches the ledger. The build fleet itself uses the installed `horch`. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- The design: "Profile validation refuses a profile whose sandbox rules allow 1 of them. When the platform module returns "not supported" for the sandbox of a CLI on an OS, profile validation refuses the profile on that OS, with the reason. ... `horch status` shows the profile as "not available on this OS: no sandbox". A worker never runs without its sandbox (06 S9)."
- Today `sandbox_rules` of `crates/horch-core/src/roster/validation.rs` (line 580) holds the `sandbox:` block to Claude only ("sandbox is Claude only"). From this phase each worker profile has a sandbox through its adapter (tickets 35 to 37), so that rule changes: the check is the boundary of the adapter.
- A worker profile with no sandbox at all allows the 3 acts, so it is refused. The orchestrator seats of `fleet.toml` (Q2) are not worker profiles and are not refused by this rule.
- Phase 4 already gives the path of a persona with no check sandbox: no dispatch, 1 needs-you item for each persona, 1 status line (ticket 18 of fleetd-p4, `check_sandbox.rs`; status lines of ticket 45 of fleetd-p4, `status.rs`). Reuse that path with the reason of this ticket; do not build a second one. The fallback rule of the roster applies when the fallback has a sandbox on this OS.
- The words: `not available on this OS: no sandbox` as the design writes them.
- A file that a ticket of fleetd-p2, fleetd-p3 or fleetd-p4 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/p5_profile_validation.rs`: `profile_whose_sandbox_allows_the_operator_path_is_refused` (1 case for each of the 3 acts), `profile_on_an_os_without_its_sandbox_is_refused`, `worker_profile_with_no_sandbox_is_refused`, `orchestrator_seat_is_not_refused_by_the_operator_rule`, `refused_persona_gets_no_dispatch_and_1_needs_you_item`, `status_shows_not_available_on_this_os_no_sandbox`, `fallback_with_a_sandbox_takes_the_task`.
2. Write `sandbox_rules.rs` (the call of each adapter boundary), change `validation.rs` and the dispatch hold of `check_sandbox.rs`, and add the status line.

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
- The 7 tests pass.
- The tests of `roster` pass with the teammates of ticket 39 still to come: a test that loads the builtin roster may list refused personas; ticket 39 makes them valid.
