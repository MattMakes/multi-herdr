# U18 a4-harness: harness modules own command lines and capabilities

Unit slug: `a4-harness`. Branch: `ard/a4-harness`. Phase: A4.
Requirements: ARC-09, ARC-10, ARC-11.

## GOAL

Every harness-specific behavior lives in one module under
`crates/horch-core/src/harness/` (claude, codex, opencode, pi, prime, none),
behind a `Harness` trait and a `Capabilities` value. The two launch flows
(`worker.rs launch_agent` and `recipes.rs pane_launch`) become one
capability-driven flow. Session discovery filters on the canonical workdir.
The argv/env of every teammate equals the A0 launch oracle.

## CONTEXT

- Read first: `ai_docs/plans/arch-refactor-dataset/01-fleet-conventions.md`.
- Read in `00-master-plan.md`: §1 (layout:
  `harness/{capabilities,launch,claude,claude_plugins,codex,opencode,pi,prime,none,headless}.rs`),
  §2 "harness", §3 "A4", §4 ARC-09..11 rows, Spec A §17 items 5 and 14.
- Design doc: `ai_docs/designs/2026-10-02-architecture-refactor-design.md`,
  key types for `harness::{HarnessKind, Capabilities, Harness}` and the
  capabilities table per harness, and the A4 phase section. The design wins
  over this plan where they differ, except for the rule below about `resume`.
- Read these merged reports in `ai_docs/reports/arch-refactor-dataset/`:
  `a0-oracles.md` (launch oracle: 33 teammates × {fresh, resume,
  unmanaged}; it does NOT cover codex `Rules` or Prime daemon args;
  `smoke` and `orchestration-worker` record errors), `a1-vocabulary.md`,
  `a2-runtime.md` (RuntimeContext, BinOverrides, transport env; A2 removed
  `launch::apply_env`), `a3-roster.md`, `a5-routing.md`, `a9a-skills.md`
  (the skills `Bundle` still drives exposure; do not change exposure — A10
  does that), `e2e-fakes.md` (fake-opencode and fake-prime session ids).
- Rules from earlier phases:
  - Do not name any field or parameter `resume: bool` or `no_tile: bool`.
    The A1 check `grep -rn 'no_tile: bool\|resume: bool' crates` must stay
    empty. Name the capability `resumes: bool` (design §4.7 says `resume`;
    this rule wins).
  - `HarnessKind` has predicate methods today in `harness/mod.rs`:
    `mints_session_id`, `harvests_session_id`, `runs_a_daemon`,
    `uses_execpolicy`. They become `Capabilities` fields.
  - `HarnessKind::` and `Agent::` matches today also exist in
    `roster/effort.rs` (effort tables) and `roster/validation.rs`. The
    `arc_10_harness_match_only_in_harness` scan allows `harness/` and
    `roster/validation.rs`. Move the effort tables (`valid_efforts`,
    `model_takes_effort`) behind `Capabilities` (for example
    `capabilities().efforts: &'static [&'static str]`) so that
    `roster/effort.rs` has no harness match; or, if that changes behavior,
    send `QUESTION:`.
- Source today: `crates/horch-core/src/launch.rs` (opencode builder about
  line 117, pi/prime family about 196, claude about 269 with
  `overlay_plugin_skills` about 466, codex about 493; `Session<'a>` enum at
  20; `FORBIDDEN_ENV` at 46; `command` and `command_with_skills`), plus
  `codex.rs`, `opencode.rs`, `prime.rs`, `plugins.rs`.
  `crates/horch/src/cmd/worker.rs` `launch_agent` (about lines 108 to 190)
  and `crates/horch/src/cmd/recipes.rs` `pane_launch` (about 516 to 640)
  duplicate the flow: execpolicy rules, session minting, Prime daemon,
  session harvest thread.

## FILES

own:
- `crates/horch-core/src/harness/**`
- `crates/horch-core/src/launch.rs`, `codex.rs`, `opencode.rs`, `prime.rs`,
  `plugins.rs` (become shims)
- `crates/horch-core/src/roster/effort.rs` (only to route effort tables
  through capabilities)
- `crates/horch/src/cmd/worker.rs` (`launch_agent`), `crates/horch/src/cmd/recipes.rs` (`pane_launch`)
- `crates/horch-core/tests/baseline_oracles.rs` (call sites only; never oracle data)
- `crates/horch-core/tests/harness.rs` (new), `crates/horch-core/tests/arch_scan.rs` (add your scan)
- `ai_docs/reports/arch-refactor-dataset/a4-harness.md`

