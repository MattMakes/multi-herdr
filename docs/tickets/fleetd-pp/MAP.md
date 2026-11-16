# Map: fleetd build, Phase P (the platform layer and the move of the OS-specific code)

Spec: `docs/specs/fleetd/design.md`

| # | Title | Blocked by | Wave |
|---|---|---|---|
| 01 | [Create the crate horch-platform with its layout and the NotSupported value](01-platform-crate.md) |  | 1 |
| 02 | [Record each test result before the move, and compare after it (the test of PL-6)](02-pl6-compare.md) |  | 1 |
| 03 | [Platform concern: process identity and liveness](03-process-identity.md) | 01 | 2 |
| 04 | [Platform concern: signals and stop](04-signals-and-stop.md) | 03 | 3 |
| 05 | [Platform concern: process groups, job objects and the detach of a child](05-process-groups-and-detach.md) | 03 | 3 |
| 06 | [Platform concern: file locks, file permissions, file owner and file identity](06-file-locks-and-permissions.md) | 01 | 2 |
| 07 | [Platform concern: paths, home folders, symlinks and links](07-paths-and-links.md) | 01 | 2 |
| 08 | [Platform concern: local sockets, pipes, shell and quoting](08-sockets-pipes-and-shell.md) | 01 | 2 |
| 09 | [Platform concern: the machine probe and the small rest](09-machine-probe-and-rest.md) | 01 | 2 |
| 10 | [Move the process sites of horch-core behind the platform layer](10-move-process-sites.md) | 02, 03, 04, 05, 06, 08, 09 | 4 |
| 11 | [Move the herdr call runner of horch-core behind the platform layer](11-move-herdr-calls.md) | 02, 05 | 4 |
| 12 | [Move the file, path and home sites of horch-core behind the platform layer](12-move-file-sites.md) | 02, 03, 06, 07, 09 | 3 |
| 13 | [Move the worker channel and report sockets behind the platform layer](13-move-socket-sites.md) | 02, 06, 08 | 3 |
| 14 | [Move the harness, probe and pane shell sites of horch-core behind the platform layer](14-move-harness-sites.md) | 02, 03, 04, 06, 07, 08, 09 | 4 |
| 15 | [Move the sites of the horch CLI, herdr-install and horch-marketplace behind the platform layer](15-move-cli-and-installer-sites.md) | 02, 03, 04, 05, 06, 07, 08, 09 | 4 |
| 16 | [The arch_scan rule of PL-2, and the removal of the unused OS dependencies](16-arch-scan-pl2.md) | 10, 11, 12, 13, 14, 15 | 5 |
| 17 | [Gate of Phase P: PL-2 and PL-6 green on macOS, Linux and Windows](17-gate-phase-p.md) | 01, 02, 03, 04, 05, 06, 07, 08, 09, 10, 11, 12, 13, 14, 15, 16 | 6 |

## Waves

- Wave 1: 01, 02
- Wave 2: 03, 06, 07, 08, 09
- Wave 3: 04, 05, 12, 13
- Wave 4: 10, 11, 14, 15
- Wave 5: 16
- Wave 6: 17

Tickets that can run at the same time: 01 and 02; then 03, 06, 07, 08 and 09; then 04, 05, 12
and 13; then 10, 11, 14 and 15. No 2 tickets of 1 wave own the same file.

## Critical path

01 -> 03 -> 04 -> 10 -> 16 -> 17

## Sites of today

The ticket grep `grep -rn "cfg(" crates/ --include=*.rs` gives 749 lines; 533 of them name
`target_os`, `unix`, `windows` or `target_family`, in 101 files. Section 6 of
`docs/specs/fleetd/preservation-06-wiring-and-platform.md` counted 518 lines in 103 files and 178
production sites at commit `99b05f19`. Counted again at commit `09472b9e` (the base of this run),
with every form and outside test code:

