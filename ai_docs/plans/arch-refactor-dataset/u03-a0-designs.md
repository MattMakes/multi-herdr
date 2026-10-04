# U03 a0-designs: the two design docs and SPEC-COVERAGE.md

Unit slug: `a0-designs`. Branch: `ard/a0-designs`. Phase: A0.

## GOAL

Two design docs exist, with every requirement ID of the master plan in a
machine-readable table and with full Rust signatures for the key types, so
that every later phase can implement from them without guessing.

## CONTEXT

- Read first: `ai_docs/plans/arch-refactor-dataset/01-fleet-conventions.md`.
- Read all of `00-master-plan.md`. It is your main input.
- Read the existing design for style:
  `ai_docs/designs/2026-09-28-fleet-telemetry-design.md` (requirement tables,
  section layout).
- Read enough source to write correct signatures: `crates/horch-core/src/lib.rs`,
  `ledger.rs` (the `Record` struct), `teammates.rs` (`Teammate`, `Agent`),
  `balance_policy.rs` (`Decision`, `GateFlags`), `launch.rs`, `skills.rs`
  (`Bundle`), `herdr.rs` (public functions), `mailbox.rs` (`Brief`).
- U01 rewrites `scripts/check-req-coverage.sh`. It parses your tables like
  this:
  - A row that matches `^\| [A-Z]{3}-[0-9]{2} \|` is a definition only when
    its table header's second cell is `Requirement` or `Check`.
  - If the header has a `Phase` cell, the ID's phase is the first token of
    that cell: one of `A0`..`A12`, `B1`..`B6`.
  - An ID defined twice in all of `ai_docs/designs/*.md` fails the gate.
  - Every defined ID needs at least 1 test whose name starts with the
    lowercase ID plus `_` (`ARC-02` → `arc_02_`).
- Spec A and Spec B verbatim are not available yet. Put the appendices in as
  headed placeholders (see step 5). The orchestrator fills them later.
- Other units run at the same time and implement A1, A8, B1 primitives and
  fakes. Your signatures guide later phases (A2 onward). Where a parallel unit
  already defines a type (A1: `ids.rs`, `harness::HarnessKind`,
  `execution::model`), describe the target shape from the master plan; small
  differences are reconciled later.

## FILES

own (all new):
- `ai_docs/designs/2026-10-02-architecture-refactor-design.md`
- `ai_docs/designs/2026-10-02-dataset-competition-design.md`
- `ai_docs/gates/architecture-refactor/SPEC-COVERAGE.md`
- `ai_docs/reports/arch-refactor-dataset/a0-designs.md`

do not touch: every other file.

## STEPS

1. Create the worktree (conventions §2).
2. Write `2026-10-02-architecture-refactor-design.md` with these sections:
   1. Status, scope, and the operator decisions OD1 to OD7 (copy from the master plan).
   2. Concept map and target layout (master plan §1), with the dependency
      direction and the module rules. For each module rule, name the
      source-scan test that enforces it.
   3. Requirements: three tables, ARC, MKT, SKL. Header exactly
      `| ID | Requirement | Phase | Tests |`. Copy every row from the master
      plan §4. Make every Tests cell list concrete test names; replace a
      wildcard such as `mkt_06_*` with the full names
      (`mkt_06_rejects_traversal`, `..._symlink`, `..._oversize`,
      `..._invalid_skill_md`, `..._hooks`) and `arc_26_e2e_lifecycle_matrix_*`
      with the 5 harness names. For non-ID references in a Tests cell
      (`bal_04`, `tel_*`, `nfr_03`), keep them, because they name existing tests.
   4. Key types: full Rust signatures (structs, enums with every variant,
      trait methods with argument and return types, and pub function
      signatures) for: `ids` (the `string_id!` macro contract, `mint_v7`,
      validation rules), `runtime::RuntimeContext` and its parts,
      `EnvSource`, `ProcessEnv`, `MapEnv`, `ExecutionStatus`, `FailureKind`,
      `ExecutionKind`, `SessionMode`, `SessionState`, `TilingMode`,
      `Execution`, `LedgerRecordV1` (every field of today's `Record` plus the
      new optional fields), the `status`/`state` compatibility rule,
      `routing::*` (`ExclusionReason`, `EligibleEntry`, `RoutingProvenance`,
      `RoutingDecision`, `eligible_fallbacks`, `roster_eligibility`),
      `execution::{SpawnRequest, ExecutionPlan, plan_launch, finish_plan,
      SpawnError, ReportTarget}`, `harness::{HarnessKind, Capabilities,
      Harness}` with a capabilities table per harness, `workspace::WorkspaceClient`,
      `messaging::brief::Brief` v1 and v2 with `transport_env`,
      `skills::{SkillId, SkillVersion, ResolvedSkillRef, SkillActivationPlan,
      InvocationPolicy, plan_activation}`, and the marketplace types
      (`SkillManifest`, `SkillSource`, `GitRevision`, lock entry, limits,
      rejection rules, install pipeline steps).
   5. Phases A0 to A12: one subsection each. For each phase: the files moved
      or created (old path → new path), the shims left behind, the tests,
      the gate, and the "do not change" list. Take the facts from the master
      plan; add the exact old line ranges from the source where the master
      plan gives them.
   6. Compatibility: ledgers, briefs, frontmatter, offline skills.
   7. Fault points (the spawn and marketplace `HORCH_FAULT` names).
   8. Appendix A: `## Appendix A: Spec A (verbatim)` with the single line
      `PENDING: the orchestrator inserts the operator's Spec A text here.`
