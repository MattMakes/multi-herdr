# 01: Run the task check in a sandbox

Status: proposed. Target: `main` at `06930d95`. Source of the idea: fleetd at `9bc3bf70`.
Report id: P2 (R4), with finding CR-05-F1 (R5). Size: M.

Labels: `CODE` (read at the pinned commit), `MEASURED` (read-only query that I ran), `REPORT` (a report says it, and I did not check it), `NOT VERIFIED`. A path with no commit is at MAIN. `FINISH:` marks a path at `9bc3bf70`.

## Problem

The problem is real on MAIN. A worker writes code, and horch then runs a check command over that code. The check runs as the operator. A wrong or hostile check can read the private files of the operator, send them over the network, and change files outside its worktree. The check also receives each environment variable of the worker, so a token in the environment reaches it. A Codex worker has an OS sandbox, but its check can read and write the horch state folder and has the open network.

MAIN has 3 check runners. The finish runner has 2 branches, so 4 code paths have a fault:

1. Finish steps `check`, `recheck` and `land`. `check_process` starts a plain shell for each worker that is not Codex. `CODE crates/horch-core/src/taskledger/finish/land.rs:218`, `:235`. The condition is `cx.tools.codex.runs(cx.worker) == false`. `CODE land.rs:246`. Two call sites use it. `CODE land.rs:295`, `:592`.
2. Codex branch of the same function. It runs `codex sandbox` with the profile `fleet_write`. That profile makes the state root a workspace root and sets `network.enabled=true`. `CODE crates/horch-core/src/harness/codex.rs:884`, `:894`, `:897`, `:904`. This is the MAIN form of CR-05-F1.
3. `horch ledger run gate`. `run_gate` starts a plain shell. `CODE crates/horch-core/src/taskledger/runs.rs:791`.
4. `horch worktree land --check`. `Landing::run_check` starts a plain `sh`. `CODE crates/horch-core/src/vcs/landing.rs:907`, `:908`. The reports do not name this runner.

Path 4 has one more defect. It does not call `scrub_child_env`, so it passes the forbidden variable of `FORBIDDEN_ENV` to the check when the shell of the operator has it. `CODE landing.rs:907-920`, `crates/horch-core/src/harness/launch.rs:226`, `crates/horch-core/src/runtime/process.rs:147`. This breaks the first rule of `CLAUDE.md`.

## Evidence on main

No count of a stolen secret or of a harmful check exists. The ledger does not record what a check reads. The counts below give the exposure: how many checks ran with each set of rights.

Query (`MEASURED`, 2026-11-22, `sqlite3 -readonly ~/.horch/ledger.sqlite3`, data from 2026-10-31 to 2026-11-22):

```sql
SELECT count(*) total, sum(w.harness='codex') codex, sum(coalesce(w.harness,'')<>'codex') full_rights
FROM task_steps s JOIN workers w USING(worker_id)
WHERE s.name IN ('check','recheck') AND s.status IN ('passed','failed');
-- total 508, codex 94, full_rights 414
SELECT count(*), sum(exit_code=0), sum(exit_code<>0) FROM run_gates;
-- 22 gates, 19 passed, 3 failed. Each gate ran with full rights.
```

The first count is low by an unknown number, because the `land` step runs the check again and records no `check` row. `CODE land.rs:592`.

Commands that the sandbox can affect (`MEASURED`, 423 tasks with a check command): 156 use `cargo`, 99 use `godot`, 68 use Python, 14 use `just`, 0 name `git`, and 0 use `--offline` or `--frozen`.

Real examples (`MEASURED`):

1. Run `solo-sp211b731ae5b59e14653018cb253e66d0`, task 10 (`e3069e1d`), worker `opus-hardening-2`, Claude, 2026-11-22T22:31:02Z: `cargo test --workspace --locked ...` ran with full rights.
2. Run `going-ham-playable`, task 17 (`98611a8f`), worker `godot-ui-developer-1`, Codex, 2026-11-18T04:18:15Z: a `godot` check ran with `fleet_write`.

