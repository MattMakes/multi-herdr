# Live check: Linux

The workspace tests pass on aarch64 Linux, which runs `procid`'s Linux paths
(pidfd signals, `group_members`, `started_by`, `stat_field`), and Godot on
Linux writes only under `XDG_DATA_HOME`, `XDG_CONFIG_HOME` and
`XDG_CACHE_HOME`. Items U-29, U-24 (Linux part).

## How to run

```bash
just test-linux        # = scripts/live/linux.sh
```

It needs `colima` and the `docker` CLI. A missing tool is `SKIP`. It starts
no model session, so it costs no tokens. A full run takes about 20 minutes
on a 4-CPU colima VM: the container builds the workspace from nothing.

The script starts colima when it is stopped and stops it again at exit,
also on failure. It builds no image: every step is 1 `docker run --rm` of
the official `rust:1-bookworm` image (`linux/arm64`). That image stays
after the run (2.1 GB on disk in `docker images`; 529 MB in
`docker image inspect`). The source is a `git archive HEAD` snapshot, mounted
read-only, so other workers' uncommitted edits in the shared checkout do not
break the build. The target dir, `CARGO_HOME` and `HOME` are inside the
container and go away with it. Logs go to `.worktrees/_scratch/live-linux/`.

- `HORCH_LINUX_STEPS="cargo-test"` or `"godot"` runs 1 part.
- `HORCH_LINUX_SRC=worktree` mounts the checkout as it is;
  `HORCH_LINUX_SRC=<dir>` mounts that directory.
- `HORCH_LINUX_IMAGE` picks another image.

Steps:

- `colima`: colima runs (started by the script when it was stopped).
- `cargo-test`: `cargo test --workspace --locked --no-fail-fast` with
  `HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1`. The container installs
  `sqlite3`, `bubblewrap` and `socat` (Claude Code's Linux sandbox needs the
  last 2, and a test checks the host for them), then runs the tests as a
  normal user: root ignores file modes, and
  `pro_07_cleanup_failure_recorded_history_intact` makes a directory
  read-only to force a failure. `/bin/sh` stays dash, the Debian and Ubuntu
  default.
- `godot-sha`: download `Godot_v4.7.2-stable_linux.arm64.zip` from the
  `4.7.2-stable` GitHub release and check it with the release's
  `SHA512-SUMS.txt`.
- `godot-version`: `--headless --version`.
- `godot-xdg`: with `HOME` and the 3 `XDG_*` set to empty scratch
  directories, `--headless --import` and 1 `--headless --script` that
  writes `user://probe.txt`. Then `find / -xdev -newer <marker>` lists every
  file written outside the scratch directories; the list must be empty, and
  nothing may be written under `HOME`.

## 2026-10-06

Host: macOS 26.5.1 (arm64), colima 0.10.3 (`vz`, 4 CPU, 4 GiB), docker CLI
23.0.1. Container: Linux 6.8.0-117-generic aarch64, Debian bookworm,
rustc 1.99.0, git 2.39.5, sqlite3 3.40.1.

| Step | Claim it proves | Tool version | Result | Evidence |
|------|-----------------|--------------|--------|----------|
| colima | The script starts and stops colima. | colima 0.10.3 | PASS | `just test-linux` from a stopped colima: `PASS colima: started`, and at exit `colima stopped`. Run time 19 min 43 s. |
| cargo-test (HEAD `db614fb`, before the fix) | The workspace tests pass on Linux. | rustc 1.99.0 | FAIL | 1036 passed, 40 failed, 6 ignored. 37 failures are 1 bug: `signal_groups` in `crates/horch-e2e/src/process.rs` ran `kill -TERM -- -<pgid>`, and dash's `kill` rejects `--` ("Illegal number"). fake-herdr `pane close` and the e2e teardown killed nothing: 36 tests failed with "the test leaked N process(es)", plus `fake_herdr_close_kills_the_exec_process`. The other 3 are the sandbox tests below. Fixed in `cd6d9f5`. |
| cargo-test, `procid` | `procid`'s Linux paths work (`crates/horch-core/src/procid.rs`: pidfd `signal_same`, `group_members`, `started_by`, `stat_field`). | rustc 1.99.0 | PASS | All 9 `procid::tests` pass, `stat_field_22_after_a_command_with_spaces` (Linux only) among them. `procid.rs` needs no change. |
| cargo-test (HEAD `cd6d9f5`, with the fix) | The workspace tests pass on Linux. | rustc 1.99.0 | FAIL, 4 tests | 1097 passed, 4 failed, 6 ignored. 3 are the sandbox test bug below. The 4th is a flake under load: `cmp_10_resume_projects_with_the_recorded_estimate` in this run, `g5_done_without_an_orchestrator` in the run before it (`b322192` plus the fix: 1084 passed, 4 failed). |
| godot-sha | The official Linux build is the one the release publishes. | Godot 4.7.2 | PASS | `Godot_v4.7.2-stable_linux.arm64.zip: OK` against `SHA512-SUMS.txt`. |
| godot-version | The Linux build runs headless in a container with no display. | Godot 4.7.2 | PASS | `4.7.2.stable.official.ed1daf0bf` |
| godot-xdg | `skills/godot-build-verify/SKILL.md` step 3: "On Linux, Godot reads `XDG_DATA_HOME` and `XDG_CONFIG_HOME`". | Godot 4.7.2 | PASS, claim incomplete | Written: `xdg/config/godot/editor_settings-4.7.tres`, `xdg/data/godot/app_userdata/LiveLinux/{probe.txt,logs/godot.log}`, `xdg/cache/godot/editor_doc_cache-4.7.res`. Nothing under `HOME`, nothing elsewhere. `OS.get_user_data_dir()` = `$XDG_DATA_HOME/godot/app_userdata/LiveLinux`. The skill omitted `XDG_CACHE_HOME`, which `--import` writes, and marked the claim not run. Fixed in `115bbef`. |

Failures that are not in `procid` and that this unit does not fix, from the
runs with the fix:

- `harness::launch::tests::no_launch_carries_the_anthropic_api_key`,
  `harness::claude::tests::a_sandbox_with_a_settings_file_is_refused`,
  `harness::claude::tests::a_sandboxed_launch_writes_the_sandbox_block_or_refuses`:
  `LaunchEnv::for_test` (`crates/horch-core/src/harness/launch.rs`) has
  `path: None`, so on Linux `sandbox_host_problem` never finds `bwrap` or
  `socat` and every sandboxed launch is refused. The claude tests'
  `host_can_sandbox()` reads the real `PATH`, so they expect a launch. They
  fail on any Linux host; macOS checks a fixed `sandbox-exec` path and
  passes. The fix goes to W1 u0, which owns these files; re-run
  `just test-linux` after it lands.
- Load flakes, 1 for each full run: `g5_done_without_an_orchestrator` (the
  worker does not reach `running` in 30 s) and
  `cmp_10_resume_projects_with_the_recorded_estimate` (PRE-09 refuses the
  round: projected $2.62 over the $1.50 ceiling). Each passes alone 3 of 3
  times in the same container. Both pass on macOS.
