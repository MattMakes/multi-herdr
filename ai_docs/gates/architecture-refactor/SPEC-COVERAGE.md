# Spec coverage: Spec A and Spec B → requirement IDs

This file maps every section of Spec A (architecture refactor) and Spec B
(dataset and competitive execution mode) to requirement IDs. The IDs are
defined in:

- `ai_docs/designs/2026-10-02-architecture-refactor-design.md` (ARC, MKT, SKL)
- `ai_docs/designs/2026-10-02-dataset-competition-design.md` (MEA, PRE, CMP, JDG, PRO, EXP, SEC, NFR-06..11)
- `ai_docs/designs/2026-09-28-fleet-telemetry-design.md` (TEL, QUO, SPC, BAL, NFR-01..05)

Status values:

- `mapped`: the master plan names this section and its IDs. A row that says
  `mapped: design <section>; tests <names>` names the closed design section
  (`A` is the architecture design, `B` the dataset design) and the tests that
  pin it.
- `PENDING spec text`: the verbatim spec is not in the repository yet. No row
  has this status now. The final audit requires 0 such rows.

The finish run closed every unresolved spec marker. The gate step `no_spec_todo`
in `scripts/phase-gate.sh` fails if a new one appears.

Update this file at the end of every phase (master plan §5, step 3).

**Final audit.** After B6, the final audit walks both verbatim specs (the
design-doc appendices) section by section against this file. It must list
zero unmapped items and zero `PENDING spec text` rows.