| Count | Sites |
| --- | --- |
| All OS `cfg` lines in `src/` and `build.rs` of the 4 crates, outside `tests/`, `tests.rs` and `#[cfg(test)] mod` | 214 |
| Of these, the form `cfg(...)` with 1 predicate | 172 |
| The forms `cfg(any(...))`, `cfg(all(...))`, `cfg(not(any(...)))` (1 over 3 lines, `procid.rs:450`) | 14 |
| The form `cfg!(...)` | 23 |
| The form `cfg_attr(...)` (section 6 did not count it) | 5 |
| Test code in a production file: 6 test modules (`horch/src/main.rs:9`, `horch/src/cmd/doctor.rs:873`, `horch/src/cmd/taskledger_watch.rs:603`, `horch-core/src/lib.rs:70`, `horch-core/src/harness/pi.rs:209`, `horch-core/src/messaging/channel.rs:750`) | 6, stay |
| The build script `crates/horch-core/build.rs:112`, `:117` | 2, exempt with its reason (ticket 16) |
| To move: 205 production sites and the test helper `horch-core/src/harness/claude.rs:1116` | 206 |
| By crate (all 214): `horch-core` 174, `horch` 24, `herdr-install` 14, `horch-marketplace` 2 | |

The differences with section 6: 214 is not 178 because the count of today has the forms
`cfg(any(...))`, `cfg!(...)` and `cfg_attr(...)` in the same number, and the code moved between
`99b05f19` and `09472b9e` (for example `harness/claude.rs` has no production OS `cfg` now, and its
sandbox host check at `claude.rs:1082` is a runtime match). Section 6 counted 35 `cfg!` sites;
today has 23 in production code. The tickets 10 to 15 list each site with its line and text.
`crates/horch-e2e` has 32 more lines; it is test code (Q4).

## Coverage

Each mechanism of the row Phase P of "What each gate needs" and of "New work for each phase", with
the 1 ticket that gives it:

| Mechanism (design) | Ticket |
| --- | --- |
| The platform interface: the leaf crate `crates/horch-platform`, `NotSupported` | 01 |
| The 3 OS modules `mac.rs`, `linux.rs`, `windows.rs` | 01 |
| Concern: process identity and liveness (alive, start time, parent, children) | 03 |
| Concern: signals and stop | 04 |
| Concern: process groups (stop and kill of a group) | 05 |
| Concern: detach of a child process | 05 |
| Concern: file locks | 06 |
| Concern: file permissions | 06 |
| Concern: file identity (from "the small rest") | 06 |
| Concern: paths and home directories | 07 |
| Concern: symlinks and links | 07 |
| Concern: local sockets and pipes | 08 |
| Concern: shell and quoting | 08 |
| Concern: the machine probe (free disk, memory, load, processes, open files) | 09 |
| Concern: the small rest (archive format, user PATH, executable extensions, install facts) | 09 |
| Concern: the sandbox host (no OS `cfg` today; Q5) | 14 |
| The move of the existing OS-specific code: process sites | 10 |
| The move: the herdr call runner | 11 |
| The move: file, path and home sites | 12 |
| The move: the worker channel and report sockets (06 S11) | 13 |
| The move: harness, probe and pane shell sites | 14 |
| The move: the `horch` CLI, `herdr-install`, `horch-marketplace` | 15 |
| The `arch_scan` rule of PL-2, with the build script named by path and reason | 16 |
| The removal of the target dependencies that the move leaves unused | 16 |
| The test of PL-6: record and compare each test result (the "run in flight" test) | 02 |
| PL-6 and PL-2 run on macOS, Linux and Windows, and the gate file | 17 |
| A Windows test of a gap of today is fixed or skipped by name with its row | 17 |
| The list of expected failures | Phase 0 (not this map) |

## Open questions

Each point where the design and the code of today do not agree, or where the design leaves a
detail open, with the choice of the tickets:

- **Q1. The count of sites.** The design says 178 production sites. Today there are 214 OS `cfg`
  lines outside test modules, of which 205 production sites move (see "Sites of today"). Choice:
  the tickets move each site of today, as the design asks ("All sites move"), and list them.
- **Q2. Shared Unix code.** The design names 3 OS modules. macOS and Linux share most code
  (`kill`, `killpg`, modes, Unix sockets). Choice: a private `src/unix.rs` with its concern files,
  called only by `mac.rs` and `linux.rs`. It is not a 4th OS module.
