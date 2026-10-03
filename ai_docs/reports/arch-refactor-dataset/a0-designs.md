# Report: U03 a0-designs

Branch `ard/a0-designs`. Phase A0. Docs only; no Rust code changed.

## Files

| File | Content |
|---|---|
| `ai_docs/designs/2026-10-02-architecture-refactor-design.md` | ARC, MKT, SKL tables; key types; phases A0-A12; compatibility; kernel fault points; Appendix A placeholder |
| `ai_docs/designs/2026-10-02-dataset-competition-design.md` | MEA, PRE, CMP, JDG, PRO, EXP, SEC, NFR-06..11 tables; key types; round state machine; phases B1-B6; crash points; LA-1..12; Appendix B placeholder |
| `ai_docs/gates/architecture-refactor/SPEC-COVERAGE.md` | spec section → IDs, status `mapped` or `PENDING spec text` |

## ID counts

| Family | IDs | Phases |
|---|---|---|
| ARC | 27 | A0-A7, A12, B3 |
| MKT | 10 | A8, A11 |
| SKL | 8 | A9, A10 |
| MEA | 11 | B1, B3 |
| PRE | 13 | B2 |
| CMP | 16 | B1-B5 |
| JDG | 10 | B4 |
| PRO | 8 | B5 |
| EXP | 7 | B3, B6 |
| SEC | 8 | B1, B3, B4 |
| NFR | 6 (NFR-06..11) | A0, A8, B1, B2 |
| Total | 124 | |

Self-check (a script run once, not committed): every one of the 124 master
plan §4 IDs is defined exactly once across `ai_docs/designs/*.md`, including
the telemetry design. Every Phase cell starts with a valid token. Phase A1
lists ARC-02, ARC-03 and ARC-04.

## Decisions

- Multi-phase cells are written `A6 / A12`, `A8 / A11`, `B3 / B6`, `B3 / B4`
  and `B3 to B5`. The first whitespace token is the phase, so both a prefix
  regex and a whitespace split read the right phase.
- Wildcard test cells are expanded: `mkt_06_*` (5 names), `skl_06_e2e_exposure_*`
  and `arc_26_e2e_lifecycle_matrix_*` (5 harnesses each), `arc_11_codex_/opencode_…`,
  `cmp_07_{…}`, `jdg_05_{…}`. `pre_01_*` became 5 names from the requirement
  text: `pre_01_git_root`, `pre_01_base_sha`, `pre_01_dirty_policy`,
  `pre_01_worktree_support`, `pre_01_namespace_free`.
- NFR-08 uses `nfr_08_phase_gate_runs_every_check` (per the unit plan). NFR-06
  keeps `scripts/check-deps.sh` as a non-test reference.
- The master plan names no test for the roster, routing and planning module
  rules. The architecture design §2.4 puts those scans in the nearest named
  test (`arc_08_*`, `arc_12_*`/`arc_13_*`, `arc_15_plan_deterministic`).
- Spawn fault points are spelled in full: `abort-after-execution-insert`,
  `abort-after-brief`, `abort-after-pane-split`, `fail-pane-split`, `fail-run`.
- Round states before JUDGING_BACKGROUND (CREATED, PREFLIGHT, ABORTED,
  PLANNED, PROVISIONING, RUNNING, VALIDATING) are a design choice; the master
  plan does not name them.
- From NEEDS_INTERVENTION, CLEANUP runs only on an operator `cleanup` command.
- Merged code was used for `runtime::machine` and `teacher::*` signatures
  (integration commit 658b57d).

## Master plan inconsistencies

1. B5 says `… → NEEDS_INTERVENTION → CLEANUP → COMPLETE` and also
   "NEEDS_INTERVENTION keeps everything". The design resolves this as above.
2. The master plan layout for `horch-marketplace` has no `error.rs`; unit plan
   U04 adds it.
3. A2 cites `tilecmd.rs:628` for the setsid code. Line 628 is
   `settle_after_close`; the setsid call is at lines 647-655.
4. ARC-11's Tests cell is shorthand (`arc_11_codex_/opencode_discovery_by_workdir`).
5. Multi-phase cells (`A6/A12`, `B3–B5`) have no whitespace around the separator.

## SPEC-TODO markers

| Marker | Where |
|---|---|
| Spec A §3 | module rule list (architecture §2.4) |
| Spec A §4 | RuntimeContext grouping; Execution field list |
| Spec A §8 | SpawnRequest field list; worker startup order |
| Spec A §10 | marketplace manifest key list |
| Spec A §13 | compatibility list |
| Spec A §16 | junior checklist items |
| Spec A §17 | acceptance criteria verbatim |
| Spec B §3 | preflight checks and thresholds |
| Spec B §10 | judge evaluator prose |
| Spec B §11 | Judgment schema; utility tie-break; Abstain/RejectAll mapping |
| Spec B §20 | non-goal list |
| Spec B event list | event kinds and payloads |
| Spec B round states | states before JUDGING_BACKGROUND |
| Spec B WorkerRun | WorkerRun field list |
| Spec B | whether a judge attempt needs its own ledger key |
| System One criteria semantics | `Question.criteria` |

## For later phases

- The coverage check reads the Phase column. Today `CURRENT_PHASE` lists no
  phase, so the gate does not require tests for these IDs yet.
- When Spec A and Spec B arrive, fill Appendix A and Appendix B. Then
  resolve every SPEC-TODO and renumber the `PENDING spec text` rows in
  `SPEC-COVERAGE.md`.
- When a merged unit's code differs from a signature in these docs, the
  merged code wins. Update the design in the same phase.
