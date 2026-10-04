# U11 a3-roster: split teammates.rs into roster/

Unit slug: `a3-roster`. Branch: `ard/a3-roster`. Phase: A3. Requirement: ARC-08.

## GOAL

`crates/horch-core/src/teammates.rs` (1923 lines) is split into focused
modules under `crates/horch-core/src/roster/`, `teammates.rs` is a re-export
shim, the roster code reads no process env (values come in as parameters),
and every existing test passes unchanged. No user-visible change.

## CONTEXT

- Read first: `ai_docs/plans/arch-refactor-dataset/01-fleet-conventions.md`.
- Read in `00-master-plan.md`: §1 (layout: `roster/{teammate,phase,effort,permission,parser,repository,validation,operator}.rs`),
  module rule "roster never calls Herdr", §3 "A3", §4 ARC-08 row.
- Read the reports of merged units in `ai_docs/reports/arch-refactor-dataset/`,
  at least `a1-vocabulary.md` (`HarnessKind` replaced `Agent`; `teammates.rs`
  re-exports it as `Agent`) and `a0-oracles.md`.
- Today's `teammates.rs` layout (line numbers at 575c2c2; A1 changed the
  `Agent` part):
  - 29: `include!(.../builtin_teammates.rs)` (build.rs output)
  - 45 to 57: `ORCHESTRATOR_TIERS`, `FLEET_ORCHESTRATORS`, `ORCHESTRATOR_ONLY_SKILLS`, `ORCHESTRATOR_DENIED_TOOLS`
  - 65 to 156: `reserved_tier`, `valid_efforts`, `model_takes_effort`, `effort_problem`, `BRIEF_DESCRIPTION_MAX`
  - 161 to 200: `Phase`
  - 353 to 420: `PermissionMode`
  - 423 to 634: `Teammate`, `ExecRule`, `Base`
  - 634 to 1185: `Roster` (load, overlays, check)
  - 1186 to 1320: operator settings (`operator_status_line`, `operator_enabled_plugins`, `operator_effort_warnings`, `effort_override_warnings`, `codex_default_effort`, `operator_settings`, `expand_home`)
  - 1330 to 1375: `overlay_dirs`, `md_files`, `split_frontmatter`
  - 1376 to 1900: `mod spawnable_tests`
  - 1902 to 1923: `parse_teammate`, `parse_base`
- Env reads in this file (you own their removal): `CLAUDE_CODE_EFFORT_LEVEL`
  (about line 1235), `HOME` (1308, 1321, 1332), `HORCH_TEAMMATES_DIR` (1335).
  Replace each with a parameter. At the `crates/horch/src/cmd/*` call sites,
  pass the value from `RuntimeContext` if the A2 unit has merged into your
  base (`ctx.paths.home`, `ctx.bins.roster_override`,
  `ctx.inherited.claude_code_effort_level`); otherwise pass the same env
  read that the function did, at the call site, with a
  `// A2: from RuntimeContext` comment.
- `balance_policy.rs` lines 306 to 370 (`fallback_problems`,
  `fallback_warnings`, `names_tool`) are static roster rules. They move to
  `roster/validation.rs` in this unit. The A5 unit (U12) splits the rest of
  `balance_policy.rs` at the same time. To avoid a conflict: you replace
  only those 3 functions in `balance_policy.rs` with
  `pub use crate::roster::validation::{fallback_problems, fallback_warnings};`
  and do not edit any other line of that file.
- Parallel units: U10 `a2-runtime` (owns `ledger.rs`, `mailbox.rs`, `agent.rs`,
  `launch.rs` env lines, most `cmd/*.rs` env reads), U12 `a5-routing`
  (owns `quota.rs`, `policy.rs`, `balance_policy.rs` except the 3 functions
  above), U09 `a7a-workspace`. The A2 unit's `arc_05_no_ambient_env_in_core`
  test has a `PENDING` allowlist with `teammates.rs` and `roster/`. If that
  test is on your base, delete those 2 entries in your last commit.

## A1 NOTE

- Do not add any `resume: bool` or `no_tile: bool` field or parameter. The A1 check `grep -rn 'no_tile: bool\|resume: bool' crates` must stay empty.

## FILES

own:
- `crates/horch-core/src/roster/**` (new)
- `crates/horch-core/src/teammates.rs` (becomes a shim)
- `crates/horch-core/src/balance_policy.rs` (only lines 306 to 370+, as above)
- `crates/horch-core/src/lib.rs` (add `pub mod roster;`)
- `crates/horch-core/build.rs` only if the `include!` path must change (it should not)
- call sites in `crates/horch/src/cmd/*.rs` and `crates/horch-core/src/*.rs`
  that must pass the new parameters (minimal edits)
