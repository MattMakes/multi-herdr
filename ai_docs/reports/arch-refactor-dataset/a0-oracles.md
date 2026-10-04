# U02 a0-oracles: report

Phase A0. Requirement ARC-01. Branch `ard/a0-oracles`.

The oracle files capture what the pre-refactor code does. They are frozen.
A later phase moves the call sites in the two test files. It never changes an
oracle file. `HORCH_BLESS=1` rewrites the files; use it only at A0.

## Files

- `crates/horch-core/tests/baseline_oracles.rs`: oracles 1 to 4 and
  `arc_01_baseline_oracles_present`.
- `crates/horch-core/tests/oracles/{launch,routing,ledgers,skills}/`.
- `crates/horch/tests/baseline_cli.rs`: oracles 4 (CLI part) and 5.
- `crates/horch/tests/oracles/{skills,sessions}/`.

No file under `crates/*/src/` changed. No `Cargo.toml` changed: `tempfile` is
already a dev-dependency of `horch-core` and a dependency of `horch`.

## Oracle counts

| Directory | Files | Test |
|---|---|---|
| `horch-core/tests/oracles/launch` | 33 (1 per teammate) | `oracle_launch_matches` |
| `horch-core/tests/oracles/routing` | 13 (12 fixtures + `fallback_problems.json`) | `oracle_routing_matches` |
| `horch-core/tests/oracles/ledgers` | 12 (4 fixtures, 4 `.loaded.json`, 4 `.saved.json`) | `oracle_ledgers_match` |
| `horch-core/tests/oracles/skills` | 132 (33 teammates x 4 phases) | `oracle_skills_match` |
| `horch/tests/oracles/skills` | 10 | `oracle_cli_skills_match` |
| `horch/tests/oracles/sessions` | 8 (4 ledgers x text and JSON) | `oracle_cli_sessions_match` |

`arc_01_baseline_oracles_present` checks these counts and that no file is
empty. The counts are constants (`TEAMMATES_AT_A0 = 33`,
`QUOTA_FIXTURES_AT_A0 = 12`).

## Pinned environment (core tests)

`World::new()` in `baseline_oracles.rs` takes the static `ENV_LOCK` mutex,
pins these variables, and restores them on drop. Each launch oracle file
records them under `_pinned_env`.

- `HOME` = `<HOME>`: a temp dir. It holds `.claude/settings.json` (enabled
  plugin `alpha@oracle-market`, disabled `beta@oracle-market`, a
  `statusLine`) and `.claude/plugins/installed_plugins.json`
  (`skill-creator@oracle-fork`).
- `PATH` = `<TMP>/bin`: an empty dir. This removes the `cpx` lookup in
  `agent::claude_bin`, so the program is `claude`.
- Removed: `HORCH_CLAUDE_BIN`, `HORCH_CODEX_BIN`, `HORCH_OPENCODE_BIN`,
  `HORCH_PI_BIN`, `HORCH_PRIME_BIN`, `HORCH_TEAMMATES_DIR`,
  `OPENCODE_CONFIG_CONTENT`, `CODEX_HOME`, `CLAUDE_CODE_EFFORT_LEVEL`,
  `HORCH_NOW`, `ANTHROPIC_API_KEY`.

The CLI tests set `HOME` and `HORCH_STATE_DIR` to temp dirs,
`HORCH_PROJECT_DIR=/oracle/project`, `HORCH_TEAMMATES_DIR` to the repo
`teammates/`, and remove `ANTHROPIC_API_KEY`, `XDG_STATE_HOME`, `HORCH_NOW`,
`HORCH_BALANCE` and `HORCH_WORKSPACE_ID`.

Placeholders: `<BUNDLE>` (the skill bundle root, which has a random UUID),
`<HOME>`, `<TMP>`, `<PROMPT>` (the fixed text `ORACLE PROMPT: do the task.`).
The temp root is canonicalized first, so macOS `/private/var` paths are
replaced too.

## Oracle 1: launch (consumer A4 `arc_09_argv_matches_baseline`)

