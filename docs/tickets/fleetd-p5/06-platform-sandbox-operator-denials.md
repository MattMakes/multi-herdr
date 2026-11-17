# 06: Platform sandbox: deny the read of the token file, the connect to the channel and a write into the request folder

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** security
**Review:** full
**Check:** `cargo test -p horch-platform --test sandbox && cargo test -p horch-platform && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-platform/src/sandbox.rs`, `crates/horch-platform/src/mac/sandbox.rs`, `crates/horch-platform/src/linux/sandbox.rs`, `crates/horch-platform/src/windows/sandbox.rs`, `crates/horch-platform/tests/sandbox.rs`

Spec: `docs/specs/fleetd/design.md` (Migration plan, item 7 "Phase 5: one ledger writer"; Architecture and roles, "The command channel"; Workspace layout and file protocol, Protocol rules).

## Goal

The sandbox of the platform layer can run 1 command so that it cannot read 1 file, cannot connect to 1 socket, and cannot write under 1 folder. The attack test of Phase 5 uses it as the worker sandbox.

## Requirements

- SR-8 (the 3 denials of the worker sandbox)
- FM-32 (the operator path from a sandbox)

## Context

- Read `docs/tickets/fleetd-p5/MAP.md` first: the sections "Open questions", "Request kinds" and "Decisions that hold".
- Self-hosting danger: this phase changes how each `horch` command reaches the ledger. The build fleet itself uses the installed `horch`. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).
- Ticket 17 of fleetd-p4 makes `crates/horch-platform/src/sandbox.rs` ("run 1 command so that it can write only a list of paths, on macOS and Linux, and NotSupported elsewhere"). These files are from that ticket. Extend that interface; do not build a second one (Open question Q-A of `docs/tickets/fleetd-p4/MAP.md`).
- macOS: the Seatbelt profile of `sandbox-exec` gets `(deny file-read* (literal <token>))`, `(deny network-outbound (literal <socket>))` and a write rule that does not include the request folder. Linux: bubblewrap mounts an empty file over the token path, and does not bind the socket and the request folder. Windows: `NotSupported`.
- The tests run the real `sandbox-exec` on this Mac. On Linux, CI runs them when `bwrap` is installed, else the tests assert `NotSupported` and the gate file names them as pending.
- Each denied act must fail with a refusal of the OS, not with a missing file.
- A file that a ticket of fleetd-p2, fleetd-p3 or fleetd-p4 names and that does not exist yet comes from that ticket. It is not a fault. Read the ticket that makes it.
- No test reads the real load, the real limits or the real clock of the machine, and no test depends on the speed of the host. Give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock, and wait for an event, not for a time.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-platform/tests/sandbox.rs` (attack tests): `sandboxed_child_cannot_read_the_denied_file`, `sandboxed_child_cannot_connect_to_the_denied_socket` (a real listener of `horch_platform::channel` at that path), `sandboxed_child_cannot_write_into_the_denied_folder`, and `sandboxed_child_still_writes_its_allowed_paths`. Each child is `/bin/sh -c` with 1 act; the test asserts its exit code and that the file of the act did not change. They fail because the interface has no denials.
2. Add the 3 denials to the input of the sandbox call (for example `Denials { read: Vec<PathBuf>, connect: Vec<PathBuf>, write: Vec<PathBuf> }`), and the code of each OS.
3. Windows returns `NotSupported`.

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
- The 4 attack tests pass on this Mac with the real `sandbox-exec`.
- The doc of `sandbox.rs` names what each OS denies.
