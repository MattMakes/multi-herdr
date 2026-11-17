# 19: Exec mode is the default worker mode: each CLI with an exec adapter runs exec, the seats that interview you stay interactive, and the background wait applies only to interactive attempts

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd::worker_profiles && cargo test -p horch-core --lib roster && cargo test -p horch-core --test p7_exec_default --test baseline_oracles --test nfr && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/worker_profiles/domain/profile.rs`, `crates/horch-core/src/roster/teammate.rs`, `crates/horch-core/src/fleetd/task_execution/domain/waits.rs`, `teammates/`, `crates/horch-core/tests/oracles/`, `crates/horch-core/tests/p7_exec_default.rs`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 9 "Phase 7: docs sync and conformance, then exec mode"; "Worker profiles and adapters" (Exec or interactive); "What a release does to a run in flight", row Phase 7).

## Goal

A new attempt of a worker persona runs in exec mode unless its file says `mode: interactive`. A live interactive attempt at the update goes on as before.

## Requirements

- FR-6 (part): `mode` of each profile.
- The release row of Phase 7: "A worker that runs interactive goes on. Exec is the default for new attempts."
- FM-18 (part): an exec worker cannot sit at a prompt.

## Context

- Read `docs/tickets/fleetd-p7/MAP.md` first: the sections "Open questions" and "Decisions that hold".
- Self-hosting danger: this phase changes the worker mode that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p6 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it (the text says "from ticket NN of fleetd-pN").
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`. No test uses the network: give each page through a local fixture. No ticket sends project text, a name or an email address to an outside service.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).
- Today (fleetd-p2 ticket 06, Q10): `mode` absent means `interactive`, and the test `mode_absent_is_interactive_in_phase_2` pins it. Change the default to `exec` and rename that test to `mode_absent_is_exec_in_phase_7`.
- The default is exec only for an agent with an exec adapter: `claude`, `codex`, `antigravity` (tickets 13 to 15). OpenCode, pi and Prime stay interactive by default ("for any CLI whose headless mode can't do the job"), and `agent: none` has no mode (MAP.md Q5).
- The seats that stay interactive, with `mode: interactive` in their file: the fleet orchestrators (`FLEET_ORCHESTRATORS`, `teammate.rs:38`), the top-tier seats, and `idea-lead` (fleetd-p6 ticket 27 sets it). A persona that you steer by hand keeps an explicit `mode: interactive` where its file says so today. Do not change a fallback: a fallback never lowers confidentiality, and a persona with a written no-fallback reason keeps it (task T-261).
- The background wait (fleetd-p2 Q23, option 1): it stays for an interactive attempt only. An exec attempt has no background wait: its process runs until it exits, and the wrapper sees the exit (MAP.md Q3).
- The launch oracles in `crates/horch-core/tests/oracles/launch/` show the launch of each persona. An oracle changes only where the mode changes the launch. Name each changed oracle in the commit message.

## Steps

1. Write the tests first in `crates/horch-core/tests/p7_exec_default.rs` and in `profile.rs`: `mode_absent_is_exec_in_phase_7`, `opencode_pi_and_prime_default_to_interactive`, `orchestrator_seats_and_idea_lead_are_interactive`, `background_wait_is_refused_for_an_exec_attempt`, `background_wait_still_holds_for_an_interactive_attempt`, `live_interactive_attempt_keeps_its_mode_after_the_update`. They fail before the code.
2. Write the code and the `mode` lines of the teammate files. Update the oracles that change.
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
- `horch teammates --check` (run with temporary folders) passes on the repository roster.
