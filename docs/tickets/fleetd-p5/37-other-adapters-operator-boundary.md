# 37: The Antigravity, OpenCode, pi, Prime and none adapters: the 3 operator denials or NotSupported

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** security
**Review:** full
**Check:** `cargo test -p horch-core --test p5_other_adapters_sandbox && cargo test -p horch-core --lib harness && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/harness/antigravity.rs`, `crates/horch-core/src/harness/opencode.rs`, `crates/horch-core/src/harness/pi.rs`, `crates/horch-core/src/harness/prime.rs`, `crates/horch-core/src/harness/none.rs`, `crates/horch-core/tests/p5_other_adapters_sandbox.rs`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 7 "Phase 5: one ledger writer"; Architecture and roles, "The command channel"; Workspace layout and file protocol, Protocol rules).

## Goal

Each other adapter states its operator boundary, so profile validation can decide each profile.

## Requirements

- SR-8
- A37 (a profile on an OS where its sandbox is not supported is refused)

## Context

- Read `docs/tickets/fleetd-p5/MAP.md` first: the sections "Open questions", "Request kinds" and "Decisions that hold".
- Self-hosting danger: this phase changes how each `horch` command reaches the ledger. The build fleet itself uses the installed `horch`. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- The design, Worker profiles and adapters: "The `sandbox` of each worker profile denies 3 things: to read `/fleet/run/operator.token`, to connect to the command channel, and to write into `/fleet/run/requests/` (SR-8). Each adapter turns this into the deny rules of its CLI." And `writable_paths: ["{task_dir}", "{worktree}"]`: nothing else, the ledger included, from Phase 5.
- The paths: the token `<fleet root>/run/operator.token`, the channel `<fleet root>/run/fleetd.sock`, the request folder `<fleet root>/run/requests/`. Deny the whole `<fleet root>/run/` for writes: the job result files (ticket 33) and the socket are there too.
- `domain/operator_boundary.rs` (ticket 35) has the type of the 3 denials and 1 function per adapter: `fn operator_boundary(teammate, fleet_root) -> Result<Boundary, NotSupported>`, where `Boundary` says for each of the 3 acts if the rendered config of the CLI denies it. Profile validation (ticket 38) refuses a profile whose boundary allows 1 act, and a profile whose adapter returns `NotSupported` on this OS.
- The attack test of each adapter ticket: the rendered config denies each act (a unit test of the config text), and, where the CLI enforces the config with the sandbox of the platform, a child under that sandbox tries each act and is refused (ticket 06). The real CLI is not run in a test; the conformance check "Operator boundary" with the real CLI is Phase 7.
- Orchestrator seats (Open question Q2) are not worker profiles: they get no operator-path deny until Phase 6.
- The design table of adapters (Worker profiles and adapters) gives `writable_paths` for Antigravity (`--add-dir` plus `permissions.allow: write_file(path/)`). OpenCode, pi and Prime are not in the design (`preservation-06-wiring-and-platform.md`, row S15); their adapters return what their config can deny, else `NotSupported`.
- The `none` harness launches nothing (`harness/none.rs`, the smoke teammate): its boundary denies each act, because no CLI runs.
- A `NotSupported` adapter removes its personas from dispatch on that OS. List each such persona in the done summary.
- A file that a ticket of fleetd-p2, fleetd-p3 or fleetd-p4 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-core/tests/p5_other_adapters_sandbox.rs`: 1 test for each adapter, `<adapter>_boundary_denies_the_3_acts_or_is_not_supported`, with the rendered config text.
2. Write the function of each adapter.

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
- The tests pass.
- The done summary lists each adapter and its result.