## How fleetd solves it

The rule: a check runs in an OS sandbox that can write only its work folders, cannot do the operator acts, and has no off-host network.

- `sandboxed_local` wraps a command for this rule. `CODE FINISH:crates/horch-platform/src/sandbox.rs:90`.
- `sandboxed_hiding` also hides folders and allows chosen paths in them. `CODE FINISH:crates/horch-platform/src/sandbox.rs:204`.
- `denying` makes a denial win over each other rule. `CODE FINISH:crates/horch-platform/src/sandbox.rs:264`.
- macOS uses `sandbox-exec`. `CODE FINISH:crates/horch-platform/src/mac/sandbox.rs:23`, `:34`.
- Linux uses `bwrap`. `CODE FINISH:crates/horch-platform/src/linux/sandbox.rs:41`, `:65`.
- Windows returns `NotSupported`. `CODE FINISH:crates/horch-platform/src/windows/sandbox.rs:11`.
- `platform_launch` calls the wrapper for a task check. `CODE FINISH:crates/horch-core/src/fleetd/worker_profiles/adapters/check_sandbox.rs:221`, `:247`.

The idea to port is the Seatbelt and bubblewrap rule set, the denial order, and the fail-closed result. The roster holds, needs-you items, operator token, channel socket and job process exist only because fleetd is a daemon. Do not port them.

Do not copy these 4 fleetd defects:

1. CR-05-F1: the Codex branch keeps `codex sandbox` with broad reads and network. `CODE FINISH:check_sandbox.rs:280`, `:291`. `REPORT research/07-confirm-codex-findings.md`.
2. `sandboxed_local` can read the whole disk except a short deny list. `CODE FINISH:sandbox.rs:46-49`.
3. `find_bwrap` searches `PATH`. This is safe in fleetd because fleetd owns its `PATH`. On MAIN the worker starts `horch ledger finish`, so the worker controls `PATH`. `CODE FINISH:linux/sandbox.rs:109`, `crates/horch/src/cmd/taskledger_finish.rs:140`.
4. On macOS, `Network::Local` also allows the LAN addresses of the host (R8-F2). No Seatbelt rule separates them from loopback. `CODE FINISH:mac/sandbox.rs:78`, `REPORT FINISH:docs/specs/fleetd/gates/macos-check-sandbox-lan.md:40`.

## Fix

The boundary: the sandbox limits the check process. It does not limit a worker that has no OS sandbox, because that worker can run the same command directly. The fix closes the check path for sandboxed workers and contains a wrong check for all workers.

### Requirements

