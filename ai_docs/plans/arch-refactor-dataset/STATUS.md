# Orchestrator status (arch-refactor-dataset)

Integration worktree: /Users/mascott/projects/mh-wt/integration (branch arch-refactor-dataset, base 575c2c2).
Merge protocol: worker sends READY-TO-MERGE ard/<unit> <sha>; orchestrator runs
`git merge --ff-only ard/<unit>` (or asks REBASE), runs `just gate`, replies MERGED.
CURRENT_PHASE: orchestrator appends a phase when its last unit merges.

## Wave 1 (spawned 2026-10-02)
| Unit | Slug | Worker | Status |
|---|---|---|---|
| U01 | a0-gate | opus-3 (codex-sol-1 retired: sandbox) | running |
| U02 | a0-oracles | opus-1 | MERGED |
| U03 | a0-designs | staff-engineer-1 | MERGED |
| U04 | a8-marketplace | backend-developer-2 | running (needs U01 allowlist) |
| U05 | a1-vocabulary | opus-2 | running |
| U06 | b1-primitives | backend-developer-1 | running (needs U01 allowlist) |
| U07 | e2e-fakes | qa-engineer-1 | MERGED |
| U08 | machine-teacher | sonnet-1 | MERGED 658b57d |

## Merged: U01 U02 U03 U04 U05 U06 U07 U08 U09. CURRENT_PHASE: A0 A1 A8.

## Wave 2/3 (running)
| Unit | Slug | Worker |
|---|---|---|
| U10 | a2-runtime | opus-4 |
| U11 | a3-roster | backend-developer-3 |
| U12 | a5-routing | opus-5 |
| U13 | b1-measure | opus-6 |
| U14 | b2-vcs | backend-developer-4 |
| U15 | b4-evaluation | opus-7 |
| U16 | a9a-skills | backend-developer-5 |

## Wave 2 plan (original notes)
- A2-core (runtime/*, bootstrap, ledger/mailbox/clock/usage/telemetry env, Brief v2), arc_05 scan with temp allowlist for A3/A4/A5 files.
- A3 roster split (owns teammates env removal, fallback_problems move).
- A4 harness ownership (owns launch/codex/opencode/prime/plugins/skills env removal). Needs U02 oracles.
- A5 routing (owns policy/quota env removal). Needs U02 oracles.
- A7a workspace move (herdr/layout/tile/balance/paneshell/tilecmd).
Later: A6 (split store early after U06+A1), A7b messaging, A9 after A8+A3, A10, A11, A12; B1 rest after U03+U06; B2 vcs after A8.

## Open items
- Spec A / Spec B verbatim text: requested from operator. Needed for design appendices, judge.md (§10), Judgment schema (§11), event list, CHECKLIST (§16), final audit.

## Gotchas carried forward (put into later plans)
- A4: launch oracle does not cover codex Rules or Prime daemon args; smoke (agent none) and orchestration-worker (no model) record errors. Tests iterate the roster: a new teammate (judge.md in B4) must be skipped by oracle tests, not given oracle files.
- A6: e2e exec commands need absolute paths (sealed PATH). fake-prime has no long-lived daemon. fail_split/fail_run are fake-side only; A6 makes horch record LaunchFailed. NFR-01 allowance is Harness::allows_program.
- B2: horch must read HORCH_GIT_BIN (A2 adds it to BinOverrides). Harness::with_git sets it.
- OpenCode fake id: ses_ + 16 hex of canonical cwd; Prime: prime_ + 16 hex of --session-dir, file <dir>/<id>.jsonl.
- Saved ledger oracle uses to_string_pretty (Ledger::write is private), no trailing newline.
| U09 | a7a-workspace | sonnet-2 | MERGED |
- A7b: design §4.7 wants PaneId newtypes, send_text/send_keys names, HerdrClient built from HarnessBins — not yet aligned. Herdr methods outside the trait: send_line, wait_output, integration_status, pane_focus*, pane_move*, pane_resize, tab_*. workspace_create(cwd: Option<&str>).
| U17 | b2-preflight | opus-8? |
- A4: Agent:: matches exist in roster/effort.rs and roster/validation.rs; arc_10 scan must allow roster/effort.rs too or move effort tables into harness capabilities.
- A6: routing::decide still returns legacy Decision; switch spawn plan to RoutingDecision in A6. RoutingMode has Resume and Ungated (B3 adds Pinned). HarnessKind has no Ord (available_harnesses is a Vec).
- A5 eligible API: eligible_fallbacks(req, roster, view), roster_eligibility(...) with EligibilityFilter; see a5-routing.md.
- B3: 0 gates → eligible=true, score 0.0 (coordinator decides). Freeze uses git add -A: a second freeze after validation can commit target/ unless ignored → validator CARGO_TARGET_DIR must be outside the worktree or freeze must exclude target/. diff_patch callers pass the main repo. Gates get all env except FORBIDDEN_ENV (consider allowlist). setsid gates escape timeout kill. GitClient has diff_digest; GitCli/CommandValidator have with_env.
- Merged also: U14 b2-vcs, U15 b4-evaluation, U17 b2-preflight, U16 a9a-skills (SKL-04 open → A6/A9b).
- B2 binary unit: PRE-06, PRE-07, PRE-12, CMP-01 tests. worktree_root must be absolute. safe_n is one wave size for all candidates (1 pi lowers it). PreflightCandidate replaces CandidatePlanned; JudgeConfig.policy is Value until WinnerPolicy (now merged) → type it. PreflightPlan.pools Vec<PoolFacts>. harness_versions keyed by as_str (add Ord to HarnessKind in a later unit).
- B4 rest: parser gives NotJson for fences; use --json-schema. Utility tie-break = highest component-score sum among acceptable tied labels, then lexical.
- A10/A11 (from a9a): materialize refuses marketplace entries (copy from store is A10/A11 work); marketplace entries have empty description; validate lock version before using it in a path; Bundle::install writes .claude-plugin/plugin.json and uses mint_uuid as dir name (switch to execution_id); ExecutionId accepts / and .. (materialize rejects path-like ids). Lock override: bundled:<id> never replaces compiled-in; git/local with same id replaces; new id adds. plan_activation(teammate, phase, catalog) 3 args.
- A2 merged. U18 a4-harness spawned (pane pP).
- A6: Settings.faults has only has(); add indexed/abort_if. Brief.workdir unset; workdir_or_project() ready.
- A7b: settle_after_close keeps own setsid → runtime::process::spawn_detached; Herdr resolves bin via agent::herdr_bin() → give ctx.bins.harness.herdr.
- B2 bin: settings.machine_file read but unused. e2e fake-herdr exec passes spawner env to pane.
- ProcessEnv shims remain (remove later): workspace/herdr.rs (A7b), opencode.rs + prime.rs (A4), telemetry/readers.rs (A6c), message.rs (A7b), roster/validation.rs (A12). set_current_dir kept in worker.rs/recipes.rs.
