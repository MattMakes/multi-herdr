# Design: dataset and competitive execution mode (Spec B, phases B1 to B6)

| | |
|---|---|
| Status | Implemented. Phases B1, B2, B3, B4, B5 and B6 landed on `arch-refactor-dataset`, on top of the architecture refactor (A0 to A12). Spec B verbatim is pending (Appendix B). |
| Date | 2026-10-02 |
| Author | The fleet (unit `a0-designs`), from the operator's master plan |
| Source of truth | `ai_docs/plans/arch-refactor-dataset/00-master-plan.md` until Appendix B holds Spec B |
| Companion | `ai_docs/designs/2026-10-02-architecture-refactor-design.md` (kernel types: ids, Execution, SpawnRequest, routing, harness) |
| Coverage map | `ai_docs/gates/architecture-refactor/SPEC-COVERAGE.md` |

Requirement tables in section 3 are machine-read by
`scripts/check-req-coverage.sh`. Do not change their header rows. Do not
define an ID in any other table. NFR-01 to NFR-05 are defined in
`2026-09-28-fleet-telemetry-design.md`; this document defines NFR-06 to
NFR-11 only.

---

## 1. Status, scope and decisions

### 1.1 Scope

A separate, explicitly expensive entrypoint, `multi-herdr-dataset`, composes
the same kernel as `horch`:

1. preflight, then a plan;
2. N candidates in isolated worktrees from one base SHA;
3. validation, then a background blind judge;
4. a deterministic winner policy, optional promotion, then export.

Every candidate and the judge are ordinary executions (Spec A §18, ARC-24).
Rust decides everything deterministic. Agents decide only where semantic
judgment is needed: the candidates write code, and the judge ranks them.

Out of scope: Spec B phase 1 (a refactor; Spec A subsumes it) and Spec B
phase 8 (training Laya).

### 1.2 Operator decisions

| # | Decision |
|---|---|
| OD2 | The dataset binary is `multi-herdr-dataset`. `horch` keeps its name. |
| OD3 | Dataset storage is `$HORCH_STATE_DIR/multi-herdr/<project-slug>/`. It must be a subdirectory, because `telemetry::collect::read_ledgers` parses every `state_root/*.json`. The marketplace store is `${XDG_DATA_HOME:-~/.local/share}/horch/` (Spec A §10). |
| OD4 | Clef and Laya are **inert** in this branch. Both speak the System One API: `POST /v1/systemone` with typed `choice`/`score`/`noul` questions, returning a probability per option. Clef is Cloudflare's 27B/9B Apache-2.0 decision model; Laya is Convai's 421M local model, fine-tuned on `{state, questions, answers}`. We ship the seam, record eligible sets, planner propensities and `teacher: none`, export System-One/Laya-shaped rows, and add a **data-readiness report**. There is no HTTP client and no API keys. Clef is enabled only once readiness says the local data justifies it, and Laya later still. |
| OD5 | **No promotion by default.** A round ends at DECIDED with a winner, NEEDS_INTERVENTION, or REJECTED. `--promote-to <branch>` or `multi-herdr-dataset promote <round>` runs the full deterministic PromotionEngine. |

### 1.3 The inert Clef/Laya decision (OD4)

| Part | In this branch |
|---|---|
| `teacher::system_one` | serde types for `systemone/v1` (merged, `crates/horch-core/src/teacher/system_one.rs`) |
| `teacher::DecisionModel` | trait; the only implementation is `teacher::inert::Inert`, which returns `None` |
| Recorded per round | eligible set, exclusion reasons, planner propensities, `teacher: {"id":"none","probabilities":null}` |
| Export | System-One/Laya-shaped rows (`mh.export/1.0.0`) |
| Readiness | a report that says when local data justifies enabling Clef, then Laya |
| Network | none. No HTTP crate (NFR-06), no API key |

### 1.4 Non-goals (Spec B §20) and what enforces them