1. `H01-R1`: All 3 runners start the check through 1 function, `check_sandbox::wrap`. No harness has a separate check path. Codex checks stop using `codex sandbox`.
2. `H01-R2`: The check can write only its check folder (`dir`), a new scratch folder with mode `0700`, and `/dev/null`.
3. `H01-R3`: The check cannot read the home folder, `/tmp` or the per-user temp folder. It can read `dir`, the git common folder, the scratch folder, the default tool folders of `H01-R4` and each path in the operator setting `read`.
4. `H01-R4`: The default tool folders below home are `.cargo`, `.rustup`, `.local/bin`, `.local/share/uv`, `.local/share/mise`, `.pyenv`, `.nvm`, `.bun` and `.asdf`. They are read-only.
5. `H01-R5`: The check can never read, write or connect below a sealed path, whatever another rule allows. Sealed paths: the active and default horch state roots, `~/.horch`, the active ledger file with its `-wal` and `-shm` files, the evidence root, `~/.ssh`, `~/.gnupg`, `~/.aws`, `~/.claude`, `~/.claude.json`, `~/.codex`, `~/.config/gh`, `~/.config/horch`, `~/.netrc`, `~/.cargo/credentials` and `~/.cargo/credentials.toml`. If a sealed folder holds `dir`, horch refuses to run the check.
6. `H01-R6`: The check cannot write `<dir>/.git` or the git common folder. When `dir` is the project root (shared checkout), it also cannot write `<root>/.worktrees`.
7. `H01-R7`: The check starts from an empty environment. horch copies only `PATH`, `LANG`, `LC_ALL`, `LC_CTYPE`, `TERM`, `USER`, `LOGNAME`, `SHELL` and `TZ`. horch sets `HOME`, `TMPDIR`, `TMP`, `TEMP` and the 4 `XDG_*_HOME` variables to folders in the scratch folder. It sets `CARGO_HOME` and `RUSTUP_HOME` to the real tool folders when they exist.
8. `H01-R8`: horch finds the real home with `getpwuid_r`, not `$HOME`. It uses `/usr/bin/sandbox-exec` on macOS, and `/usr/bin/bwrap` or `/bin/bwrap` on Linux. It never finds the sandbox tool through `PATH`.
9. `H01-R9`: The default network rule is `local`: loopback and Unix sockets that the file rules allow. horch sets `CARGO_NET_OFFLINE=true` in this mode, so `cargo` fails at once with a clear error.
10. `H01-R10`: 1 task asks for the network with the ticket line `**Check-Network:** open` or `horch ledger add --check-network open`. A gate asks with `horch ledger run gate --check-network open`, and a worker gets a refusal. `horch worktree land --check` is always `local`. Mode `open` changes only the network rule.
11. `H01-R11`: On macOS the profile also denies `lsopen`, `appleevent-send`, read and write of `/dev/tty`, and the Mach service `com.apple.coreservices.launchservicesd`.
12. `H01-R12`: On Linux the sandbox hides `/run/user/<uid>`. On both systems it denies connect to `/var/run/docker.sock` and `/run/docker.sock`.
13. `H01-R13`: Before each check, horch runs `/bin/sh -c :` in the same sandbox. If the sandbox tool is missing or this probe fails, the check step fails with the verdict `no sandbox` and names the cause and the fix. horch never falls back to an unsandboxed run unless `H01-R14` applies.
14. `H01-R14`: On a host with no sandbox (Windows, or Linux with no `bwrap`), the check runs only when the operator file sets `"unsandboxed": true`. Each such run writes 1 warning line in the check log.
15. `H01-R15`: A failed sandboxed check names the write, read and network rules in the step note and in the log, so a worker can tell a sandbox denial from a code fault.

### Files of MAIN to change

1. New `crates/horch-core/src/runtime/check_sandbox.rs`, registered in `runtime/mod.rs`. Port `Network`, `Rule`, `denying`, `resolved` and the macOS and Linux `args` from FINISH. Use `std::process::Command` directly. Do not port the `Start` trait or the crate `horch-platform`.
2. `crates/horch-core/src/taskledger/finish/land.rs`: `check_process` calls `wrap`. `check_in` drops the Codex branch and keeps the scratch folder until the check ends. Add `set_check_network` and `check_network` beside `set_check_timeout`.
3. `crates/horch-core/src/taskledger/finish/mod.rs`: replace `CodexSandbox` with `CheckSandbox` in `Tools`. `CheckSandbox` has no `Default`, so no caller gets an open sandbox by accident.
4. `crates/horch-core/src/taskledger/runs.rs`: `run_gate` takes `&CheckSandbox` and the network rule.
5. `crates/horch-core/src/vcs/landing.rs`: `run_check` uses `wrap`. Add `with_check_sandbox`. `crates/horch/src/cmd/worktree.rs:13` passes it.
6. `crates/horch-core/src/taskledger/map.rs`: `ticket_facts` reads `**Check-Network:**`. Follow the `**check:**` branch at `:247`.
7. `crates/horch/src/cmd/taskledger.rs`: build `CheckSandbox` in `tools` (`:2406`), add the flags to `add` and `run gate`, write the network file in the `add` and `add-map` paths, and call `orchestrator_only` (`:533`) for a gate with `open`.
8. Test literals of `Tools`: `land.rs`, `crates/horch-core/tests/w3_finish_steps.rs`, `cms_comment_sweep.rs`, `trf_18_shared_diff_base.rs` and `trf_27_review_loop.rs`.
9. `.github/workflows/ci.yml`: install `bubblewrap` in the Ubuntu test job and allow unprivileged user namespaces. Without this, the Linux sandbox tests cannot run. `CODE .github/workflows/ci.yml:13`.
10. Delete `codex::sandbox_args` if the compiler reports it as unused. Keep the worker launch profile.

