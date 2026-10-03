# U10 a2-runtime: RuntimeContext, bootstrap, no ambient env in core

Unit slug: `a2-runtime`. Branch: `ard/a2-runtime`. Phase: A2.
Requirements: ARC-05, ARC-06, ARC-07.

## GOAL

The `horch` binary reads the process environment once, in
`crates/horch/src/bootstrap.rs`, into a `RuntimeContext`, and passes
`&RuntimeContext` (or the values taken from it) inward. Core modules do not
read or mutate the process environment, except the files that the A3 and A5
units own (listed below). Worker environment travels to the child process
only (`cmd.envs()`), never through `std::env::set_var`. Brief v2 carries every
binary override. Behavior is unchanged.

## CONTEXT

- Read first: `ai_docs/plans/arch-refactor-dataset/01-fleet-conventions.md`.
- Read in `00-master-plan.md`: "Hard constraints", §1 (layout: `runtime/`,
  `crates/horch/src/{lib,bootstrap}.rs`), §2 "RuntimeContext", §3 "A2", §4
  ARC-05..07 rows.
- Read the design doc `ai_docs/designs/2026-10-02-architecture-refactor-design.md`
  if it is on your base (section "Key types": RuntimeContext, EnvSource,
  ProcessEnv, MapEnv, Brief v2). If it is absent, use the master plan.
- Read the reports of merged units in `ai_docs/reports/arch-refactor-dataset/`:
  `a1-vocabulary.md` (ids, `HarnessKind`, `TilingMode`, `SessionMode`) and
  `machine-teacher.md` (`runtime/machine.rs` already exists;
  `runtime/mod.rs` declares it).
- Environment access today (44 sites, from
  `grep -rn 'std::env::\|env::var\|set_var\|remove_var' crates/horch-core/src crates/horch/src`):
  `agent.rs` 27 (the `*_bin()` resolvers with `HORCH_*_BIN`, `which`,
  `home_dir`, `prepend_own_dir_to_path`), `cmd/worker.rs` 15
  (`export_brief` sets `HORCH_ROLE`, `HORCH_TEAMMATE`, ..., `HORCH_CLAUDE_BIN`,
  `HORCH_CODEX_BIN`, `HORCH_TEAMMATES_DIR`), `cmd/recipes.rs` 13,
  `cmd/smoke.rs` 9, `ledger.rs` 8 (`state_root`, `project_dir`, `home_dir`,
  `HORCH_WORKSPACE_ID`), `launch.rs` 7 (`apply_env` sets
  `CLAUDE_CODE_SUBAGENT_MODEL` and teammate env; `OPENCODE_CONFIG_CONTENT`;
  a test that sets `HOME`), `cmd/spawn.rs` 5, `teammates.rs` 5,
  `mailbox.rs` 5 (`HORCH_WORKSPACE_ID`, `HERDR_PANE_ID`, `set_var` in
  `register`), `cmd/tilecmd.rs` 4, `cmd/telemetry.rs` 3, `cmd/messaging.rs` 3,
  `quota.rs` 3, `cmd/install.rs` 2, `telemetry/collect.rs` 2 (`HORCH_FAULT`),
  1 each in `main.rs`, `teammatescmd.rs`, `route.rs`, `balancecmd.rs`,
  `usage.rs`, `telemetry/lock.rs`, `skills.rs` (`OPENCODE_CONFIG_CONTENT`),
  `policy.rs` (`HORCH_BALANCE`), `plugins.rs` (`HOME`), `codex.rs`
  (`CODEX_HOME`), `clock.rs` (`HORCH_NOW`).
- Brief today: `crates/horch-core/src/mailbox.rs` `struct Brief` carries only
  `claude_bin` and `codex_bin` overrides. That is the bug that Brief v2 fixes.
