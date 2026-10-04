# U10 a2-runtime report (phase A2)

Branch `ard/a2-runtime`. Requirements ARC-05, ARC-06, ARC-07. Base:
`arch-refactor-dataset` at 28cb4cc (A3, A5, A9, B2, B3 and B4 merged). The
rebase over A9 had 1 conflict, in `skills.rs` `Bundle::briefing`. I kept A9's
`briefing::render` body and moved it into `briefing_in(teammate, home)`.

## Result

- `crates/horch/src/bootstrap.rs` reads the process environment once into a
  `RuntimeContext`. `main` passes `&mut RuntimeContext` to every command.
- No `set_var` or `remove_var` is left in `horch-core/src` or `horch/src`
  outside tests (baseline: 44 environment access sites).
- `horch-core` reads the environment only in `runtime/` and in the
  `teammates.rs` PENDING entry.
- Brief v2 carries every `HORCH_*_BIN` override. The worker gives its agent
  the brief's `transport_env()` on the child command only.
- `HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate` prints `GATE GREEN`.
  `scripts/check-req-coverage.sh --phase A2` passes. The base had no failing
  tests.

## Commits

1. `A2: Add RuntimeContext and EnvSource`
2. `A2: Move path and binary resolution into runtime/`
3. `A2: Thread RuntimeContext through core and the horch commands`. It holds
   the plan's 3 groups "Add horch lib and bootstrap", "Remove ambient env
   from core modules" and "Brief v2 and child-only transport env". They share
   every call site, so no split of them compiles alone.
4. `A2: Add ARC-05..07 tests`
5. This report.

## RuntimeContext (`horch_core::runtime`)

| Group | Field | Source |
|---|---|---|
| `paths` | `cwd` | the current directory (new; `--cwd` default, relative labels) |
| | `project_dir: Option` | `HORCH_PROJECT_DIR`, else cwd. `paths.project()` gives the old error |
| | `state_root` | `HORCH_STATE_DIR`, else `${XDG_STATE_HOME:-$HOME/.local/state}/horch` |
| | `state_override` | `HORCH_STATE_DIR` only (passed on to panes and briefs) |
| | `data_root` | `${XDG_DATA_HOME:-$HOME/.local/share}/horch` (OD3) |
| | `temp_root` | `std::env::temp_dir()` (`EnvSource::temp_dir`) |
| | `home` | `HOME` (`USERPROFILE` on Windows), else `.` |
| `herdr` | `workspace: Option<WorkspaceId>` | `HORCH_WORKSPACE_ID` |
| | `pane: Option<PaneId>` | `HERDR_PANE_ID` |
| `bins` | `roster_override` | `HORCH_TEAMMATES_DIR` |
| | `current_exe`, `horch_exe` | `current_exe()`; sibling `horch`, else PATH, else `horch` |
| | `overrides: BinOverrides` | `HORCH_{CLAUDE,CODEX,OPENCODE,PI,PRIME,HERDR,SQLITE3,OLLAMA,GIT}_BIN` (`GIT` is new) |
| | `harness: HarnessBins` | the overrides, else the default names; claude: override, `cpx` on PATH, `claude` |
| `settings` | `now`, `now_unparsable` | `HORCH_NOW` |
| | `tiling` | `HORCH_TILE` (`0`/`false`/`no`/`off` = `Disabled`) |
| | `balance_override` | `HORCH_BALANCE` (raw) |
| | `quota_file` | `HORCH_QUOTA_FILE` |
| | `machine_file` | `HORCH_MACHINE_FILE` (read, no consumer yet) |
| | `probe_timeout` | `HORCH_PROBE_TIMEOUT_MS` |
| | `faults: Faults` | `HORCH_FAULT` (comma-separated) |
| `inherited` | `home_var` | `HOME` exactly as set (plugin registry, Claude settings, roster) |
| | `path`, `pathext` | `PATH`, `PATHEXT` |
| | `opencode_config_content` | `OPENCODE_CONFIG_CONTENT` (raw) |
| | `codex_home` | `CODEX_HOME` |
| | `claude_code_effort_level` | `CLAUDE_CODE_EFFORT_LEVEL` (raw) |
| | `pi_session_dir`, `opencode_db`, `xdg_data_home` | `PI_CODING_AGENT_SESSION_DIR`, `HORCH_OPENCODE_DB`, `XDG_DATA_HOME` |
| | `local_app_data` | `LOCALAPPDATA` |
| | `hostname`, `herdr_session` | `HOSTNAME`/`COMPUTERNAME`, `HERDR_SESSION` |
| `worker` | `Option<WorkerEnv>` | `HORCH_{ROLE,TEAMMATE,AGENT,MODEL,RECORD_ID,SESSION_ID,RESUME,TASK}` |