### New settings

1. File `<real home>/.config/horch/check-sandbox.json`. An environment variable cannot hold this setting, because the worker sets the environment of `horch ledger finish`.
2. `read`: a list of absolute paths. Default `[]`, because each added path widens what a check can read. Sealed paths still win.
3. `unsandboxed`: Default `false`, because a missing sandbox must stop the check and not remove the protection.
4. Task file `<evidence root>/<task>/check-network`, with the value `open`. Default: no file, which means `local`. This copies the pattern of `check-timeout-s` and needs no ledger migration. `CODE land.rs:28-45`.

### What is not ported

The daemon, operator token, channel, needs-you items, roster fallback, job process and `horch-platform` crate are not ported. MAIN has no such parts. A worker sandbox is out of scope. A Windows sandbox is out of scope, because no tool exists in the code at FINISH.

### Effect at upgrade

1. Ledger: no schema change. Old rows stay valid.
2. Running worker: its next `horch ledger finish` uses the sandbox. A check that needs the network or a hidden path can fail.
3. Recovery for a running task: the operator writes `open` to its `check-network` file, or adds a path to `read`.
4. Existing worktrees and runs need no change.
5. Windows: each check stops until the operator sets `unsandboxed`. Put this in the release note.

## Reproduce

### Automated (NOT RUN)

The limits of this run forbid `cargo`. Write this test first.

- Crate and file: `horch-core`, new `crates/horch-core/tests/check_sandbox.rs`.
- Harness to copy: `World` in `crates/horch-core/tests/w3_finish_steps.rs:52`.
- Test name: `h01_a_check_cannot_read_home_or_write_outside_its_folder`.
- Setup: create `<top>/home/secret` with the text `probe-secret`. After the fix, give `CheckSandbox` the fake home `<top>/home`.
- Action: use the check `cat <top>/home/secret > <top>/leak`, then call `finish`.
- Assertion: `assert!(!top.join("leak").exists())`.
- MAIN today: the assertion fails, because the check runs with full rights and `<top>/leak` holds `probe-secret`.

### Manual (NOT RUN)

Use a plain terminal outside a fleet pane. The scratch variables keep the live ledger safe.

1. `S=$(mktemp -d); export HORCH_LEDGER_DB="$S/ledger.sqlite3" HORCH_STATE_DIR="$S/state"`
2. `P=$(mktemp "$HOME/.horch-check-probe.XXXXXX"); echo probe-secret > "$P"`
3. `git init -q -b main "$S/repo"; cd "$S/repo"; git commit -q --allow-empty -m base`
4. `horch ledger run new probe --steps check,land,cleanup`
5. `horch ledger run gate --run probe --check "cat $P"`. MAIN prints `Run gate passed on <sha>. Log: <path>.`, and the log holds `probe-secret`.
6. `HORCH_CHECK_CANARY=1 horch ledger run gate --run probe --check 'test -n "$HORCH_CHECK_CANARY"'`. MAIN prints `Run gate passed`.
7. `horch ledger run gate --run probe --check 'curl -sS -m 5 https://example.com -o /dev/null'`. With a network, MAIN prints `Run gate passed`.
8. `horch ledger run gate --run probe --check 'touch "$(git rev-parse --git-common-dir)/probe"'`. MAIN prints `Run gate passed`, and `.git/probe` exists.
9. Clean up: `cd /; rm -f "$P"; rm -rf "$S"`.