3. Write `2026-10-02-dataset-competition-design.md` with these sections:
   1. Status, scope, OD2 to OD5, the inert Clef/Laya decision (OD4), non-goals
      with the enforcing IDs (master plan §4 "Spec B §20 non-goals").
   2. Storage layout (`DatasetPaths`, OD3) and permissions (0600/0700).
   3. Requirements: tables MEA, PRE, CMP, JDG, PRO, EXP, SEC, NFR. Header
      exactly `| ID | Requirement | Phase | Tests |` (for PRE, the second
      header cell is `Check`: `| ID | Check | Phase | Tests |`). Copy every
      row from the master plan §4, with concrete test names as in step 2.3.
      The NFR table holds NFR-06 to NFR-11 only (NFR-01 to NFR-05 are in the
      telemetry design; do not repeat them). For NFR-08, use the test name
      `nfr_08_phase_gate_runs_every_check`.
   4. Key types with full signatures: `EventEnvelope`, `Actor`, the event
      kind enum (list every event the master plan names: `experiment.created`,
      `preflight.completed`, `experiment.aborted`, `round.created`,
      `candidate.planned`, `worktree.created`, `candidate.spawned`,
      `candidate.completed`, `candidate.failed`, `candidate.frozen`,
      `validation.completed`, `judge.scheduled`, `judge.started`,
      `judge.completed`, `judge.failed`, `winner.selected`, `winner.rejected`,
      `promotion.started`, `promotion.completed`, `promotion.conflicted`,
      `promotion.rolled_back`, `worktree.cleanup_failed`, `outcome.recorded`,
      plus an `Unknown` variant that preserves the raw JSON; mark the list
      `SPEC-TODO(Spec B event list)`), `NewEvent`, `Recorder`, `Appended`,
      `WorkerRun` 1.0.0 fields, money types, `GitClient`, `WorktreeManager`,
      `FrozenCandidate`, `Validator`, `ValidationReport`, `JudgeInput`,
      `Judgment` (mark `SPEC-TODO(Spec B §11)`), `WinnerPolicy`,
      `WinnerOutcome`, `PromotionEngine`, `PromotionReceipt`, the teacher
      types, the export row, `MachineSnapshot`, `PreflightReport`,
      `DatasetConfig`.
   5. Round state machine: every state and every allowed transition, as a
      table, for both the default path and the `--promote-to` path (master
      plan B5).
   6. Phases B1 to B6: one subsection each, as in step 2.5.
   7. Crash points and the resume invariants (master plan §5).
   8. Local acceptance LA-1 to LA-12.
   9. Appendix B: `## Appendix B: Spec B (verbatim)` with the single line
      `PENDING: the orchestrator inserts the operator's Spec B text here.`
4. Write `ai_docs/gates/architecture-refactor/SPEC-COVERAGE.md`: a table
   `| Spec | Section | Topic | IDs | Status |`. Fill the rows you can derive
   from the master plan (for example Spec A §17 criteria 1 to 16, §13, §10,
   §3, §4, §8, §14, §16, §18; Spec B §3, §10, §11, §15, §16, §18, §20). Set
   Status to `mapped` or `PENDING spec text`. Add a note that the final audit
   after B6 walks both verbatim specs against this file.
5. Self-check with a script you run once (do not commit it): extract every
   ID row from both docs; confirm each ID appears exactly once as a
   definition; confirm every ID listed in master plan §4 is present; confirm
   every Phase cell starts with a valid token.
6. Commit: `A0: Add architecture-refactor and dataset-competition designs`.
   Write and commit the report.
7. Run the full gate. Note: the gate's coverage step may now report your new
   IDs as missing tests only if `CURRENT_PHASE` lists their phase; today it
   lists none, so the gate stays green. Follow conventions §6 to finish.

## CONSTRAINTS

- Do not invent requirements. Every ID comes from the master plan.
- Write plainly. Prefer tables and code blocks to prose.
- Do not edit the telemetry design.

## DONE WHEN

- Both design docs and `SPEC-COVERAGE.md` exist and are committed.
- The step 5 self-check passes: every master plan ID is defined exactly once.
- `./scripts/check-req-coverage.sh --phase A1` (after U01 merges, or with
  your own copy of the logic) lists ARC-02, ARC-03 and ARC-04.
- The full gate is green.

## REPORT

- `horch note` after each doc.
- `horch done` summary: ID counts per family, every SPEC-TODO, and any master
  plan inconsistency you found.
