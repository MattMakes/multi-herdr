# Report: spec-b-events (T3)

Unit: `spec-b-events`. Branch: `ds/spec-b-events`. Plan:
`ai_docs/plans/finish/t3-spec-b-events.md`.

## Outcome

Every SPEC-TODO in my files is closed. The implemented, tested behaviour is
now the spec text. No code behaviour changed: the code already made every
decision, and the tests already pin it. Code changes are comments only.

## Markers closed

| # | Location (before) | What it says now | Evidence | Code changed |
|---|---|---|---|---|
| 1 | design §1.4, `SPEC-TODO(Spec B §20)` | The complete non-goal list, 8 rows, each with its requirement and the test that pins it. The DB-server and distributed-run lines are split from the online-RL line. | `exp_01_inert_returns_none`, `nfr_09_no_async_runtime_deps`, `nfr_06_dependency_allowlist`, `sec_04_judge_cwd_bundle_tools_readonly`, `sec_07_gates_only_from_config`, `pro_04_conflict_needs_intervention_preserves_worktrees`, `sec_03_no_transcript_copies_by_default`, `arc_24_candidates_are_ordinary_executions`, `jdg_06_policy_table` | no |
| 2 | design §4.1, `SPEC-TODO(Spec B event list)` | The complete list of 27 kinds with every payload. Adds the 4 kinds the master plan does not name (`round.needs_intervention`, `round.cleanup_started`, `round.completed`, `operator.promote`) and their payloads, the `publish` and `validation_ids` fields of `promotion.started`, the `kind` tag of `JudgeFailure`, the `final_outcome` rule, and a table of actor, writer and idempotency key per kind. | `mea_02_envelope_roundtrip_every_kind` (pins `EventKind::KNOWN`), `mea_02_unknown_kind_preserved`, `mea_02_known_kind_bad_payload_is_an_error`, `mea_05_round_needs_intervention_and_completion_checks`, `mea_05_full_lifecycles_fold_without_anomalies` | comments only |
| 3 | `measure/event.rs` `EventKind` doc | Points to design §4.1. | as 2 | comment only |
| 4 | `measure/event.rs` `RoundNeedsIntervention` | Says why the kind exists; points to §4.1. | as 2 | comment only |
| 5 | `measure/event.rs` `RoundCleanupStarted` | as 4 | as 2 | comment only |
| 6 | `measure/event.rs` `RoundCompleted` | as 4 | as 2 | comment only |
| 7 | `measure/projection.rs` module doc | Points to §4.1 for why the 3 round kinds exist. | as 2 | comment only |
| 8 | design §4.3, `SPEC-TODO(Spec B WorkerRun)` | The complete field list plus a source table: the event or record each field comes from, and its value when the source is missing. | `mea_06_worker_run_schema` (exact key set per object), `mea_06_no_winner_field`, `mea_11_fake_lifecycle_replays_identical_worker_run`, golden `tests/golden/worker-run-1.0.0.json` (unchanged) | no |
| 9 | `measure/worker_run.rs` module doc | Points to §4.3. The stale sentence "The `Execution` struct arrives in A6" now names `horch/src/dataset/export.rs:facts_of`. | as 8 | comment only |
| 10 | design §5, `SPEC-TODO(Spec B round states)` | The 16 states, why the states before JUDGING_BACKGROUND exist, and the full `TABLE` as 3 tables (default path, promotion path, operator transitions). The old table did not match the code; it now matches row for row. Also lists the checks the fold makes beyond the table. | `cmp_03_transition_table` (every row, every refused pair), `cmp_03_prop_no_invalid_path_to_promoted`, `mea_05_*` | no |
| 11 | `competition/model.rs` `RoundState` doc | Points to §5. | as 10 | comment only |
| 12 | `competition/state.rs` module doc | Points to §5. | as 10 | comment only |
| 13 | `competition/state.rs` `TABLE`, `SPEC-TODO(Spec B §promote)` | `promote <round>` re-enters at DECIDED from COMPLETE as well as NEEDS_INTERVENTION. Written in design §5.3. | `operator_promote_reenters_revalidation`, `pro_08_promote_to_and_promote_cmd` | comment only |

Marker 13 is not in the plan's GOAL list. It is in `competition/state.rs`,
which the plan gives me, and no other unit names it, so I closed it.

## Decisions

- The design §5 table was written before the code. Where they differed, the
  code (`competition/state.rs:TABLE`) is the spec, because `cmp_03` pins
  every row. Differences fixed in the text: a failed preflight stays
  PREFLIGHT until `experiment.aborted`; 0 eligible goes through
  `validation.completed:none_eligible` then `winner.rejected`; the CAS-lost
  case and a dirty target use `round.needs_intervention:promotion`;
  `promotion.rolled_back` is valid in PROMOTED and COMPLETE; the operator can
  stop 6 live states; `operator.promote` is valid from COMPLETE.
- The §20 table splits "No online RL, no self-training, no DB server, no
  distributed runs" into 3 rows, so each line names the test that pins it.

## Gotchas

- `promotion.requested` is a `RoundEvent`, not a logged event kind. The
  fold applies it after `winner.selected{requested}` and `operator.promote`.
- The table row `(CREATED, experiment.aborted, ABORTED)` is never reached by
  the fold, because `experiment.created` moves an experiment to PREFLIGHT at
  once. It is harmless.

## Not done / outside scope

- `ai_docs/designs/...` Appendix B still says "PENDING: the orchestrator
  inserts the operator's Spec B text here". Not mine.
- Markers in `ai_docs/plans` and `ai_docs/reports` that cite these sections
  are for unit T6.
- Flaky test (outside scope): `exp_07_candidate_and_judge_in_usage`
  (`crates/horch-e2e/tests/usage_dataset.rs:159`) failed 1 time under the
  full gate at high machine load. The report counted the judge 2 times: 4
  sessions and 9000 input tokens, against 3 sessions expected. The test
  passed alone, and the next full gate was green. A judge retry under load
  is the likely cause. Nobody has fixed it.

## Checks

- Full gate green before the §20 commit and before the event-list commit
  (after the 1 flake above).
- After rule change 2: targeted checks on the rebased branch (see the
  READY-TO-MERGE message).