Empty values count as unset where the old code filtered them. "Raw" fields
keep an empty value, as the old reads did.

`EnvSource` has 5 methods: `var`, `var_os`, `current_dir`, `current_exe`,
`temp_dir`. The plan named 3; the design names `current_exe`; `temp_dir`
replaces the core `std::env::temp_dir()` calls.

## Other new API

- `runtime::process`: `which`, `which_in`, `path_extensions`,
  `make_executable`, `path_with_prepended`, `path_with_own_dir`, `on_path_in`,
  `strip_forbidden`, `inherit_env` (adds a variable unless the command
  already sets or removes it; never a `FORBIDDEN_ENV` key), `spawn_detached`.
- `RuntimeContext::{apply_overrides, refresh_bins, prepend_own_dir_to_path}`.
- `messaging::brief::{Brief, SCHEMA}`; `mailbox::Brief` is a re-export.
  `Brief::{from_json, overrides, set_overrides, workdir_or_project,
  transport_env}`. Writers write `schema: 2` and keep `claude_bin` and
  `codex_bin`.
- `Mailbox::{under, in_context, resolve(herdr, temp_root, workspace, pane),
  resolve_in, register(herdr, temp_root, pane, role), register_in}`.
  `Mailbox::new` is gone. `register` no longer calls `set_var`;
  `register_in` writes the workspace into the context.
- `Ledger::open_in(ctx)`, `with_project`, `with_workspace`. `insert` fills
  `project` and `workspace_id` from these, not from the environment.
- `clock::install(now, unparsable)`: bootstrap pins the clock once.
- `launch::{LaunchEnv, command_in, command_with_skills_in, teammate_env}`.
  `apply_env` is deleted.
- `Bundle::configure(teammate, home)`, `Bundle::briefing_in(teammate, home)`,
  `Bundle::apply_env(cmd, teammate, inherited)`.
- `plugins::{resolve(teammate, plugin, home), resolve_all_in,
  installed_plugins_in, resolve_in(.., home)}`.
- `codex::codex_home(home, codex_home_var)`;
  `Rules::install(home, codex_home, state_root, role, rules)`.
- `usage::Locations::{from_context, under_home(home, inherited)}`.
- `telemetry::lock::this_process(now, ctx)`;
  `Collector::open_in(ctx, loc, probing, now)` and `with_faults`.
  `Collector::open` and `open_at` now read no environment (no
  `HORCH_BALANCE`, no quota file, default names, scratch under the state
  root). The tests use them.
- `routing::snapshot::QuotaEnv::from_context` with a new `bins: ProbeBins`
  field; `ProbeBins::{new, from_context}`. `ProbeBins::from_env` is deleted.
- `horch` lib: `bootstrap::context()`, `output`, `exit::{SUCCESS, FAILURE,
  COLLECTOR_HELD, REFUSED}`.
- `cmd::{load_roster, path_text}` in the binary.

## PENDING allowlist (`crates/horch-core/tests/arch_scan.rs`)

Only `teammates.rs`: `Roster::load` and `Roster::load_with` read `HOME` and
`HORCH_TEAMMATES_DIR`. Their callers are `tests/baseline_oracles.rs`,
`tests/nfr.rs` and roster unit tests. The scan fails when an entry no longer
reads the environment, so the list can only shrink. `roster/`, `quota.rs`,
`policy.rs` and `routing/` are not in the list, as the orchestrator asked.

## Shims that still read the environment through `ProcessEnv`

They contain no `std::env` call, so the scan passes. A12 removes them.

- `agent::*_bin`, `agent::which`, `agent::on_path`, `agent::home_dir`.
  Callers: `workspace/herdr.rs` (`Herdr` has no bin field yet), `opencode.rs`,
  `prime.rs`, `telemetry/readers.rs`, `tests/common/mod.rs`.
- `ledger::state_root`, `ledger::project_dir`, `Ledger::open`. Caller:
  `message.rs::spool_dir` (`horch tell` now builds the spool path from the
  context).
- `launch::command`, `launch::command_with_skills`, `Bundle::briefing`:
  `LaunchEnv::from_process()`. Callers: the oracle tests and launch unit tests.
