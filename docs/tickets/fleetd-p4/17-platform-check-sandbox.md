# 17: The check sandbox of the platform layer: run 1 command so that it can write only a list of paths, on macOS and Linux, and NotSupported elsewhere

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-platform --test sandbox && cargo test -p horch-platform && cargo test -p horch-core --test arch_scan && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc`
**Own:** `crates/horch-platform/src/sandbox.rs`, `crates/horch-platform/src/mac/sandbox.rs`, `crates/horch-platform/src/linux/sandbox.rs`, `crates/horch-platform/src/windows/sandbox.rs`, `crates/horch-platform/src/lib.rs`, `crates/horch-platform/src/mac.rs`, `crates/horch-platform/src/linux.rs`, `crates/horch-platform/src/windows.rs`, `crates/horch-platform/tests/sandbox.rs`, `.github/workflows/ci.yml`

Spec: `docs/specs/fleetd/design.md`, SR-4, PL-2 ("the sandbox host" is a concern of the platform layer), PL-3, "Architecture and roles" (A worker never runs without its sandbox; the Sandbox row of "What each OS provides").

## Goal

The platform layer gives 1 function that wraps a command so that it can write only the paths in a list (plus its temp folder) and can read the rest of the disk: `sandbox-exec` with a generated Seatbelt profile on macOS, `bwrap` (bubblewrap) on Linux when it is installed. Windows, and Linux without `bwrap`, return the explicit value NotSupported with the reason.

## Requirements

- SR-4 (the OS side): "A check that writes outside the worker's allowed paths is refused."
- Answer of the orchestrator (Q-A, rule 1): reuse the sandbox host that horch has. Today it is not in `crates/horch-platform`: the Seatbelt and bubblewrap facts are in `crates/horch-core/src/harness/claude.rs` (`SANDBOX_EXEC`, `sandbox_host_problem` near line 1082, Phase P Q5 kept these runtime OS matches there). Move the host facts that a check needs into this concern and make `claude.rs` call it, or call `sandbox_host_problem` from here; do not build a second, different sandbox. If the move needs `claude.rs`, send QUESTION (ticket 14 owns it).
- Answer of the orchestrator (Q-A, rule 3): add bubblewrap to the Linux test job of `.github/workflows/ci.yml` (`sudo apt-get install -y bubblewrap`). If the workflow cannot install it, the Linux tests of the check sandbox assert NotSupported, and the gate file names them as pending.
- PL-3: NotSupported is a value with a reason, never a silent run without the sandbox.

## Context

- Read `docs/tickets/fleetd-p4/MAP.md`, section "Open questions", first.
- The concern follows the other concerns of Phase P and Phase 1 (`crates/horch-platform/src/lock.rs` and its `mac/`, `linux/`, `windows/` modules). The interface: `pub fn sandboxed(cmd: Command, writable: &[PathBuf]) -> Result<Command, NotSupported>`.
- macOS profile: `(version 1)(allow default)(deny file-write*)(allow file-write* (subpath "<p>") ...)` for each path, plus the temp folder and `/dev/null`. Linux: `bwrap --ro-bind / / --dev /dev --proc /proc --bind <p> <p> ... --tmpfs /tmp`.
- Windows code cannot be compiled here: keep it to the NotSupported value, and name the Windows risk in the done summary.
- No test reads the real load, the real limits or the real clock of the machine: give these facts through `HORCH_MACHINE_FILE`, a test `fleet.toml` and the fleet clock.
- The guard tests run in the Check. A new crate, type name or module needs its list entry in the guard tests; add it in this ticket (the orchestrator permits that file outside `Own`).

## Steps

1. Write the tests first in `crates/horch-platform/tests/sandbox.rs`: `sandboxed_command_writes_inside_its_paths`, `sandboxed_command_cannot_write_outside_its_paths` (macOS and Linux with `bwrap`), `windows_and_linux_without_bwrap_are_not_supported_with_a_reason` (unit test of the OS module with a missing `bwrap` path).
2. Write the concern and the CI change.
3. Run the Check. Commit the exact files.

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

Self-hosting: this phase changes `horch spawn`, the claim and the finish steps. The build fleet itself uses the installed `horch` for these. No worker installs a development build, and each test runs the development binary only with temporary folders (build rule 1).

## Done when

The Check passes. A sandboxed command on this Mac writes in its paths and is refused outside them, and an OS with no sandbox returns NotSupported with a reason.