- Parallel units at the same time as you:
  - U11 `a3-roster` owns `teammates.rs` (and the new `roster/`), including
    its 5 env reads.
  - U12 `a5-routing` owns `quota.rs`, `policy.rs`, `balance_policy.rs` (and the
    new `routing/`), including their env reads.
  - U09 `a7a-workspace` moves `herdr.rs`, `layout.rs`, `tile.rs`,
    `balance.rs`, `paneshell.rs` into `workspace/`. It does not touch
    `tilecmd.rs`.
  Do not edit their files. Where your new context must reach a function they
  own, pass a plain value at the call site in `crates/horch/src/cmd/*` and
  leave a `// A2: <field> comes from RuntimeContext once <unit> takes it`
  comment if their function still reads env.
- Phase A4 comes after you and reworks `launch.rs`, `worker.rs launch_agent`
  and `recipes.rs pane_launch`. You may edit the env-related lines in those
  files; do not restructure them.

## FILES

own:
- `crates/horch-core/src/runtime/{context,paths,bins,process,fault}.rs` (new)
- `crates/horch-core/src/runtime/mod.rs` (add your modules)
- `crates/horch-core/src/agent.rs` (becomes a shim over `runtime::bins` and `runtime::process`)
- `crates/horch-core/src/ledger.rs`, `mailbox.rs`, `clock.rs`, `usage.rs`,
  `launch.rs` (env lines only), `skills.rs` (env line only), `plugins.rs`,
  `codex.rs`, `telemetry/collect.rs`, `telemetry/lock.rs`
- `crates/horch/src/lib.rs` (new), `crates/horch/src/bootstrap.rs` (new),
  `crates/horch/src/main.rs`, `crates/horch/Cargo.toml` (a `[lib]` section if needed)
- `crates/horch/src/cmd/*.rs` (env reads and call sites only), except you do
  not move logic out of `tilecmd.rs`
- `crates/horch-core/tests/arch_scan.rs` (new; the source-scan tests)
- `crates/horch-e2e/tests/brief.rs` (new; `arc_07_e2e_bin_overrides_reach_worker`)
- `ai_docs/reports/arch-refactor-dataset/a2-runtime.md`

do not touch: `teammates.rs`, `quota.rs`, `policy.rs`, `balance_policy.rs`,
`herdr.rs`, `layout.rs`, `tile.rs`, `balance.rs`, `paneshell.rs`,
`workspace/**`, `roster/**`, `routing/**`, golden files, oracle files.

## STEPS

1. Create the worktree (conventions §2).
2. `runtime/context.rs`:
   - `pub trait EnvSource { fn var(&self, key: &str) -> Option<String>; fn var_os(&self, key: &str) -> Option<OsString>; fn current_dir(&self) -> Option<PathBuf>; }`
   - `pub struct ProcessEnv;` (the only type in core that calls `std::env`)
     and `pub struct MapEnv { vars: BTreeMap<String, String>, cwd: Option<PathBuf> }`
     for tests. Empty values count as unset where today's code filters them.
   - `pub struct RuntimeContext` with groups, each a plain struct:
     `paths: Paths { project_dir, state_root, data_root, temp_root, home }`,
     `herdr: HerdrEnv { workspace: Option<WorkspaceId>, pane: Option<PaneId> }`,
     `bins: Bins { roster_override: Option<PathBuf>, current_exe: Option<PathBuf>, horch_exe: PathBuf, harness: HarnessBins, overrides: BinOverrides }`,
     `settings: Settings { now: Option<DateTime<Utc>>, tiling: TilingMode, balance_override: Option<String>, quota_file: Option<PathBuf>, machine_file: Option<PathBuf>, probe_timeout: Option<Duration>, faults: Faults }`,
     `inherited: Inherited { opencode_config_content: Option<String>, codex_home: Option<PathBuf>, claude_code_effort_level: Option<String>, ... }`,
     `worker: Option<WorkerEnv>` (the `HORCH_ROLE`, `HORCH_TEAMMATE`, ... values
     a worker pane receives).
   - `RuntimeContext::from_env(env: &dyn EnvSource) -> Result<RuntimeContext>`.
     Every rule that a resolver applies today (empty value = unset,
     `XDG_STATE_HOME` fallback, `USERPROFILE` on windows, the claude bin
     search order in `agent.rs:14-25`) moves here unchanged.
   - `BinOverrides` holds every `HORCH_*_BIN` override:
     claude, codex, opencode, pi, prime, herdr, sqlite3, ollama, git
     (`HORCH_GIT_BIN` is new). `HarnessBins` holds the resolved paths.
   - `data_root` is `${XDG_DATA_HOME:-$HOME/.local/share}/horch` (OD3).
