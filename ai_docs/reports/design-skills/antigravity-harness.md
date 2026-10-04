# D08 antigravity-harness: report

Unit: `antigravity-harness`. Branch: `ds/antigravity-harness`. Author: opus-30.

## Summary

horch can spawn workers on the Antigravity CLI (`agy`). The new harness kind
is `antigravity` (`HarnessKind::Antigravity`). The new teammate is
`antigravity`. Everything was verified only against `fake-antigravity`.
`agy` is not installed on this Mac, so no step ran against the real CLI.

Research: `ai_docs/reports/design-skills/antigravity-research.md` (sources,
flags, paths, auth). The orchestrator approved the target and the decisions
below.

## Capabilities

| Capability | Value | Why |
|---|---|---|
| binary | `agy`, override `HORCH_ANTIGRAVITY_BIN` | installer puts it in `~/.local/bin/agy` |
| prompt | `--prompt-interactive <prompt>`, last | `agy` has no positional prompt; `-p` exits after 1 turn |
| model | `--model <slug>` | |
| effort | `--effort low\|medium\|high` | |
| session | discovered, not caller-minted | `agy` mints the id; `--conversation <unknown id>` starts a different conversation |
| discovery | `~/.gemini/antigravity-cli/cache/last_conversations.json`, entry for the workdir, file mtime at or after the launch | the cache maps workspace path to newest conversation id |
| resume | `--conversation <id>` | |
| skill exposure | `SkillExposure::None` | no flag or env var points `agy` at a skills dir |
| tool lists / denylist | no | no per-launch tool flags |
| permission_mode | `plan` → `--mode plan`; `acceptEdits` → `--mode accept-edits`; `auto` → `--sandbox --dangerously-skip-permissions`; `bypassPermissions` → `--dangerously-skip-permissions`; `manual`, `dontAsk` → refused | |
| daemon, exec policy, headless runner | no | |
| usage pool | `google` (`POOL_GOOGLE`) for every `agy` model | all models draw on the operator's Google account |

## Auth and forbidden env

- `agy` uses the operator's Google login (browser OAuth, OS keyring).
- `GEMINI_API_KEY`, `GOOGLE_API_KEY` and `GOOGLE_GEMINI_BASE_URL` move `agy`
  off that login or to another endpoint. They are in
  `harness::antigravity::ANTIGRAVITY_FORBIDDEN_ENV`.
- The adapter removes them from every `agy` child (`env_remove`, so no later
  env layer can add them back). `ANTHROPIC_API_KEY` stays removed by the
  global `FORBIDDEN_ENV`.
- Decision: the 3 keys are **not** added to the global `FORBIDDEN_ENV`.
  That list applies to every child (codex, opencode, git, gate commands). A
  worker's own tools can need a Gemini key.
- `horch teammates --check` refuses an `antigravity` teammate whose `env`
  sets one of the 3 keys.

## Teammate `antigravity`

- `model: gemini-3-1-pro`, `effort: medium`, `permission_mode: auto`,
  `trains_on_input: true`, no `phase`, no `skills`, no fallbacks.
- `brief_description` states the model, the strength (general work) and the
  data policy: it trains on input unless the operator confirms the opt-out,
  so the orchestrator treats it like `opencode-*`.
- `trains_on_input: true` makes the worker brief carry the "trains on what it
  is sent" block, as for `opencode-*`.
- No phase: a phase selects skills, and a harness with no skill exposure
  refuses selected skills (`skills::selection::check`).
