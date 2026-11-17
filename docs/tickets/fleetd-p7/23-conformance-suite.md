# 23: The conformance suite: 9 checks of the design as real launches in a throwaway task folder, with a verdict for each check and conformance.json

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** security
**Review:** full
**Check:** `cargo test -p horch-core --lib fleetd::worker_profiles && cargo test -p horch-core --test p7_conformance && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/worker_profiles/domain/conformance.rs`, `crates/horch-core/src/fleetd/worker_profiles/application/conformance.rs`, `crates/horch-core/src/fleetd/worker_profiles/adapters/conformance_runner.rs`, `crates/horch-core/tests/p7_conformance.rs`

Spec: `docs/specs/fleetd/design.md` ("Docs sync and adapter conformance" (Conformance checks per adapter, What one sync does, step 3)).

## Goal

The suite runs each check of the design except "Operator boundary" (ticket 24) against 1 CLI binary, and gives 1 verdict for each check and the text of `conformance.json`.

## Requirements

- FR-8 (part): run the adapter conformance tests.
- FR-9: the check "Context limit" reads back 256000 where the CLI supports it.
- SR-1: the check "Sandbox boundary": a write outside the allowed paths is refused.
- FM-18: the check "No trust prompt".
- SR-7 (part): each conformance launch starts through the launcher.

## Context

- Read `docs/tickets/fleetd-p7/MAP.md` first: the sections "Open questions" and "Decisions that hold".
- Self-hosting danger: this phase changes the worker mode that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p6 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it (the text says "from ticket NN of fleetd-pN").
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`. No test uses the network: give each page through a local fixture. No ticket sends project text, a name or an email address to an outside service.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).
- The checks: Launch headless, Private home honoured, Exact MCP set, Shared write, Sandbox boundary, Context limit, Hooks fire, No trust prompt, Stop gate. "Each is a real launch in a throwaway task folder with a tiny fixed prompt." Keep the 10 names of the design in 1 list in `domain/conformance.rs`; ticket 24 fills "Operator boundary".
- The suite runs for `claude`, `codex` and `antigravity`, the 3 CLIs of the adapter table, with the exec launch of tickets 13 to 15 through the wrapper of ticket 17, so that "Hooks fire" sees the events that reach the wrapper (MAP.md Q5).
- Each launch starts through the launcher, in a temporary task folder with its own private config home. A test gives the CLI binary as a fake agent of `crates/horch-e2e` (ticket 16). The real CLI runs only in the production `conformance` job.
- "Launch headless" has a limit of 60 s for the first stream event. The runner takes the limit as a value. A test of the failure uses a fake that exits with no event, not a wait.
- `conformance.json`: the CLI, its version text, the adapter hash (ticket 25), and for each check its name, `pass` or `fail`, and 1 line of evidence.
- Contract (Slice 7): `adapter-conformance-failed` is the result of a run with at least 1 failed check; it names the CLI, the version and each failed check.

## Steps

1. Write the tests first in `crates/horch-core/tests/p7_conformance.rs`, with the fakes: 1 test that passes and 1 that fails for each check, for example `launch_headless_fails_when_the_cli_emits_no_event`, `private_home_marker_is_visible`, `global_only_mcp_server_is_not_listed`, `shared_write_reaches_outbox_result_json`, `sandbox_write_outside_is_refused`, `context_limit_reads_back_256000`, `hooks_reach_the_wrapper`, `trust_prompt_fails_the_check`, `exit_without_result_triggers_the_stop_gate`, and `conformance_json_lists_each_check`. They fail before the code.
2. Write the code. Run the Check and the guard tests. Commit the exact files.

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
- Each of the 9 checks has a passing and a failing test.
