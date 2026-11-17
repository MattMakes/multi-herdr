# 35: The Claude Code adapter denies the 3 operator acts and writes only the task folder and the worktree

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** security
**Review:** full
**Check:** `cargo test -p horch-core --test p5_claude_sandbox && cargo test -p horch-core --lib harness::claude && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/harness/claude.rs`, `crates/horch-core/src/fleetd/worker_profiles/domain/operator_boundary.rs`, `crates/horch-core/tests/p5_claude_sandbox.rs`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 7 "Phase 5: one ledger writer"; Architecture and roles, "The command channel"; Workspace layout and file protocol, Protocol rules).

## Goal

Each Claude worker runs with a sandbox config that cannot read the token, cannot connect to the channel, and cannot write the run folder or the ledger.

## Requirements

- SR-8 (the deny rules of the worker sandbox)
- FM-32
- RR-5 (no worker writes the ledger)

## Context

- Read `docs/tickets/fleetd-p5/MAP.md` first: the sections "Open questions", "Request kinds" and "Decisions that hold".
- Self-hosting danger: this phase changes how each `horch` command reaches the ledger. The build fleet itself uses the installed `horch`. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- The design, Worker profiles and adapters: "The `sandbox` of each worker profile denies 3 things: to read `/fleet/run/operator.token`, to connect to the command channel, and to write into `/fleet/run/requests/` (SR-8). Each adapter turns this into the deny rules of its CLI." And `writable_paths: ["{task_dir}", "{worktree}"]`: nothing else, the ledger included, from Phase 5.
- The paths: the token `<fleet root>/run/operator.token`, the channel `<fleet root>/run/fleetd.sock`, the request folder `<fleet root>/run/requests/`. Deny the whole `<fleet root>/run/` for writes: the job result files (ticket 33) and the socket are there too.
- `domain/operator_boundary.rs` (ticket 35) has the type of the 3 denials and 1 function per adapter: `fn operator_boundary(teammate, fleet_root) -> Result<Boundary, NotSupported>`, where `Boundary` says for each of the 3 acts if the rendered config of the CLI denies it. Profile validation (ticket 38) refuses a profile whose boundary allows 1 act, and a profile whose adapter returns `NotSupported` on this OS.
- The attack test of each adapter ticket: the rendered config denies each act (a unit test of the config text), and, where the CLI enforces the config with the sandbox of the platform, a child under that sandbox tries each act and is refused (ticket 06). The real CLI is not run in a test; the conformance check "Operator boundary" with the real CLI is Phase 7.
- Orchestrator seats (Open question Q2) are not worker profiles: they get no operator-path deny until Phase 6.
- Today the Claude sandbox settings are built in `crates/horch-core/src/harness/claude.rs` (`allowWrite` and `allowUnixSockets` near line 861, `denyWrite` near lines 914 and 1002; the test at line 1512 shows `allowWrite` of the state root and the ledger folder, and `allowUnixSockets` of the state root). After this ticket: `allowWrite` holds the task folder and the worktree only; `denyRead` holds the token file; `denyWrite` holds `<fleet root>/run/` and the ledger folder; `allowUnixSockets` never holds the channel socket or a folder that contains it.
- The relay socket of the Claude channel (`crates/horch-core/src/messaging/channel.rs`) stays allowed when it is not under `run/`. If it is under `run/`, stop and send QUESTION.
- `sandbox_host_problem` (line 1082) stays: macOS needs `sandbox-exec`, Linux needs `bwrap` and `socat`, Windows is not supported (06 S9).
- A file that a ticket of fleetd-p2, fleetd-p3 or fleetd-p4 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/p5_claude_sandbox.rs`: `claude_worker_settings_deny_reading_the_token`, `claude_worker_settings_never_allow_the_channel_socket`, `claude_worker_settings_deny_writing_the_run_folder_and_the_ledger`, `claude_worker_writes_only_the_task_folder_and_the_worktree`, `claude_boundary_is_not_supported_on_windows`, and the attack test `claude_rules_under_the_platform_sandbox_refuse_the_3_acts` (the same 3 paths through ticket 06, macOS).
2. Write `operator_boundary.rs` and the Claude adapter function, and change the settings builder.

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
- The 6 tests pass.
- The tests of `harness::claude` pass.