This is the complete non-goal list of Spec B. The original Spec B text is not
available; this list is the master plan's §20 table
(`ai_docs/plans/arch-refactor-dataset/00-master-plan.md`, "Spec B §20
non-goals"), and the tests below pin each line. A change that breaks a
non-goal fails one of these tests.

| Non-goal | Enforced by | Test that pins it |
|---|---|---|
| No online RL and no self-training: no recorded outcome changes a later plan or decision | OD4; `teacher::inert::Inert` is the only `DecisionModel`, and it returns `None` | `teacher::system_one::tests::exp_01_inert_returns_none` |
| No DB server: the event log is JSONL files under `DatasetPaths` | NFR-09 (no database crate, no async runtime) | `tests/nfr.rs:nfr_09_no_async_runtime_deps` |
| No distributed runs: one coordinator process on one host, no network | NFR-06, NFR-09 (no HTTP crate) | `tests/nfr.rs:nfr_06_dependency_allowlist`, `nfr_09_no_async_runtime_deps` |
| No judge-driven code edits: the judge reads a read-only bundle, and its output is never executed | SEC-04, SEC-07 | `horch-e2e/tests/judge.rs:sec_04_judge_cwd_bundle_tools_readonly`, `tests/vcs.rs:sec_07_gates_only_from_config` |
| No automatic conflict resolution: a promotion conflict stops at NEEDS_INTERVENTION with the worktrees kept | PRO-04 | `tests/promotion.rs:pro_04_conflict_needs_intervention_preserves_worktrees` |
| No transcript hoarding: events carry transcript refs and digests; raw copies need `retain_transcripts: true` | SEC-03 | `horch-e2e/tests/dataset.rs:sec_03_no_transcript_copies_by_default` |
| No second worker registry: a candidate is an ordinary `Execution` in the ledger | ARC-24 | `tests/coordinator.rs:arc_24_candidates_are_ordinary_executions` |
| No opaque composite score: the winner policy is a fixed table, and the judgment keeps every component score | JDG-06 | `tests/evaluation.rs:jdg_06_policy_table` |

---

## 2. Storage layout and permissions

### 2.1 `DatasetPaths` (OD3, `measure/paths.rs`)

Root: `$HORCH_STATE_DIR/multi-herdr/<project-slug>/`. `<project-slug>` is
`ledger::slug(project)`. The root is a subdirectory, so
`telemetry::collect::read_ledgers` (which reads `state_root/*.json`) never
parses a dataset file (MEA-08).

```
<root>/
  events/YYYY-MM-DD.jsonl              append-only event log (UTC day)
  events.lock/                         DirLock for appends and the idempotency index
  experiments/<exp>/manifest.json      experiment manifest, PreflightReport, environment_digest
  experiments/<exp>/artifacts/<round>/judge-input/{task.md, rubric.md, schema.json,
                                                   candidates/<L>/{diff.patch, validation.json},
                                                   manifest.json}
  experiments/<exp>/artifacts/<round>/validation/<L>/<gate>.log   redacted, ≤ 256 KiB
  judgements/<round>.json              the Judgment, create_immutable
  promotions/<round>.json              the PromotionReceipt, create_immutable
  exports/<label_policy_version>/<ts>.jsonl
  jobs/<round>/judge-<n>/{heartbeat, output.json, exit.json}
  worktrees/<exp>/<label>/             default --worktree-root
```

No projection is stored. `status`, `rebuild` and `export` fold the events on
each read (MEA-05), so a projection file would be a second copy that can go
stale. D19 deleted the `rounds/<round>.json` entry and
`DatasetPaths::round_file`, which no code wrote or read.

```rust
pub struct DatasetPaths { root: PathBuf }
impl DatasetPaths {
    pub fn new(state_root: &Path, project: &Path) -> DatasetPaths;
    pub fn root(&self) -> &Path;
    pub fn events_dir(&self) -> PathBuf;
    pub fn events_file(&self, day: chrono::NaiveDate) -> PathBuf;
    pub fn events_lock_dir(&self) -> PathBuf;
    pub fn experiment_dir(&self, exp: &ExperimentId) -> PathBuf;
    pub fn manifest(&self, exp: &ExperimentId) -> PathBuf;
    pub fn artifacts_dir(&self, exp: &ExperimentId, round: &RoundId) -> PathBuf;
    pub fn judge_input_dir(&self, exp: &ExperimentId, round: &RoundId) -> PathBuf;
    pub fn judgement(&self, round: &RoundId) -> PathBuf;
    pub fn promotion(&self, round: &RoundId) -> PathBuf;
    pub fn exports_dir(&self, label_policy_version: &str) -> PathBuf;
    pub fn job_dir(&self, round: &RoundId, attempt: u32) -> PathBuf;
    pub fn default_worktree_root(&self, exp: &ExperimentId) -> PathBuf;
}
```

### 2.2 Permissions (SEC-05)

| Object | Mode | Written by |
|---|---|---|
| every directory under `<root>` | 0700 | `fsx::ensure_private_dir` |
| every file under `<root>` | 0600 | `fsx::write_atomic`, `create_immutable` with `PRIVATE_FILE` |
| judge-input bundle files | 0400 | `evaluation::judge_input` after the digest |

Windows: modes are not set; the `cfg(windows)` path is a no-op.

### 2.3 Write disciplines (MEA-09)

| Function | Semantics | Used for |
|---|---|---|
| `fsx::create_immutable(path, bytes, mode) -> Result<Created>` | `create_new` + fsync + dir fsync; same bytes → `AlreadyIdentical`; different bytes → `FsxError::Conflict` | judgment, receipt, judge-input files, judge output and exit, manifest, `run.json`, export files |
| `fsx::write_atomic(path, bytes, mode)` | temp + fsync + rename + dir fsync | the dataset workspace file, usage records, retained transcripts, gate logs, judge heartbeat (and outside `<root>`: the execution ledger, briefs) |
| `measure::store` append | one line per event, under `DirLock events.lock`, fsync per append | events |

---

## 3. Requirements

Header rule: `| ID | Requirement | Phase | Tests |` (PRE uses `Check`). The
first token of the Phase cell is the ID's phase. Test names are exact.
Lowercase non-ID entries (`golden_prompts`, `nfr_06`) name tests that exist
or that another row defines.

### 3.1 MEA: measurement

| ID | Requirement | Phase | Tests |
|---|---|---|---|
| MEA-01 | v7 IDs; task, config, judge-policy, env and repo digests | B1 | mea_01_ids_v7, mea_01_digests_stable |
| MEA-02 | Envelope and every event kind | B1 | mea_02_envelope_roundtrip_every_kind, mea_02_unknown_kind_preserved |
| MEA-03 | Append-only, fsync, lock, torn-tolerant | B1 | mea_03_torn_line_skipped, mea_03_two_process_appends_ordered |
| MEA-04 | Idempotency | B1 | mea_04_duplicate_key_noop, mea_04_prop_duplicates_no_effect |
| MEA-05 | Rebuildable projections | B1 | mea_05_prop_fold_deterministic, mea_05_prop_replay_identical, mea_05_rebuild_equals_live |
| MEA-06 | WorkerRun 1.0.0; facts ≠ scores; no winner field | B1 | mea_06_worker_run_schema, mea_06_no_winner_field |
| MEA-07 | Integer µ$ and cost_source | B1 | mea_07_nano_accumulation_exact, mea_07_rounding_once, mea_07_cost_source |
| MEA-08 | Dataset dir is a subdir | B1 | mea_08_dataset_dir_not_read_as_ledger |
| MEA-09 | Atomic and immutable writes | B1 | mea_09_create_immutable_refuses_overwrite, mea_09_write_atomic_replaces_durably |
| MEA-10 | Every invocation recorded | B3 | mea_10_every_spawn_has_terminal_event |
| MEA-11 | Fake lifecycle replays to identical WorkerRun | B1 | mea_11_fake_lifecycle_replays_identical_worker_run |

### 3.2 PRE: preflight (Spec B §3, one ID per check)

| ID | Check | Phase | Tests |
|---|---|---|---|
| PRE-01 | Git: root, base SHA, dirty policy, worktree support, namespace free | B2 | pre_01_git_root, pre_01_base_sha, pre_01_dirty_policy, pre_01_worktree_support, pre_01_namespace_free |
| PRE-02 | Disk: N × (checkout + build) + artifacts + headroom | B2 | pre_02_disk_budget |
| PRE-03 | Memory, plus local model and process footprint | B2 | pre_03_memory_footprint |
| PRE-04 | CPU/GPU; local-inference bound | B2 | pre_04_local_inference_bound |
| PRE-05 | Process and fd limits | B2 | pre_05_rlimits |
| PRE-06 | Every harness and version resolved before worktrees | B2 | pre_06_harness_resolution_before_worktree |
| PRE-07 | Models/providers via no-turn probes; no secrets persisted | B2 | pre_07_probe_no_secret_persisted |
| PRE-08 | Safe N | B2 | pre_08_safe_n_waves |
| PRE-09 | Budget projection under the soft limit; hard ceiling configured | B2 | pre_09_budget_projection_soft_limit |
| PRE-10 | Judge available; budget reserved | B2 | pre_10_judge_available_and_reserved |
| PRE-11 | Storage: write, lock, fsync, rename | B2 | pre_11_storage_probe |
| PRE-12 | Refuse before any worktree or model; report persisted | B2 | pre_12_e2e_refuses_before_worktree_or_model |
| PRE-13 | herdr reachable; horch exe found; trust warning | B2 | pre_13_herdr_and_horch_exe |

### 3.3 CMP: competition

| ID | Requirement | Phase | Tests |
|---|---|---|---|
| CMP-01 | Separate expensive entrypoint | B2 | cmp_01_cli_args |
| CMP-02 | Domain free of adapters | B1 | cmp_02_domain_has_no_adapter_imports |
| CMP-03 | Round state machine | B3 | cmp_03_transition_table, cmp_03_prop_no_invalid_path_to_promoted |
| CMP-04 | N worktrees, one base_sha | B3 | cmp_04_n_worktrees_same_base_modify_same_file |
| CMP-05 | Kernel spawn, dedicated workspace, cwd = worktree | B3 | cmp_05_e2e_candidates_in_dataset_workspace |
| CMP-06 | Baseline + diversity + exploration; eligible set and propensities persisted | B3 | cmp_06_planner_deterministic, cmp_06_propensities, cmp_06_eligible_set_persisted, cmp_06_baseline_routed_through_gate |
| CMP-07 | Completion observation | B3 | cmp_07_done, cmp_07_pane_vanished, cmp_07_agent_exit, cmp_07_timeout, cmp_07_candidate_crash_kept_in_round |
| CMP-08 | Freeze | B3 | cmp_08_freeze_deterministic_sha_and_numstat |
| CMP-09 | Independent validation | B3 | cmp_09_gates_per_candidate_with_timeouts |
| CMP-10 | Live hard budget | B3 | cmp_10_hard_budget_cancels_and_retains |
| CMP-11 | Disk pressure | B3 | cmp_11_disk_pressure_stops_new_work |
| CMP-12 | Judge only on a terminal set or deadline | B4 | cmp_12_judge_waits_for_terminal_set |
| CMP-13 | Crash/restart without duplicates | B3 to B5 | cmp_13_crash_every_boundary, cmp_13_rebuild_after_power_loss |
| CMP-14 | All fail → reject, keep data | B3 | cmp_14_all_candidates_fail_round_rejected |
| CMP-15 | Rules via template; goldens unchanged | B3 | cmp_15_candidate_task_carries_rules, golden_prompts |
| CMP-16 | Single authority; judge is the only background job | B4 | cmp_16_only_judge_detached |

### 3.4 JDG: judge

| ID | Requirement | Phase | Tests |
|---|---|---|---|
| JDG-01 | judge.md read-only, verbatim prose, passes check, not spawnable | B4 | jdg_01_judge_teammate_check, jdg_01_spawn_judge_refused |
| JDG-02 | Immutable, anonymous bundle | B4 | jdg_02_bundle_digest_and_readonly, jdg_02_labels_seeded_shuffle, jdg_02_no_identity_cost_latency |
| JDG-03 | Versioned rubric; policy digest | B4 | jdg_03_policy_digest_changes_with_inputs |
| JDG-04 | Background job; parent writes the judgment | B4 | jdg_04_job_writes_only_job_dir, jdg_04_parent_writes_judgment_atomically |
| JDG-05 | Strict parser | B4 | jdg_05_unknown_enum, jdg_05_missing_field, jdg_05_duplicate, jdg_05_impossible_label, jdg_05_non_finite, jdg_05_malformed_no_promotion |
| JDG-06 | Winner policy; component scores kept | B4 | jdg_06_policy_table |
| JDG-07 | Utility tie-break only when enabled | B4 | jdg_07_tie_needs_intervention_by_default |
| JDG-08 | Timeout/crash recorded as data; retry; restart | B4 | jdg_08_crash_records_and_retries, jdg_08_timeout, jdg_08_restart_discovers_job |
| JDG-09 | Headless, no API key; counted as an execution | B4 | jdg_09_argv_and_env, jdg_09_judge_execution_in_ledger |
| JDG-10 | Blindness flag | B4 | jdg_10_identity_leak_flagged |

### 3.5 PRO: promotion

| ID | Requirement | Phase | Tests |
|---|---|---|---|
| PRO-01 | Frozen SHAs; stale judgment rejected | B5 | pro_01_stale_judgment_cannot_promote |
| PRO-02 | Revalidate the integrated commit | B5 | pro_02_revalidation_failure_rejected, pro_02_e2e_only_valid_candidate_promotable |
| PRO-03 | ff, cherry-pick or CAS; dirty checkout untouched | B5 | pro_03_ff, pro_03_cherry_pick_when_target_moved, pro_03_checked_out_dirty_needs_intervention |
| PRO-04 | Conflict → NEEDS_INTERVENTION, worktrees kept | B5 | pro_04_conflict_needs_intervention_preserves_worktrees |
| PRO-05 | Receipt durable before cleanup | B5 | pro_05_receipt_fields, pro_05_cleanup_after_receipt_only |
| PRO-06 | No double promotion | B5 | pro_06_crash_after_update_ref_resumes_once |
| PRO-07 | Cleanup keeps branches; failures recorded | B5 | pro_07_cleanup_failure_recorded_history_intact |
| PRO-08 | No promotion by default (OD5); opt-in paths | B5 | pro_08_default_collects_only, pro_08_promote_to_and_promote_cmd |

### 3.6 EXP: export, teacher and readiness

| ID | Requirement | Phase | Tests |
|---|---|---|---|
| EXP-01 | Inert System One seam; no HTTP | B6 | exp_01_inert_returns_none, exp_01_system_one_serde_shape, nfr_06 |
| EXP-02 | Eligible set, propensities and teacher: none recorded | B3 / B6 | exp_02_round_records_teacher_none |
| EXP-03 | Laya row shape, label_policy_version | B6 | exp_03_row_shape |
| EXP-04 | Regenerable without agents; byte-identical | B6 | exp_04_export_golden, exp_04_regenerate_twice_identical |
| EXP-05 | Readiness report | B6 | exp_05_coverage_and_verdict |
| EXP-06 | Outcomes | B6 | exp_06_outcome_recorded_and_exported |
| EXP-07 | Telemetry and cost see candidates and the judge | B6 | exp_07_candidate_and_judge_in_usage |

### 3.7 SEC: security (Spec B §16)

| ID | Requirement | Phase | Tests |
|---|---|---|---|
| SEC-01 | Redact before persisting prompts, outputs, env metadata and diffs | B1 | sec_01_redaction_patterns, sec_01_e2e_sentinel_scan |
| SEC-02 | No keys and no full env; allowlisted snapshot | B1 | sec_02_env_snapshot_allowlist |
| SEC-03 | Transcript refs and digests by default; raw retention explicit | B3 | sec_03_no_transcript_copies_by_default |
| SEC-04 | Judge gets only the bundle, with read-only tools | B4 | sec_04_judge_cwd_bundle_tools_readonly |
| SEC-05 | 0600/0700 permissions | B1 | sec_05_permissions |
| SEC-06 | Output untrusted until parsed; size caps | B4 | sec_06_caps_enforced |
| SEC-07 | Never execute judge output | B4 | sec_07_gates_only_from_config |
| SEC-08 | ANTHROPIC_API_KEY in no child process | B3 / B4 | sec_08_e2e_no_api_key_in_any_child |

### 3.8 NFR (continues the telemetry design's NFR-01 to NFR-05)

| ID | Requirement | Phase | Tests |
|---|---|---|---|
| NFR-06 | Allowlist amendment (sha2, marketplace list, no HTTP crate) | A8 | nfr_06_dependency_allowlist, scripts/check-deps.sh |
| NFR-07 | Hermetic; real git only on temp repos | A8 | nfr_07_git_only_on_temp_repos |
| NFR-08 | Every commit gated | A0 | nfr_08_phase_gate_runs_every_check |
| NFR-09 | Sync only; no daemon or DB (Spec A and B non-goals) | A0 | nfr_09_no_async_runtime_deps |
| NFR-10 | Linux + macOS; Windows cfg | B2 | nfr_10_machine_probe_cfg_paths |
| NFR-11 | In-repo PRNG; no proptest | B1 | nfr_11_no_proptest |

### 3.9 Spec B §15 failure matrix → tests

| Failure | Test |
|---|---|
| Candidate crash | cmp_07_candidate_crash_kept_in_round |
| Power loss | cmp_13_rebuild_after_power_loss |
| Judge crash | jdg_08_crash_records_and_retries |
| Invalid judge JSON | jdg_05_malformed_no_promotion |
| Winner changed after judgment | pro_01_stale_judgment_cannot_promote |
| Promotion conflict | pro_04_conflict_needs_intervention_preserves_worktrees |
| Hard budget | cmp_10_hard_budget_cancels_and_retains |
| Disk pressure | cmp_11_disk_pressure_stops_new_work |
| Cleanup fails | pro_07_cleanup_failure_recorded_history_intact |
| All fail | cmp_14_all_candidates_fail_round_rejected |

### 3.10 Spec B §18 test categories

| Category | Tests |
|---|---|
| Unit | mea_06, jdg_05, jdg_06, mea_07, pre_09, cmp_03, mea_04, sec_01, cmp_06 |
| Property (SplitMix64, 10k cases, fixed seeds) | mea_05_prop_fold_deterministic, mea_05_prop_replay_identical, mea_04_prop_duplicates_no_effect, cmp_03_prop_no_invalid_path_to_promoted |
| Integration (temp repo, real worktrees, fake deterministic commits, fake judge winner/abstain/malformed) | cmp_04, cmp_08, pro_03, pro_04 |
| Crash | cmp_13_crash_every_boundary |
| Golden | mea_11, exp_04 |
| E2E | pro_02_e2e_only_valid_candidate_promotable |

---

## 4. Key types

Kernel types (`ExecutionId`, `Execution`, `SpawnRequest`, `RoutingProvenance`,
`EligibleEntry`, `HarnessKind`) are in the architecture design §4. The
signatures below are the target shape; field names and serde spellings are
binding.

### 4.1 Events (`measure/event.rs`, B1)

```rust
pub const EVENT_SCHEMA_VERSION: &str = "1.0.0";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub schema_version: String,            // "1.0.0"
    pub event_id: EventId,                 // v7
    pub kind: String,                      // "candidate.spawned", … (dotted name of EventKind)
    pub occurred_at: String,               // RFC 3339, millisecond precision, UTC ("…T12:00:00.123Z")
    pub actor: Actor,
    pub experiment_id: ExperimentId,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub round_id: Option<RoundId>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub execution_id: Option<ExecutionId>,
    pub idempotency_key: String,
    pub payload: serde_json::Value,        // typed through EventKind::from_envelope
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Actor { Coordinator, Worker, JudgeJob, Operator }

/// The complete event list (27 kinds). `EventKind::KNOWN` holds the dotted
/// names in this order; `mea_02_envelope_roundtrip_every_kind` pins it.
#[derive(Debug, Clone, PartialEq)]
pub enum EventKind {
    ExperimentCreated(ExperimentCreated),       // "experiment.created"
    PreflightCompleted(PreflightCompleted),     // "preflight.completed"
    ExperimentAborted(ExperimentAborted),       // "experiment.aborted"
    RoundCreated(RoundCreated),                 // "round.created"
    CandidatePlanned(CandidatePlanned),         // "candidate.planned"
    WorktreeCreated(WorktreeCreated),           // "worktree.created"
    CandidateSpawned(CandidateSpawned),         // "candidate.spawned"
    CandidateCompleted(CandidateCompleted),     // "candidate.completed"
    CandidateFailed(CandidateFailed),           // "candidate.failed"
    CandidateFrozen(CandidateFrozen),           // "candidate.frozen"
    ValidationCompleted(ValidationCompleted),   // "validation.completed"
    JudgeScheduled(JudgeScheduled),             // "judge.scheduled"
    JudgeStarted(JudgeStarted),                 // "judge.started"
    JudgeCompleted(JudgeCompleted),             // "judge.completed"
    JudgeFailed(JudgeFailed),                   // "judge.failed"
    WinnerSelected(WinnerSelected),             // "winner.selected"
    WinnerRejected(WinnerRejected),             // "winner.rejected"
    PromotionStarted(PromotionStarted),         // "promotion.started"
    PromotionCompleted(PromotionCompleted),     // "promotion.completed"
    PromotionConflicted(PromotionConflicted),   // "promotion.conflicted"
    PromotionRolledBack(PromotionRolledBack),   // "promotion.rolled_back"
    WorktreeCleanupFailed(WorktreeCleanupFailed), // "worktree.cleanup_failed"
    OutcomeRecorded(OutcomeRecorded),           // "outcome.recorded"
    RoundNeedsIntervention(RoundNeedsIntervention), // "round.needs_intervention"
    RoundCleanupStarted(RoundCleanupStarted),   // "round.cleanup_started"
    RoundCompleted(RoundCompleted),             // "round.completed"
    OperatorPromote(OperatorPromote),           // "operator.promote"
    /// An unknown kind, kept byte-for-byte so a newer writer's events survive
    /// a rebuild by an older reader (MEA-02).
    Unknown { kind: String, payload: serde_json::Value },
}
impl EventKind {
    pub fn name(&self) -> &str;                                    // dotted name
    pub fn payload(&self) -> serde_json::Value;
    pub fn from_parts(kind: &str, payload: &serde_json::Value) -> Result<EventKind, EventError>; // unknown → Unknown
}

// Payloads. No payload sets `deny_unknown_fields`, so payloads stay forward-compatible.
pub struct ExperimentCreated { pub task_id: TaskId, pub task_digest: Digest, pub config_digest: Digest,
                               pub base_sha: String, pub repo_digest: Digest, pub environment_digest: Digest,
                               pub candidates: u32, pub strategy: String, pub budget_usd_micro: i64,
                               pub promote_to: Option<String> }
pub struct PreflightCompleted { pub report: PreflightReport }
pub struct ExperimentAborted { pub reason: String, pub failed_checks: Vec<String> }  // "PRE-02", …
pub struct RoundCreated { pub index: u32, pub base_sha: String, pub labels: Vec<String>,
                          pub eligible_set: Vec<EligibleEntry>, pub propensities: BTreeMap<String, f64>,
                          pub teacher: TeacherRef, pub seed: u64, pub label_policy_version: String }
pub struct CandidatePlanned { pub label: String, pub teammate: TeammateName, pub harness: HarnessKind,
                              pub model: ModelId, pub effort: Option<String>, pub slot: SlotKind,
                              pub propensity: f64, pub config_id: String }
pub enum SlotKind { Baseline, Diversity, Exploration }
pub struct WorktreeCreated { pub label: String, pub path: PathBuf, pub branch: String, pub base_sha: String }
pub struct CandidateSpawned { pub label: String, pub pane: PaneId, pub routing: RoutingProvenance }
pub struct CandidateCompleted { pub label: String, pub exit_code: Option<i32> }
pub struct CandidateFailed { pub label: String, pub failure: FailureKind }
pub struct CandidateFrozen { pub label: String, pub head_sha: String, pub numstat: Vec<NumstatLine>, pub diff_digest: Digest }
pub struct ValidationCompleted { pub label: String, pub report: ValidationReport }
pub struct JudgeScheduled { pub attempt: u32, pub input_digest: Digest, pub judge_policy_digest: Digest, pub job_dir: PathBuf }
pub struct JudgeStarted { pub attempt: u32, pub pid: u32 }
pub struct JudgeCompleted { pub attempt: u32, pub judgment_id: JudgmentId, pub output_digest: Digest }
pub struct JudgeFailed { pub attempt: u32, pub cause: JudgeFailure }
pub enum JudgeFailure { Crashed { code: Option<i32> }, TimedOut, Lost, Malformed { error: String }, OverCap } // #[serde(tag = "kind")]
pub struct WinnerSelected { pub label: String, pub execution_id: ExecutionId, pub head_sha: String,
                            pub judgment_id: JudgmentId, pub promotion: PromotionIntent }
pub enum PromotionIntent { NotRequested, Requested { target: String } }   // "not_requested" | {"requested":…}
pub struct WinnerRejected { pub reason: RejectReason }
pub enum RejectReason { NoEligible, JudgeRejected, BelowConfidence, Tie, StaleJudgment, RevalidationFailed }
pub struct PromotionStarted { pub target: String, pub dest_before: String, pub planned_after: String, pub strategy: PromotionStrategy,
                              #[serde(default)] pub publish: String,            // "update_ref_cas" | "update_ref_cas_read_tree"
                              #[serde(default)] pub validation_ids: Vec<String> } // the revalidation of planned_after
pub struct PromotionCompleted { pub receipt_digest: Digest, pub dest_after: String }
pub struct PromotionConflicted { pub paths: Vec<String> }
pub struct PromotionRolledBack { pub target: String, pub restored: String }
pub struct WorktreeCleanupFailed { pub label: String, pub path: PathBuf, pub error: String }
pub struct OutcomeRecorded { pub kind: OutcomeKind, pub post_merge_score: f64, pub note: Option<String> }
pub enum OutcomeKind { Regression, Revert, Verified }
pub struct RoundNeedsIntervention { pub reason: String, pub source: InterventionSource }
pub enum InterventionSource { Judge, Promotion, Operator }
pub struct RoundCleanupStarted {}                     // worktrees go, branches stay
pub struct RoundCompleted { pub final_outcome: FinalOutcome }
pub enum FinalOutcome { Winner, Rejected, NeedsIntervention, Promoted } // the state cleanup started from
pub struct OperatorPromote { pub target: String }
```

`PromotionStarted.publish` names the publish step; `merge_ff_only` is its
legacy spelling in events written before D17, and an event written before B5
has it empty. `RoundCompleted.final_outcome` must match the state the round
held at `round.cleanup_started` (DECIDED → `winner`, REJECTED → `rejected`,
PROMOTED → `promoted`, NEEDS_INTERVENTION → `needs_intervention`); the fold
records any other value as an anomaly.

**Who writes each event, and its idempotency key.** Every event is written
by the coordinator process except where the actor column says otherwise. The
key makes a re-run append a no-op (MEA-04). `<from>` is the round state when
cleanup started; `<n>` is the judge or promotion attempt.

| Kind | Actor | Written by (`path:symbol`) | Idempotency key |
|---|---|---|---|
| `experiment.created` | coordinator | `horch/src/dataset/preflight.rs:record` | `experiment.created:<exp>` |
| `preflight.completed` | coordinator | `horch/src/dataset/preflight.rs:record` | `preflight.completed:<exp>` |
| `experiment.aborted` | coordinator | `horch/src/dataset/preflight.rs:record` | `experiment.aborted:<exp>` |
| `round.created` | coordinator | `competition/coordinator.rs` | `round.created:<round>` |
| `candidate.planned` | coordinator | `competition/coordinator.rs` | `plan:<round>:<label>` |
| `worktree.created` | coordinator | `competition/coordinator.rs` | `worktree:<round>:<label>` |
| `candidate.spawned` | coordinator | `competition/coordinator.rs` | `spawn:<round>:<label>` |
| `candidate.completed`, `candidate.failed` | coordinator | `competition/coordinator.rs` | `ended:<round>:<label>` |
| `candidate.frozen` | coordinator | `competition/coordinator.rs` | `freeze:<round>:<label>` |
| `validation.completed` | coordinator | `competition/coordinator.rs` | `validation:<round>:<label>` |
| `judge.scheduled`, `judge.started`, `judge.completed`, `judge.failed` | coordinator | `competition/judging.rs` | `<kind>:<round>:<n>` |
| `winner.selected` | coordinator | `competition/judging.rs` | `winner:<round>` |
| `winner.rejected` | coordinator | `judging.rs` (judge verdict), `coordinator.rs` (0 eligible), `promotion.rs` (stale or failed revalidation) | `winner:<round>`, `winner.rejected:<round>`, `promotion.rejected:<round>:<n>` |
| `promotion.started`, `promotion.conflicted` | coordinator | `competition/promotion.rs` | `<kind>:<round>:<n>` |
| `promotion.completed` | coordinator | `competition/promotion.rs` | `promotion.completed:<round>` |
| `promotion.rolled_back` | operator | `competition/promotion.rs` (`rollback`) | `promotion.rolled_back:<round>` |
| `worktree.cleanup_failed` | coordinator | `competition/cleanup.rs` | `worktree.cleanup_failed:<round>:<from>:<label>` |
| `outcome.recorded` | operator | `dataset/outcome.rs` | `outcome:<round>:<kind>:<occurred_at>` |
| `round.needs_intervention` | coordinator | `judging.rs` (source `judge`), `promotion.rs` (source `promotion`), `coordinator.rs` | `needs_intervention:<round>`, `round.needs_intervention:promotion:<round>:<n>` |
| `round.cleanup_started` | coordinator | `competition/cleanup.rs` | `round.cleanup_started:<round>:<from>` |
| `round.completed` | coordinator | `competition/cleanup.rs` | `round.completed:<round>:<from>` |
| `operator.promote` | operator | `horch/src/dataset/promote.rs` | `operator.promote:<round>:<n>` |

The last 4 kinds are not in the master plan. They exist so a rebuild from
the log alone reaches every state of §5: without `round.needs_intervention`
a judge tie or a dirty target leaves no event that moves the round, and
without `round.cleanup_started` / `round.completed` a rebuild never reaches
CLEANUP or COMPLETE. `operator.promote` records the operator's re-entry at
DECIDED. Tests: `mea_02_envelope_roundtrip_every_kind` (every kind
round-trips, and `EventKind::KNOWN` equals this list),
`mea_02_unknown_kind_preserved`, `mea_02_known_kind_bad_payload_is_an_error`,
`mea_05_round_needs_intervention_and_completion_checks`,
`mea_05_full_lifecycles_fold_without_anomalies` (all in
`crates/horch-core/tests/measure.rs`).

### 4.2 Recorder and store (B1)

```rust
pub struct NewEvent {
    pub kind: EventKind,
    pub actor: Actor,
    pub experiment_id: ExperimentId,
    pub round_id: Option<RoundId>,
    pub execution_id: Option<ExecutionId>,
    pub idempotency_key: String,          // e.g. "spawn:<round>:<label>", "freeze:<round>:<label>"
    pub occurred_at: DateTime<Utc>,
}
#[derive(Debug, Clone, PartialEq)]
pub enum Appended {
    Recorded(EventEnvelope),
    Duplicate(EventEnvelope),             // the earlier envelope with the same key
}
pub trait Recorder {
    fn append(&self, event: NewEvent) -> anyhow::Result<Appended>;
}
pub struct NoopRecorder;                   // normal horch mode
impl Recorder for NoopRecorder { /* returns Recorded without writing */ }
pub struct JsonlRecorder { /* DatasetPaths, DirLock events.lock, idempotency index */ }
impl JsonlRecorder {
    pub fn open(paths: &DatasetPaths, faults: &Faults) -> anyhow::Result<JsonlRecorder>; // rebuilds the index
    pub fn read_all(&self) -> anyhow::Result<ReadEvents>;   // torn lines skipped and counted
}
impl Recorder for JsonlRecorder { /* … */ }
pub struct ReadEvents { pub events: Vec<EventEnvelope>, pub torn_lines: u32 }

// projection.rs
pub struct Projection { pub experiments: BTreeMap<ExperimentId, ExperimentView>,
                        pub rounds: BTreeMap<RoundId, RoundView>, pub anomalies: Vec<Anomaly> }
pub struct Anomaly { pub event_id: EventId, pub reason: String }   // invalid transition, never applied
/// Deterministic: same events in the same order → the same Projection.
pub fn fold(events: &[EventEnvelope]) -> Projection;
```

### 4.3 WorkerRun 1.0.0 (`measure/worker_run.rs`, MEA-06)

A projection of one `Execution` plus measure events. `worker_run_id ≡
execution_id`. Facts and scores are separate objects. There is no winner
field: the winner is a property of the round.

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerRun {
    pub schema_version: String,             // "1.0.0"
    pub worker_run_id: ExecutionId,
    pub experiment_id: ExperimentId,
    pub round_id: RoundId,
    pub label: String,
    pub task_id: TaskId,
    pub task_digest: Digest,
    pub config: RunConfig,
    pub facts: RunFacts,
    pub scores: RunScores,
}
pub struct RunConfig {
    pub config_id: String,                  // "<teammate>|<harness>|<model>|<effort>"
    pub teammate: TeammateName, pub harness: HarnessKind, pub model: ModelId, pub effort: Option<String>,
    pub slot: SlotKind, pub propensity: f64,
    pub skills: Vec<ResolvedSkillRef>,
    pub routing: RoutingProvenance,
}
pub struct RunFacts {
    pub status: ExecutionStatus,
    pub base_sha: String,
    pub head_sha: Option<String>,
    pub numstat: Vec<NumstatLine>,
    pub diff_digest: Option<Digest>,
    pub session_id: Option<SessionId>,
    pub transcript_ref: Option<String>,     // path, never a copy (SEC-03)
    pub transcript_digest: Option<Digest>,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub latency_ms: Option<u64>,
    pub tokens: Tokens,                     // usage::Tokens
    pub cost_microusd: MicroUsd,
    pub cost_source: CostSource,
    pub environment_digest: Digest,
}
pub struct RunScores {
    pub gates: Vec<GateResult>,
    pub mechanical_score: Option<f64>,      // passed gates / total gates
    pub judge_components: Option<BTreeMap<String, f64>>, // from the Judgment, per candidate label
}
pub struct NumstatLine { pub added: Option<u32>, pub removed: Option<u32>, pub path: String } // None = binary
```

This is the complete WorkerRun 1.0.0 field list. Every struct sets
`deny_unknown_fields`, so a reader refuses an extra field such as `winner`.
`WorkerRun::project` (`measure/worker_run.rs`) builds it; each field comes
from one source:

| Field | Source |
|---|---|
| `schema_version` | the constant `"1.0.0"` (`WORKER_RUN_SCHEMA_VERSION`) |
| `worker_run_id` | `ExecutionFacts.execution_id`; it matches the `execution_id` of the candidate's `candidate.spawned` envelope |
| `experiment_id`, `round_id`, `label` | the round that holds the candidate |
| `task_id`, `task_digest` | `experiment.created` |
| `config.config_id`, `teammate`, `harness`, `model`, `effort`, `slot`, `propensity` | `candidate.planned` |
| `config.skills` | the execution record (`ExecutionFacts.skills`) |
| `config.routing` | `candidate.spawned.routing`; the execution record when no spawn event exists |
| `facts.status`, `session_id`, `started_at`, `finished_at` | the execution record |
| `facts.base_sha` | `worktree.created.base_sha`; `round.created.base_sha` when no worktree exists |
| `facts.head_sha`, `numstat`, `diff_digest` | `candidate.frozen`; `null` / `[]` when not frozen |
| `facts.transcript_ref`, `transcript_digest`, `tokens`, `cost_microusd`, `cost_source` | the usage record the coordinator's meter wrote at freeze; no record → zero tokens, `unpriced` |
| `facts.latency_ms` | `finished_at − started_at` in ms; `null` when either is missing or the clock went backwards |
| `facts.environment_digest` | `experiment.created` |
| `scores.gates`, `mechanical_score` | `validation.completed`; `[]` / `null` when not validated |
| `scores.judge_components` | the judgment's component scores for this label; `null` without a judgment |

The CLI builds the execution part from the ledger record and the usage
record (`horch/src/dataset/export.rs:facts_of`). Tests:
`mea_06_worker_run_schema` (the exact key set of each object),
`mea_06_no_winner_field`, and `mea_11_fake_lifecycle_replays_identical_worker_run`
(all in `crates/horch-core/tests/measure.rs`). The golden
`crates/horch-core/tests/golden/worker-run-1.0.0.json` freezes this shape;
it is never re-blessed, and a change to the shape bumps `schema_version`.

### 4.4 Money (`usage/money.rs`, B1, MEA-07)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(transparent)]
pub struct MicroUsd(pub i64);               // stored
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct NanoUsd(pub i128);               // accumulated
impl NanoUsd {
    pub fn add_tokens(&mut self, tokens: u64, nano_per_token: i128);
    pub fn to_micro_half_even(self) -> MicroUsd;   // rounds once
}
/// $/MTok × 1000 = n$/token. Errors when not an integer within 1e-9.
pub fn nano_per_token(dollars_per_mtok: f64) -> Result<i128, MoneyError>;
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CostSource { PriceTable { date: String }, PricingFile { sha12: String }, HarnessReported, Unpriced }
// Display and serde: "price_table@<date>" | "pricing_file@<sha12>" | "harness_reported" | "unpriced"
pub enum MoneyError { InexactPrice { dollars_per_mtok: f64 }, Overflow }
```

### 4.5 VCS (`vcs/git.rs` B2, `vcs/worktree.rs` B3)

```rust
pub enum CherryPick { Clean { head: String }, Conflict { paths: Vec<String> } }
pub enum CheckoutLocation { NotCheckedOut, CheckedOut { path: PathBuf, clean: bool } }
pub struct GitIdentity { pub name: String, pub email: String, pub date: String } // deterministic

pub trait GitClient {
    fn toplevel(&self, dir: &Path) -> anyhow::Result<PathBuf>;
    fn head(&self, dir: &Path) -> anyhow::Result<String>;
    fn current_branch(&self, dir: &Path) -> anyhow::Result<Option<String>>;
    fn status_porcelain(&self, dir: &Path) -> anyhow::Result<String>;
    fn version(&self) -> anyhow::Result<String>;
    fn worktree_add(&self, repo: &Path, path: &Path, branch: &str, base: &str) -> anyhow::Result<()>;
    fn worktree_remove(&self, repo: &Path, path: &Path, force: bool) -> anyhow::Result<()>;
    fn worktree_list(&self, repo: &Path) -> anyhow::Result<Vec<(PathBuf, Option<String>)>>;
    /// Hooks off, no gpg, deterministic identity and date.
    fn commit_all(&self, dir: &Path, message: &str, id: &GitIdentity) -> anyhow::Result<Option<String>>;
    fn rev_parse(&self, dir: &Path, rev: &str) -> anyhow::Result<Option<String>>;
    fn rev_list(&self, dir: &Path, range: &str) -> anyhow::Result<Vec<String>>;
    fn diff_numstat(&self, dir: &Path, base: &str, head: &str) -> anyhow::Result<Vec<NumstatLine>>;
    fn diff_patch(&self, dir: &Path, base: &str, head: &str, cap_bytes: usize) -> anyhow::Result<(String, bool)>; // (patch, truncated)
    fn is_ancestor(&self, dir: &Path, a: &str, b: &str) -> anyhow::Result<bool>;
    /// `update-ref <ref> <new> <expected_old>`: compare and swap.
    fn update_ref_cas(&self, repo: &Path, refname: &str, new: &str, expected_old: &str) -> anyhow::Result<bool>;
    fn cherry_pick(&self, dir: &Path, range: &str, id: &GitIdentity) -> anyhow::Result<CherryPick>;
    fn merge_ff_only(&self, dir: &Path, rev: &str) -> anyhow::Result<()>;
    fn branch_checkout_location(&self, repo: &Path, branch: &str) -> anyhow::Result<CheckoutLocation>;
}
/// Wraps horch_marketplace::git::GitRunner. GIT_TERMINAL_PROMPT=0, LC_ALL=C,
/// FORBIDDEN_ENV stripped. Bin from RuntimeContext (HORCH_GIT_BIN).
pub struct GitCli { runner: horch_marketplace::git::GitRunner }
impl GitClient for GitCli { /* … */ }

pub struct WorktreeSpec { pub repo: PathBuf, pub root: PathBuf, pub exp8: String, pub round_index: u32,
                          pub label: String, pub base_sha: String }
// branch: "mh/exp/<exp8>/r<idx>/<label>"; path: <root>/<label>
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrozenCandidate {
    pub label: String,
    pub execution_id: ExecutionId,
    pub worktree: PathBuf,
    pub branch: String,
    pub base_sha: String,
    pub head_sha: String,
    pub numstat: Vec<NumstatLine>,
    pub diff_digest: Digest,
    pub frozen_at: String,
}
pub struct WorktreeManager<'a, G: GitClient> { pub git: &'a G }
impl<'a, G: GitClient> WorktreeManager<'a, G> {
    pub fn create(&self, spec: &WorktreeSpec) -> anyhow::Result<PathBuf>;       // idempotent
    pub fn freeze(&self, spec: &WorktreeSpec, execution: &ExecutionId, at: DateTime<Utc>) -> anyhow::Result<FrozenCandidate>;
    pub fn remove(&self, spec: &WorktreeSpec) -> anyhow::Result<()>;             // keeps the branch
}
```

### 4.6 Validation (`evaluation/validator.rs`, B3)

```rust
pub struct GateSpec { pub name: String, pub command: String, pub timeout: Duration } // from config only (SEC-07)
pub enum GateStatus { Passed, Failed { code: i32 }, TimedOut, Error { reason: String } }
pub struct GateResult { pub name: String, pub status: GateStatus, pub duration_ms: u64,
                        pub log_ref: PathBuf, pub log_digest: Digest, pub log_truncated: bool }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationReport {
    pub validation_id: String,              // v7
    pub label: String,
    pub head_sha: String,
    pub gates: Vec<GateResult>,
    pub mechanical_score: f64,              // passed / total; 0.0 when no gates
    pub eligible: bool,                     // every required gate passed
}
pub trait Validator {
    fn validate(&self, candidate: &FrozenCandidate) -> anyhow::Result<ValidationReport>;
}
/// `sh -c <gate>` per gate, per-gate timeout, worktree-local CARGO_TARGET_DIR,
/// FORBIDDEN_ENV stripped, logs redacted and capped at 256 KiB.
pub struct CommandValidator { pub gates: Vec<GateSpec>, pub artifacts: PathBuf, pub faults: Faults }
impl Validator for CommandValidator { /* … */ }
```

### 4.7 Judge (`evaluation/{judge_input,rubric,judgment,parser,winner,scheduler}.rs`, B4)

```rust
pub struct JudgeInput {                     // immutable bundle, files 0400
    pub dir: PathBuf,                       // artifacts/<round>/judge-input/
    pub labels: Vec<String>,                // seeded shuffle of round_id
    pub manifest: JudgeInputManifest,
    pub digest: Digest,                     // over manifest.json
    pub blindness: Vec<BlindnessFlag>,      // vendor tokens found in diffs (JDG-10)
}
pub struct JudgeInputManifest {
    pub schema_version: String,             // "1.0.0"
    pub round_id: RoundId,
    pub rubric_version: String,             // "rubric-1"
    pub schema_digest: Digest,
    pub files: BTreeMap<String, Digest>,    // relative path → digest
}
pub struct BlindnessFlag { pub label: String, pub token: String, pub file: String }
/// Model, vendor, cost, latency and history are omitted. The label →
/// execution map stays in round state and never enters the bundle.
pub fn build_judge_input(round: &RoundView, task_text: &str, candidates: &[(String, FrozenCandidate, ValidationReport)],
                         paths: &DatasetPaths, git: &dyn GitClient) -> anyhow::Result<JudgeInput>;

/// sha256(judge.md ‖ rubric ‖ schema ‖ model ‖ effort ‖ WinnerPolicy ‖ label_policy_version)
pub fn judge_policy_digest(judge_md: &str, rubric: &str, schema: &str, model: &str, effort: &str,
                           policy: &WinnerPolicy, label_policy_version: &str) -> Digest;

/// SPEC-TODO(Spec B §11): the Judgment schema verbatim. This shape is provisional.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Judgment {
    pub schema_version: String,             // "1.0.0"
    pub verdict: JudgmentVerdict,
    pub winner: Option<String>,             // a label; required when verdict = winner
    pub ranking: Vec<String>,               // every label exactly once
    pub candidates: BTreeMap<String, CandidateAssessment>,
    pub confidence: f64,                    // finite, 0.0..=1.0
    pub rationale: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JudgmentVerdict { Winner, Tie, Abstain, RejectAll }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateAssessment {
    pub scores: BTreeMap<String, f64>,      // rubric component → score; kept, never collapsed (JDG-06)
    pub acceptable: bool,
    pub notes: String,
}
/// The record the coordinator writes to judgements/<round>.json.
pub struct JudgmentRecord { pub judgment_id: JudgmentId, pub round_id: RoundId, pub attempt: u32,
                            pub input_digest: Digest, pub judge_policy_digest: Digest,
                            pub execution_id: ExecutionId, pub judgment: Judgment }

pub enum ParseError { NotJson(String), UnknownField(String), UnknownEnum(String), MissingField(String),
                      DuplicateKey(String), ImpossibleLabel(String), NonFinite(String), TooLarge(usize) }
/// Strict. Never repairs (no fence stripping, no trailing-comma fix).
pub fn parse_judgment(raw: &[u8], labels: &[String], cap_bytes: usize) -> Result<Judgment, ParseError>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WinnerPolicy {
    pub min_confidence: f64,                // default 0.7
    pub tie_break: TieBreak,                // default Disabled
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum TieBreak { Disabled, Utility { v: String } }   // SPEC-TODO(Spec B §11): the utility definition
#[derive(Debug, Clone, PartialEq)]
pub enum WinnerOutcome {
    Winner { label: String },
    NeedsIntervention { reason: String },
    Rejected { reason: RejectReason },
}
/// Pure.
pub fn decide_winner(j: Option<&Judgment>, eligible: &BTreeSet<String>, policy: &WinnerPolicy) -> WinnerOutcome;
```

`decide_winner` table (JDG-06, JDG-07):

| Input | Outcome |
|---|---|
| `eligible` empty (judge skipped) | Rejected{NoEligible} |
| no valid judgment after 2 attempts | NeedsIntervention |
| verdict RejectAll | Rejected{JudgeRejected} |
| verdict Abstain | NeedsIntervention |
| verdict Tie, `tie_break: Disabled` | NeedsIntervention |
| verdict Tie, `tie_break: Utility` | Winner by utility among the tied labels |
| verdict Winner, label not in `eligible` | Rejected{JudgeRejected} |
| verdict Winner, `confidence < min_confidence` | NeedsIntervention |
| verdict Winner, eligible, confident | Winner |

`SPEC-TODO(Spec B §11)`: confirm the Abstain and RejectAll mappings.

Scheduler:

```rust
pub struct JudgeJobSpec { pub round: RoundId, pub attempt: u32, pub input_dir: PathBuf, pub job_dir: PathBuf,
                          pub model: String, pub effort: String, pub timeout: Duration }
pub enum JobState { OutputPresent(PathBuf), Running { pid: u32 }, Lost, Exited { code: i32 } }
/// spawn_detached(multi-herdr-dataset judge-job …). Retry: 2 attempts, then NEEDS_INTERVENTION.
pub fn schedule(ctx: &RuntimeContext, spec: &JudgeJobSpec) -> anyhow::Result<u32>;
/// output present → parse; heartbeat fresh and pid alive → wait; else Lost.
pub fn discover(job_dir: &Path, now: DateTime<Utc>, stale_after: Duration) -> JobState;
// harness/headless.rs
/// claude -p --output-format json --session-id <minted>, prompt on stdin, FORBIDDEN_ENV stripped;
/// --json-schema only if `claude --help` lists it.
pub fn headless_command(ctx: &RuntimeContext, teammate: &Teammate, session: &SessionId, schema: Option<&Path>) -> std::process::Command;
```

The job writes only `jobs/<round>/judge-<n>/{heartbeat, output.json ≤ 1 MiB,
exit.json}`. The coordinator emits every `judge.*` event, writes
`judgements/<round>.json` via `create_immutable`, then emits
`winner.selected` or `winner.rejected`.

### 4.8 Promotion (`competition/promotion.rs`, B5)

```rust
pub struct BranchRef { pub repo: PathBuf, pub name: String }        // "refs/heads/<name>"
pub enum PromotionStrategy { FastForward, CherryPick }               // serde snake_case
pub enum PublishMode { UpdateRefCas, MergeFfOnly { checkout: PathBuf } }
pub struct PromotionPlan {
    pub round: RoundId, pub judgment_id: JudgmentId,
    pub gates: Vec<GateSpec>,                // revalidation gates
    pub identity: GitIdentity,
    pub integration_root: PathBuf,           // temp integration worktree parent
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PromotionReceipt {
    pub schema_version: String,              // "1.0.0"
    pub round_id: RoundId,
    pub label: String,
    pub source_shas: Vec<String>,            // base..head
    pub dest_ref: String,
    pub dest_before: String,
    pub dest_after: String,
    pub strategy: PromotionStrategy,
    pub publish: String,                     // "update_ref_cas" | "merge_ff_only"
    pub validation_ids: Vec<String>,
    pub judgment_id: JudgmentId,
    pub promoted_at: String,
}
pub enum PromotionResult { Promoted(PromotionReceipt), NeedsIntervention { reason: String }, Rejected { reason: RejectReason } }
pub trait PromotionEngine {
    fn promote(&self, candidate: &FrozenCandidate, target: &BranchRef, plan: &PromotionPlan) -> anyhow::Result<PromotionResult>;
}
pub struct GitPromotionEngine<'a, G: GitClient, V: Validator, R: Recorder> { pub git: &'a G, pub validator: &'a V, pub recorder: &'a R, pub paths: &'a DatasetPaths, pub faults: &'a Faults }
```

### 4.9 Teacher (`teacher/`, merged in B6 unit U08)

```rust
pub const API: &str = "systemone/v1";
pub struct DecisionRequest { pub api: String, pub model: String, pub state: serde_json::Value,
                             pub questions: BTreeMap<String, Question> }
pub struct Question { #[serde(rename = "type")] pub kind: QuestionKind, pub instructions: String,
                      pub options: Vec<String>,               // skip if empty
                      pub criteria: Option<serde_json::Value> } // SPEC-TODO(System One criteria semantics)
pub enum QuestionKind { Choice, Score, Noul }                  // serde lowercase
pub struct DecisionResponse { pub answers: BTreeMap<String, Answer>, pub usage: Option<serde_json::Value> }
pub struct Answer { pub choice: Option<String>, pub probabilities: BTreeMap<String, f64>, pub confidence: Option<f64> }
pub trait DecisionModel {
    fn id(&self) -> &str;
    fn decide(&self, req: &DecisionRequest) -> Option<DecisionResponse>;
}
pub struct TeacherRef { pub id: String, pub probabilities: Option<BTreeMap<String, f64>> }
impl TeacherRef { pub fn none() -> TeacherRef; }               // {"id":"none","probabilities":null}
pub struct Inert;                                              // decide → None, id → "none"
```

### 4.10 Export row (`dataset/export.rs`, B6)

```rust
pub const EXPORT_SCHEMA: &str = "mh.export/1.0.0";
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportRow {
    pub schema: String,                       // "mh.export/1.0.0"
    pub api: String,                          // "systemone/v1"
    pub label_policy_version: String,
    pub experiment_id: ExperimentId,
    pub round_id: RoundId,
    pub task_id: TaskId,
    pub state: ExportState,
    pub questions: BTreeMap<String, Question>,  // "best_worker": choice over eligible config ids;
                                                // "quality:<config_id>": score per config
    pub answers: BTreeMap<String, Option<Answer>>, // empirical winner; null on abstain/tie
    pub eligible_set: Vec<EligibleEntry>,
    pub planner_propensities: BTreeMap<String, f64>,
    pub teacher: TeacherRef,                  // {"id":"none","probabilities":null}
    pub worker_runs: Vec<WorkerRun>,
    pub winner: Option<String>,               // config id
    pub component_quality: BTreeMap<String, BTreeMap<String, f64>>,
    pub cost_microusd: MicroUsd,
    pub latency_ms: u64,
    pub outcomes: Vec<OutcomeRecorded>,
}
pub struct ExportState { pub task_features: BTreeMap<String, serde_json::Value>,
                         pub task_digest: Digest, pub repo_digest: Digest,
                         pub environment_digest: Digest, pub base_sha: String }
/// Only from events, judgments, receipts, the execution store and outcomes.
/// Sorted by (experiment_id, round_id). Byte-identical on regeneration.
pub fn export(paths: &DatasetPaths, store: &ExecutionStore, label_policy_version: &str) -> anyhow::Result<Vec<ExportRow>>;
```

Readiness (`dataset/readiness.rs`):

```rust
pub struct ArmKey { pub teammate: TeammateName, pub harness: HarnessKind, pub model: ModelId, pub effort: Option<String> }
pub struct ArmCoverage { pub runs: u32, pub terminal_runs: u32, pub judged_rounds: u32, pub wins: u32,
                         pub gate_pass_rate: f64, pub distinct_tasks: u32 }
pub struct ReadinessThresholds {             // dataset-policy.json
    pub clef_judged_rounds: u32,             // 50
    pub clef_min_arms: u32,                  // 4
    pub clef_runs_per_arm: u32,              // 10
    pub clef_distinct_tasks: u32,            // 30
    pub laya_judged_rounds: u32,             // 500
    pub laya_runs_per_arm: u32,              // 50
}
pub enum ReadinessVerdict { NotReady, ClefReady, LayaReady }
pub struct ReadinessReport { pub arms: BTreeMap<String, ArmCoverage>, pub judged_rounds: u32,
                             pub distinct_tasks: u32, pub verdict: ReadinessVerdict, pub gaps: Vec<String> }
pub fn readiness(rows: &[ExportRow], t: &ReadinessThresholds) -> ReadinessReport;
```

### 4.11 Machine, preflight, config (B2)

```rust
// runtime/machine.rs (merged)
pub enum Known<T> { Known(T), Unknown }      // serde: value or null
pub enum GpuClass { AppleSilicon, Nvidia { count: u32 }, None, Unknown }
#[serde(deny_unknown_fields)]
pub struct MachineSnapshot {
    pub os: String, pub arch: String, pub cpus: Known<u32>,
    pub mem_total_bytes: Known<u64>, pub mem_available_bytes: Known<u64>,
    pub disk_free_bytes: Known<u64>, pub disk_total_bytes: Known<u64>,
    pub gpu: GpuClass, pub max_open_files: Known<u64>, pub max_processes: Known<u64>,
}
pub struct ProbeBins { pub sysctl: PathBuf, pub vm_stat: PathBuf, pub uname: PathBuf, pub nvidia_smi: PathBuf }
pub fn probe(path: &Path, bins: &ProbeBins, fixture: Option<&Path>) -> MachineSnapshot;

// competition/config.rs — CLI flags + .multi-herdr/dataset.yaml
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatasetConfig {
    pub candidates: u32,
    pub strategy: Strategy,                   // Diverse
    pub budget: BudgetConfig,
    pub judge: JudgeConfig,
    #[serde(default)] pub baseline: Option<TeammateName>,
    #[serde(default)] pub gates: Vec<GateConfig>,
    #[serde(default)] pub caps: Caps,
    #[serde(default)] pub exclude: Vec<TeammateName>,
    #[serde(default)] pub promote_to: Option<String>,
    #[serde(default)] pub worktree_root: Option<PathBuf>,
    #[serde(default)] pub allow_dirty: bool,
    #[serde(default)] pub prune_branches: bool,
    #[serde(default)] pub retain_transcripts: bool,  // SEC-03, explicit opt-in
}
pub enum Strategy { Diverse }
pub struct BudgetConfig { pub soft_usd_micro: i64, pub hard_usd_micro: i64, pub judge_reserve_usd_micro: i64 }
pub struct JudgeConfig { pub mode: JudgeMode /* Auto */, pub model: String /* "opus" */, pub effort: String /* "high" */,
                         pub timeout_s: u64, pub policy: WinnerPolicy }
pub struct GateConfig { pub name: String, pub command: String, pub timeout_s: u64, pub required: bool }
pub struct Caps { pub candidate_deadline_s: u64, pub max_parallel: Option<u32>, pub disk_headroom_bytes: u64,
                  pub log_cap_bytes: u64 /* 262144 */, pub output_cap_bytes: u64 /* 1048576 */ }
pub fn load(project: &Path, flags: &RunFlags) -> anyhow::Result<DatasetConfig>;   // flags override the file

// competition/preflight.rs
pub enum CheckStatus { Pass, Warn, Fail }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckResult { pub id: String /* "PRE-01" */, pub status: CheckStatus, pub detail: String,
                         pub measured: serde_json::Value }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PreflightReport {
    pub schema_version: String,               // "1.0.0"
    pub checks: Vec<CheckResult>,             // PRE-01..PRE-13, in order
    pub safe_n: u32,
    pub waves: u32,
    pub projected_cost_microusd: MicroUsd,
    pub machine: MachineSnapshot,
    pub environment_digest: Digest,
    pub passed: bool,                         // no Fail
}
pub struct PreflightPlan { pub config: DatasetConfig, pub candidates: Vec<CandidatePlanned>,
                           pub harness_versions: BTreeMap<HarnessKind, Option<String>>,
                           pub git: GitFacts, pub storage_probe: StorageProbe, pub herdr_reachable: bool,
                           pub horch_exe: Option<PathBuf>, pub quota: QuotaView }
/// Pure.
pub fn evaluate(plan: &PreflightPlan, snapshot: &MachineSnapshot) -> PreflightReport;
```

`SPEC-TODO(Spec B §3)`: the preflight check list and thresholds verbatim.

---

## 5. Round state machine (CMP-03)

`SPEC-TODO(Spec B round states)`: the master plan names only the states from
JUDGING_BACKGROUND on. The earlier states below follow the B2/B3 flow.

States: `CREATED`, `PREFLIGHT`, `ABORTED`, `PLANNED`, `PROVISIONING`,
`RUNNING`, `VALIDATING`, `JUDGING_BACKGROUND`, `DECIDED`, `REVALIDATING`,
`PROMOTING`, `PROMOTED`, `NEEDS_INTERVENTION`, `REJECTED`, `CLEANUP`,
`COMPLETE`.

### 5.1 Default path (no promotion, OD5)

| From | Event / condition | To |
|---|---|---|
| CREATED | `experiment.created` | PREFLIGHT |
| PREFLIGHT | `preflight.completed` (passed) | PLANNED |
| PREFLIGHT | `experiment.aborted` (a check failed; no worktree, no model) | ABORTED |
| PLANNED | `round.created`, every `candidate.planned` | PROVISIONING |
| PROVISIONING | every `worktree.created` | RUNNING |
| RUNNING | `candidate.spawned` per wave; `candidate.completed` / `candidate.failed` per candidate | RUNNING |
| RUNNING | every candidate terminal, or round deadline | VALIDATING |
| VALIDATING | `candidate.frozen` + `validation.completed` per terminal candidate | VALIDATING |
| VALIDATING | every candidate validated; ≥ 1 eligible | JUDGING_BACKGROUND |
| VALIDATING | 0 eligible | REJECTED (`winner.rejected{no_eligible}`, judge skipped) |
| JUDGING_BACKGROUND | `judge.scheduled`, `judge.started`, `judge.failed` (attempt < 2) | JUDGING_BACKGROUND |
| JUDGING_BACKGROUND | `judge.completed` + `winner.selected{promotion: not_requested}` | DECIDED |
| JUDGING_BACKGROUND | `judge.completed` + outcome NeedsIntervention, or `judge.failed` on attempt 2 | NEEDS_INTERVENTION |
| JUDGING_BACKGROUND | `judge.completed` + `winner.rejected` | REJECTED |
| DECIDED | no promotion requested | CLEANUP |
| REJECTED | — | CLEANUP |
| CLEANUP | every worktree removed (failures → `worktree.cleanup_failed`, branches kept) | COMPLETE |
| NEEDS_INTERVENTION | operator: `promote <round>` (only from a DECIDED winner) or `cleanup` | DECIDED or CLEANUP |

### 5.2 `--promote-to` / `promote <round>` path

| From | Event / condition | To |
|---|---|---|
| DECIDED | promotion requested | REVALIDATING |
| REVALIDATING | worktree HEAD or branch ≠ frozen SHA | REJECTED (`winner.rejected{stale_judgment}`) |
| REVALIDATING | integrate (ff or cherry-pick in a temp worktree) conflicts | NEEDS_INTERVENTION (`promotion.conflicted`, worktrees kept) |
| REVALIDATING | gates fail on the integrated commit | REJECTED (`winner.rejected{revalidation_failed}`) |
| REVALIDATING | gates pass; target checked out and dirty | NEEDS_INTERVENTION |
| REVALIDATING | gates pass; `promotion.started{dest_before, planned_after}` | PROMOTING |
| PROMOTING | publish (CAS `update-ref`, or `merge --ff-only` in the clean checkout) → receipt via `create_immutable` → `promotion.completed` | PROMOTED |
| PROMOTING | CAS lost (ref ≠ dest_before) | NEEDS_INTERVENTION |
| PROMOTED | receipt durable | CLEANUP |
| REJECTED | — | CLEANUP |
| CLEANUP | — | COMPLETE |
| COMPLETE | `rollback` (CAS back to dest_before) → `promotion.rolled_back` | COMPLETE |

Every other pair is invalid. `measure::projection::fold` records an invalid
transition as an anomaly and does not apply it. No path reaches PROMOTED
without DECIDED → REVALIDATING → PROMOTING
(`cmp_03_prop_no_invalid_path_to_promoted`). NEEDS_INTERVENTION keeps every
worktree.

Master plan note: the B5 text lists `NEEDS_INTERVENTION → CLEANUP → COMPLETE`
and also "NEEDS_INTERVENTION keeps everything". This design resolves it: from
NEEDS_INTERVENTION, CLEANUP runs only on an operator `cleanup` command.

---

## 6. Phases

### B1 Measurement foundation

| | |
|---|---|
| Created | `fsx.rs` (`write_atomic`, `create_immutable`, `replace_durable`, `ensure_private_dir`, `DirLock`, `FsxError`); `measure/{mod,digest,event,store,recorder,projection,worker_run,paths,redact,testkit}.rs`; `usage/money.rs`; `competition/{mod,model}.rs` (Experiment, Round, Candidate, ValidationRun, Judgment, Promotion, Outcome) |
| Moved | `usage.rs` → `usage/{mod,prices,read}.rs` when `money.rs` is added (shim: `usage/mod.rs` re-exports) |
| Store | `events/YYYY-MM-DD.jsonl` under `DirLock events.lock`; idempotency index rebuilt on open and refreshed under the lock via `telemetry::cursor::poll_lines`; torn lines tolerated |
| Redaction | `ANTHROPIC_API_KEY=`, `sk-ant-`, `sk-`, `ghp_`, `github_pat_`, `xox?-`, `AKIA`, PEM keys, `(api_key\|secret\|token\|password)[:=]` |
| Tests | MEA-01..09, MEA-11, CMP-02, SEC-01, SEC-02, SEC-05, NFR-11 (section 3) |
| Done when | `mea_11_fake_lifecycle_replays_identical_worker_run` matches golden `worker-run-1.0.0.json` |
| Gate | `just gate` |
| Do not change | ledger format; telemetry store format |

### B2 Entrypoint and preflight

| | |
|---|---|
| Created | `crates/horch/src/bin/multi-herdr-dataset.rs`; `crates/horch/src/dataset/{run,preflight,status}.rs`; `vcs/{mod,git}.rs`; `runtime/machine.rs` (merged); `competition/{config,preflight}.rs`; e2e `Harness::with_git()` |
| Command | `run <task> --candidates N --strategy diverse --budget-usd X --judge auto [--baseline <teammate>] [--promote-to <branch>] [--worktree-root <dir>] [--allow-dirty]` |
| Exit codes | 0 ok, 3 budget/quota refusal, 4 preflight failed, 5 needs intervention, 6 rejected |
| Install | `horch install` and `just install` install both binaries |
| Rule | a failing check emits `experiment.aborted` before any worktree or model; the report goes in `preflight.completed` and the manifest with `environment_digest` |
| Tests | PRE-01..13, CMP-01, NFR-10 |
| Gate | `just gate` + pre_12 |
| Do not change | `horch` CLI |

### B3 Competitive worktrees

| | |
|---|---|
| Created | `vcs/worktree.rs`; `competition/{planner,diversity,state,budget,observe,coordinator}.rs`; `evaluation/{mod,validator}.rs`; `teammates/_base/competition-candidate.md`; `crates/horch-e2e/tests/dataset.rs` |
| Kernel | `SpawnRequest.workdir`, `kind: Candidate`, `report_to: None`, `RoutingMode::Pinned`; `horch sessions` hides candidate and judge executions unless `--all`; resume refuses them |
| Planner | slot 1 = baseline (`--baseline` or config), routed through the PR #14 gate; remaining slots maximize distinct (harness, model, effort) over `roster_eligibility`; 1 exploration slot via SplitMix64 seeded from `sha256(round_id)`, propensity 1/k; eligible set, reasons, propensities, `teacher: none` in `round.created` and `candidate.planned` |
| Coordinator | single-threaded tick loop; dedicated workspace `multi-herdr-dataset <exp8>` (no focus, root pane runs `watch`, no `orchestrator` registration, tiling Disabled); waves of `safe_n` through ExecutionService; idempotency key `spawn:<round>:<label>`; observation table below; UsageMeter (telemetry readers → µ$); disk check each tick; freeze → CommandValidator → `validation.completed` + `mechanical_score` |
| Fakes | fake-herdr split/close/state, kill on close, dataset workspaces exempt from violations; fake-claude/codex/opencode candidate mode from `$HORCH_FAKE_LOG.candidates.json` `{write, commit, exit: done\|crash\|hang\|exit0}` |
| Tests | CMP-03..11, CMP-13 (B3 part), CMP-14, CMP-15, MEA-10, SEC-03, SEC-08 (candidates), EXP-02 (recorded part), ARC-24 |
| Gate | `just gate` + cmp_05 |
| Do not change | golden prompts (rules travel in the task text) |

Observation:

| Observation | Outcome |
|---|---|
| Done | completed |
| pane gone while live | Failed(PaneVanished) |
| agent exits | Failed(AgentExited) |
| deadline | close the pane → TimedOut |
| budget | Cancelled |

### B4 Judge

| | |
|---|---|
| Created | `teammates/judge.md` (`hidden: true`, no `base`, `agent: claude`, `model: opus`, `effort: high`, `inherit_plugins: false`, `mcp_servers: {}`, `tools: [Read, Grep, Glob]`, `disallowed_tools: [Agent, Edit, Write, NotebookEdit, Bash]`; body = Spec B §10 evaluator prose verbatim + `{rubric}` + `{schema}`); roster rule `HEADLESS_ONLY`; `harness/headless.rs`; `evaluation/{judge_input,rubric,judgment,parser,winner,scheduler}.rs`; `rubric-1.md`, `judgment-schema-1.0.0.json`; `crates/horch/src/dataset/judge_job.rs` |
| Rules | the job writes only its job dir; the coordinator is the single authority; the judge is a `kind: judge` execution; zero eligible → judge skipped → REJECTED |
| Tests | JDG-01..10, CMP-12, CMP-16, SEC-04, SEC-06, SEC-07, SEC-08 (judge) |
| Gate | `just gate` + jdg e2e |
| Do not change | other teammates; golden prompts |

`SPEC-TODO(Spec B §10)`: the evaluator prose for `teammates/judge.md`.

### B5 Promotion (opt-in, OD5)

| | |
|---|---|
| Created | `competition/{promotion,cleanup}.rs`; `crates/horch/src/dataset/{promote,rollback,cleanup}.rs` |
| Engine | section 4.8 and section 5.2 |
| Restart | ref == planned_after → write the receipt; ref == dest_before → retry; else NEEDS_INTERVENTION |
| Cleanup | only after a durable receipt (or at round end when not promoting); branches kept (`--prune-branches` opt-in); failures → `worktree.cleanup_failed` |
| Tests | PRO-01..08, CMP-13 (crash suite) |
| Gate | `just gate` + pro_02 |
| Do not change | default `run` behavior (collect only) |

### B6 Router telemetry, export, readiness and outcome

| | |
|---|---|
| Created | `teacher/{mod,system_one,inert}.rs` (merged); `dataset/{mod,export,readiness,outcome}.rs`; `crates/horch/src/dataset/{export,readiness,outcome,rebuild,watch}.rs`; `dataset-policy.json` |
| Commands | `export`, `readiness`, `outcome <round> --kind regression\|revert\|verified` (sets `post_merge_score`), `rebuild <exp>` |
| Tests | EXP-01..07 |
| Gate | `just gate` + exp_04; the final audit (section 7.3) |
| Do not change | event formats (a change bumps `schema_version`) |

---

## 7. Crash points and resume invariants

### 7.1 Dataset fault points (`HORCH_FAULT`)

| Point | Where |
|---|---|
| `abort-after-event-append` | recorder, after the fsync of any append |
| `abort-after-experiment-created` | coordinator, after `experiment.created` |
| `abort-after-preflight` | after `preflight.completed` |
| `abort-after-worktree:<n>` | after the n-th `worktree.created` |
| `abort-after-candidate-spawned:<n>` | after the n-th `candidate.spawned` |
| `abort-after-freeze:<L>` | after `candidate.frozen` for label L |
| `abort-after-validation:<L>` | after `validation.completed` for label L |
| `abort-after-judge-scheduled` | after `judge.scheduled` |
| `abort-in-judge-job-before-output` | inside the judge job, before `output.json` |
| `abort-after-judge-output` | inside the judge job, after `output.json` |
| `abort-after-judgment-written` | coordinator, after `judgements/<round>.json` |
| `abort-after-winner-selected` | after `winner.selected` |
| `abort-after-promotion-started` | after `promotion.started` |
| `abort-after-update-ref` | after the publish, before the receipt |
| `abort-after-receipt` | after the receipt, before `promotion.completed` |
| `abort-during-cleanup:<n>` | after the n-th worktree removal |

Kernel and marketplace points are in the architecture design §7.

### 7.2 Resume invariants

After each fault, `multi-herdr-dataset resume` (or `run` re-entry) must give:

1. exactly N executions per round (idempotency key `spawn:<round>:<label>`; an existing execution is adopted);
2. at most 1 judgment (`create_immutable`) and at most 1 receipt;
3. the target ref advanced at most once (restart rule in B5);
4. the projection equals a rebuild from events (`rebuild <exp>`).

`cmp_13_crash_every_boundary` runs every point above. `cmp_13_rebuild_after_power_loss`
truncates the last event line and checks rule 4.

### 7.3 Final audit

After B6, the final audit walks both verbatim specs section by section
against `SPEC-COVERAGE.md`. It must list zero unmapped items.

---

## 8. Local acceptance on the operator's Mac

The hermetic suite cannot prove these. They use real herdr, claude and codex.

| # | Check |
|---|---|
| LA-1 | `just install` installs both binaries; `horch doctor` passes. |
| LA-2 | After A6/A7: herdr-fleet, tile and spawn smokes. |
| LA-3 | A real fleet with sonnet, codex-sol and opencode: session discovery, resume, and `horch cost` totals equal an A0 ledger copy. |
| LA-4 | After A10: only activated skills are visible in Claude `/skills` and in codex's `CODEX_HOME`. |
| LA-5 | `horch skills install MattMakes/skill-marketplace@<tag>` pins a SHA, then spawn works offline. |
| LA-6 | B2 preflight numbers match Activity Monitor. |
| LA-7 | B3 on a scratch repo with sonnet and codex-sol candidates: do the Claude/Codex trust dialogs stall panes in new worktrees? Test `--worktree-root` under a trusted parent. |
| LA-8 | B4 with a real `claude -p` judge: bundle unchanged; `--json-schema` availability; strict-parse rejection rate. |
| LA-9 | B5 `--promote-to` into an unchecked-out branch, a checked-out clean branch, a dirty branch (expect NEEDS_INTERVENTION), and a moved target with a conflict. |
| LA-10 | Export twice gives identical bytes; the readiness report makes sense. |
| LA-11 | `kill -9` the coordinator while running, judging and promoting; `resume` finishes without duplicates. |
| LA-12 | With `ANTHROPIC_API_KEY=SENTINEL` exported, run a full experiment: `grep -r SENTINEL` finds 0 hits in the state dir, and `ps eww` shows the key absent from candidates and the judge. |

---

## Appendix B: Spec B (verbatim)

PENDING: the orchestrator inserts the operator's Spec B text here.
