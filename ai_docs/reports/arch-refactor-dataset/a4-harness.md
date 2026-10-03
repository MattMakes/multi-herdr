# A4 a4-harness: harness modules own command lines and capabilities

Unit U18. Branch `ard/a4-harness`. Requirements ARC-09, ARC-10, ARC-11.
Status: complete. The gate is green after a rebase onto `arch-refactor-dataset`
(B3, B6 and A7b merged).

## Module map

| Module | Owns |
|---|---|
| `harness/mod.rs` | `HarnessKind`, `adapter()`, `capabilities()`, `Harness` trait, `Prepared`, `PrepareRequest`, `CommandSpec`, `generic_validate`, `Workdir` |
| `harness/capabilities.rs` | `Capabilities`, `SkillExposure`, one `const` row per harness, `HARNESS_FOOTPRINT_BYTES`, `NONE_FOOTPRINT_BYTES` |
| `harness/launch.rs` | `LaunchEnv`, `Session`, `FORBIDDEN_ENV`, `teammate_env`, `model_for`, `command(_in)`, `command_with_skills(_in)`, and the launch flow: `run_flow`, `LaunchRequest`, `DiscoveryTarget`, `session_for`, `agent_command`, `Discovery` |
| `harness/claude.rs` | `claude_command`, `overlay_skill_switches`, `AMBIENT_SKILL_CREATOR`, adapter `Claude` |
| `harness/claude_plugins.rs` | old `plugins.rs`, plus `overlay_plugin_skills` |
| `harness/codex.rs` | old `codex.rs` (rules, rollouts) + `codex_command` + adapter `Codex` |
| `harness/opencode.rs` | old `opencode.rs` + `opencode_command`, `opencode_variant_config` + adapter `OpenCode` |
| `harness/pi.rs` | `pi_family_command` (Prime calls it with its own binary) + adapter `Pi` |
| `harness/prime.rs` | old `prime.rs` (daemon) + adapter `Prime` |
| `harness/none.rs` | adapter `NoAgent` (smoke) |

Shims until A12: `launch.rs`, `codex.rs`, `opencode.rs`, `prime.rs` and
`plugins.rs` are glob re-exports (`pub use crate::harness::<x>::*;`). The
moves used `git mv`, so `git log --follow` works.

## Capabilities

| Harness | caller_minted_session | resumes | effort | daemon | exec_policy | skill_exposure | tool_lists | tool_denylist | headless | footprint | local_model |
|---|---|---|---|---|---|---|---|---|---|---|---|
| claude | yes | yes | low..max | no | no | PluginDir | yes | yes | yes | 600 MiB | no |
| codex | no | yes | none, low..max | no | yes | CodexHome | no | no | no | 600 MiB | no |
| opencode | no | yes | none, minimal..max | no | no | ConfigPaths | no | no | no | 600 MiB | no |
| pi | yes | yes | off, minimal..max | no | no | SkillFlag | yes | yes | no | 600 MiB | yes |
| prime | no | yes | off, minimal..max | yes | no | SkillFlag | yes | no | no | 600 MiB | no |
| none | no | no | (empty) | no | no | None | no | no | no | 64 MiB | no |

- `Capabilities::discovers_session()` = `resumes && !caller_minted_session`
  (the old `harvests_session_id`).
- `Capabilities::footprint(local_model_bytes)` replaces the match in
  `competition/preflight.rs` (orchestrator-approved). The values are unchanged.
- The legacy predicates on `HarnessKind` delegate to capabilities. A12
  removes them.
- `effort` is the list of valid levels, not a bool (the unit plan asks for
  this). `roster/effort.rs` reads it. The per-model rule
  (`Harness::model_takes_effort`) lives in the claude and opencode adapters.

## Launch flow API (A6's service calls this)

```rust
harness::launch::run_flow(ctx: &RuntimeContext, req: LaunchRequest<'_>) -> Result<ExitCode>
LaunchRequest { role, teammate, session: &SessionMode, prompt, model_override,
                exec_rules: &[ExecRule], child_env: Vec<(String, String)>,
                record: Option<DiscoveryTarget { mailbox, record_id, workdir }> }
```

Order: `Bundle::install` → `adapter().prepare` (codex rules and private
`CODEX_HOME`, Prime daemon socket and session dir) → teammate + `extra_args`
→ `agent_command` (builder, teammate env, child env, strip) → `Prepared.env`
(set last, wins) → strip `FORBIDDEN_ENV` → discovery thread when
`discovers_session()` and the session is fresh and `record` is `Some` → run →
stop discovery → `Prepared::finish` (codex home cleanup, then daemon stop).

