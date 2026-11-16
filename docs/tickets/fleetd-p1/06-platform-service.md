# 06: Platform: the service definitions, install and restart through a service manager, and the systemd watchdog ping

<!-- horch-ticket: 1 -->
**Persona:** primary `opus-hardening`, fallback `codex-sol` · **Parallel:** yes
**Kind:** feature
**Review:** full
**Check:** `cargo test -p horch-platform`
**Own:** `crates/horch-platform/src/service.rs`, `crates/horch-platform/src/mac/service.rs`, `crates/horch-platform/src/linux/service.rs`, `crates/horch-platform/src/windows/service.rs`, `crates/horch-platform/src/unix/service.rs`, `crates/horch-platform/tests/service.rs`

Spec: `docs/specs/fleetd/design.md`, "Architecture and roles" (row fleetd; Supervision tree and restart policy; table "What each OS provides": Service manager, Hang detection), PL-1, PL-2, PL-3, PL-5, FM-12; "Migration plan" item 3; `docs/specs/fleetd/preservation-06-wiring-and-platform.md` row S14.

## Goal

The platform builds the service definition of fleetd for each OS as text, installs it and restarts it through the service manager command of the OS, and pings the systemd watchdog. Each function takes the service manager command as an input, so a test runs a stand-in and never registers a real service.

## Requirements

- **PL-2** (part): the concerns "service install and restart" and "hang detection".
- **PL-3** (part): `watchdog_ping` returns `NotSupported` on macOS and Windows, and core uses `horch fleetd check` there.
- **FM-12** and **PL-5** (part): the definitions carry the hang detection of each OS: on Linux `WatchdogSec=30`; on macOS and Windows a timer that runs `horch fleetd check` every 30 s.

## Context

- Read `docs/tickets/fleetd-p1/MAP.md`, section "Open questions", first. It records each choice of this phase (Q1 to Q21), and each ticket follows it.
- Phase P (`docs/tickets/fleetd-pp/`) builds `crates/horch-platform`. Its concern tickets propose function names. The module doc of each concern file in `crates/horch-platform/src/` gives the final name. Read that doc before you call a platform function. A name in this ticket is the proposed name.
- The definitions (the design):
  - Linux: a systemd user unit `horch-fleetd.service` with `Type=notify`, `ExecStart=<horch> fleetd`, `Restart=always`, `WatchdogSec=30`, and the env of the fleet (`HORCH_LEDGER_DB`, `FLEET_ROOT`). No timer: systemd kills a fleetd that stops its pings.
  - macOS: a launchd agent plist `<label>.plist` with `ProgramArguments` `<horch> fleetd`, `KeepAlive` true and `RunAtLoad` true, and a second plist `<label>.check.plist` with `ProgramArguments` `<horch> fleetd check` and `StartInterval` 30.
  - Windows: 2 Task Scheduler task XML files: 1 runs `<horch> fleetd` at logon with restart on failure, and 1 runs `<horch> fleetd check` every 30 s (check whether Task Scheduler accepts a repetition interval of `PT30S`; if its minimum is 1 min, stop and send QUESTION with the options, because the design fixes 30 s).
- Proposed names: `service::Spec { label, exe, env, folder }`, `service::definitions(&Spec) -> Vec<(PathBuf, String)>`, `service::install(&Spec, manager: &Path) -> io::Result<()>` (write the files, then run the manager command: `launchctl bootstrap gui/<uid> <plist>`, `systemctl --user daemon-reload` and `enable --now`, `schtasks /Create /XML`), `service::restart(&Spec, manager: &Path)` (`launchctl kickstart -k`, `systemctl --user restart`, `schtasks /End` then `/Run`), `service::watchdog_ping() -> Result<(), NotSupported>` (Linux: a datagram `WATCHDOG=1` to `$NOTIFY_SOCKET`, and `READY=1` once), `service::os_user_home() -> io::Result<PathBuf>` (the home of the OS user from the user database: `getpwuid_r` on unix, the profile folder on Windows; never `$HOME`).
- `os_user_home` serves the rule of MAP.md, Q8: core uses the service manager of the OS only for the default ledger of the OS user. The platform does not decide it.
- Safety (build rule 1): no test of this ticket runs `launchctl`, `systemctl` or `schtasks`. Each test passes a stand-in manager: a small script in a temp folder that writes its arguments to a file.

## Steps

1. Write the tests first, in `crates/horch-platform/tests/service.rs`. Each fails before the code exists:
   - `linux_unit_has_restart_always_and_watchdogsec_30` (`#[cfg(target_os = "linux")]`, run by CI), `mac_plists_have_keepalive_and_a_30_s_check` (`#[cfg(target_os = "macos")]`), `windows_tasks_run_fleetd_and_its_check` (`#[cfg(windows)]`, run by CI): parse the text and check each key.
   - `install_writes_the_files_then_calls_the_manager` and `restart_calls_the_manager_once`, with a stand-in manager script in a temp folder (`#[cfg(unix)]`; a `.cmd` stand-in for `#[cfg(windows)]`).
   - `watchdog_ping_is_not_supported_off_linux` and `watchdog_ping_sends_watchdog_1_to_notify_socket` (Linux, a `UnixDatagram` in a temp folder as `NOTIFY_SOCKET`).
   - `os_user_home_ignores_home_env`: set `HOME` to a temp folder in a child process and compare.
2. Write the code. Each `unsafe` block has a `// SAFETY:` note.
3. Run the Check. A Linux or Windows result comes from CI.

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

The Check passes on macOS, and no test ran a real service manager. The definitions name `horch fleetd` and the 30 s hang detection of each OS.