### Static proof (RUN, 2026-11-22)

```text
$ git grep -c -i sandbox 06930d95 -- crates/horch-core/src/taskledger/runs.rs crates/horch-core/src/vcs/landing.rs
(no output: 0 matches in the gate and worktree-land runners)
```

## Validate

### Automated (each NOT RUN)

Each test goes in `crates/horch-core/tests/check_sandbox.rs`, unless the item names another file.

1. `H01-R1` `h01_r1_each_runner_uses_the_sandbox`: finish, gate and worktree land each fail to read the fixture. MAIN: fails for all 3.
2. `H01-R2` `h01_r2_writes_only_dir_and_scratch`: a write beside `dir` is refused. MAIN: fails.
3. `H01-R3` `h01_r3_home_and_temp_are_hidden`: reads of the fake home and a `/tmp` fixture are refused. MAIN: fails.
4. `H01-R4` `h01_r4_tool_folders_are_read_only`: a read below `.cargo` works, and a write is refused. MAIN: the write check fails.
5. `H01-R5` `h01_r5_sealed_path_wins_over_read`: a sealed file listed in `read` stays unreadable. MAIN: fails.
6. `H01-R6` `h01_r6_git_folder_is_read_only`: `git status` works, and a write to `.git/config` is refused. MAIN: the write check fails.
7. `H01-R7` `h01_r7_env_is_an_allow_list`: `HORCH_CHECK_CANARY` is absent, and no `FORBIDDEN_ENV` name is in the allow list. MAIN: fails. Do not set the forbidden variable in this test.
8. `H01-R8` `h01_r8_fake_home_and_path_do_not_move_rules`: a fake `HOME` and a fake `bwrap` first on `PATH` change nothing. MAIN: fails, because no rule exists.
9. `H01-R9` `h01_r9_local_network`: a loopback listener of the check connects, and an off-host connect is refused. MAIN: the refusal fails.
10. `H01-R10` `h01_r10_ticket_asks_for_network`: in `map.rs`, the ticket line parses, and a worker gate with `open` is refused. MAIN: fails.
11. `H01-R11` `h01_r11_macos_no_launch_escape` (macOS only): `open`, `osascript` and `launchctl submit` do not create a marker file. MAIN: fails.
12. `H01-R12` `h01_r12_linux_runtime_dir_hidden` (Linux only): a Unix socket in the fake runtime folder refuses the connect. MAIN: fails.
13. `H01-R13` `h01_r13_probe_failure_fails_closed`: a failed probe gives the verdict `no sandbox`, and the check does not run. MAIN: fails.
14. `H01-R14` `h01_r14_opt_in_is_explicit`: with no sandbox, the check runs only with `unsandboxed: true`, and the log has the warning. MAIN: fails.
15. `H01-R15` `h01_r15_failure_names_the_rules`: a denied check note names the rules. MAIN: fails.
16. Regression guard: `h01_real_check_still_runs` runs `cargo --version` and `git status` in the sandbox. MAIN: passes, so this test proves no fix. It only guards compatibility.

A test that starts `sandbox-exec` fails when the repository check already runs in the sandbox, because a nested `sandbox-exec` is refused. `REPORT FINISH:docs/specs/fleetd/gates/macos-check-sandbox-lan.md:45`. horch sets `HORCH_IN_CHECK_SANDBOX=1` in the check. These tests skip only when it is set, and CI never sets it. Production code must ignore this variable.

### Manual (NOT RUN)

Repeat steps 1 to 4 and 9 of `## Reproduce`. Expected output after the fix:

1. Step 5 prints `Run gate failed on <sha>: exit 1.` The log has `Operation not permitted` on macOS or `No such file` on Linux, and has no `probe-secret`. MAIN passes and shows the secret.
2. Step 6 prints `Run gate failed`. MAIN passes.
3. Step 7 prints `Run gate failed`. MAIN passes.
4. Step 8 prints `Run gate failed`, and `.git/probe` does not exist. MAIN passes and creates it.
5. In the multi-herdr repository, run `cargo test --workspace --locked` as a gate with a warm cargo cache. It must pass. This also passes on MAIN, so it proves compatibility only.