- No oracle files (the run's rule). `antigravity` is in `SKIP_NEW_TEAMMATES`
  in `tests/baseline_oracles.rs` and `tests/skills_catalog.rs`.

## Tests

- Unit (`harness/antigravity.rs`): fresh argv, resume argv and order,
  no `--conversation` on a fresh launch, permission mapping and refusal,
  forbidden keys removed, cache parsing by workdir, stale cache ignored.
- Unit (`roster/validation.rs`): `an_antigravity_teammate_cannot_set_a_google_key`.
- Unit (`routing/quota.rs`): `pool_for("antigravity", ..)` is `google`.
- e2e `arc_26_e2e_lifecycle_matrix_antigravity`: spawn, session discovered
  from the fake's cache, `horch done`, `spawn --resume` passes
  `--conversation <same id>`.
- e2e `antigravity_child_never_receives_a_forbidden_key`: the worker's env
  holds all 4 keys; the `agy` child gets none. I checked that this test
  fails when the adapter does not remove the keys.
- e2e `skl_06_e2e_exposure_antigravity`: a teammate on `antigravity` that
  names skills is refused at spawn. No pane is split and `agy` never runs.
- Gate: `HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate` is green.

## Existing files changed beyond additions

- `harness/mod.rs` tests: `ALL` and the legacy table gain the new kind.
- `runtime/context.rs` test: the override count is 10, not 9.
- `roster/validation.rs` test `builtin_phase_defaults_...`: `antigravity`
  joins `smoke` and `judge` as a teammate with no phase.
- `harness/launch.rs` test: the `unreachable!()` arm names the new kind.
- `tests/lifecycle.rs`: `lifecycle` now calls a new `lifecycle_in`, so the
  antigravity test can install its fake first. Behaviour is unchanged.
- `README.md`: the harness table gains 1 row; "Five harnesses" is now "Six".
- `harness/mod.rs`: `HarnessKind::ALL` (from D09) gains `Antigravity`, and the new
  test `all_names_every_kind` keeps it complete. `horch agent-list` shows
  `antigravity` (`unavailable` here, because `agy` is not installed).
- No oracle or golden changed.

## Verified only with the fake

All of these are unverified against a real `agy`:

- The flag names (`--prompt-interactive`, `--effort`, `--mode`, `--sandbox`,
  `--conversation`). Part of the flag table comes from a third-party cheat
  sheet.
- The model slug `gemini-3-1-pro`.
- The format of `last_conversations.json`. The parser takes
  `{"<dir>": "<id>"}` and also `{"<dir>": {"id": ...}}`.
- That `agy` updates the cache soon after launch, not only at exit. If it
  writes only at exit, discovery finds the id at `horch done` (the final
  discovery), not while the worker runs.
- Whether `--sandbox` lets a worker write `.git` of a worktree outside its
  cwd (codex has this problem).

## For the operator

### Third-party use

The Antigravity FAQ says third-party tools cannot use Antigravity
credentials. horch never reads or uses them. It runs your own `agy` binary in
a terminal pane, the same as you do. You decide if this is acceptable under
Google's terms.

### Data policy

A personal Google account's interactions can train Google's models. Turn
Activity and Telemetry off in the Antigravity settings to opt out. Until you
confirm the opt-out, the orchestrator sends this tier public work only.

### Workspace trust

`agy` asks "Do you trust the contents of this project?" on the first launch
in each directory. Trust is exact-path (`trustedWorkspaces` in
`~/.gemini/antigravity-cli/settings.json`). A subdirectory or a new worktree
is not covered. No flag or env var skips the prompt. horch cannot answer it
and does not write your `settings.json`. A worker in an untrusted directory
stops at the prompt.

Before you spawn `antigravity` in a directory, run `agy` there once and pick
"Yes, I trust this folder". Then exit.

### Local acceptance check

1. Install: `curl -fsSL https://antigravity.google/cli/install.sh | bash`.
2. Sign in: run `agy` once and finish the Google login in the browser.
3. Check the flags: `agy --help`. Confirm `--prompt-interactive`, `--model`,
   `--effort`, `--mode`, `--sandbox`, `--dangerously-skip-permissions` and
   `--conversation`.
4. Check the model slug with `/model` in the TUI. If it is not
   `gemini-3-1-pro`, change `model:` in `teammates/antigravity.md`.
5. Make sure `GEMINI_API_KEY` and `GOOGLE_API_KEY` are not needed for `agy`
   (no `modelProvider: gemini` in its `settings.json`).
6. Trust the project directory (see Workspace trust).
7. `horch teammates --check` passes.
8. In a fleet: `horch spawn antigravity "Print the repo's top-level files and run horch done."`.
9. Check `horch sessions --json`: the record has a `session_id`.
10. Check that `~/.gemini/antigravity-cli/cache/last_conversations.json`
    holds that id for the directory.
11. `horch spawn --resume <record_id> "Say what you did before."`. The
    worker must recall the first task.

## Not done, and follow-ups

- `horch cost` / telemetry reads no `agy` usage. `agy` has no documented
  transcript file. Its headless JSON has per-run tokens, but horch runs the
  TUI. `telemetry/readers.rs` reports "unknown agent 'antigravity'".
- The `google` pool has no probe. It assesses `unknown` ("no reading"), and
  routing spawns as for any unknown pool. It is not in `POOLS`, so
  `horch quota` does not show it.
- `crates/horch/src/dataset/preflight.rs` `kind_of` lists the kinds by hand.
  It lacks `Antigravity`, so dataset preflight shows no `agy` version. It is
  outside my files and still compiles.
- `crates/horch-e2e/src/harness.rs` `FAKES` does not install
  `fake-antigravity`. The tests install it with `with_agy` in
  `tests/lifecycle.rs`. Adding it to `FAKES` is a 2-line change outside my
  files.
- Skill exposure: a later unit could expose skills through a private
  `.agents/skills` link if `agy` reads one from `--add-dir` directories. The
  docs do not say.
- `agy` loads your global skills, plugins and `GEMINI.md`. It has no switch
  to turn them off, so `inherit_plugins` is not set on the teammate.