- Roster: `Roster::load_with(Some(<repo>/teammates))`, all names, hidden ones
  included.
- Per teammate: `skills::Bundle::install(<TMP>/state, &t)`, then
  `launch::command_with_skills(&t, session, PROMPT, None, bundle.as_ref())`
  for `Session::Fresh(FRESH_ID)`, `Session::Resume(RESUME_ID)` and
  `Session::Unmanaged`. With no bundle this is `launch::command`.
- Recorded: `program`, `args`, `env` (`get_envs()`; `null` = removed),
  `current_dir`, or `error` (`{e:#}`).
- 2 teammates record errors: `smoke` (agent `none`) and
  `orchestration-worker` (no model; the orchestration recipe supplies
  `model_override`, which this oracle does not pass).
- Not covered: codex `Rules` (private `CODEX_HOME`) and Prime `Daemon`
  args, which `cmd/worker.rs launch_agent` adds after the builder.
- How to call it at A4: build the same command through the harness
  adapter with the same inputs and compare the JSON from `launch_oracle`.

## Oracle 2: routing (consumer A5 `arc_12_decisions_match_baseline`)

- View: `QuotaFile::read(fixture)`, then `QuotaView::new(file,
  2026-09-28T18:00:00Z, Policy::default(), true)`. This is the path of the
  QUO unit tests in `quota.rs`.
- Per key `<teammate>|<none|exact|force>|<auto|advise|off>`:
  - `route_json`: the object `horch route --json` prints. `route.rs` builds
    it inline, so `route_json()` in the test copies that code.
  - `decision`: `serde_json::to_value(&Decision)`.
  - `resolved`: `name`, `agent`, `model`, `effort` of
    `balance_policy::resolve`, or `null` for a refusal.
- `fallback_problems.json`: `balance_policy::fallback_problems(&roster)`.
  It is `[]` for the repo roster.

## Oracle 3: ledgers (consumer A6 `arc_17_*`)

Hand-made fixtures, from the field history in `ledger.rs`:

- `bash-era.json`: only the bash fields (`record_id`, `session_id`, `agent`,
  `tier`, `model`, `role`, `status`, `task`, `history`, `created_at`,
  `updated_at`). One record has `session_id: null`.
- `pre-effort.json`: adds `phase` (#1). No `effort`, `kind` or later fields.
- `pr14-substituted.json`: `effort`, `project`, `plan`, `workspace_id`,
  `via`, `substitution_reason`.
- `orchestrator.json`: 1 `kind: orchestrator` record and 2 worker records.

`<name>.loaded.json` lists every `Record` field, defaults included, plus
`is_orchestrator`. `<name>.saved.json` holds the bytes that
`Ledger::write` produces. `Ledger::write` is private and no public method
saves without a change, so the test uses `serde_json::to_string_pretty`, the
same call `write` makes. The file has no trailing newline, because `write`
writes none. At A6, compare against the bytes the store writes.

## Oracle 4: skills (consumers A9 `skl_08_*`, A11 `mkt_09_*`)

- Core: per teammate x phase, clone the teammate, set `phase`, call
  `Bundle::install` and `Bundle::briefing`. `smoke` records
  `ERROR: ... disabled skills or no agent`.
- CLI: `horch skills`, `skills --json`, and `skills --phase <p>` with and
  without `--json`, from `CARGO_BIN_EXE_horch`.

## Oracle 5: sessions (consumer A6)

Each ledger fixture is copied to `Ledger::for_project(<state>,
"/oracle/project").path()`. Then `horch sessions` and `horch sessions --json`
run.

## Gotchas

- The comparison tests iterate the current roster. A new teammate has no
  oracle file, so the test fails on it. A unit that adds a teammate must
  change the test code to skip teammates with no oracle file. It must not
  add or edit oracle files.
- No shipped teammate sets `setting_sources`, so the `statusLine` overlay
  never shows in the launch oracle.
- The core env pins are process-wide. Any new env-touching test in
  `baseline_oracles.rs` must create a `World` (it holds `ENV_LOCK`).