- `crates/horch-core/tests/fixtures/roster/**` (new)
- `crates/horch-core/tests/arch_scan.rs` (only the PENDING entries, if present)
- `ai_docs/reports/arch-refactor-dataset/a3-roster.md`

do not touch: `quota.rs`, `policy.rs`, the rest of `balance_policy.rs`,
`ledger.rs`, `mailbox.rs`, `launch.rs`, `workspace/**`, `teammates/*.md`,
golden and oracle files.

## STEPS

1. Create the worktree (conventions §2).
2. Create the modules and move code with no logic change:
   - `roster/phase.rs`: `Phase`.
   - `roster/effort.rs`: an `Effort` newtype (`pub struct Effort(String)`
     with `as_str()`, serde transparent) plus `valid_efforts`,
     `model_takes_effort`, `effort_problem`. Keep the existing function
     signatures working (they take `&str`); `Effort` is additive.
   - `roster/permission.rs`: `PermissionMode`.
   - `roster/teammate.rs`: `Teammate`, `ExecRule`, `Base`, the constants
     (`ORCHESTRATOR_*`, `FLEET_ORCHESTRATORS`, `BRIEF_DESCRIPTION_MAX`),
     `reserved_tier`. Keep `#[serde(deny_unknown_fields)]` and every serde
     attribute exactly.
   - `roster/parser.rs`: `split_frontmatter`, `parse_teammate`, `parse_base`, `md_files`.
   - `roster/repository.rs`: `Roster` loading, the built-in `include!`,
     `overlay_dirs` (now taking `home: &Path` and
     `roster_override: Option<&Path>`), overlay precedence.
   - `roster/validation.rs`: `Roster::check`, `check_teammate` and the
     other validation helpers, plus `fallback_problems`, `fallback_warnings`,
     `names_tool` from `balance_policy.rs`.
   - `roster/operator.rs`: the operator-settings functions, taking `home`
     and `claude_code_effort_level` as parameters.
   - `roster/mod.rs`: `pub mod` lines and `pub use` of the main types.
   - Move `mod spawnable_tests` into the module whose code it tests (most
     likely `validation.rs`), with test names and bodies unchanged except
     for `use` paths.
   - `teammates.rs`: only `pub use crate::roster::*;` style re-exports, so
     every old path (`teammates::Teammate`, `teammates::Agent`,
     `teammates::Roster`, `teammates::Phase`, functions) still resolves.
   Check after each move: `cargo build --workspace --all-targets`.
3. Remove the env reads (see CONTEXT). Thread the parameters through
   `Roster::load` (or its equivalent) and the operator functions. Keep the
   resolution order identical: explicit dir, then `$HORCH_TEAMMATES_DIR`
   (now a parameter), then the user overlay under home, then built-ins.
4. Tests:
   - `arc_08_unknown_field_rejected`: a teammate file with an unknown
     frontmatter key fails to parse with an error that names the key.
   - `arc_08_overlay_precedence`: built-in, user overlay (under a temp home)
     and explicit dir each define the same teammate with a different
     `brief_description`; the explicit dir wins, then the user overlay, then
     built-in. Use temp dirs, no env.
   - `arc_08_legacy_frontmatter_corpus_parses`: every file in the repo
     `teammates/*.md` parses, plus fixtures under
     `crates/horch-core/tests/fixtures/roster/`: a pre-#12 teammate without
     `fallbacks` (copy a teammate from `git show 575c2c2~20:teammates/opus.md`
     or any commit before fallbacks were added; find one with
     `git log --oneline -S fallbacks -- teammates/`), and a pre-effort teammate
     without `effort`.
   - A roster-purity scan test `arc_08_roster_never_calls_herdr` (not a
     master-plan ID, but name it with the arc_08 prefix): no file under
     `roster/` contains `herdr`, `Herdr`, `std::process` or `std::env::var`.
   - `nfr_03` (existing) and `golden_prompts` stay green.
5. Gate after each step. Commits: `A3: Split roster modules out of teammates.rs`,
   `A3: Move static fallback rules into roster validation`,
   `A3: Pass roster env as parameters`, `A3: Add ARC-08 tests`.
6. Verify: `HORCH_TEAMMATES_DIR=teammates cargo run --quiet --bin horch -- teammates --check`
   prints the same output as on your base (diff the two outputs); the oracle
   tests pass; the golden prompts are byte-identical.
7. Write and commit the report. Follow conventions §6 to finish.

## DONE WHEN

- `teammates.rs` holds only re-exports (under 40 lines).
- The 4 arc_08 tests pass. `teammates --check` output is unchanged.
- No env read remains in `roster/` (grep).
- The full gate is green apart from named base failures.

## REPORT

- `horch note` after each commit.
- `horch done` summary: module map (old line range → new file), new
  parameters and their call sites, gotchas.
