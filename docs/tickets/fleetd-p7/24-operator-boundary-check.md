# 24: The check "Operator boundary" of the suite for each adapter: from inside the sandbox, the token read, the channel connect and the request write are each refused

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** security
**Review:** full
**Check:** `cargo test -p horch-core --test p7_operator_boundary && cargo test -p horch-core --lib fleetd::worker_profiles && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-core/src/fleetd/worker_profiles/domain/conformance.rs`, `crates/horch-core/src/fleetd/worker_profiles/application/conformance.rs`, `crates/horch-core/src/fleetd/worker_profiles/adapters/conformance_runner.rs`, `crates/horch-core/tests/p7_operator_boundary.rs`

Spec: `docs/specs/fleetd/design.md` ("Docs sync and adapter conformance" (the row Operator boundary); SR-8; "Worker profiles and adapters" (The sandbox of a profile denies the operator path); Migration plan, item 9 "Phase 7: docs sync and conformance, then exec mode").

## Goal

For each adapter, the suite launches 1 attempt that tries the 3 operator acts from inside its sandbox, and the check passes only when each act is refused.

## Requirements

- SR-8 (the check of the suite): a read of `run/operator.token`, a connection to the command channel and a write into `run/requests/` are each refused.
- The gate test: the check "Operator boundary" passes for each adapter.

## Context

- Read `docs/tickets/fleetd-p7/MAP.md` first: the sections "Open questions" and "Decisions that hold".
- Self-hosting danger: this phase changes the worker mode that the build fleet itself uses. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- A file that a ticket of fleetd-p2 to fleetd-p6 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it (the text says "from ticket NN of fleetd-pN").
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time. No test starts a real agent CLI: use the fake agents of `crates/horch-e2e`. No test uses the network: give each page through a local fixture. No ticket sends project text, a name or an email address to an outside service.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).
- Phase 5 gives the deny rules of each adapter and `operator_boundary(teammate, fleet_root) -> Result<Boundary, NotSupported>` in `crates/horch-core/src/fleetd/worker_profiles/domain/operator_boundary.rs` (fleetd-p5 tickets 35 to 38). This ticket proves the rules with a launch.
- Each adapter: Claude Code, Codex, Antigravity, OpenCode, pi and Prime (MAP.md Q5). For OpenCode, pi and Prime the suite runs only this check. An adapter whose boundary is `NotSupported` on this OS gives the result `not supported` with the reason, not `pass`; profile validation already refuses such a profile on that OS.
- The fakes (ticket 16) try each act in a child under the platform sandbox with the rules of their rendered config, and write the result. The fleet root of the test is a temporary folder with a real token file (mode 0600), a real channel socket and a request folder.

## Steps

1. Write the tests first in `crates/horch-core/tests/p7_operator_boundary.rs`: `conformance_operator_boundary_check_runs_for_each_adapter` (Slice 7), `operator_boundary_fails_when_the_rules_allow_the_token_read`, `operator_boundary_fails_when_the_rules_allow_the_channel`, `operator_boundary_fails_when_the_rules_allow_a_request_write`, `operator_boundary_not_supported_is_not_pass`. They fail before the code.
2. Write the check. Run the Check and the guard tests. Commit the exact files.

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

- The Check passes on macOS. The Linux result comes from CI; name it as pending CI in the commit message.