3. `runtime/paths.rs`: move `state_root`, `project_dir`, `home_dir` logic out
   of `ledger.rs` (lines 177 to 205) as pure functions of `&dyn EnvSource`.
   `ledger::state_root()` etc. become thin wrappers that call them with
   `ProcessEnv`, marked `#[deprecated(note = "A2: pass RuntimeContext")]`
   only if that does not break `-D warnings` (check: the gate does not deny
   warnings today; if it does, skip the attribute).
4. `runtime/bins.rs`: the `*_bin()` resolvers from `agent.rs` as functions of
   `&BinOverrides`. `agent.rs` keeps its public functions as shims that call
   these with overrides read through `ProcessEnv`.
5. `runtime/process.rs`: `which`, `which_in`, `make_executable`,
   `path_with_prepended`, `on_path_in` (pure) move here; and
   `spawn_detached(cmd: &mut Command) -> Result<Child>` copied from the setsid
   code in `crates/horch/src/cmd/tilecmd.rs` (`settle_after_close`, the setsid call at about lines 647 to 655). Leave the
   tilecmd copy in place; A7 part (b) switches it.
6. `runtime/fault.rs`: `pub struct Faults(BTreeSet<String>)` parsed from
   `HORCH_FAULT` (comma-separated). `faults.has("after-append")`. Replace
   the 2 reads in `telemetry/collect.rs` with a `Faults` value that the
   caller passes.
7. `crates/horch/src/lib.rs` with `pub mod bootstrap; pub mod output; pub mod exit;`
   (move `output` and exit-code helpers from `main.rs` if they exist; if
   `exit` does not exist, create it with the exit-code constants that
   `main.rs` uses today). `bootstrap.rs`:
   `pub fn context() -> Result<RuntimeContext> { RuntimeContext::from_env(&ProcessEnv) }`.
   `main` builds the context once and passes `&ctx` to every command
   function. Convert each `cmd/*.rs` env read to a `ctx` field.
8. Core modules: remove every env read and `set_var` in the files you own.
   - `mailbox.rs`: `Mailbox::resolve` and `register` take the workspace and
     pane ids as parameters. `register` no longer calls `set_var`; it returns
     the workspace id and the caller keeps it in the context or passes it on.
   - `ledger.rs`: the `HORCH_WORKSPACE_ID` read at line 363 becomes a
     parameter.
   - `clock.rs`: `HORCH_NOW` becomes `settings.now`, passed in.
   - `usage.rs` `Locations`: built from `Paths` and `Inherited`.
   - `codex.rs` `CODEX_HOME`, `plugins.rs` `HOME`, `skills.rs` and `launch.rs`
     `OPENCODE_CONFIG_CONTENT`: parameters.
   - `launch.rs` `apply_env`: delete it. The teammate env and
     `CLAUDE_CODE_SUBAGENT_MODEL` go onto the child `Command` with
     `cmd.envs(...)` where the command is built. Keep the `FORBIDDEN_ENV`
     removal.