- `plugins::resolve_all(teammate)`. Caller: `roster/validation.rs`.

## Exceptions in `crates/horch/src`

`bootstrap.rs` is the only file that reads the process environment, except:
- `cmd/worker.rs` and `cmd/recipes.rs` call `std::env::set_current_dir`. It
  changes the working directory, not a variable, as before. A relative
  `HORCH_TEAMMATES_DIR` resolves from the project dir only this way.
- Test code: `HORCH_BLESS` in `cmd/telemetry.rs`, `current_dir` in
  `cmd/recipes.rs` tests, and `vars_os` in the arc_06 test (read only).

## Decisions

- The clock is a process-wide pin that bootstrap installs. Threading `now`
  through every ledger and routing call was out of scope. The stderr notice
  still prints at the first clock read only.
- `HORCH_WORKSPACE_ID`, `PATH` (own dir first), the brief's transport
  variables, the teammate's `env` and `CLAUDE_CODE_SUBAGENT_MODEL` go to the
  agent through `inherit_env`. A value the builder set on the command wins,
  as it did over inherited variables. `FORBIDDEN_ENV` is stripped again last.
- `pane_launch` gives its agent `HORCH_WORKSPACE_ID`, `HORCH_STATE_DIR`,
  `HORCH_RECORD_ID`, `HORCH_TEAMMATES_DIR` and `PATH` the same way.
- The worker builds its context from the brief: project, state dir and every
  override (`ctx.apply_overrides`). The smoke worker's `horch note` and
  `horch done` children get the same child environment.
- `horch spawn` loads the roster with `HORCH_TEAMMATES_DIR` both as the
  override and as the explicit dir, as `Roster::load_with(env)` did.
- `launch.rs` tests that set `HOME` now pass a `LaunchEnv` with a fake home.
  `ledger.rs` `tel_08` uses `with_workspace`. The `HORCH_TILE` test moved to
  `runtime/context.rs`. The `agent.rs` tests moved to `runtime/bins.rs` and
  `runtime/process.rs`, on `MapEnv`.
- I edited `teammates.rs`, `routing/quota_probe.rs` and `routing/snapshot.rs`
  on the orchestrator's instruction.

## Behavior differences (deliberate, small)

- `RuntimeContext::from_env` fails when `HORCH_WORKSPACE_ID` or
  `HERDR_PANE_ID` holds a control character (id validation).
- `horch teammates --new` with `HORCH_TEAMMATES_DIR=""` now reports "no target
  directory" instead of failing to create `""`.
- A missing current directory fails only the commands that need it, with the
  same top-level message, but without the OS error text.
- `skills.rs` unit tests pass `home = None`, so they no longer read the
  operator's real `~/.claude`.

## Gotchas for later phases

- A4: `launch.rs` builders take `&LaunchEnv`. Fold it into the `Harness`
  trait's `ctx`. `worker.rs::agent_command` is the one place the worker
  command is built; `arc_06_transport_env_applied_to_child` tests it.
- A4/A6: `Brief.workdir` exists but nothing sets it yet.
  `workdir_or_project()` is ready for `SpawnRequest.workdir`.
- A6: `Settings.faults` has only `has()`. Add `indexed` and `abort_if` (design
  §4.2) when the new fault points land.
- A7 part b: `tilecmd::settle_after_close` keeps its own setsid code; switch
  it to `runtime::process::spawn_detached`. `Herdr` still resolves its binary
  through `agent::herdr_bin()`; give it `ctx.bins.harness.herdr`.
- A12: delete the shims above, `teammates.rs`, and the PENDING entry.
- B2: `settings.machine_file` is read but unused.
- The e2e fake-herdr `exec` scenario passes the spawner's environment to the
  pane. The arc_07 e2e test therefore runs `horch worker` itself, without the
  3 overrides, to prove that they come from the brief.

## Tests

34 added, 13 removed (12 `agent.rs` tests and 1 `tilecmd.rs` test, all
moved). The named ones:
- `runtime::context::tests::arc_05_context_from_map_env`
- `tests/arch_scan.rs`: `arc_05_no_ambient_env_in_core`,
  `arc_05_scan_skips_tests_and_comments`, `arc_06_env_mutation_sites_reduced`
- `cmd::worker::tests::arc_06_transport_env_applied_to_child`
- `messaging::brief::tests::arc_07_brief_v1_readable`
- `crates/horch-e2e/tests/brief.rs`: `arc_07_e2e_bin_overrides_reach_worker`

No golden, oracle or serialization golden changed.