- **Q3. Other targets.** Today `procid.rs` and `spawn.rs` have `android` and "other OS" branches.
  The release builds 5 targets of the 3 OSes. Choice: `horch-platform` refuses any other target
  with `compile_error!`, and the `android` branches go away. No supported OS changes behavior.
- **Q4. What test code is.** PL-2 says "Test code is not in scope". Choice: `crates/horch-e2e`
  (fake harness binaries and end-to-end tests), each `tests/` folder, each `tests.rs`, each
  `#[cfg(test)]` module (also after other attributes) and each item behind a `cfg` that names
  `test`. The test helper at `harness/claude.rs:1116` calls the platform, so the rule needs no
  item-level skip.
- **Q5. Runtime OS matches.** PL-2 forbids `cfg`, and lists "the sandbox host" as a concern. The
  code of today matches `std::env::consts::OS` at runtime in `harness/claude.rs:1082`
  (`sandbox_host_problem`), `claude.rs:287`, `:1454`, `harness/launch.rs:1300`,
  `runtime/machine.rs:99`, `project_notes.rs:108` and `herdr-install/src/manifest.rs:94`, `:442`.
  Choice: these stay in Phase P. They are not `cfg`, their tests run each OS branch on every host,
  and a move would change test names. The rule of PL-2 does not see them. Ticket 14 records this.
- **Q6. No OS name for core.** Choice: the platform gives facts and actions (for example
  `path::private_link_home_supported`, `shell::CHECK_SHELL`), never a function that returns the OS
  name for core code to branch on.
- **Q7. Env reads.** `arc_05_no_ambient_env_in_core` keeps env reads in `runtime/`. Choice: the
  platform gives the variable names (`HOME` or `USERPROFILE`, `APPDATA`, `LOCALAPPDATA`,
  `PATHEXT`), and the caller reads the variable as today.
- **Q8. No shim.** `arc_25_no_shim_modules` refuses a file that only re-exports and a `pub use`
  of another crate. Choice: callers call `horch_platform` directly, or a core function keeps its
  own logic (for example the error text of today).
- **Q9. Tests keep their place.** PL-6 compares each test by name. Choice: no move ticket
  deletes, renames or moves a test. A private helper that a core test calls today becomes a
  `pub` function of the platform crate (`filetime_to_system`, `parse_vm_stat`, `parse_meminfo`).
- **Q10. Linux and Windows builds.** This Mac has Homebrew Rust with only `aarch64-apple-darwin`.
  A `cargo check --target` for another OS fails on the C code of the bundled SQLite without a
  cross compiler. Choice (orchestrator, option 2): each ticket compiles and tests macOS, runs
  `cargo check --target` only when the target is installed, and CI on the draft pull request of
  `fleet/fleetd-build` gives Linux and Windows. A CI fault after a land comes back as a fix task.
- **Q11. CI logs for PL-6.** The compare of ticket 17 needs a complete test log for each OS at
  `scripts/pl6/BASE` and at the tip. CI runs `cargo test --workspace --locked` today, which stops
  at the first failing test binary. Choice: Phase 0 (the Windows test job and the runner of the
  list of expected failures) must give a complete log. If a log is not complete, ticket 17 sends
  BLOCKED. Workers can read CI and cannot start it.
- **Q12. The gate needs new CI runs.** A fix of ticket 17 needs a new CI run. Choice: ticket 17
  commits, sends a NOTE with the commit, and ends its turn; the orchestrator puts the commit on
  the branch of the draft pull request and tells it when CI is in.
- **Q13. Links to Phase 0.** Phase P needs the list of expected failures of Phase 0 and, for
  Linux and Windows, its CI test job. The tickets do not name Phase 0 files. The orchestrator
  connects tickets 01 and 02 to the gate of Phase 0.
- **Q14. Names of platform functions.** The concern tickets propose names. A worker may choose a
  better name and states the final name in the module doc of the concern file. The move tickets
  read those docs.
- **Q15. Windows dependencies.** Today `windows-sys` has 5 features, `zip` and `tar` serve the
  self-update archive, and `winreg` the user PATH. Choice: `horch-platform` takes them with the
  same versions and features (ticket 01), and ticket 16 removes them from the crates that no longer
  use them.