do not touch: oracle and golden files, `skills/**` exposure logic,
`routing/**`, `workspace/**`, `ledger.rs`, `mailbox.rs`.

## STEPS

1. Create the worktree (conventions §2).
2. `harness/capabilities.rs`: `Capabilities { caller_minted_session, resumes, effort (the valid levels), daemon, exec_policy, skill_exposure (an enum: PluginDir, SkillFlag, OpencodeConfig, CodexHome, None), tool_lists, tool_denylist, headless }`.
   `HarnessKind::capabilities()` returns a `&'static Capabilities` per kind.
   The old predicate methods delegate to it (keep them until A12).
3. `harness/mod.rs`: the `Harness` trait per design (kind, capabilities,
   validate, prepare, build_command, discover_session) and
   `HarnessKind::adapter() -> &'static dyn Harness`. `build_command` always
   removes `FORBIDDEN_ENV`.
4. Move builders with `git mv` where possible, then split:
   `launch.rs` claude parts → `harness/claude.rs` (+ `claude_plugins.rs`
   from `plugins.rs` and `overlay_plugin_skills`), codex → `harness/codex.rs`
   (with `codex.rs` rules and harvest), opencode → `harness/opencode.rs`
   (with `opencode.rs`), pi and prime → `harness/pi.rs` and
   `harness/prime.rs` (with `prime.rs` daemon), none → `harness/none.rs`.
   `harness/launch.rs` keeps the shared parts (`Session`, `command`,
   `command_with_skills` dispatching through `adapter()`). Old files become
   shims. Check after each move: `oracle_launch_matches` passes.
5. One launch flow: `harness::launch::run_flow(ctx, teammate, session, brief, ...)`
   = prepare (codex rules, prime daemon, skill exposure via the existing
   `Bundle`) → build → spawn discovery thread unless
   `caller_minted_session` → wait. `worker.rs launch_agent` and
   `recipes.rs pane_launch` both call it; delete their duplicated branches
   (no `mints_session_id()`/`harvests_session_id()`/`runs_a_daemon()`/
   `uses_execpolicy()` calls outside `harness/`).
6. Discovery by canonical workdir: codex rollout harvest and
   `opencode session list` filter by `std::fs::canonicalize(workdir)` and
   compare canonical paths (macOS `/private/var` vs `/var`).
7. Tests:
   - `arc_09_argv_matches_baseline`: the A0 launch oracle matrix through the
     new `adapter().build_command` path (reuse the oracle helpers).
   - `arc_10_capabilities_match_legacy_predicates`: for every kind, the
     capabilities equal what the old predicates returned (keep a private
     `#[cfg(test)]` copy of the old predicate table).
   - `arc_10_harness_match_only_in_harness` (in `arch_scan.rs`): outside
     tests, `HarnessKind::` / `Agent::` variant matches appear only under
     `harness/` and in `roster/validation.rs`.
   - `arc_11_codex_discovery_by_workdir`, `arc_11_opencode_discovery_by_workdir`:
     two sessions in different workdirs; discovery picks the one whose
     canonical workdir matches.
   - `arc_11_canonical_tmp_paths`: a workdir given as `/var/...` matches a
     session recorded as `/private/var/...` (`cfg(target_os = "macos")`;
     on Linux use a symlinked temp dir).
   - Extend the oracle coverage gap: add `harness_codex_rules_snapshot` and
     `harness_prime_daemon_args_snapshot` comparing the codex execpolicy
     rules text and the Prime daemon argv to the values the code produced
     on your base before the move (capture them in your first commit as
     new fixture files under `crates/horch-core/tests/fixtures/harness/`).
8. Gate after each step. Commits: `A4: Add harness capabilities`,
   `A4: Add Harness trait and adapters`, `A4: Move builders into harness modules`,
   `A4: One capability-driven launch flow`, `A4: Discover sessions by canonical workdir`,
   `A4: Add ARC-09..11 tests`.
9. Write and commit the report. Follow conventions §6.

## DONE WHEN

- The 6 arc tests and the 2 snapshot tests pass; `oracle_launch_matches`
  passes unchanged.
- `grep -rn 'mints_session_id()\|harvests_session_id()\|runs_a_daemon()\|uses_execpolicy()' crates --include=*.rs`
  shows matches only under `harness/`.
- `just gate` is green.

## REPORT

- `horch note` after each commit.
- `horch done` summary: module map, Capabilities table, the launch flow API
  (A6's service calls it), gotchas.
