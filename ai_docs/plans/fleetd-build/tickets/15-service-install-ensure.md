# The service install is safe to run again: each label is loaded with its current definition

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-platform && cargo test -p horch install && cargo test -p horch-core --test nfr --test arch_scan --test architecture_doc && cargo fmt --all --check`
**Own:** `crates/horch-platform/src/service.rs`, the service files under `crates/horch-platform/src/os` (`mac`, `linux`, `windows`), `crates/horch-platform/tests/service.rs`, `crates/horch/src/cmd/install.rs`

## Goal

After `horch install` or `horch self-update` in service mode, each service label of fleetd (the
fleetd service and the check job or task) is loaded in the service manager with its current
definition. This is true on a first install, on a second install, after an install that failed
in the middle, and after a changed definition.

## Context

Review finding F2 of T-87 (ticket 24 of Phase 1). `crates/horch/src/cmd/install.rs` (about line
119) treats "the definition files are equal" as "the service is installed". This is not a proof:

- `service::install` writes each file first, then runs the manager commands. On macOS,
  `launchctl bootstrap` fails for a label that is loaded, and install stops at the first command
  that fails. So after a partial failure the files match, the next install returns early, and
  the check agent `<label>.check` is never loaded. Then nothing restarts a hung fleetd (PL-5).
- A changed definition stays stale in launchd, because bootstrap refuses a loaded label.

## Prior work

- T-69 (ticket 06 of Phase 1) wrote `crates/horch-platform/src/service.rs`. Read its head comment.
- T-86 added the manager command name and the default definition folder of each OS there.
- T-87 wrote the install path. Read its done summary (`horch ledger show T-87`).
- Service mode is only for the default ledger of the OS user with no `FLEET_ROOT`, and the Spec
  env is empty on each OS.

## Steps

1. Write the failing tests first, with a stand-in manager that records each command and can fail
   on a chosen command:
   - install, then install again: both labels end loaded, no error.
   - the first install fails at the second label: the next install loads the missing label.
   - a changed definition: the next install reloads that label.
2. Add 1 platform operation, for example `service::ensure(&Spec, manager)`, that makes each label
   loaded with its current definition:
   - macOS: for each label, `launchctl bootout gui/<uid>/<label>` (an error "not loaded" is not a
     failure), then `launchctl bootstrap gui/<uid> <plist>`.
   - Linux: `systemctl --user daemon-reload`, then `enable --now`, and `restart` when the
     definition changed.
   - Windows: `schtasks /Create /XML ... /F` replaces a task already. Check that each task is
     created each time, then `/Run`.
3. Make `install.rs` call this operation each time in service mode. Remove the early return on
   equal files, or keep it only as a skip of the file write, never as a skip of the manager step.
4. Keep the order safe for a live fleetd: the old fleetd is stopped by PID and start time and the
   lock is free before the service starts the new one, as T-86 and T-87 do.
5. Run the Check.

## Constraints

- Before the commit, run `git remote -v` and `git config user.email`. The remote must be
  `git@git-alt:MattMakes/multi-herdr.git` and the email `m4ttplu5@gmail.com`. If not, send BLOCKED.
- No co-author line. Never write the em dash character. NEVER use `ANTHROPIC_API_KEY`.
- Do not use a shared `CARGO_TARGET_DIR`. Never install a development build. Never run
  `horch self-update` or `cargo install`. NEVER register a real service and never run the real
  `launchctl`, `systemctl` or `schtasks`: each test passes a stand-in manager and temporary folders.
- OS-specific code stays in `crates/horch-platform` (PL-2). No `cfg(target_os)` and no match on
  the OS name in `crates/horch` or `crates/horch-core`.
- Keep the Windows code small. Read it 2 times. Name each Windows risk in your report.
- Run `horch ledger progress` each 5 min during a long run.
- If the GitHub SSH connection times out at `horch ledger finish`, run it again, 3 times maximum.

## Done when

The Check passes, each new test fails without the change, and the done summary names the manager
commands of each OS in order.
