# 02b: Addendum to ticket 02 (T-3). The fix does not pass in the real sandbox

Read ticket 02 first: `docs/tickets/formal-verification-intake/02-fix-intake-role-brief-spawn.md`.
This addendum replaces its Steps. Own and Do not touch stay the same.

## Facts

- Commit `f46c5f79` is on branch `fleet/solo-w60-T3-fix-the-refused-brief-spawn-of-skill-imp`.
  It adds `procid::is_same_sandboxed` (sysctl `KERN_PROC_PID` start time) as a
  fallback for the agent identity check in `runs_in`.
- That build is installed in `~/.local/bin/horch` (14:03 build).
- skill-importer-1 ran `intake spawn auditor` again with it. horch refused again
  with the same TRF-43 refusal. So `runs_in` still returns false in the real
  Claude Code sandbox.
- The ledger row of skill-importer-1 is correct:
  - `agent_pid` = 18990, `agent_started` = 1794775131080384 (the claude process).
  - `supervisor_pid` = 18342 (horch launcher, parent of claude).
- Process chain outside the sandbox: claude 18990 -> horch 18342 -> herdr 41305 -> launchd 1.
- In the test profile `(deny process-info*) (allow process-info* (target same-sandbox))`,
  `PROC_PIDTBSDINFO` worked on the process itself. The real Claude Code
  sandbox profile is stricter or different. `parent_pid` may return `None`
  for processes inside the sandbox too.

## Steps

1. Add a sysctl fallback to `parent_pid` on macOS: read `kinfo_proc` for
   `KERN_PROC_PID` and take `kp_eproc.e_ppid`. Try it when
   `PROC_PIDTBSDINFO` returns fewer bytes than its size.
   Check: a unit test with fake `Procs` covers "primary parent query fails, fallback gives the parent".
2. Add 1 `#[ignore]` probe test in `crates/horch/src/cmd/spawn.rs`, named
   `trf_43_probe_runs_in_from_env`. It reads `HORCH_PROBE_AGENT_PID` and
   `HORCH_PROBE_AGENT_STARTED`. It calls `runs_in` with `Procs::os()`. It prints
   each pid of the walk, each primary and fallback result, and the final bool.
   It asserts the final bool is true. This is a test only. Production code
   reads no new env var.
   Check: `cargo test -p horch -- --ignored trf_43_probe_runs_in_from_env` builds.
3. Run `cargo test -p horch -- spawn` and `cargo test -p horch-e2e --test skill_intake_spawn`.
   Check: both exit 0.
4. Commit the exact files. Use explicit paths.
5. Send QUESTION: to the orchestrator with the exact path of the probe test
   binary under `target/debug/deps/`. The orchestrator asks skill-importer-1 to
   run it inside the real sandbox, and sends you the output. End your turn.
6. If the probe fails, fix the failing call from its output. Repeat step 5.
7. When the probe passes in the real sandbox, run `cargo build --release -p horch`.
   Send QUESTION: to the orchestrator to install it. End your turn.
8. After the orchestrator confirms the install, run `horch ledger finish`.

## Constraints

- Keep the security property of ticket 02: only the recorded agent process or
  a descendant passes. A non-descendant still fails.
- Do not message skill-importer-1. Do not type into its pane.
- Remove every debug print from production code before the last commit.