9. `messaging` transport: create `crates/horch-core/src/messaging/mod.rs` and
   `messaging/brief.rs`. Move `Brief` from `mailbox.rs` into
   `messaging/brief.rs` (shim `pub use crate::messaging::brief::Brief;` in
   `mailbox.rs`). Brief v2: add `#[serde(default)] schema: u32` (v1 files
   have no field and read as 1; new files write 2), `workdir: Option<String>`,
   and `bin_overrides: BinOverrides` (`#[serde(default)]`), keeping
   `claude_bin`/`codex_bin` readable for v1 (fold them into
   `bin_overrides` on read). Add
   `pub fn transport_env(&self) -> Vec<(String, String)>` that returns every
   `HORCH_*` variable that `export_brief` sets today, plus every
   `HORCH_*_BIN` override. Replace `worker.rs export_brief` with applying
   `transport_env()` to the agent child `Command` only. The worker's own
   in-process lookups read the brief (or the context built from it), not the
   process env.
10. Tests:
    - `crates/horch-core/tests/arch_scan.rs`:
      `arc_05_no_ambient_env_in_core`: scan every `.rs` under
      `crates/horch-core/src/`; fail on `std::env::var`, `env::var_os`,
      `set_var`, `remove_var`, `std::env::current_dir`, `std::env::temp_dir`
      outside `runtime/` and outside `#[cfg(test)]` modules. A
      `const PENDING: &[&str]` allowlist holds exactly
      `teammates.rs`, `roster/`, `quota.rs`, `policy.rs`, `routing/`, with
      a comment that A3 and A5 remove their entries. Do not add other entries.
      `std::env::consts` is allowed.
      `arc_06_env_mutation_sites_reduced`: count `set_var`/`remove_var`
      outside tests in both crates; assert fewer than 44 (and print the count).
    - `arc_05_context_from_map_env` (in `runtime/context.rs` tests): a
      `MapEnv` with every variable builds the expected context; an empty map
      gives the defaults.
    - `arc_06_transport_env_applied_to_child`: build the worker child command
      from a brief; assert `get_envs()` holds every transport var and
      `ANTHROPIC_API_KEY` is removed; assert the test process env did not change.
    - `arc_07_brief_v1_readable`: a v1 brief JSON (today's shape, with
      `claude_bin` and `codex_bin`) reads; `schema` is 1; overrides fold in.
    - `arc_07_e2e_bin_overrides_reach_worker` (in
      `crates/horch-e2e/tests/brief.rs`): with the e2e harness, spawn a worker
      with `HORCH_OPENCODE_BIN`, `HORCH_PI_BIN`, `HORCH_SQLITE3_BIN` set to
      fakes; assert the worker side sees them (the brief file on disk has them,
      and the fake's recorded env has them). Read `crates/horch-e2e/src/harness.rs`
      and existing e2e tests for the pattern.
11. Gate after every step. Commit per step group:
    `A2: Add RuntimeContext and EnvSource`, `A2: Move path and binary resolution into runtime/`,
    `A2: Add horch lib and bootstrap`, `A2: Remove ambient env from core modules`,
    `A2: Brief v2 and child-only transport env`, `A2: Add ARC-05..07 tests`.
12. Write and commit the report: the context fields, which env var maps to
    which field, the PENDING allowlist, and gotchas for A3, A4, A5, A6.
    Follow conventions §6 to finish.

## CONSTRAINTS

- No behavior change. All existing tests and oracle tests stay green (except
  failures that exist on the base; name them in your report).
- Tests that set env vars today (for example `ledger.rs:940`) should switch
  to `MapEnv`. Do not add new `set_var` calls in tests.

## DONE WHEN

- The 7 named tests pass.
- `arc_05_no_ambient_env_in_core` passes with only the PENDING entries.
- `crates/horch/src/bootstrap.rs` is the only file in the `horch` crate's
  `src/` that reads the process env (check with grep; list any exception and
  why in the report).
- The full gate is green apart from named base failures.

## REPORT

- `horch note` after each commit.
- `QUESTION:` before you edit a file outside `own`.
- `horch done` summary: context fields, env → field table, PENDING list,
  remaining exceptions, gotchas.