`cmd/worker.rs launch_agent` and `cmd/recipes.rs pane_launch` only build a
`LaunchRequest`. `worker::start_harvest`, both `run` helpers and every
`mints_session_id()`/`harvests_session_id()`/`runs_a_daemon()`/
`uses_execpolicy()` call outside `harness/` are gone.

## Deviations from the design (§4.6)

- `Harness::command` is the per-harness builder. `build_command` is a
  provided method that calls it and strips `FORBIDDEN_ENV`. No adapter
  overrides `build_command`.
- `build_command` takes `(&LaunchEnv, &CommandSpec)`, not `(ctx, plan,
  prepared)`. `LaunchPlan` does not exist before A6, and the orchestrator
  asked to fold `LaunchEnv` into the trait context. The flow applies
  `Prepared.extra_args` to the teammate and `Prepared.env` to the command.
- `discover_session` is `discover_sessions(ctx, workdir, since,
  sessions_dir) -> Vec<String>`. It returns candidates newest first, and the
  flow skips ids the ledger already holds. `sessions_dir` is Prime's private
  directory from `Prepared`.
- The `resumes` field is named for the A1 rule (`resume: bool` is banned).
- The `Capabilities` struct also has `footprint_bytes` and `local_model`
  for the preflight.

## Behavior changes (intentional, small)

- A `SessionMode::Fresh(None)` on a caller-minted harness launches as
  `Session::Unmanaged`. Before, `horch worker` passed `--session-id ""`.
  `horch spawn` always mints an id, so this case does not occur in a normal
  run.
- In `pane_launch`, the teammate's `env` block no longer leaks
  `ANTHROPIC_API_KEY`. Before, there was no strip after `inherit_env`.
- Discovery that cannot start (marker write, ledger open) is a warning on
  stderr in both flows. Before, the worker failed its launch and
  `pane_launch` ignored the error.
- A Prime orchestrator pane would get its own daemon. No such pane kind
  exists today.
- Codex rollout matching parses each `"cwd"` value and compares it through
  `Workdir`, instead of matching a substring of the exact JSON spelling.

## Tests added (10)

- `arc_09_argv_matches_baseline` (`baseline_oracles.rs`): runs all 33+
  teammates × {fresh, resume, unmanaged} through `adapter().build_command`
  and compares the result with the frozen A0 launch oracle. It only
  compares and never writes, even under `HORCH_BLESS=1`. `launch_oracle`
  now takes the build function. `oracle_launch_matches` still uses
  `command_with_skills` and passes unchanged.
- `arc_10_capabilities_match_legacy_predicates` (`harness/mod.rs`, with a
  private copy of the old predicate and effort table).
- `arc_10_harness_match_only_in_harness` and
  `arc_10_scan_tells_matches_from_comparisons` (`arch_scan.rs`).
- `arc_10_session_for_follows_capabilities` (`tests/harness.rs`).
- `arc_11_codex_discovery_by_workdir`, `arc_11_opencode_discovery_by_workdir`,
  `arc_11_canonical_tmp_paths` (`tests/harness.rs`).
- `harness_codex_rules_snapshot`, `harness_prime_daemon_args_snapshot`
  (`tests/harness.rs`). They compare against fixtures in
  `tests/fixtures/harness/`. Commit 1 captured these fixtures from the
  pre-move code, and the tests now run through the adapters.

## Gotchas for later phases

- The ARC-10 scan counts match arms and `matches!` only. A comparison
  (`t.agent == Agent::None`, including one in a match guard) or an
  assignment does not count. Comparisons that remain outside `harness/`:
  `cmd/worker.rs` (smoke), `cmd/spawn.rs`, `cmd/teammatescmd.rs`,
  `skills/selection.rs`, `routing/eligible.rs`, `skills.rs`.
- `HARNESS_MATCH_PENDING = ["skills.rs"]`. A10 must remove that entry when it
  moves skill exposure into the adapters (`SkillExposure` is ready). The scan
  fails if the entry is stale.
- `Prepared` is not `Send`, because its finish steps are `Box<dyn FnOnce()>`.
  Dropping a `Prepared` without calling `finish` cleans up the codex home
  (through `Rules::drop`), but it does not stop a Prime daemon. That matches
  the old error paths. Tests must drop a Prime `Prepared` and never finish
  it, because `Daemon::finish` runs the real `prime-agent status`.
- `prime::Daemon::finish` still resolves the binary through `agent::prime_bin()`
  (process env). `opencode::find_sessions` now takes the binary as a
  parameter.
- `LaunchEnv::from_process`, `launch::command` and `command_with_skills`
  (process env) stay for the oracle tests. A12 removes them.
- `harness/launch.rs` uses `Herdr::with_bin(&ctx.bins.harness.herdr)` for
  the discovery thread, which follows A7b.
- No `teammates.rs` wrapper lost its last caller, so arc_05 PENDING is
  unchanged.