| Spec | Section | Topic | IDs | Status |
|---|---|---|---|---|
| A | §1 | Purpose and scope | — | mapped: design A §1.1; tests arc_01_baseline_oracles_present (no behaviour of its own; pins the baseline the scope protects) |
| A | §2 | Concepts and ownership (teammate, harness, model, worker, task, execution, session, pane, skill) | ARC-02, ARC-03, ARC-04 | mapped: design A §2.1, §4.1 to §4.3; tests arc_02_ids_validate, arc_03_harness_kind_serde_compat, arc_04_status_legacy_roundtrip |
| A | §3 | Module rules (roster never calls Herdr; routing never launches; planning pure; telemetry observes; marketplace knows no teammates; CLI holds no policy; prompts hold no execution policy; no `std::env` in domain) | ARC-05, ARC-10, ARC-15, ARC-22, ARC-23, ARC-27, MKT-01, SKL-05 | mapped |
| A | §4 | Execution model and RuntimeContext (Execution fields, status, kind, session) | ARC-02, ARC-04, ARC-05, ARC-06, ARC-07, ARC-17 | mapped |
| A | §5 | Roster | ARC-08 | mapped: design A §2.2, §2.4, §3.1 (ARC-08); tests arc_08_unknown_field_rejected, arc_08_overlay_precedence |
| A | §6 | Harness ownership and capabilities | ARC-09, ARC-10, ARC-11 | mapped: design A §4.6; tests arc_09_argv_matches_baseline, arc_10_capabilities_match_legacy_predicates, arc_11_codex_discovery_by_workdir |
| A | §7 | Routing and provenance | ARC-12, ARC-13, ARC-14 | mapped: design A §4.4; tests arc_12_decisions_match_baseline, arc_13_routing_never_launches, arc_14_provenance_on_spawn_substitute_resume |
| A | §8 | SpawnRequest, spawn workflow and worker startup order | ARC-15, ARC-16, ARC-18 | mapped |
| A | §9 | Workspace and messaging | ARC-19, ARC-20, ARC-21, ARC-27 | mapped: design A §4.7, §4.8; tests arc_19_herdr_parsing_contract, arc_20_send_line_prompt_first, arc_21_done_order, arc_27_tile_balance_pure |
| A | §10 | Skill marketplace (SkillManifest, SkillSource, GitRevision, store, lock) | MKT-01, MKT-02, MKT-03, MKT-04, MKT-05, MKT-06, MKT-07, MKT-08, MKT-10 | mapped |
| A | §11 | Skills as first-class versioned objects; activation | SKL-01, SKL-02, SKL-03, SKL-04, SKL-07, SKL-08 | mapped: design A §3.3, §4.9; tests skl_01_bundled_catalog_versions_and_digests, skl_02_activation_matches_legacy_selection, skl_03_policy_mapping, skl_04_execution_records_skill_refs, skl_07_plugin_skills_separate, skl_08_briefing_matches_baseline_modulo_path |
| A | §12 | Harness skill exposure; marketplace CLI | SKL-05, SKL-06, MKT-09 | mapped: design A §3.3, §4.9, §4.10; tests skl_05_core_skill_model_has_no_harness_flags, skl_06_e2e_exposure_claude, mkt_09_cli_list_show_json |
| A | §13 | Compatibility (ledgers, frontmatter, offline skills) and the E2E list | ARC-08, ARC-14, ARC-16, ARC-17, ARC-26, SKL-01, SKL-06, MKT-08, bal_04..06 | mapped |
| A | §14 | Presentation map (cost, usage, sessions, route, quota, tile, balance) | ARC-22, ARC-23 | mapped |
| A | §15 | Phases 0 to 12 | ARC-01, ARC-25, NFR-08 | mapped: design A §5; tests arc_01_baseline_oracles_present, arc_25_no_shim_modules, nfr_08_phase_gate_runs_every_check |
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
| B | §1 | Purpose; phase 1 refactor subsumed by Spec A | — (Spec A) | mapped: design B §1.1; tests no test of its own; the Spec A tests pin the subsumed refactor |
| B | §2 | Entrypoint `multi-herdr-dataset`, CLI, exit codes | CMP-01 | mapped: design B §6 (B2); tests cmp_01_cli_args |
| B | §3 | Preflight (one ID per check) | PRE-01, PRE-02, PRE-03, PRE-04, PRE-05, PRE-06, PRE-07, PRE-08, PRE-09, PRE-10, PRE-11, PRE-12, PRE-13, NFR-10 | mapped |
| B | §4 | Measurement: IDs, digests, events, store, idempotency | MEA-01, MEA-02, MEA-03, MEA-04, MEA-05, MEA-08, MEA-09 | mapped: design B §2.3, §3.1, §4.1, §4.2; tests mea_01_digests_stable, mea_02_envelope_roundtrip_every_kind, mea_03_torn_line_skipped, mea_04_duplicate_key_noop, mea_05_full_lifecycles_fold_without_anomalies, mea_08_dataset_dir_not_read_as_ledger, mea_09_create_immutable_refuses_overwrite |
| B | §5 | WorkerRun schema; money | MEA-06, MEA-07, MEA-10, MEA-11 | mapped: design B §3.1, §4.3, §4.4; tests mea_06_worker_run_schema, mea_07_every_builtin_price_is_exact, mea_10_every_spawn_has_terminal_event, mea_11_fake_lifecycle_replays_identical_worker_run |
| B | §6 | Round state machine; domain types | CMP-02, CMP-03 | mapped: design B §3.3, §5; tests cmp_02_planner_files_have_no_adapter_imports, cmp_03_transition_table |
| B | §7 | Worktrees, one base SHA | CMP-04, CMP-08 | mapped: design B §3.3, §4.5; tests cmp_04_n_worktrees_same_base_modify_same_file, cmp_08_freeze_deterministic_sha_and_numstat |
| B | §8 | Planner: baseline, diversity, exploration | CMP-06, EXP-02 | mapped: design B §3.3, §3.6; tests cmp_06_planner_deterministic, exp_02_round_records_teacher_none |
| B | §9 | Coordinator: spawn, observe, budget, disk, validation | CMP-05, CMP-07, CMP-09, CMP-10, CMP-11, CMP-15 | mapped: design B §3.3, §4.6; tests cmp_05_e2e_candidates_in_dataset_workspace, cmp_07_done, cmp_09_gates_per_candidate_with_timeouts, cmp_10_hard_budget_cancels_and_retains, cmp_11_disk_pressure_stops_new_work, cmp_15_candidate_task_carries_rules |
| B | §10 | Judge: evaluator prose, bundle, background job | JDG-01, JDG-02, JDG-03, JDG-04, JDG-08, JDG-09, JDG-10, CMP-12, CMP-16 | mapped |
| B | §11 | Judgment schema; strict parsing; winner policy | JDG-05, JDG-06, JDG-07 | mapped |
| B | §12 | Promotion | PRO-01, PRO-02, PRO-03, PRO-04, PRO-05, PRO-06, PRO-08 | mapped: design B §3.5, §4.8, §5.2; tests pro_01_stale_judgment_cannot_promote, pro_02_revalidation_failure_rejected, pro_03_ff, pro_04_conflict_needs_intervention_preserves_worktrees, pro_05_receipt_fields, pro_06_crash_after_update_ref_resumes_once, pro_08_default_collects_only |
| B | §13 | Cleanup | PRO-07 | mapped: design B §3.5; tests pro_07_cleanup_failure_recorded_history_intact |
| B | §14 | Export, teacher seam, readiness, outcomes | EXP-01, EXP-03, EXP-04, EXP-05, EXP-06, EXP-07 | mapped: design B §3.6, §4.9, §4.10; tests exp_01_inert_returns_none, exp_03_row_shape, exp_04_export_golden, exp_05_coverage_and_verdict, exp_06_outcome_recorded_and_exported, exp_07_candidate_and_judge_in_usage |
| B | §15 | Failure matrix | CMP-07, CMP-10, CMP-11, CMP-13, CMP-14, JDG-05, JDG-08, PRO-01, PRO-04, PRO-07 | mapped |
| B | §16 | Security | SEC-01, SEC-02, SEC-03, SEC-04, SEC-05, SEC-06, SEC-07, SEC-08 | mapped |
| B | §17 | Non-functional (dependencies, hermetic tests, platforms) | NFR-06, NFR-07, NFR-10, NFR-11 | mapped: design B §3.8; tests nfr_06_dependency_allowlist, nfr_07_git_only_on_temp_repos, nfr_10_machine_probe_cfg_paths, nfr_11_splitmix64_matches_reference |
| B | §18 | Test categories (unit, property, integration, crash, golden, E2E) | MEA-04, MEA-05, MEA-06, MEA-07, MEA-11, CMP-03, CMP-04, CMP-06, CMP-08, CMP-13, JDG-05, JDG-06, PRE-09, PRO-02, PRO-03, PRO-04, SEC-01, EXP-04, NFR-11 | mapped |
| B | §19 | Phases | NFR-08 | mapped: design B §6; tests nfr_08_phase_gate_runs_every_check |
| B | §19 phase 8 | Training Laya | out of scope (OD4) | mapped |
| B | §20 | Non-goals (no online RL, self-training, DB server, distributed runs; no judge-driven edits; no automatic conflict resolution; no transcript hoarding; no second worker registry; no opaque composite score) | NFR-09, SEC-03, SEC-04, SEC-07, PRO-04, ARC-24, JDG-06 | mapped |

The implemented, tested behaviour is the spec (`ai_docs/plans/finish/00-conventions.md`).
Section numbers of the rows that were `PENDING spec text` stay provisional.
