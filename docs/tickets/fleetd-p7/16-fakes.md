# 16: The fake agents of Phase 7: exec mode for fake-claude, fake-codex and fake-antigravity, the probes of the conformance checks, a usage limit and a long context

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** light
**Check:** `cargo test -p horch-e2e --test fakes && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-e2e/src/bin/fake-claude.rs`, `crates/horch-e2e/src/bin/fake-codex.rs`, `crates/horch-e2e/src/bin/fake-antigravity.rs`, `crates/horch-e2e/src/bin/fake-opencode.rs`, `crates/horch-e2e/src/bin/fake-pi.rs`, `crates/horch-e2e/src/bin/fake-prime.rs`, `crates/horch-e2e/tests/fakes.rs`

Spec: `docs/specs/fleetd/design.md` ("Docs sync and adapter conformance" (Conformance checks per adapter); "Worker profiles and adapters" (Exec or interactive)).

## Goal

Each fake agent runs in exec mode with the argv of its adapter, emits the stream lines of the fixtures of tickets 13 to 15, and answers each probe of the conformance suite, so that no test starts a real agent CLI.

## Requirements

- The fakes for FR-8, FR-9, SR-1, SR-8 (Operator boundary), FM-18, FM-19 and FM-20 tests.

## Context

- Read `docs/tickets/fleetd-p7/MAP.md` first: the sections "Open questions" and "Decisions that hold".
- Self-hosting danger: this phase changes the worker mode that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p6 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it (the text says "from ticket NN of fleetd-pN").
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`. No test uses the network: give each page through a local fixture. No ticket sends project text, a name or an email address to an outside service.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).
- Read the scenario keys of the fakes of Phases 4 and 6 first (fleetd-p4 ticket 48, fleetd-p6 ticket 53). Add keys; do not change an old key.
- Exec mode: the fake accepts the exec argv of its adapter (tickets 13 to 15), reads the prompt path, and prints the lines of `crates/horch-core/tests/fixtures/streams/<cli>-exec.jsonl`. Scenario keys: `usage_limit` (prints the usage-limit line and exits), `long_context` (prints token counts past 256000 and does not stop by itself), `exit_without_result`, `trust_prompt` (prints a dialog and waits).
- The probes: with `--version` the fake prints a version from the scenario. For the conformance prompt, the fake writes what it sees into its result: the skills and rules in its private config home, the MCP servers of its config, the effective compaction setting (env var or config key), and the hook events that it sent to the wrapper. It tries a write outside the allowed paths and the 3 operator acts (read `run/operator.token`, connect to the channel, write into `run/requests/`) in a child under `horch_platform::sandbox::sandboxed` with the rules of its rendered config (fleetd-p4 ticket 17, fleetd-p5 ticket 06), and records each result (MAP.md Q6).
- fake-opencode, fake-pi and fake-prime need only the Operator boundary probe (MAP.md Q5).
- The fakes are test binaries. They are not production code for the rule of ticket 31.

## Steps

1. Write the tests first in `crates/horch-e2e/tests/fakes.rs`: 1 test for each new key and each probe, for example `fake_claude_exec_prints_the_fixture_stream`, `fake_codex_usage_limit_prints_the_signal`, `fake_agy_long_context_passes_256000_tokens`, `fake_probe_sandbox_write_outside_is_refused`, `fake_probe_operator_acts_are_refused`, `fake_probe_reads_back_the_compaction_setting`. They fail before the code.
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
- Each old key of the fakes still passes its test.