### Ledger count after 2 weeks (NOT RUN)

```sql
SELECT count(*) ran, sum(s.status='failed') failed, sum(s.verdict='no sandbox') no_sandbox
FROM task_steps s WHERE s.name IN ('check','recheck')
AND s.status IN ('passed','failed') AND s.started_at >= '<release date>';
```

Baseline (`MEASURED`, 2026-11-22): 508 ran and 14 failed (2.8%). Target: `no_sandbox` is 0 on macOS and Linux, and the failed rate stays below 5%. Read each new failure with a sandbox denial in its log, and add a default only when the path holds no secret.

## Size and risks

Size: M, 4 worker-days. Porting the rules takes 1 day, the 3 runners and settings take 1 day, tests and CI take 1.5 days, and live checks on macOS and Linux take 0.5 days.

### Risks

1. `RK1`: A hidden path breaks a real check. Scratch `HOME` covers tools that write config, such as Godot. `NOT VERIFIED`. Run the 4 main check families (`cargo`, `godot`, Python, `just`) before release.
2. `RK2`: `cargo` may need a write in a read-only `CARGO_HOME` for its package lock. `NOT VERIFIED`. Do not make `CARGO_HOME` writable, because a check could then plant code that the operator later runs. If it fails, give the check a scratch `CARGO_HOME` that reads a copy or a source replacement.
3. `RK3`: A sandboxed Claude teammate that runs `horch ledger finish` is already in Seatbelt, so `sandbox-exec` fails and `H01-R13` stops its check. 7 teammates declare `sandbox:` (`CODE teammates/*.md`). Whether they run `finish` is `NOT VERIFIED`.
4. `RK4`: `(allow default)` leaves other Mach services open on macOS. `H01-R11` closes the known launch paths, but it does not prove that no other path exists. `NOT VERIFIED`.
5. `RK5`: On macOS a `local` check can reach loopback services and LAN addresses of the host. Blocking them makes 24 fleetd tests fail. `REPORT FINISH:docs/specs/fleetd/gates/macos-check-sandbox-lan.md`.
6. `RK6`: A check can still change tracked and ignored files in `dir`, and an unsandboxed worker can write `.git` directly. horch passes only `core.hooksPath=/dev/null` to git, so a `core.fsmonitor` value that a worker plants can still run. `CODE crates/horch-marketplace/src/git.rs:55`, `:182`. Fix this in a separate change.
7. `RK7`: An unsandboxed worker can edit `check-sandbox.json` or a `check-network` file. This matches the boundary stated at the top of `## Fix`.
8. `RK8`: Ubuntu runners can restrict user namespaces, which stops `bwrap`. `NOT VERIFIED`. `H01-R13` makes this a clear failure.

### Files that other specs can also change

`finish/land.rs`, `finish/mod.rs`, `runs.rs` and `cmd/taskledger.rs` are likely shared with spec 09 (verdict tied to commit) and spec 10 (ticket and ledger compare). `map.rs` is likely shared with spec 10. `NOT VERIFIED`: those specs did not exist when I wrote this one. Land this spec first, or rebase on the `Tools` field change.

### Open questions for the operator

1. `Q1`: Is the default network `local`, with `open` per task? I recommend yes. The cost is that a check with a cold dependency cache fails until the cache is warm.
2. `Q2`: Does Windows stop each check until the operator sets `unsandboxed`? I recommend yes. The ledger has 0 Windows hosts (`MEASURED`: 2 hosts, both macOS names), and `main` still ships a Windows build. `CODE .github/workflows/release.yml:57`.
3. `Q3`: Is the hidden-home rule acceptable, or do you prefer the smaller deny-list rule of fleetd? I recommend hidden home, because a deny list misses each secret that it does not name.
