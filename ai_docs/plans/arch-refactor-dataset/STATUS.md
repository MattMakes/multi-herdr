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
- SPEC-TODO from U29: Spec B §3 build_bytes estimate, local_model_bytes, trusted_parents (PRE-13 warns every run); outcome default score per kind.

- Running: U32 b5-cli (p15). Queued: U33 last.

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
- A10 (from A11): roster/validation.rs:46 skills::selected rejects marketplace ids → accept catalog with lock; worker.rs/recipes.rs Bundle::install must use catalog with lock from data_root; add e2e spawn variant of mkt_08 no-network.
- Merged U13 b1-measure (B1 landed). NumstatLine serializes `deleted` (golden frozen). occurred_at must be monotonic. fold needs execution_id on candidate.planned/spawned.
## Wave 4 running: U18 a4-harness (opus-9), U19 a6a-store (pQ), U20 a11-marketplace-cli (backend-developer-6), U21 a7b-messaging (pS), U22 b3-planner (pT), U23 b4-judge-input (pV), U24 b6-export (pW)
- Merged U19 a6a-store. A6b needs: Execution type + to/from, find_by_idempotency, set_skills call (SKL-04 wiring), ExecutionStore::open(paths, project). Store bytes = to_string_pretty no trailing newline, mode 0644. Lock <state_root>/<slug>.json.lock/.
- Merged U20 a11 (A11 landed), U21 a7b (A7 landed), U24 b6-export. A6b: fold DoneSteps.mark_done into ExecutionStore; add run_worker in lifecycle.rs. A12 shims: mailbox.rs, message.rs, Herdr::send_line, tilecmd::after_change, balancecmd::equalize_quietly, ledger::state_root (no core caller).
- B5: needs operator.promote event kind (COMPLETE/NEEDS_INTERVENTION -> DECIDED re-entry, b3-planner allows COMPLETE->DECIDED).
- Merged U22 b3-planner, U24 b6-export. Security: RoundId/ExperimentId allow / and .. → DatasetPaths must validate path components (assign to B5 unit). Judgment/receipt reads need size caps. CLI must check round state before outcome. export takes ExecutionFactsSource (CLI adapts store). Don't share CARGO_TARGET_DIR across worktrees.
- B3 coordinator: winner.selected with promotion requested applies DECIDED then REVALIDATING; emit round.cleanup_started from NEEDS_INTERVENTION only on operator cleanup; no round-deadline event → emit candidate.failed TimedOut per candidate. LABEL_POLICY_VERSION=slot-order-1. Planner API: plan_round(PlanInput) → RoundPlan; BudgetPolicy::check.
- Wave 5 running: U18 a4 (opus-9), U23 b4-judge-input (opus-13), U25 a6c (opus-14), U27 b5-promotion (pY). Queued: U26 a6b-service (after A4), A10 (after A4), then B2 binary/B3 coordinator/B4 scheduler, A12.
- Merged U25 a6c-presentation, U23 b4-judge-input. A6b: cmd/spawn.rs:115 compares STATUS_WORKING; smoke.rs:748 STATUS_DONE → typed. tests/execution_store.rs:15 unused import. ExecutionStore::open_in exists. done_record_ids maps unknown status words to done.
- B4 job unit: headless.rs must substitute {rubric}/{schema} in judge.md (agent_prompt can't render it). Cleanup must chmod 0500 bundle dirs before removal. Partial bundle build with changed bytes → Conflict forever; coordinator must not retry it (use a fresh attempt dir or rebuild policy). task.md not blindness-scanned. HEADLESS_ONLY at roster::teammate::HEADLESS_ONLY. build_judge_input(round_id, ..., repo). tel_02_fleet_writes_orchestrator_record flaky under load.
- Merged U27 b5-promotion. Engine API and gotchas: ai_docs/reports/arch-refactor-dataset/b5-promotion.md
- Merged U29 b2-binary (CLI in dataset/cli.rs, dispatch dataset/mod.rs).
- Merged U28 a10-exposure (A10 landed).
- Merged U26 a6b-service (A6 landed). spawn.rs 241 lines (target ~150), worker.rs 27.
- SPEC-TODO from U26: Spec A §4 Execution fields; Spec A §8 SpawnRequest fields and worker startup order; Spec B judge attempt key (label judge:N).
- Merged U34 tel02-flake (guard + retrying read; flake not reproduced).
- Merged U31 b4-judge-job (B4 landed; full-round e2e and B4 crash points pending on U30). Gotchas in b4-judge-job.md 'Gotchas for U30 and U32'.
- U30 now also owns jdg_e2e_round_decided and the B4 crash points (no judging stub).
- Merged U30 b3-coordinator (B3 landed). Kernel gaps: worker dying before set_running leaves Starting+live pane (deadline only); CommandValidator gets no HORCH_FAULT set (fail-gate unreachable from run) -> U35.
- SPEC-TODO from U30: Spec B §budget expected spend of a running candidate (committed = 0).
- Merged U35 worker-startup-failure (enter_context/register failure -> Failed(AgentExited{None})).
