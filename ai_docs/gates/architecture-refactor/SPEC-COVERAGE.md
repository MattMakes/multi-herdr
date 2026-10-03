# Spec coverage: Spec A and Spec B → requirement IDs

This file maps every section of Spec A (architecture refactor) and Spec B
(dataset and competitive execution mode) to requirement IDs. The IDs are
defined in:

- `ai_docs/designs/2026-10-02-architecture-refactor-design.md` (ARC, MKT, SKL)
- `ai_docs/designs/2026-10-02-dataset-competition-design.md` (MEA, PRE, CMP, JDG, PRO, EXP, SEC, NFR-06..11)
- `ai_docs/designs/2026-09-28-fleet-telemetry-design.md` (TEL, QUO, SPC, BAL, NFR-01..05)

Status values:

- `mapped`: the master plan names this section and its IDs.
- `PENDING spec text`: the verbatim spec is not in the repository yet. The
  row is a placeholder. Fill it from Appendix A or Appendix B.

Update this file at the end of every phase (master plan §5, step 3).

**Final audit.** After B6, the final audit walks both verbatim specs (the
design-doc appendices) section by section against this file. It must list
zero unmapped items and zero `PENDING spec text` rows.

| Spec | Section | Topic | IDs | Status |
|---|---|---|---|---|
| A | §1 | Purpose and scope | — | PENDING spec text |
| A | §2 | Concepts and ownership (teammate, harness, model, worker, task, execution, session, pane, skill) | ARC-02, ARC-03, ARC-04 | PENDING spec text |
| A | §3 | Module rules (roster never calls Herdr; routing never launches; planning pure; telemetry observes; marketplace knows no teammates; CLI holds no policy; prompts hold no execution policy; no `std::env` in domain) | ARC-05, ARC-10, ARC-15, ARC-22, ARC-23, ARC-27, MKT-01, SKL-05 | mapped |
| A | §4 | Execution model and RuntimeContext (Execution fields, status, kind, session) | ARC-02, ARC-04, ARC-05, ARC-06, ARC-07, ARC-17 | mapped |
| A | §5 | Roster | ARC-08 | PENDING spec text |
| A | §6 | Harness ownership and capabilities | ARC-09, ARC-10, ARC-11 | PENDING spec text |
| A | §7 | Routing and provenance | ARC-12, ARC-13, ARC-14 | PENDING spec text |
| A | §8 | SpawnRequest, spawn workflow and worker startup order | ARC-15, ARC-16, ARC-18 | mapped |
| A | §9 | Workspace and messaging | ARC-19, ARC-20, ARC-21, ARC-27 | PENDING spec text |
| A | §10 | Skill marketplace (SkillManifest, SkillSource, GitRevision, store, lock) | MKT-01, MKT-02, MKT-03, MKT-04, MKT-05, MKT-06, MKT-07, MKT-08, MKT-10 | mapped |
| A | §11 | Skills as first-class versioned objects; activation | SKL-01, SKL-02, SKL-03, SKL-04, SKL-07, SKL-08 | PENDING spec text |
| A | §12 | Harness skill exposure; marketplace CLI | SKL-05, SKL-06, MKT-09 | PENDING spec text |
| A | §13 | Compatibility (ledgers, frontmatter, offline skills) and the E2E list | ARC-08, ARC-14, ARC-16, ARC-17, ARC-26, SKL-01, SKL-06, MKT-08, bal_04..06 | mapped |
| A | §14 | Presentation map (cost, usage, sessions, route, quota, tile, balance) | ARC-22, ARC-23 | mapped |
| A | §15 | Phases 0 to 12 | ARC-01, ARC-25, NFR-08 | PENDING spec text |
| A | §16 | Junior checklist | NFR-08 (`CHECKLIST.md`) | mapped |
| A | §17 | Acceptance criteria 1 to 16 (see the architecture design §3.4) | ARC-02..ARC-06, ARC-09, ARC-10, ARC-12..ARC-17, ARC-19, ARC-22..ARC-27, SKL-01, SKL-03, SKL-04, SKL-06, MKT-02, MKT-08, MKT-10, bal_04..06 | mapped |
| A | §17.1 | horch is thin; policy in core | ARC-22, ARC-25 | mapped |
| A | §17.2 | spawn is a workflow from a pure plan | ARC-15, ARC-16 | mapped |
| A | §17.3 | separate types | ARC-02, ARC-03, ARC-04 | mapped |
| A | §17.4 | no ambient env in domain/planner | ARC-05, ARC-06 | mapped |
| A | §17.5 | harness behavior in harness modules | ARC-09, ARC-10 | mapped |
| A | §17.6 | routing pure; provenance persisted | ARC-12, ARC-13, ARC-14 | mapped |
| A | §17.7 | telemetry consumes execution identity | ARC-23 | mapped |
| A | §17.8 | old ledgers usable | ARC-17 | mapped |
| A | §17.9 | immutable resolved skill identities | SKL-01, SKL-04 | mapped |
| A | §17.10 | git skills pinned before runtime | MKT-02, MKT-08 | mapped |
| A | §17.11 | deterministic vs model-routed skills explicit | SKL-03 | mapped |
| A | §17.12 | tile/balance pure | ARC-19, ARC-27 | mapped |
| A | §17.13 | E2E covers all harness lifecycle and routing paths | ARC-26, ARC-14, bal_04..06, SKL-06 | mapped |
| A | §17.14 | adding a harness ≈ one module | ARC-10 | mapped |
| A | §17.15 | adding a skill source ≈ resolution/fetch | MKT-10 | mapped |
| A | §17.16 | competition composes ordinary executions | ARC-24 | mapped |
| A | §18 | Competition is a composition of ordinary executions | ARC-24, CMP-05 | mapped |
| A | non-goals | Sync only; no daemon or DB | NFR-09 | mapped |
| B | §1 | Purpose; phase 1 refactor subsumed by Spec A | — (Spec A) | PENDING spec text |
| B | §2 | Entrypoint `multi-herdr-dataset`, CLI, exit codes | CMP-01 | PENDING spec text |
| B | §3 | Preflight (one ID per check) | PRE-01, PRE-02, PRE-03, PRE-04, PRE-05, PRE-06, PRE-07, PRE-08, PRE-09, PRE-10, PRE-11, PRE-12, PRE-13, NFR-10 | mapped |
| B | §4 | Measurement: IDs, digests, events, store, idempotency | MEA-01, MEA-02, MEA-03, MEA-04, MEA-05, MEA-08, MEA-09 | PENDING spec text |
| B | §5 | WorkerRun schema; money | MEA-06, MEA-07, MEA-10, MEA-11 | PENDING spec text |
| B | §6 | Round state machine; domain types | CMP-02, CMP-03 | PENDING spec text |
| B | §7 | Worktrees, one base SHA | CMP-04, CMP-08 | PENDING spec text |
| B | §8 | Planner: baseline, diversity, exploration | CMP-06, EXP-02 | PENDING spec text |
| B | §9 | Coordinator: spawn, observe, budget, disk, validation | CMP-05, CMP-07, CMP-09, CMP-10, CMP-11, CMP-15 | PENDING spec text |
| B | §10 | Judge: evaluator prose, bundle, background job | JDG-01, JDG-02, JDG-03, JDG-04, JDG-08, JDG-09, JDG-10, CMP-12, CMP-16 | mapped |
| B | §11 | Judgment schema; strict parsing; winner policy | JDG-05, JDG-06, JDG-07 | mapped |
| B | §12 | Promotion | PRO-01, PRO-02, PRO-03, PRO-04, PRO-05, PRO-06, PRO-08 | PENDING spec text |
| B | §13 | Cleanup | PRO-07 | PENDING spec text |
| B | §14 | Export, teacher seam, readiness, outcomes | EXP-01, EXP-03, EXP-04, EXP-05, EXP-06, EXP-07 | PENDING spec text |
| B | §15 | Failure matrix | CMP-07, CMP-10, CMP-11, CMP-13, CMP-14, JDG-05, JDG-08, PRO-01, PRO-04, PRO-07 | mapped |
| B | §16 | Security | SEC-01, SEC-02, SEC-03, SEC-04, SEC-05, SEC-06, SEC-07, SEC-08 | mapped |
| B | §17 | Non-functional (dependencies, hermetic tests, platforms) | NFR-06, NFR-07, NFR-10, NFR-11 | PENDING spec text |
| B | §18 | Test categories (unit, property, integration, crash, golden, E2E) | MEA-04, MEA-05, MEA-06, MEA-07, MEA-11, CMP-03, CMP-04, CMP-06, CMP-08, CMP-13, JDG-05, JDG-06, PRE-09, PRO-02, PRO-03, PRO-04, SEC-01, EXP-04, NFR-11 | mapped |
| B | §19 | Phases | NFR-08 | PENDING spec text |
| B | §19 phase 8 | Training Laya | out of scope (OD4) | mapped |
| B | §20 | Non-goals (no online RL, self-training, DB server, distributed runs; no judge-driven edits; no automatic conflict resolution; no transcript hoarding; no second worker registry; no opaque composite score) | NFR-09, SEC-03, SEC-04, SEC-07, PRO-04, ARC-24, JDG-06 | mapped |

Section numbers in `PENDING spec text` rows are provisional. The master plan
names only §3, §4, §8, §10, §13, §14, §16, §17, §18 of Spec A and §3, §10,
§11, §15, §16, §18, §20 of Spec B. Renumber and split the other rows when the
verbatim text arrives.
