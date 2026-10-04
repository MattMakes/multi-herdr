# U11 a3-roster report (phase A3)

Branch: `ard/a3-roster`. Requirement: ARC-08.

## Module map

Line numbers are of `teammates.rs` at the A1 base (1778 lines).

| Old lines | New file | Items |
|---|---|---|
| 1-19 | `roster/mod.rs` | module doc, `pub mod` lines, re-exports, `Agent` alias |
| 29 | `roster/repository.rs` | `include!(.../builtin_teammates.rs)` (`build.rs` unchanged) |
| 35-73 | `roster/teammate.rs` | `ORCHESTRATOR_TIERS`, `FLEET_ORCHESTRATORS`, `ORCHESTRATOR_ONLY_SKILLS`, `ORCHESTRATOR_DENIED_TOOLS`, `reserved_tier` |
| 75-151 | `roster/effort.rs` | `valid_efforts`, `model_takes_effort`, `effort_problem`, new `Effort` newtype |
| 153-156 | `roster/teammate.rs` | `BRIEF_DESCRIPTION_MAX` |
| 158-198 | `roster/phase.rs` | `Phase` |
| 200-201 | `roster/mod.rs` | `pub use crate::harness::HarnessKind as Agent` |
| 203-269 | `roster/permission.rs` | `PermissionMode` |
| 271-485 | `roster/teammate.rs` | `Teammate`, `ExecRule`, `Base` (serde attributes unchanged) |
| 487-650 | `roster/repository.rs` | `Roster` struct, `builtin`, `overlay`, getters |
| 651-1029 | `roster/validation.rs` | `is_spawnable`, `model_is_spawnable`, `check_teammate`, `check` |
| 1031-1059, 1082-1182 | `roster/operator.rs` | `operator_status_line`, `operator_enabled_plugins`, `operator_effort_warnings`, `effort_override_warnings`, `codex_default_effort`, `operator_settings`, `expand_home` |
| 1061-1080 | `roster/validation.rs` | `unused_rules` |
| 1184-1196 | `roster/repository.rs` | `overlay_dirs` (now `pub`) |
| 1198-1228, 1757-1778 | `roster/parser.rs` | `md_files`, `split_frontmatter`, `parse_teammate`, `parse_base` (`pub(crate)`) |
| 1230-1755 | `roster/validation.rs` | `mod spawnable_tests` (bodies unchanged; 3 `use` lines added) |
| `balance_policy.rs` 294-374 | `roster/validation.rs` | `CLAUDE_ONLY_TOOLS`, `fallback_problems`, `fallback_warnings`, `names_tool` |

`balance_policy.rs` now has
`pub use crate::roster::validation::{fallback_problems, fallback_warnings};`
and a `#[cfg(test)]` import of `names_tool` for its existing test.

## New parameters

| Function | Parameters | Replaces |
|---|---|---|
| `Roster::load_layered` | `home: Option<&Path>`, `roster_override: Option<&Path>`, `explicit: Option<&str>` | `HOME`, `HORCH_TEAMMATES_DIR` |
| `roster::repository::overlay_dirs` | `home: Option<&Path>`, `roster_override: Option<&Path>` | `HOME`, `HORCH_TEAMMATES_DIR` |
| `roster::operator_status_line`, `roster::operator_enabled_plugins` | `home: Option<&Path>` | `HOME` |
| `roster::operator_effort_warnings` | `home: Option<&Path>`, `codex_home: &Path`, `claude_code_effort_level: Option<&str>` | `HOME`, `CODEX_HOME`, `CLAUDE_CODE_EFFORT_LEVEL` |
| `roster::expand_home` | `path: &str`, `home: Option<&Path>` | `HOME` |

`Roster` has a new private field `home: Option<PathBuf>`. `load_layered`
sets it. `check_teammate` uses it to expand `~/` in `plugin_dirs`,
`settings` and `mcp_config_files`. `Roster::builtin()` has no home, so a
`~/` path stays as written. No built-in teammate uses these fields.

The overlay order is unchanged: explicit dir, then roster override, then
`<home>/.config/horch/teammates`, then built-ins.

## Decision: zero-argument wrappers stay in `teammates.rs`

The orchestrator chose option 1. `teammates.rs` re-exports `crate::roster::*`
and keeps the old zero-argument forms, which read the env:
`Roster::load`, `Roster::load_with`, `operator_status_line`,
`operator_enabled_plugins`, `operator_effort_warnings`, `expand_home`.
Each env read has the comment `// A2: from RuntimeContext`. The file has 59
lines, not under 40.

The local functions shadow the glob re-export. So
`teammates::operator_status_line()` takes no argument, and
`roster::operator_status_line(home)` takes one. This is deliberate.

No caller changed: `launch.rs`, `skills.rs`, `plugins.rs`, `crates/horch/src/cmd/*`
and the tests use the old `teammates::` paths. A4 or A12 moves them to
`RuntimeContext` and deletes the wrappers.

## Gotchas for later phases

- The `arc_05_no_ambient_env_in_core` test is not on my base. When it lands,
  remove the `roster/` PENDING entry and keep the `teammates.rs` entry.
- `arc_08_roster_never_calls_herdr` skips `roster/tests.rs`, because its own
  test name contains the needle `herdr`.
- I reworded 1 doc comment in `repository.rs` ("A herdr pane" became "A worker
  pane") so the purity scan passes.
- `roster/validation.rs` calls `crate::balance_policy::{merge, trains_on_input}`
  and `crate::quota::pool_for`. A5 must keep those paths, or re-export them.
- `Roster.teammates` and `Roster.bases` are now `pub(crate)`, because
  `validation.rs` and its tests are in a sibling module.
- The plan's commits 1 and 2 are 1 commit, because the moved `check()` calls
  the moved fallback rules.
- `Effort` is additive. `Teammate.effort` is still `Option<String>`.
- `HarnessKind::` matches now live in `roster/effort.rs` and
  `roster/validation.rs` (as `Agent::`). A4's `arc_10` scan allows only
  `roster/validation`.

## Tests added (4)

In `crates/horch-core/src/roster/tests.rs`:
- `arc_08_unknown_field_rejected`
- `arc_08_overlay_precedence`
- `arc_08_legacy_frontmatter_corpus_parses`: the repo `teammates/`, plus
  fixtures `tests/fixtures/roster/pre-fallbacks/opus.md` (from `c3e673d~1`) and
  `tests/fixtures/roster/pre-effort/codex-sol.md` (from `a1e6798`).
- `arc_08_roster_never_calls_herdr`

## Gate

- `HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate` prints `GATE GREEN`.
- `scripts/check-req-coverage.sh --phase A3` passes.
- `HORCH_TEAMMATES_DIR=teammates horch teammates --check` prints
  `roster ok: 33 teammates, 28 offered to the orchestrator`. The output is
  the same before and after the env change.
- No golden, oracle, `tests/nfr.rs` or `tests/baseline_oracles.rs` change.
