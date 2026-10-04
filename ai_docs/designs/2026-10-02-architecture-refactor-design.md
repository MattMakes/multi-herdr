# Design: the architecture refactor (Spec A, phases A0 to A12)

| | |
|---|---|
| Status | Implemented. Phases A0, A1, A2, A3, A4, A5, A6, A7, A8, A9, A10, A11 and A12 landed on `arch-refactor-dataset`. Spec A verbatim is pending (Appendix A). |
| Date | 2026-10-02 |
| Author | The fleet (unit `a0-designs`), from the operator's master plan |
| Source of truth | `ai_docs/plans/arch-refactor-dataset/00-master-plan.md` until Appendix A holds Spec A |
| Companion | `ai_docs/designs/2026-10-02-dataset-competition-design.md` (Spec B, phases B1 to B6) |
| Coverage map | `ai_docs/gates/architecture-refactor/SPEC-COVERAGE.md` |
| Base | branch `arch-refactor-dataset`, off `combine-open-prs` @ `575c2c2` |

Requirement tables in section 3 are machine-read by
`scripts/check-req-coverage.sh`. Do not change their header row. Do not
define an ID in any other table. A `SPEC-TODO(Spec A §n)` marks a place where
the verbatim spec text is needed and the master plan does not give it.

---

## 1. Status, scope and operator decisions

### 1.1 Scope

Spec A makes ownership explicit for: teammate, harness, model, worker, task,
execution, session, pane and skill. The rules:

- A functional core with an imperative shell.
- No ambient environment reads in domain code.
- Pure planning. Effects run only after a plan exists.
- Skills are first-class and versioned, through an internal marketplace.
- Old ledgers, old briefs, old frontmatter and offline built-in skills stay usable.

Spec A goes first (A0 to A12). Spec B (dataset and competition, B1 to B6) is
built on top of it, because Spec A §18 says competition is a composition of
ordinary executions.

Out of scope: Spec B's own phase 1 (a refactor; Spec A subsumes it) and Spec B
phase 8 (training Laya).

### 1.2 Operator decisions (copied from the master plan)

| # | Decision |
|---|---|
| OD1 | One branch, `arch-refactor-dataset`, off `origin/combine-open-prs` @ `575c2c2`. One commit series per phase, and every commit builds and passes `just gate`. Phase boundaries can be cut into stacked PRs later, which meets Spec A's "phases 3–10 independently reviewable". No PR unless the operator asks. |
| OD2 | The dataset binary is `multi-herdr-dataset`. `horch` keeps its name. |
| OD3 | Dataset storage is `$HORCH_STATE_DIR/multi-herdr/<project-slug>/`. It must be a subdirectory, because `telemetry::collect::read_ledgers` parses every `state_root/*.json`. The marketplace store is `${XDG_DATA_HOME:-~/.local/share}/horch/` (Spec A §10). |
| OD4 | Clef and Laya are **inert** in this branch. Both speak the System One API: `POST /v1/systemone` with typed `choice`/`score`/`noul` questions, returning a probability per option. Clef is Cloudflare's 27B/9B Apache-2.0 decision model; Laya is Convai's 421M local model, fine-tuned on `{state, questions, answers}`. We ship the seam, record eligible sets, planner propensities and `teacher: none`, export System-One/Laya-shaped rows, and add a **data-readiness report**. There is no HTTP client and no API keys. Clef is enabled only once readiness says the local data justifies it, and Laya later still. |
| OD5 | **No promotion by default.** A round ends at DECIDED with a winner, NEEDS_INTERVENTION, or REJECTED. `--promote-to <branch>` or `multi-herdr-dataset promote <round>` runs the full deterministic PromotionEngine. |
| OD6 | `sha2 0.10.9` is allowlisted for horch-core and horch-marketplace (NFR-06). It is already in Cargo.lock through herdr-install, so nothing new is downloaded. |
| OD7 | UUIDv7 is minted by hand from v4 bytes (48-bit ms prefix, version nibble 7). The uuid feature set does not change. `ExecutionId` accepts legacy non-UUID ids (`rec-o1`, `perf-3`). |

### 1.3 Hard constraints

- Never use `ANTHROPIC_API_KEY`. Every child process strips it (`FORBIDDEN_ENV`).
- No new crates except `sha2` (OD6). No `thiserror`: hand-written error enums
  with `Display` and `std::error::Error`. No `proptest`: the in-repo
  SplitMix64. No tokio or other async runtime. No HTTP crate.
- Golden prompts are never regenerated. Prose changes go in as named
  sanctioned blocks. A serialization format change bumps the schema version;
  a serialization golden is never re-blessed.
- Every phase follows the junior checklist (Spec A §16), written to
  `ai_docs/gates/architecture-refactor/CHECKLIST.md` and quoted in commit
  messages. `SPEC-TODO(Spec A §16)`: the checklist items verbatim.

---

## 2. Concept map and target layout

### 2.1 Concept map

| Spec A | Spec B | Today (PR #14) | Target |
|---|---|---|---|
| Teammate | role (`roles/*.md`) | `teammates/*.md`, `Teammate` | `roster::Teammate`, `TeammateName`; the judge is `teammates/judge.md` |
| Harness | candidate.harness | `teammates::Agent`, `Record.agent` | `harness::HarnessKind` |
| Model | candidate.model | `Record.model` | `ModelId` |
| Task | task{task_id, task_digest} | `Record.task` text | `execution::Task{TaskId, …}`; one Task per Round |
| Execution | **WorkerRun** | ledger `Record` (`record_id`) | `execution::Execution`. **`worker_run_id ≡ ExecutionId`**. WorkerRun is a projection of Execution plus measure events |
| Session | — | `Record.session_id` | `SessionState{Pending, Known(SessionId), Unavailable}` |
| Worker | — | role + mailbox `.id` | `WorkerId = "<workspace>:<role>"` |
| Pane | — | herdr pane id | `PaneId`; new optional `pane_id` on records |
| Skill | candidate features | `skills.rs` Bundle | `SkillId`, `SkillVersion`, `ResolvedSkillRef` |
| Competition | **Round** | — | `competition::Round` (`RoundId`); an Experiment groups rounds |
| Candidate | Candidate | — | a planned `SpawnRequest` plus an anonymous label |
| Evaluation | Judgment | — | `evaluation::Judgment`, a versioned label |
| RoutingProvenance | eligible set / teacher | `via`, `substitution_reason` | `routing::{RoutingProvenance, EligibleEntry}`, `teacher::*` |
| worktree isolation | WorktreeManager | — | `vcs::worktree`, as execution infrastructure (`SpawnRequest.workdir`) |

### 2.2 Target layout

```
crates/
  horch/                          CLI, presentation and bootstrap only
    src/lib.rs (new)              pub mod bootstrap; output; exit;  (shared by both bins)
    src/main.rs                   horch (thin)
    src/bootstrap.rs              RuntimeContext::from_env(&ProcessEnv), the only env reader
    src/cmd/*.rs                  existing commands, thinned; + skillscmd.rs, marketplacecmd.rs (A11)
    src/bin/multi-herdr-dataset.rs (B2) thin
    src/dataset/{run,preflight,status,watch,judge_job,promote,rollback,cleanup,export,readiness,outcome,rebuild}.rs
  horch-core/src/
    ids.rs  fsx.rs (atomic writes, DirLock)  clock.rs
    runtime/{context,paths,bins,process,fault,machine}.rs
    roster/{teammate,phase,effort,permission,parser,repository,validation,operator}.rs
    harness/{capabilities,launch,claude,claude_plugins,codex,opencode,pi,prime,none,headless}.rs
    routing/{policy,quota,quota_probe,snapshot,balance,eligible,decision}.rs
    execution/{model,legacy,store,plan,service,lifecycle}.rs
    skills/{catalog,selection,activation,briefing,materialize}.rs
    workspace/{client,herdr,model,layout,tile,balance,arrange,paneshell}.rs
    messaging/{mailbox,brief,delivery,message}.rs
    prompts/  telemetry/ (cohesive)  usage/{prices,read,money}.rs
    measure/{digest,event,store,recorder,projection,worker_run,paths,redact,testkit}.rs   (B1)
    vcs/{git,worktree}.rs                                                                 (B2/B3)
    competition/{model,config,state,preflight,planner,diversity,budget,coordinator,observe,promotion,cleanup}.rs
    evaluation/{validator,judge_input,rubric,judgment,parser,winner,scheduler}.rs        (B3/B4)
    teacher/{system_one,inert}.rs     dataset/{export,readiness,outcome}.rs               (B6)
  horch-marketplace/src/{model,catalog,manifest,source,resolver,store,installer,lockfile,integrity,git,fsx,error}.rs (A8)
  horch-e2e/   fakes extended + new fake-opencode, fake-prime; Harness::with_git(); tests/{e2e,scenario,dataset}.rs
teammates/judge.md   teammates/_base/competition-candidate.md
ai_docs/designs/2026-10-02-architecture-refactor-design.md   (ARC, MKT, SKL; Spec A verbatim as an appendix)
ai_docs/designs/2026-10-02-dataset-competition-design.md     (MEA, PRE, CMP, JDG, PRO, EXP, SEC, NFR-06..11; Spec B verbatim)
ai_docs/gates/architecture-refactor/{BASELINE.md, CHECKLIST.md, CURRENT_PHASE, SPEC-COVERAGE.md}
```

`error.rs` in `horch-marketplace` comes from unit plan U04 (one hand-written
`MarketplaceError`). The master plan layout does not list it.

### 2.3 Dependency direction

```
horch (both bins)  ──►  horch-core  ──►  horch-marketplace
```

- `horch-marketplace` never imports `horch-core` (MKT-01).
- The low-level git runner is `horch_marketplace::git::GitRunner`.
  `horch_core::vcs::git::GitCli` wraps it.
- Inside `horch-core`, the arrows point inward to the pure modules:
  `execution::service` → `execution::plan` → `routing`, `roster`, `harness`,
  `skills`. `workspace` and `messaging` are effect adapters that the service
  calls. `telemetry` reads the execution store. Nothing in `routing`,
  `roster` or `skills` calls `workspace`.

### 2.4 Module rules (Spec A §3) and the scan test that enforces each

Every rule is a source-scan test. A scan test reads `.rs` files under
`crates/*/src` as text and fails with the file and line of each violation.

| Rule | Enforced by (test) | Phase | What the scan checks |
|---|---|---|---|
| roster never calls Herdr | `arc_19_herdr_parsing_contract` and `arc_27_tile_balance_pure` cover workspace purity; the roster rule itself is `arc_08_unknown_field_rejected`'s module scan | A3/A7 | `roster/**` has no `workspace::`, `herdr::`, `Herdr`, `std::process` |
| routing never launches | `arc_12_decisions_match_baseline` (pure function signatures) and `arc_13_candidates_equivalent` | A5 | `routing/**` except `quota_probe.rs` and `snapshot.rs` has no `std::process`, `Command`, `workspace::` |
| planning is pure | `arc_15_plan_deterministic` | A6 | `execution/plan.rs` has no `std::fs`, `std::process`, `std::env`, `workspace::`, `clock::now` |
| telemetry observes and does not route | `arc_23_telemetry_reads_executions` | A6 | `telemetry/**` has no `routing::decision`, no `RoutingDecision` construction |
| the marketplace knows nothing about teammates | `mkt_01_no_core_dependency` | A8 | `horch-marketplace` has no `horch-core` dependency and no `horch_core`, `teammate` (case-insensitive) in `src/` |
| the CLI holds no policy | `arc_22_no_error_string_matching` | A6 | `crates/horch/src/cmd/**` does not match on error strings (`.to_string().contains(`, `msg.starts_with(`) and does not call `routing::balance::decide` directly |
| prompts hold no execution policy | `cmp_15_candidate_task_carries_rules` with `golden_prompts` | B3 | `prompts/**` has no `HarnessKind::`, `BalanceMode`, `ExecutionStatus` |
| domain code never calls `std::env` | `arc_05_no_ambient_env_in_core` | A2 | `horch-core/src/**` outside `runtime/` has no `std::env::var`, `env::var_os`, `set_var`, `remove_var`, `current_dir` |
| harness match only in harness | `arc_10_harness_match_only_in_harness` | A4 | `HarnessKind::` match arms appear only in `harness/**` and `roster/validation.rs` |
| skills core has no harness flags | `skl_05_core_skill_model_has_no_harness_flags` | A10 | `skills/**` has no `HarnessKind` |
| source dispatch localized | `mkt_10_source_dispatch_localized` | A8 | `SkillSource::` match arms appear only in `resolver.rs` and the fetch module |
| tile/balance pure | `arc_27_tile_balance_pure` | A7 | `workspace/{layout,tile,balance}.rs` import no client and no `std::process` |
| competition domain free of adapters | `cmp_02_domain_has_no_adapter_imports` | B1 | `competition/model.rs`, `evaluation/{winner,parser}.rs` have no `vcs::`, `workspace::`, `std::process` |
| shims removed | `arc_25_no_shim_modules` | A12 | no `pub use crate::…::*;` re-export shim module remains |

The roster, routing and planning rules have no dedicated ID in the master
plan. Their scans live inside the named tests above. `SPEC-TODO(Spec A §3)`:
confirm the full rule list against the verbatim text.

---

## 3. Requirements

Header rule: `| ID | Requirement | Phase | Tests |`. The first token of the
Phase cell is the ID's phase. Test names are the exact test function names.
Lowercase non-ID entries (`bal_04`, `tel_*`, `nfr_03`, `quo_07`,
`golden_prompts`, `spc_04_render_goldens`) name tests that exist today.

### 3.1 ARC: architecture

| ID | Requirement | Phase | Tests |
|---|---|---|---|
| ARC-01 | Baseline and oracles | A0 | arc_01_baseline_oracles_present, spc_04_render_goldens |
| ARC-02 | Identity newtypes, v7 | A1 | arc_02_ids_validate, arc_02_mint_v7_layout, arc_02_legacy_ids_accepted |
| ARC-03 | HarnessKind compat | A1 | arc_03_harness_kind_serde_compat |
| ARC-04 | Typed session/status/tiling | A1 | arc_04_status_legacy_roundtrip, arc_04_tiling_mode |
| ARC-05 | RuntimeContext; no ambient env | A2 | arc_05_no_ambient_env_in_core, arc_05_context_from_map_env |
| ARC-06 | Env is child transport only | A2 | arc_06_transport_env_applied_to_child, arc_06_env_mutation_sites_reduced |
| ARC-07 | Brief carries only inherited context | A2 | arc_07_brief_v1_readable, arc_07_e2e_bin_overrides_reach_worker |
| ARC-08 | Roster split, strict serde | A3 | arc_08_unknown_field_rejected, arc_08_overlay_precedence, arc_08_legacy_frontmatter_corpus_parses, nfr_03 |
| ARC-09 | Harness owns command lines | A4 | arc_09_argv_matches_baseline |
| ARC-10 | Capabilities; dispatch-only lifecycle | A4 | arc_10_capabilities_match_legacy_predicates, arc_10_harness_match_only_in_harness |
| ARC-11 | Workdir-scoped discovery | A4 | arc_11_codex_discovery_by_workdir, arc_11_opencode_discovery_by_workdir, arc_11_canonical_tmp_paths |
| ARC-12 | Pure routing equals baseline | A5 | arc_12_decisions_match_baseline, bal_03, bal_04, bal_05, bal_06, quo_07 |
| ARC-13 | Eligible set and reasons | A5 | arc_13_exclusion_reasons, arc_13_candidates_equivalent |
| ARC-14 | Provenance on every execution | A5 | arc_14_provenance_on_spawn_substitute_resume |
| ARC-15 | Pure planning | A6 | arc_15_plan_deterministic, arc_15_plan_table |
| ARC-16 | Apply order; LaunchFailed; no phantoms | A6 | arc_16_split_failure_launch_failed, arc_16_run_failure_closes_pane, arc_16_e2e_fail_split, arc_16_crash_after_insert_not_live |
| ARC-17 | Legacy DTO and compat | A6 | arc_17_legacy_ledgers_load_and_resume, arc_17_roundtrip_byte_identical, arc_17_old_reader_sees_compat_status |
| ARC-18 | Worker workflow; exit code | A6 | arc_18_worker_startup_order, arc_18_agent_exit_recorded |
| ARC-19 | WorkspaceClient; tiler unchanged | A7 | arc_19_herdr_parsing_contract, moved tile/balance/layout tests |
| ARC-20 | Delivery identical | A7 | arc_20_send_line_prompt_first, arc_20_fallback_waits_tail |
| ARC-21 | done lifecycle order | A7 | arc_21_done_order |
| ARC-22 | Thin CLI; typed errors; exit codes | A6 / A12 | bal_04 (exit 3), arc_22_no_error_string_matching |
| ARC-23 | Telemetry uses executions | A6 | arc_23_telemetry_reads_executions, tel_* |
| ARC-24 | Competition uses the kernel | B3 | arc_24_candidates_are_ordinary_executions |
| ARC-25 | Shims removed; pub(crate) | A12 | arc_25_no_shim_modules |
| ARC-26 | E2E lifecycle matrix for all 5 harnesses | A6 | arc_26_e2e_lifecycle_matrix_claude, arc_26_e2e_lifecycle_matrix_codex, arc_26_e2e_lifecycle_matrix_opencode, arc_26_e2e_lifecycle_matrix_pi, arc_26_e2e_lifecycle_matrix_prime |
| ARC-27 | tile/balance pure | A7 | arc_27_tile_balance_pure |

### 3.2 MKT: marketplace

| ID | Requirement | Phase | Tests |
|---|---|---|---|
| MKT-01 | Independent of core | A8 | mkt_01_no_core_dependency |
| MKT-02 | Branch/tag pinned to SHA | A8 | mkt_02_branch_resolves_to_sha |
| MKT-03 | Lifecycle order | A8 | mkt_03_lifecycle_order |
| MKT-04 | Store and lock fields | A8 | mkt_04_lock_entry_fields |
| MKT-05 | Transactional install | A8 | mkt_05_install_is_transactional |
| MKT-06 | Reject traversal, symlink, oversize, bad SKILL.md, hooks | A8 | mkt_06_rejects_traversal, mkt_06_rejects_symlink, mkt_06_rejects_oversize, mkt_06_rejects_invalid_skill_md, mkt_06_rejects_hooks |
| MKT-07 | No credentials | A8 | mkt_07_rejects_credentials_in_url |
| MKT-08 | Offline and repeatable | A8 / A11 | mkt_08_offline_reinstall_from_lock, mkt_08_e2e_install_update_repeatable, mkt_08_runtime_needs_no_network |
| MKT-09 | CLI and just; legacy flags | A11 | mkt_09_legacy_skills_flags_output_unchanged, mkt_09_cli_list_show_json |
| MKT-10 | Source dispatch localized | A8 | mkt_10_source_dispatch_localized |

### 3.3 SKL: skills

| ID | Requirement | Phase | Tests |
|---|---|---|---|
| SKL-01 | Bundled entries form a versioned catalog | A9 | skl_01_bundled_catalog_versions_and_digests |
| SKL-02 | Plan before launch equals legacy | A9 | skl_02_activation_matches_legacy_selection |
| SKL-03 | InvocationPolicy mapping; no NLP | A9 | skl_03_policy_mapping |
| SKL-04 | Executions store refs | A9 | skl_04_execution_records_skill_refs |
| SKL-05 | No harness flags in core | A10 | skl_05_core_skill_model_has_no_harness_flags |
| SKL-06 | Only activated skills exposed | A10 | skl_06_e2e_exposure_claude, skl_06_e2e_exposure_codex, skl_06_e2e_exposure_opencode, skl_06_e2e_exposure_pi, skl_06_e2e_exposure_prime |
| SKL-07 | Plugin skills separate | A9 | skl_07_plugin_skills_separate |
| SKL-08 | Briefing unchanged | A9 | skl_08_briefing_matches_baseline_modulo_path |

### 3.4 Spec A §17 acceptance criteria → IDs

`SPEC-TODO(Spec A §17)`: the criterion texts below are the master plan's
wording. Replace them with the verbatim text when Appendix A is filled.

| # | Criterion | IDs |
|---|---|---|
| 1 | horch is thin and orchestration policy lives in core | ARC-22, ARC-25 |
| 2 | spawn is an application workflow built from a pure plan | ARC-15, ARC-16 |
| 3 | teammate, harness, model, worker, execution, session and pane are separate types | ARC-02, ARC-03, ARC-04 |
| 4 | domain/planner code has no ambient environment lookup | ARC-05, ARC-06 |
| 5 | harness-specific behavior is owned by harness modules | ARC-09, ARC-10 |
| 6 | PR #14 routing is pure and provenance is persisted | ARC-12, ARC-13, ARC-14 |
| 7 | telemetry consumes execution identity | ARC-23 |
| 8 | old ledgers remain usable | ARC-17 |
| 9 | skills have immutable resolved identities; executions record exact versions | SKL-01, SKL-04 |
| 10 | Git skills are locally materialized/pinned before runtime | MKT-02, MKT-08 |
| 11 | deterministic vs model-routed skill behavior is explicit | SKL-03 |
| 12 | tile/balance remain pure | ARC-19, ARC-27 |
| 13 | E2E covers all harness lifecycle and routing paths | ARC-26, ARC-14, bal_04..06, SKL-06 |
| 14 | adding a harness ≈ one harness module | ARC-10 (`arc_10_harness_match_only_in_harness`) |
| 15 | adding a skill source ≈ marketplace resolution/fetch | MKT-10 |
| 16 | competition composes ordinary executions | ARC-24 |

### 3.5 Spec A §13 E2E list → IDs

| E2E case | IDs |
|---|---|
| fresh, resume, session discovery, done | ARC-26 |
| substitution and refusal | ARC-14, bal_04..06 |
| skill activation | SKL-06 |
| git skill install | MKT-08 |
| launch failure cleanup | ARC-16 |

---

## 4. Key types

Signatures are the target shape. Field names, variant names and serde
spellings are binding. Private helpers and derive lists beyond the ones shown
are free. A1, A8 and B1 implement some of these in parallel; differences are
reconciled at merge in favor of the merged code, and this section is updated.

### 4.1 `ids` (A1)

```rust
// crates/horch-core/src/ids.rs

/// Defines `pub struct $name(String)` with:
///   #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
///   #[serde(try_from = "String", into = "String")]   // deserialization validates
///   impl $name { pub fn new(s: impl Into<String>) -> Result<Self, IdError>;
///                pub fn as_str(&self) -> &str; }
///   impl Display, AsRef<str>, FromStr<Err = IdError>, TryFrom<String, Error = IdError>,
///   From<$name> for String.
/// `$validator: fn(&str) -> Result<(), IdError>` runs on every constructor path.
macro_rules! string_id { ($name:ident, $validator:path) => { /* … */ } }

string_id!(ExecutionId, validate_execution_id);
string_id!(TaskId, validate_plain);
string_id!(SessionId, validate_plain);
string_id!(WorkerId, validate_worker_id);
string_id!(RoleName, validate_role_name);
string_id!(SkillId, validate_plain);       // marketplace has its own SkillId with SKILL.md name rules
string_id!(ModelId, validate_plain);
string_id!(TeammateName, validate_plain);
string_id!(PaneId, validate_plain);
string_id!(WorkspaceId, validate_plain);
string_id!(ExperimentId, validate_plain);
string_id!(RoundId, validate_plain);
string_id!(EventId, validate_plain);
string_id!(JudgmentId, validate_plain);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdError {
    Empty { kind: &'static str },
    ControlChar { kind: &'static str },
    PathLike { kind: &'static str, value: String },      // RoleName: '/', '\\', ".."
    BadWorkerId { value: String },                       // not "<workspace>:<role>"
}
impl std::fmt::Display for IdError { /* … */ }
impl std::error::Error for IdError {}

/// UUIDv7 from v4 bytes (OD7). Bytes 0..6 = big-endian Unix ms of `at`;
/// byte 6 high nibble = 0x7; RFC 4122 variant bits (10xx) kept.
pub fn mint_v7(at: chrono::DateTime<chrono::Utc>) -> uuid::Uuid;

impl ExecutionId  { pub fn mint(at: chrono::DateTime<chrono::Utc>) -> Self; }
impl EventId      { pub fn mint(at: chrono::DateTime<chrono::Utc>) -> Self; }
impl ExperimentId { pub fn mint(at: chrono::DateTime<chrono::Utc>) -> Self; }
impl RoundId      { pub fn mint(at: chrono::DateTime<chrono::Utc>) -> Self; }
impl JudgmentId   { pub fn mint(at: chrono::DateTime<chrono::Utc>) -> Self; }
impl WorkerId     { pub fn new_for(workspace: &WorkspaceId, role: &RoleName) -> Self; } // "<ws>:<role>"
```

Validation rules:

| Type | Rule |
|---|---|
| every id | non-empty; no control character (`char::is_control`) |
| `RoleName` | also rejects `/`, `\` and the substring `..` |
| `WorkerId` | exactly one `:`; both sides non-empty |
| `ExecutionId` | any UUID (v4 or v7), and any legacy non-UUID id such as `rec-o1`, `perf-3` (OD7) |

`Digest([u8; 32])` displays as `sha256:<64 hex>`. It lives in
`measure/digest.rs` (B1). See the dataset design §4.

### 4.2 `runtime` (A2)

```rust
// runtime/context.rs
pub trait EnvSource {
    fn var(&self, key: &str) -> Option<String>;          // empty string counts as None
    fn var_os(&self, key: &str) -> Option<std::ffi::OsString>;
    fn current_dir(&self) -> std::io::Result<std::path::PathBuf>;
    fn current_exe(&self) -> std::io::Result<std::path::PathBuf>;
}
/// The real process environment. Constructed only in `horch::bootstrap`.
pub struct ProcessEnv;
impl EnvSource for ProcessEnv { /* … */ }
/// A fixed map for tests.
#[derive(Debug, Clone, Default)]
pub struct MapEnv {
    pub vars: std::collections::BTreeMap<String, String>,
    pub cwd: std::path::PathBuf,
    pub exe: std::path::PathBuf,
}
impl MapEnv { pub fn new(cwd: impl Into<PathBuf>) -> Self; pub fn with(self, key: &str, value: &str) -> Self; }
impl EnvSource for MapEnv { /* … */ }

#[derive(Debug, Clone)]
pub struct RuntimeContext {
    pub paths: Paths,
    pub herdr: HerdrContext,
    pub bins: Bins,
    pub settings: Settings,
    pub env: Inherited,
}
impl RuntimeContext {
    pub fn from_env(env: &dyn EnvSource) -> anyhow::Result<RuntimeContext>;
}

// runtime/paths.rs   (moved from ledger.rs:177-205)
#[derive(Debug, Clone)]
pub struct Paths {
    pub project_dir: PathBuf,     // $HORCH_PROJECT_DIR, else cwd
    pub state_root: PathBuf,      // $HORCH_STATE_DIR, else ${XDG_STATE_HOME:-$HOME/.local/state}/horch
    pub data_root: PathBuf,       // ${XDG_DATA_HOME:-$HOME/.local/share}/horch  (marketplace store, OD3)
    pub temp_root: PathBuf,       // std::env::temp_dir() at bootstrap
    pub home: PathBuf,            // $HOME (USERPROFILE on windows), else "."
}
pub fn state_root(env: &dyn EnvSource, home: &Path) -> PathBuf;
pub fn project_dir(env: &dyn EnvSource) -> anyhow::Result<PathBuf>;
pub fn home_dir(env: &dyn EnvSource) -> PathBuf;

#[derive(Debug, Clone, Default)]
pub struct HerdrContext {
    pub workspace: Option<WorkspaceId>,   // HERDR_WORKSPACE_ID
    pub pane: Option<PaneId>,             // HERDR_PANE_ID
}

// runtime/bins.rs   (absorbs agent.rs resolvers)
#[derive(Debug, Clone)]
pub struct Bins {
    pub roster_override: Option<PathBuf>,   // HORCH_TEAMMATES_DIR
    pub current_exe: PathBuf,
    pub horch_exe: PathBuf,                 // sibling `horch` of current_exe, then PATH
    pub harness: HarnessBins,
    pub overrides: BinOverrides,
}
#[derive(Debug, Clone)]
pub struct HarnessBins {                    // resolved values
    pub claude: PathBuf,   // HORCH_CLAUDE_BIN, else `cpx` on PATH, else `claude`
    pub codex: PathBuf, pub opencode: PathBuf, pub pi: PathBuf,
    pub prime: PathBuf,    // default `prime-agent`
    pub herdr: PathBuf, pub sqlite3: PathBuf, pub ollama: PathBuf,
    pub git: PathBuf,      // HORCH_GIT_BIN, else `git`
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinOverrides {                   // explicit HORCH_*_BIN values only; travel in Brief v2
    pub claude: Option<PathBuf>, pub codex: Option<PathBuf>, pub opencode: Option<PathBuf>,
    pub pi: Option<PathBuf>, pub prime: Option<PathBuf>, pub herdr: Option<PathBuf>,
    pub sqlite3: Option<PathBuf>, pub ollama: Option<PathBuf>, pub git: Option<PathBuf>,
}
impl Bins { pub fn resolve(env: &dyn EnvSource, path_var: Option<&OsStr>) -> Bins; }

#[derive(Debug, Clone)]
pub struct Settings {
    pub clock: Clock,                         // Clock::System or Clock::Fixed(DateTime<Utc>) (HORCH_NOW, if the e2e uses it)
    pub tiling: TilingMode,                   // HORCH_NO_TILE → Disabled
    pub balance_override: Option<BalanceMode>,// HORCH_BALANCE
    pub quota_file: Option<PathBuf>,          // HORCH_QUOTA_FILE
    pub machine_file: Option<PathBuf>,        // HORCH_MACHINE_FILE (B2)
    pub probe_timeout: std::time::Duration,   // HORCH_PROBE_TIMEOUT_MS
    pub faults: Faults,                       // HORCH_FAULT
}

#[derive(Debug, Clone, Default)]
pub struct Inherited {
    pub opencode_config_content: Option<String>,  // OPENCODE_CONFIG_CONTENT
    pub codex_home: Option<PathBuf>,              // CODEX_HOME
    pub path: Option<OsString>,                   // PATH, for `which`
    pub worker: Option<WorkerEnv>,
}
#[derive(Debug, Clone)]
pub struct WorkerEnv {                            // set only inside a worker pane
    pub role: RoleName,                           // HORCH_ROLE
    pub brief: PathBuf,                           // HORCH_BRIEF
}

// runtime/fault.rs
#[derive(Debug, Clone, Default)]
pub struct Faults { points: Vec<String> }         // HORCH_FAULT, comma-separated
impl Faults {
    pub fn parse(raw: Option<&str>) -> Faults;
    pub fn is(&self, point: &str) -> bool;        // exact match, e.g. "abort-after-execution-insert"
    pub fn indexed(&self, prefix: &str) -> Option<String>; // "abort-after-worktree:<n>" → Some(n)
    /// Exits the process with code 86 and a stderr line when `point` is armed.
    pub fn abort_if(&self, point: &str);
}

// runtime/process.rs   (absorbs agent.rs `which`, tilecmd.rs:628 spawn_detached)
pub fn which(path_var: &OsStr, name: &str) -> Option<PathBuf>;
pub fn spawn_detached(cmd: std::process::Command) -> std::io::Result<u32>; // setsid on unix
pub fn strip_forbidden(cmd: &mut std::process::Command);                   // removes FORBIDDEN_ENV
```

`SPEC-TODO(Spec A §4)`: the exact `RuntimeContext` field grouping. The fault
abort exit code (86) is a design choice; change it if Spec A names one.

### 4.3 `execution::model` (A1) and legacy compatibility (A6)

```rust
// execution/model.rs
/// Serde: internally tagged, snake_case.
/// {"state":"planned"} | {"state":"running"} | {"state":"done"}
/// {"state":"failed","failure":{"kind":"agent_exited","code":1}}
/// {"state":"launch_failed","stage":"split","reason":"…"}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ExecutionStatus {
    Planned,
    Starting,
    Running,
    Done,
    Failed { failure: FailureKind },
    LaunchFailed { stage: LaunchStage, reason: String },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FailureKind {
    AgentExited { code: Option<i32> },
    PaneVanished,
    TimedOut,
    Cancelled { reason: String },
    Crashed,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaunchStage { Brief, Split, Run }

impl ExecutionStatus {
    pub fn is_live(&self) -> bool;                    // Starting | Running
    pub fn is_terminal(&self) -> bool;                // Done | Failed | LaunchFailed
    pub fn legacy_status(&self) -> &'static str;      // "working" | "done"
    pub fn from_legacy(status: &str) -> ExecutionStatus; // "working" → Running; else → Done
    pub fn resolve(state: Option<&ExecutionStatus>, status: &str) -> ExecutionStatus; // prefers state
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExecutionKind {
    Worker,
    Orchestrator,
    Candidate { experiment: ExperimentId, round: RoundId, label: String },
    Judge { round: RoundId, attempt: u32 },
}
impl ExecutionKind { pub fn legacy_kind(&self) -> &'static str; } // Candidate, Judge → "worker"

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionMode { Fresh(Option<SessionId>), Resume(SessionId) }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "session", content = "id", rename_all = "snake_case")]
pub enum SessionState { Pending, Known(SessionId), Unavailable }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TilingMode { Automatic, Disabled }
impl TilingMode { pub fn from_no_tile(no_tile: bool) -> TilingMode; }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub text: String,
    pub phase: Option<Phase>,
    pub plan: Option<String>,          // plan-file slug
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Execution {
    pub id: ExecutionId,
    pub kind: ExecutionKind,
    pub teammate: TeammateName,
    pub harness: HarnessKind,
    pub model: ModelId,
    pub effort: Option<String>,
    pub phase: Option<Phase>,
    pub role: RoleName,
    pub worker: WorkerId,
    pub task: Task,
    pub status: ExecutionStatus,
    pub session: SessionState,
    pub exit_code: Option<i32>,
    pub plan: Option<String>,
    pub project: PathBuf,
    pub workdir: PathBuf,
    pub workspace: Option<WorkspaceId>,
    pub pane: Option<PaneId>,
    pub skills: Vec<ResolvedSkillRef>,
    pub routing: Option<RoutingProvenance>,
    pub history: Vec<HistoryEntry>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}
```

`SPEC-TODO(Spec A §4)`: the Execution field list verbatim.

**`LedgerRecordV1`** (`execution/legacy.rs`, A6). It is today's `Record`
serde, byte for byte, plus optional skip-if-empty fields. Today's `Record`
has no `deny_unknown_fields`, so old binaries ignore the new keys. This is
the compatibility path.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerRecordV1 {
    // ── today's Record, unchanged, in this order ──
    pub record_id: String,
    pub session_id: Option<String>,                 // null until known
    pub agent: String,
    pub tier: String,                               // the teammate name; key stays "tier"
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub phase: Option<Phase>,
    pub role: String,
    pub status: String,                             // "working" | "done" (legacy compat)
    pub task: String,
    pub history: Vec<HistoryEntry>,                 // {at, event, text}
    pub created_at: String,
    pub updated_at: String,
    #[serde(default = "worker_kind", skip_serializing_if = "is_worker")] pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub plan: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub via: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub substitution_reason: Option<String>,
    // ── new, optional, skip-if-empty ──
    #[serde(default, skip_serializing_if = "Option::is_none")] pub pane_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub workdir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub experiment_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub round_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub state: Option<ExecutionStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub finished_at: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]   pub skills: Vec<ResolvedSkillRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub routing: Option<RoutingProvenance>,
}
impl LedgerRecordV1 {
    pub fn to_execution(&self) -> Result<Execution, LegacyError>;
    pub fn from_execution(e: &Execution) -> LedgerRecordV1;
}
```

`kind` on disk stays `"worker"` or `"orchestrator"`. A Candidate or Judge
execution writes `kind: "worker"` (legacy) plus `experiment_id`, `round_id`,
`label` and a `state`; the store reconstructs `ExecutionKind` from these.
`SPEC-TODO(Spec B)`: whether the judge attempt needs its own key.

**The `status` / `state` compatibility rule.**

| Typed status | `status` written | `state` written |
|---|---|---|
| Planned, Starting, Running | `"working"` | the typed status |
| Done | `"done"` | the typed status |
| Failed(_), LaunchFailed{..} | `"done"` | the typed status |

- Readers prefer `state`. Without `state`, they derive from `status`:
  `"working"` → Running, anything else → Done.
- Old binaries read only `status`, so a Failed or LaunchFailed record looks
  done, never phantom-live (ARC-17 `arc_17_old_reader_sees_compat_status`).
- A record without the new keys serializes byte-identical to today
  (`arc_17_roundtrip_byte_identical`).

### 4.4 `routing` (A5)

```rust
// routing/eligible.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExclusionReason {
    NotInRoster, Hidden, ReservedTier, TrainsOnInput, PoolBlocked, Unspawnable,
    EffortUnsupported, HarnessUnavailable, AgentNone, ExcludedByConfig, OverBudget,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "verdict", content = "reason", rename_all = "snake_case")]
pub enum Verdict { Eligible, Excluded(ExclusionReason) }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EligibleEntry {
    pub teammate: TeammateName,
    pub harness: HarnessKind,
    pub model: ModelId,
    pub effort: Option<String>,
    pub fallback_index: Option<u32>,   // None = the requested teammate; Some(i) = fallbacks[i]
    pub pool: String,
    pub pool_state: quota::State,
    pub verdict: Verdict,
}
#[derive(Debug, Clone, Default)]
pub struct EligibilityFilter {          // B3 config exclusions and budget
    pub excluded: BTreeSet<TeammateName>,
    pub max_cost_microusd: Option<i64>,
    pub available_harnesses: Option<BTreeSet<HarnessKind>>,
}
/// The requested teammate, then its fallbacks, in `fallbacks:` order.
/// Same order and same drop rules as today's private `candidates()`.
pub fn eligible_fallbacks(req: &Teammate, roster: &Roster, view: &QuotaView) -> Vec<EligibleEntry>;
/// Every roster teammate, sorted by name, each with a verdict.
pub fn roster_eligibility(roster: &Roster, view: &QuotaView, filter: &EligibilityFilter) -> Vec<EligibleEntry>;

// routing/decision.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingMode { Auto, Advise, Off, Exact, Force, Pinned }   // Pinned: B3 candidates

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoutingProvenance {
    pub requested: TeammateName,
    pub resolved: TeammateName,
    pub fallback_index: Option<u32>,
    pub pool: String,
    pub pool_state: quota::State,
    pub reason: Option<String>,
    pub mode: RoutingMode,
}
#[derive(Debug, Clone, PartialEq)]
pub enum RoutingDecision {
    Spawn { teammate: TeammateName, note: Option<String>, provenance: RoutingProvenance },
    Substitute { requested: TeammateName, resolved: TeammateName, reason: String, provenance: RoutingProvenance },
    Refuse { requested: TeammateName, reason: String, pools: Vec<PoolLine> },
}
/// Pure. Same logic as today's `balance_policy::decide`.
pub fn decide(req: &Teammate, roster: &Roster, view: &QuotaView, mode: BalanceMode, flags: GateFlags) -> RoutingDecision;
pub fn resolve(req: &Teammate, roster: &Roster, d: &RoutingDecision) -> Option<Teammate>;

/// Renders today's `Decision` JSON byte for byte:
/// {"decision":"spawn","teammate":…,"note":…} | {"decision":"substitute","original":…,"via":…,"reason":…}
/// | {"decision":"refuse","teammate":…,"reason":…,"pools":[{pool,state,detail}]}
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "decision", rename_all = "lowercase")]
pub enum DecisionDto {
    Spawn { teammate: String, note: Option<String> },
    Substitute { original: String, via: String, reason: String },
    Refuse { teammate: String, reason: String, pools: Vec<PoolLine> },
}
impl From<&RoutingDecision> for DecisionDto { /* … */ }
impl RoutingDecision { pub fn line(&self) -> Option<String>; } // today's Decision::line text

pub struct GateFlags { pub exact: bool, pub force: bool }          // unchanged
pub struct PoolLine { pub pool: String, pub state: String, pub detail: String } // unchanged
```

`routing/snapshot.rs`: `pub fn obtain(ctx: &RuntimeContext, policy: &Policy) -> anyhow::Result<QuotaView>` is the only path that probes.

### 4.5 `execution` planning and service (A6)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportTarget { Orchestrator, None }

#[derive(Debug, Clone)]
pub struct SpawnRequest {
    // SPEC-TODO(Spec A §8): the field list verbatim.
    pub teammate: Option<TeammateName>,     // None with resume
    pub task: Option<String>,
    pub phase: Option<Phase>,
    pub effort: Option<String>,
    pub session: SessionMode,               // Fresh(None) | Resume(id)
    pub balance: BalanceMode,
    pub flags: GateFlags,
    pub routing_mode: RoutingMode,          // Pinned for candidates (B3)
    pub tiling: TilingMode,
    pub workdir: Option<PathBuf>,           // None → project_dir
    pub kind: ExecutionKind,
    pub report_to: ReportTarget,
    pub idempotency_key: Option<String>,    // "spawn:<round>:<label>" (B3)
}

#[derive(Debug, Clone)]
pub struct LaunchPlan {                     // what the harness will run
    pub teammate: Teammate,                 // resolved (merged with fallback when substituted)
    pub session: SessionMode,
    pub prompt: String,
    pub argv: Vec<String>,
    pub env: BTreeMap<String, String>,      // child-only
    pub env_remove: Vec<String>,            // FORBIDDEN_ENV
}
#[derive(Debug, Clone)]
pub struct WorkspacePlan {
    pub workspace: WorkspaceId,
    pub split_from: PaneId,
    pub direction: Direction,
    pub tiling: TilingMode,
}
#[derive(Debug, Clone)]
pub struct ExecutionPlan {
    pub execution: Execution,               // status Planned
    pub worker: WorkerId,
    pub launch: LaunchPlan,
    pub skills: SkillActivationPlan,
    pub workspace: WorkspacePlan,
    pub gate_line: Option<String>,          // NOTE/SUBSTITUTED line, printed before the pane id
}

/// Inputs gathered by the shell before planning. Pure data.
pub struct PlanInputs<'a> {
    pub roster: &'a Roster,
    pub quota: &'a QuotaView,
    pub catalog: &'a SkillCatalog,
    pub lock: &'a LockView,
    pub existing: Option<&'a Execution>,    // for resume
    pub now: DateTime<Utc>,
    pub ids: MintedIds,                     // ExecutionId, SessionId pre-minted by the shell
    pub ctx: &'a RuntimeContext,
}
/// Pure: resume rules, routing, reserved-tier check, effort, skill support.
/// Logic from cmd/spawn.rs:76-262.
pub fn plan_launch(req: &SpawnRequest, inputs: &PlanInputs) -> Result<ExecutionPlan, PlanError>;
/// Pure: completes the plan once the role is allocated (impure, under DirLock).
pub fn finish_plan(plan: ExecutionPlan, role: RoleName, workspace: &WorkspaceId) -> ExecutionPlan;

#[derive(Debug)]
pub enum PlanError {
    NothingToSpawn, UnknownTeammate(String), ReservedTier { model: String, tier: String },
    NotResumable { id: ExecutionId, reason: String }, BadEffort(String), SkillUnsupported(String),
}
#[derive(Debug)]
pub enum SpawnError {
    Refused { decision: RoutingDecision },                        // CLI exit 3
    Plan(PlanError),
    LaunchFailed { id: ExecutionId, stage: LaunchStage, source: anyhow::Error },
}
impl std::fmt::Display for SpawnError { /* … */ }
impl std::error::Error for SpawnError {}

// execution/service.rs
pub struct ExecutionService<'a, W: WorkspaceClient> {
    pub ctx: &'a RuntimeContext,
    pub store: &'a ExecutionStore,
    pub workspace: &'a W,
}
impl<'a, W: WorkspaceClient> ExecutionService<'a, W> {
    /// insert(Planned) → brief → split → run → mark_starting(pane) → tile (best effort).
    pub fn spawn(&self, req: &SpawnRequest) -> Result<SpawnOutcome, SpawnError>;
}
pub struct SpawnOutcome { pub execution: ExecutionId, pub pane: PaneId, pub gate_line: Option<String> }

// execution/store.rs
pub struct ExecutionStore { /* <state_root>/<slug>.json, DirLock */ }
impl ExecutionStore {
    pub fn open(paths: &Paths, project: &Path) -> ExecutionStore;
    pub fn load(&self) -> anyhow::Result<Vec<Execution>>;
    pub fn insert(&self, e: &Execution) -> anyhow::Result<()>;
    pub fn update(&self, id: &ExecutionId, f: impl FnOnce(&mut Execution)) -> anyhow::Result<()>;
    pub fn find_by_idempotency(&self, key: &str) -> anyhow::Result<Option<Execution>>;
}

// execution/lifecycle.rs
pub fn run_worker(ctx: &RuntimeContext, store: &ExecutionStore, brief: &Brief) -> anyhow::Result<i32>;
pub fn done<W: WorkspaceClient>(ctx: &RuntimeContext, store: &ExecutionStore, ws: &W, summary: &str) -> anyhow::Result<()>;
```

`SPEC-TODO(Spec A §8)`: the worker startup order verbatim
(`arc_18_worker_startup_order` asserts it).

### 4.6 `harness` (A1 enum, A4 trait)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HarnessKind {
    Claude,
    Codex,
    #[serde(rename = "opencode")]
    OpenCode,
    Pi,
    Prime,
    None,
}
impl HarnessKind {
    pub fn as_str(self) -> &'static str;                 // "claude" … "none"
    pub fn adapter(self) -> &'static dyn Harness;        // A4
    // until A4, every legacy predicate stays (mints_session_id, harvests_session_id,
    // runs_a_daemon, uses_execpolicy, takes_tool_lists, takes_tool_denylist, unsupported_fields)
}
// shim in teammates.rs (A1 to A12): pub use crate::harness::HarnessKind as Agent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillExposure { None, PluginDir, SkillFlag, ConfigPaths, CodexHome }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    pub caller_minted_session: bool,
    pub resume: bool,
    pub effort: bool,
    pub daemon: bool,
    pub exec_policy: bool,
    pub skill_exposure: SkillExposure,
    pub tool_lists: bool,
    pub tool_denylist: bool,
    pub headless: bool,
}

pub struct Prepared {                     // files written before launch; removed on drop
    pub env: BTreeMap<String, String>,
    pub extra_args: Vec<String>,
    pub cleanup: Vec<PathBuf>,
}
pub trait Harness: Sync {
    fn kind(&self) -> HarnessKind;
    fn capabilities(&self) -> Capabilities;
    /// Teammate fields this harness cannot express (today's unsupported_fields).
    fn validate(&self, t: &Teammate) -> Vec<&'static str>;
    /// codex rules + private CODEX_HOME, prime daemon socket, skill exposure.
    fn prepare(&self, ctx: &RuntimeContext, plan: &LaunchPlan, skills: &MaterializedSkills) -> anyhow::Result<Prepared>;
    /// Strips FORBIDDEN_ENV on the returned Command.
    fn build_command(&self, ctx: &RuntimeContext, plan: &LaunchPlan, prepared: &Prepared) -> anyhow::Result<std::process::Command>;
    /// Finds the session the agent minted, filtered by the canonical workdir.
    fn discover_session(&self, ctx: &RuntimeContext, workdir: &Path, since: DateTime<Utc>) -> anyhow::Result<Option<SessionId>>;
}
```

Capabilities per harness (from today's predicates in `teammates.rs:225-275`
and `valid_efforts` at `teammates.rs:81`):

| Harness | caller_minted_session | resume | effort | daemon | exec_policy | skill_exposure | tool_lists | tool_denylist | headless |
|---|---|---|---|---|---|---|---|---|---|
| claude | yes | yes | yes | no | no | PluginDir (`--plugin-dir` + settings overlay) | yes | yes | yes (B4) |
| codex | no | yes | yes | no | yes | CodexHome (`Rules::attach_skills`) | no | no | no |
| opencode | no | yes | yes | no | no | ConfigPaths (`OPENCODE_CONFIG_CONTENT.skills.paths`) | no | no | no |
| pi | yes | yes | yes | no | no | SkillFlag (`--skill <dir>`) | yes | yes | no |
| prime | no | yes | yes | yes | no | SkillFlag (`--skill <dir>`) | yes | no | no |
| none | no | no | no | no | no | None | no | no | no |

`arc_10_capabilities_match_legacy_predicates` checks the first 9 columns
against the A0 predicates.

### 4.7 `workspace::WorkspaceClient` (A7)

```rust
pub trait WorkspaceClient {
    fn pane_get(&self, pane: &PaneId) -> anyhow::Result<Pane>;
    fn pane_list(&self, workspace: &WorkspaceId) -> anyhow::Result<Vec<Pane>>;
    fn pane_split(&self, from: &PaneId, direction: Direction) -> anyhow::Result<PaneId>;
    fn pane_run(&self, pane: &PaneId, command: &str) -> anyhow::Result<()>;
    fn pane_close(&self, pane: &PaneId) -> anyhow::Result<()>;
    fn agent_prompt(&self, pane: &PaneId, text: &str) -> anyhow::Result<()>;
    fn send_text(&self, pane: &PaneId, text: &str) -> anyhow::Result<()>;
    fn send_keys(&self, pane: &PaneId, keys: &str) -> anyhow::Result<()>;
    fn pane_read(&self, pane: &PaneId, source: &str) -> anyhow::Result<String>;
    fn workspace_create(&self, label: &str, cwd: Option<&Path>, focus: bool) -> anyhow::Result<NewWorkspace>;
    fn workspace_close(&self, workspace: &WorkspaceId) -> anyhow::Result<()>;
}
pub struct HerdrClient { bin: PathBuf }               // wire DTOs private to workspace/herdr.rs
impl HerdrClient { pub fn new(bins: &HarnessBins) -> Self; }
impl WorkspaceClient for HerdrClient { /* … */ }

pub mod testing {
    #[derive(Default)]
    pub struct FakeWorkspace {
        pub calls: RefCell<Vec<String>>,               // "split p1 right", "run p2 …", …
        pub fail_split: Cell<bool>,
        pub fail_run: Cell<bool>,
        pub panes: RefCell<BTreeMap<PaneId, Pane>>,
    }
    impl WorkspaceClient for FakeWorkspace { /* … */ }
}
// workspace/model.rs: public Pane, Rect, Layout, LayoutPane, Tab, Workspace, Direction,
// FocusDir, NewTab, NewWorkspace (moved from herdr.rs:18-300, fields unchanged).
```

### 4.8 `messaging::brief` (A2 v2, A7 move)

```rust
/// v1: today's mailbox.rs Brief (no `schema` key). v2 adds schema, workdir, bins.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Brief {
    #[serde(default = "schema_v1")] pub schema: u32,          // 1 when absent; writers write 2
    pub role: String,
    pub teammate: String,
    pub agent: String,
    pub model: String,
    pub record_id: String,
    #[serde(default)] pub session_id: String,
    pub resume: bool,
    #[serde(default)] pub task: String,
    pub project_dir: String,
    #[serde(default)] pub state_dir: Option<String>,
    #[serde(default)] pub claude_bin: Option<String>,         // v1; v2 readers fold into bins
    #[serde(default)] pub codex_bin: Option<String>,          // v1
    #[serde(default)] pub resolved: Option<Teammate>,
    #[serde(default)] pub teammates_dir: Option<String>,
    // v2
    #[serde(default, skip_serializing_if = "Option::is_none")] pub workdir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub bins: Option<BinOverrides>,
}
impl Brief {
    pub fn agent(&self) -> anyhow::Result<HarnessKind>;
    /// v1 → claude_bin/codex_bin only; v2 → the full BinOverrides.
    pub fn bin_overrides(&self) -> BinOverrides;
    pub fn workdir_or_project(&self) -> &str;
}
/// The env a child needs to rebuild its RuntimeContext: HORCH_STATE_DIR,
/// HORCH_TEAMMATES_DIR, every HORCH_*_BIN in `bins`. Applied with `cmd.envs()`
/// to the child only. Replaces worker.rs:70 export_brief.
pub fn transport_env(brief: &Brief) -> Vec<(String, String)>;
```

v2 keeps `claude_bin` and `codex_bin` written too, so a v1 worker binary can
still read a v2 brief.

### 4.9 `skills` (A9)

```rust
pub use horch_marketplace::model::{SkillId as MarketSkillId, SkillVersion};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SkillVersion(pub String);   // "bundled+<digest12>" | "git+<commit12>" | "local+<digest12>"

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedSkillRef {
    pub id: SkillId,
    pub version: SkillVersion,
    pub digest: String,                // "sha256:<hex>"
    pub source: String,                // "bundled" | "git:<url>@<commit>" | "local:<path>"
    pub policy: InvocationPolicy,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvocationPolicy {
    Explicit,        // teammate `skills:` (expected)
    Deterministic,   // phase selector
    Available,       // in the catalog, not activated
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillActivationPlan {
    pub activated: Vec<ResolvedSkillRef>,   // Explicit + Deterministic, sorted by id
    pub available: Vec<ResolvedSkillRef>,
    pub plugin_skills: BTreeMap<String, Vec<String>>,  // external Claude plugins, kept separate (SKL-07)
}
pub struct SkillCatalog { /* bundled entries + lock entries */ }
impl SkillCatalog {
    pub fn bundled() -> anyhow::Result<SkillCatalog>;             // skills/ + provenance.json via build.rs
    pub fn with_lock(self, lock: &LockView) -> SkillCatalog;
    pub fn get(&self, id: &SkillId) -> Option<&CatalogEntry>;
}
/// Pure. No NLP: only names and phases decide.
pub fn plan_activation(teammate: &Teammate, phase: Option<Phase>, catalog: &SkillCatalog, lock: &LockView) -> anyhow::Result<SkillActivationPlan>;
// materialize.rs
pub struct MaterializedSkills { pub root: PathBuf /* state_root/skill-bundles/<execution_id>/ */ }
impl MaterializedSkills { pub fn materialize(paths: &Paths, id: &ExecutionId, plan: &SkillActivationPlan, catalog: &SkillCatalog) -> anyhow::Result<Option<Self>>; }
impl Drop for MaterializedSkills { /* removes only files it created */ }
```

`SkillId` in core reuses `ids::SkillId`; the marketplace keeps its own
validated `SkillId` (MKT-01 forbids the dependency the other way). The core
converts at the boundary.

### 4.10 Marketplace (A8)

```rust
// horch-marketplace/src/model.rs
pub struct SkillId(String);                 // SKILL.md name rules: [a-z0-9-], no leading/trailing '-', no "--"
pub struct SkillVersion(pub String);
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SkillSource {
    Bundled { name: String },
    Local { path: PathBuf },
    Git { url: String, revision: GitRevision, subdir: Option<String> },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum GitRevision { Branch(String), Tag(String), Commit(String) }
pub fn parse_source(spec: &str) -> Result<SkillSource, MarketplaceError>;
// "owner/repo[@rev]" → https://github.com/owner/repo; "https://…[@rev]"; "file://…"; absolute path.
// @rev of 40 hex → Commit; else tag, then branch, at resolve time.

// manifest.rs — the manifest key list (Spec A §10) is the table below the store layout.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillManifest {
    pub name: String,
    pub description: String,                // non-empty, ≤ 1024 bytes
    #[serde(default)] pub license: Option<String>,
    #[serde(default)] pub metadata: BTreeMap<String, String>,
    #[serde(default, rename = "allowed-tools")] pub allowed_tools: Option<serde_yaml::Value>,
}

// lockfile.rs: marketplace.lock = {"version":1,"skills":[LockEntry…]} sorted by id
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockEntry {
    pub id: String,
    pub source: String,
    pub requested_revision: Option<String>,
    pub resolved_commit: Option<String>,    // 40 hex for git; None for local/bundled
    pub version: String,
    pub digest: String,                     // "sha256:<hex>"
    pub installed_at: String,               // RFC 3339
}

// integrity.rs
pub const MAX_FILES: usize = 512;
pub const MAX_FILE_BYTES: u64 = 1 << 20;    // 1 MiB
pub const MAX_TOTAL_BYTES: u64 = 8 << 20;   // 8 MiB
/// sha256 over sorted "<relative path>\0<sha256 hex of bytes>\n".
pub fn tree_digest(dir: &Path) -> Result<String, MarketplaceError>;

// git.rs
pub struct GitRunner { bin: PathBuf }
pub struct GitOutput { pub status: i32, pub stdout: Vec<u8>, pub stderr: Vec<u8> }
impl GitRunner {
    pub fn new(bin: PathBuf) -> GitRunner;
    /// Always: GIT_TERMINAL_PROMPT=0, LC_ALL=C, GIT_CONFIG_NOSYSTEM=1,
    /// -c core.hooksPath=/dev/null, ANTHROPIC_API_KEY removed.
    pub fn run(&self, dir: &Path, args: &[&str]) -> Result<GitOutput, GitError>;
}

// installer.rs
pub enum Step { Catalog, Resolve, Fetch, Validate, Verify, Materialize, Lock }
pub enum FaultPoint { AbortAfterMaterializeBeforeLock }
#[derive(Default)] pub struct InstallOptions { pub fault: Option<FaultPoint> }
pub struct Installer { store: Store, git: GitRunner }
impl Installer {
    pub fn new(store_root: PathBuf, git: GitRunner) -> Installer;
    pub fn install(&self, source: &SkillSource, opts: &InstallOptions, observer: Option<&mut Vec<Step>>) -> Result<LockEntry, MarketplaceError>;
    pub fn update(&self, id: Option<&str>) -> Result<Vec<LockEntry>, MarketplaceError>;
    pub fn reinstall_from_lock(&self) -> Result<Vec<LockEntry>, MarketplaceError>;
}
pub enum MarketplaceError {
    BadSource(String), Credentials(String), Git(GitError), Manifest(String), SkillMd(String),
    Traversal(PathBuf), AbsolutePath(PathBuf), Symlink(PathBuf), TooManyFiles(usize),
    FileTooLarge { path: PathBuf, bytes: u64 }, TotalTooLarge(u64), DigestMismatch { expected: String, actual: String },
    Io(std::io::Error), Fault(&'static str),
}
```

Store layout under `${XDG_DATA_HOME:-~/.local/share}/horch/`:

```
staging/<random hex>/          temporary; removed on any error
skills/<id>/<version>/         materialized, immutable
marketplace.lock               atomic replace
```

Install pipeline:

| Step | Action | Failure |
|---|---|---|
| 1 catalog | look up the source spec | `BadSource` |
| 2 resolve | branch/tag → full 40-hex SHA (`git ls-remote`); a commit is verified by fetch | `Git` |
| 3 fetch | bare clone into `staging/<id>`, hooks off, `GIT_TERMINAL_PROMPT=0`; check out the tree | `Git` |
| 4 validate | `SkillManifest` (deny unknown keys, so `hooks:` fails) + SKILL.md rules from `skills.rs:39-73` | `Manifest`, `SkillMd` |
| 5 verify | integrity walk + tree digest | `Traversal`, `AbsolutePath`, `Symlink`, `TooManyFiles`, `FileTooLarge`, `TotalTooLarge` |
| 6 materialize | atomic rename into `skills/<id>/<version>/` | `Io` |
| 7 lock | atomic replace of `marketplace.lock` | `Io` |

Rejection rules: absolute paths, `..` components, symlinks, over 512 files,
a file over 1 MiB, total over 8 MiB, unknown manifest keys (so no hooks), and
credentials in a URL (userinfo, or a `token=` query).

**The SKILL.md manifest keys (Spec A §10).** The frontmatter is the YAML
between a leading `---` line and the next `---` line (CRLF reads as LF). It
uses only these 5 keys:

| Key | Required | Type | Rule |
|---|---|---|---|
| `name` | yes | string | `[a-z0-9-]`, not empty, no leading or trailing `-`, no `--`; equals the skill directory name when that is known |
| `description` | yes | string | not empty after trim; at most 1024 bytes (`MAX_DESCRIPTION_BYTES`) |
| `license` | no | string | free text, not checked |
| `metadata` | no | map of string to string | free text, not checked |
| `allowed-tools` | no | any YAML (a string or a list in practice) | kept as YAML, not checked |

- Any other key is a `Manifest` error (`deny_unknown_fields`). `hooks` is the
  key this rule exists for: a skill never brings hooks.
- A missing frontmatter, bad YAML or an unknown key is `Manifest`. A name or
  description rule is `SkillMd`.
- `SKILL.md` must be a regular UTF-8 file; a symlinked `SKILL.md` is
  `Manifest` before it is read.
- Code: `crates/horch-marketplace/src/manifest.rs:SkillManifest::parse`,
  `SkillManifest::read`, `model.rs:is_valid_skill_name`. Tests:
  `manifest.rs:tests::accepts_known_keys`, `tests::rejects_bad_frontmatter`,
  `tests/marketplace.rs:mkt_06_rejects_hooks`,
  `mkt_06_rejects_invalid_skill_md`.

---

## 5. Phases

Each phase is a commit series on `arch-refactor-dataset`. Every commit passes
`just gate`. "Do not change" lists files and behaviors the phase must leave
byte-identical.

### A0 Baseline

| | |
|---|---|
| Created | `scripts/phase-gate.sh`, `justfile` recipe `gate`, `ai_docs/gates/architecture-refactor/{BASELINE.md,CHECKLIST.md,CURRENT_PHASE,SPEC-COVERAGE.md}`, both design docs, `crates/horch-core/tests/baseline_oracles.rs` + `tests/oracles/**`, `crates/horch/tests/baseline_cli.rs` + `tests/oracles/**`, telemetry render goldens (`crates/horch/tests/golden/telemetry-*.txt`, `crates/horch-e2e/tests/golden/telemetry-e2e.txt`, blessed once in a separate commit) |
| Changed | `scripts/check-req-coverage.sh` (scans all designs, Phase column, `--through`, `--phase`, duplicate check), `scripts/check-deps.sh` and `nfr.rs` (sha2 allowlist) |
| Moved | none |
| Shims | none |
| Oracles | argv/env per teammate × {fresh, resume, unmanaged}; routing decisions: 7 quota fixtures × teammates × {∅, exact, force} × {auto, advise, off}; legacy ledgers (bash-era, pre-effort, pr14-substituted, orchestrator); skills briefings and `horch skills --json`; `sessions` render |
| Tests | `arc_01_baseline_oracles_present`, `nfr_08_phase_gate_runs_every_check`, `nfr_09_no_async_runtime_deps` |
| Gate | the 7 commands of conventions §4; `just gate` prints `GATE GREEN` |
| Do not change | `crates/*/src/**`; `crates/horch-core/tests/golden/**` (golden prompts) |

### A1 Vocabulary and newtypes

| | |
|---|---|
| Created | `horch-core/src/ids.rs`, `harness/mod.rs` (HarnessKind only), `execution/mod.rs`, `execution/model.rs` |
| Moved | `teammates.rs:200-350` `enum Agent` + impl → `harness/mod.rs` `HarnessKind` |
| Shims | `teammates.rs`: `pub use crate::harness::HarnessKind as Agent;` (call sites change `Agent::Opencode` → `Agent::OpenCode`) |
| Changed | every `no_tile: bool` → `tiling: TilingMode`; every `resume: bool` → `SessionMode` (or a two-variant enum); clap flags unchanged |
| Tests | `arc_02_ids_validate`, `arc_02_mint_v7_layout`, `arc_02_legacy_ids_accepted`, `arc_03_harness_kind_serde_compat`, `arc_04_status_legacy_roundtrip`, `arc_04_tiling_mode` |
| Gate | `just gate`; no user-visible change; `grep -rn 'no_tile: bool\|resume: bool' crates` is empty. Review rule: no new raw string IDs |
| Do not change | `ledger::Record` serde; teammate frontmatter syntax; CLI flags; golden prompts |

### A2 RuntimeContext

| | |
|---|---|
| Created | `runtime/{mod,context,paths,bins,process,fault}.rs`; `crates/horch/src/{lib,bootstrap}.rs` |
| Moved | `ledger.rs:177-205` (`state_root`, `project_dir`, `home_dir`) → `runtime/paths.rs`; `agent.rs:17-80` resolvers → `runtime/bins.rs` (+ `HORCH_GIT_BIN`); `agent.rs:85-120` `which`/`which_in` and the setsid code of `tilecmd.rs:628` (`settle_after_close`, setsid at :647-655) → `runtime/process.rs`; `HORCH_FAULT` reads (`telemetry/collect.rs:204-205`) → `runtime/fault.rs` |
| Ambient env removed from | ledger, mailbox (`register` no longer calls `set_var`), policy (`HORCH_BALANCE`), quota (`HORCH_QUOTA_FILE`, probe timeout, ProbeBins), clock, launch/skills (`OPENCODE_CONFIG_CONTENT`), teammates operator settings, `usage::Locations`, codex (`CODEX_HOME`), telemetry (`HORCH_FAULT`) |
| Replaced | `launch::apply_env` (`launch.rs:33`) → `cmd.envs()`; `worker.rs:70 export_brief` → `messaging::brief::transport_env()` on the child only |
| Shims | `ledger::{state_root, project_dir}` and `agent::*_bin` keep their names as thin wrappers over `ProcessEnv` until their last caller moves (removed in A12) |
| New format | Brief v2 (`schema: 2`, `workdir`, full `BinOverrides`); v1 stays readable |
| Tests | `arc_05_no_ambient_env_in_core`, `arc_05_context_from_map_env`, `arc_06_transport_env_applied_to_child`, `arc_06_env_mutation_sites_reduced` (fewer than the 44 baseline sites), `arc_07_brief_v1_readable`, `arc_07_e2e_bin_overrides_reach_worker` |
| Gate | `just gate` + `arc_07_e2e` |
| Do not change | CLI output; ledger bytes; golden prompts |

### A3 Roster split

| | |
|---|---|
| Created | `roster/{mod,teammate,phase,effort,permission,parser,repository,validation,operator}.rs` |
| Moved | `teammates.rs` (1923 lines): `Teammate`/`Base`/`ExecRule` DTOs (`:423-610`, still `deny_unknown_fields`) → `teammate`; `Phase` (`:158-198`) → `phase`; `valid_efforts`/`model_takes_effort`/`effort_problem` (`:81-150`) + an `Effort` newtype → `effort`; `PermissionMode` (`:353-420`) → `permission`; frontmatter parsing → `parser`; `Roster::{builtin,load,load_with,overlay}` (`:634-720`) + the `builtin_teammates.rs` include → `repository`; `check_teammate`/`check` (`:822-1185`) + `balance_policy.rs:306-370` (`fallback_problems`, `fallback_warnings`) → `validation`; `operator_*` (`:1186-1320`) → `operator` |
| Shims | `teammates.rs`: `pub use crate::roster::*;` plus the `Agent` alias; `balance_policy::{fallback_problems, fallback_warnings}` re-exported |
| Tests | existing tests move unchanged; `arc_08_unknown_field_rejected`, `arc_08_overlay_precedence`, `arc_08_legacy_frontmatter_corpus_parses` (every PR #14 teammate + a pre-#12 fixture without `fallbacks`) |
| Gate | `just gate`; `teammates --check` passes; golden prompts byte-identical; `nfr_03` passes |
| Do not change | teammate files; roster check messages; golden prompts |

### A4 Harness ownership

| | |
|---|---|
| Created | `harness/{capabilities,launch,claude,claude_plugins,codex,opencode,pi,prime,none}.rs` |
| Moved | `launch.rs` builders: `claude_command` (`:272`), `overlay_skill_switches` (`:408`), `overlay_plugin_skills` (`:469`) → `harness/claude.rs`; `codex_command` (`:496`) + `codex.rs` → `harness/codex.rs`; `opencode_command` (`:120`), `opencode_variant_config` (`:174`) + `opencode.rs` → `harness/opencode.rs`; `pi_family_command` (`:199`) → `harness/pi.rs` + `prime.rs`; `plugins.rs` → `harness/claude_plugins.rs` |
| Unified | `worker.rs launch_agent` (`:103-192`) and `recipes.rs pane_launch` (`:509-638`) → one flow in `harness/launch.rs`: prepare → build → [discovery thread unless `caller_minted_session`] → wait. Removes the `mints_session_id`, `harvests_session_id`, `runs_a_daemon`, `uses_execpolicy` branches |
| Shims | `launch.rs`: `pub use crate::harness::launch::{command, command_with_skills, Session, FORBIDDEN_ENV};`; `codex.rs`, `opencode.rs`, `prime.rs`, `plugins.rs`: `pub use crate::harness::<x>::*;` |
| Tests | `arc_09_argv_matches_baseline`, `arc_10_capabilities_match_legacy_predicates`, `arc_10_harness_match_only_in_harness`, `arc_11_codex_discovery_by_workdir`, `arc_11_opencode_discovery_by_workdir`, `arc_11_canonical_tmp_paths` |
| Gate | `just gate`; argv oracle byte-identical |
| Do not change | argv/env of every teammate (the A0 oracle); FORBIDDEN_ENV stripping |

### A5 Routing consolidation and provenance

| | |
|---|---|
| Created | `routing/{mod,policy,quota,quota_probe,snapshot,balance,eligible,decision}.rs` |
| Moved | `policy.rs` → `routing/policy.rs`; `quota.rs` pure part (QuotaView, Assessment, State, pools) → `routing/quota.rs`; `quota.rs` Child (`:618`), probes, `harness_version` → `routing/quota_probe.rs`; `current_view` → `routing/snapshot::obtain` (the only probing path); `balance_policy.rs` → `routing/{balance,decision,eligible}` (private `candidates()` becomes a filter over `eligible_fallbacks`, same order, same drop rules) |
| Shims | `policy.rs`, `quota.rs`, `balance_policy.rs`: `pub use crate::routing::<x>::*;`; `Decision` = alias of `DecisionDto` for JSON callers |
| Ledger | records gain `routing` (RoutingProvenance); `via` and `substitution_reason` are still written |
| Tests | `arc_12_decisions_match_baseline`, `arc_13_exclusion_reasons` (one test per reason), `arc_13_candidates_equivalent`, `arc_14_provenance_on_spawn_substitute_resume` (e2e) |
| Gate | `just gate`; BAL-01..09 and QUO-07 unchanged |
| Do not change | `route`/`spawn` CLI semantics: exit 3, `--force`, `--exact`, balance modes; `Decision` JSON bytes |

### A6 Execution planning, service, store and lifecycle

Five commits:

| Commit | Created / moved | Fault points |
|---|---|---|
| 1 Store | `execution/{legacy,store}.rs`; `fsx::DirLock` (generalizes the ledger mkdir lock and `telemetry/lock.rs`); writes = temp + fsync + rename + dir fsync; same `<state_root>/<slug>.json`; `ledger.rs` becomes a facade | — |
| 2 Pure plan | `execution/plan.rs` from `cmd/spawn.rs:76-262`; role allocation stays impure, moves under `DirLock`; briefs written atomically | — |
| 3 Service | `execution/service.rs`: insert(Planned) → brief → split (error: `LaunchFailed{Split}` + unregister) → run (error: `LaunchFailed{Run}`, close pane, unregister) → `mark_starting(pane)` → tile best effort | `abort-after-execution-insert`, `abort-after-brief`, `abort-after-pane-split`, `fail-pane-split`, `fail-run` |
| 4 Worker lifecycle | `execution/lifecycle::run_worker` (Spec A §8 order); records `agent_exited(code)` (Running → Failed(AgentExited)); propagates the exit code; `cmd/spawn.rs` → about 80 lines (from 471); `cmd/worker.rs` → about 30 lines (from 351); `main` maps `SpawnError::Refused` → exit 3 | — |
| 5 Telemetry and presentation | `telemetry/collect.rs:29,212,423` read through the store; live rows use `is_live()`; `cmd/cost.rs`, `cmd/usagecmd.rs`, `cmd/ledgercmd.rs` (`sessions`) render from the store; `cmd/route.rs`, `cmd/quotacmd.rs` render `routing::*`; tile/balance commands call `workspace::arrange` (after A7) | — |

| | |
|---|---|
| Fakes | fake-herdr: `fail_split`, `fail_run`, split adds the pane to state; new `fake-opencode`, `fake-prime` (unit U07) |
| Shims | `ledger.rs` facade over `ExecutionStore` (`Ledger`, `Record` = `LedgerRecordV1`) until A12 |
| Tests | `arc_15_plan_deterministic`, `arc_15_plan_table`, `arc_16_split_failure_launch_failed`, `arc_16_run_failure_closes_pane`, `arc_16_e2e_fail_split`, `arc_16_crash_after_insert_not_live`, `arc_17_legacy_ledgers_load_and_resume`, `arc_17_roundtrip_byte_identical`, `arc_17_old_reader_sees_compat_status`, `arc_18_worker_startup_order`, `arc_18_agent_exit_recorded`, `arc_22_no_error_string_matching`, `arc_23_telemetry_reads_executions`, `arc_26_e2e_lifecycle_matrix_{claude,codex,opencode,pi,prime}` |
| Gate | `just gate` + bal_04/05/06, arc_16_e2e, arc_26 matrix; no failed launch leaves a live-looking record |
| Do not change | ledger file path and bytes for records without new fields; `horch sessions` text (risk 5: typed statuses need a sanctioned golden block, decided here); `horch cost` totals |

### A7 Workspace and messaging boundaries

| | |
|---|---|
| Created | `workspace/{mod,client,model,arrange}.rs`, `messaging/{mod,delivery}.rs` |
| Moved | `herdr.rs` → `workspace/herdr.rs` (wire DTOs private) + `workspace/model.rs` (public types); `layout.rs`, `tile.rs`, `balance.rs`, `paneshell.rs` → `workspace/` byte-for-byte apart from imports; `cmd/tilecmd.rs` orchestration → `workspace/arrange.rs`; `mailbox.rs` → `messaging/mailbox.rs`; `Brief` → `messaging/brief.rs`; `message.rs` → `messaging/message.rs`; `herdr.rs:704 send_line`, `wait_for_tail`, `squash`, `tail_needle` → `messaging/delivery.rs` over `WorkspaceClient` |
| Changed | `done` → `lifecycle::done`: mark done → report (skipped when `report_to: None`) → unregister → settle → close |
| Shims | `herdr.rs`, `layout.rs`, `tile.rs`, `balance.rs`, `paneshell.rs`, `mailbox.rs`, `message.rs`: `pub use crate::<new path>::*;` |
| Tests | moved tile/balance/layout/herdr tests unchanged (`arc_19_herdr_parsing_contract`), `arc_20_send_line_prompt_first`, `arc_20_fallback_waits_tail`, `arc_21_done_order`, `arc_27_tile_balance_pure` |
| Gate | `just gate`; `horch tile --plan` output unchanged on fixtures; message race tests unchanged |
| Do not change | the tiler algorithm; delivery timing and text |

### A8 Marketplace core

| | |
|---|---|
| Created | `crates/horch-marketplace/` (section 4.10); root `Cargo.toml` member + workspace dep |
| Moved | none (SKILL.md rules from `skills.rs:39-73` are copied, not imported) |
| Shims | none |
| Tests | `mkt_01_no_core_dependency`, `mkt_02_branch_resolves_to_sha`, `mkt_03_lifecycle_order`, `mkt_04_lock_entry_fields`, `mkt_05_install_is_transactional`, `mkt_06_rejects_{traversal,symlink,oversize,invalid_skill_md,hooks}`, `mkt_07_rejects_credentials_in_url`, `mkt_08_offline_reinstall_from_lock`, `mkt_10_source_dispatch_localized`, `nfr_06_dependency_allowlist`, `nfr_07_git_only_on_temp_repos` |
| Gate | `just gate` with `HORCH_REQUIRE_GIT=1`; `check-deps.sh` passes with the crate |
| Do not change | `horch-core` (no dependency on the marketplace yet) |

### A9 Bundled skills become the catalog

| | |
|---|---|
| Created | `skills/{mod,catalog,selection,activation,briefing,materialize}.rs` |
| Moved | `skills.rs` `phase_skills` (`:23`), `selected` (`:75`) → `selection.rs`; `catalog()` (`:39-73`) → `catalog.rs`; `Bundle::install` (`:140`) → `materialize.rs`; `Bundle::briefing` (`:257`) → `briefing.rs` |
| Changed | `build.rs` also compiles in `skills/provenance.json` (pins `MattMakes/skill-marketplace@d476703`, per-skill sha256); bundled versions are `bundled+<digest12>`; lock entries merge in; executions record `skills: [ResolvedSkillRef]`; materialize to `state_root/skill-bundles/<execution_id>/` |
| Shims | `skills.rs` → `skills/mod.rs` keeps `Bundle` as a type alias over `MaterializedSkills` until A12 |
| Tests | `skl_01_bundled_catalog_versions_and_digests`, `skl_02_activation_matches_legacy_selection` (every teammate × phase), `skl_03_policy_mapping`, `skl_04_execution_records_skill_refs`, `skl_07_plugin_skills_separate`, `skl_08_briefing_matches_baseline_modulo_path` |
| Gate | `just gate`; `horch skills --json` equals the A0 oracle |
| Do not change | briefing text; external Claude `plugin_skills` handling |

### A10 Harness skill exposure

| | |
|---|---|
| Changed | each adapter's `prepare`: Claude `--plugin-dir` + settings overlay; Pi and Prime `--skill <dir>`; OpenCode `OPENCODE_CONFIG_CONTENT.skills.paths`; Codex private `CODEX_HOME` via `Rules::attach_skills` |
| Fakes | `inspect_skills` scenario in every fake harness |
| Shims | none new |
| Tests | `skl_05_core_skill_model_has_no_harness_flags`, `skl_06_e2e_exposure_{claude,codex,opencode,pi,prime}` (activated exposed, non-activated hidden) |
| Gate | `just gate` + skl_06 |
| Do not change | argv oracle for teammates with no skills |

### A11 Marketplace CLI and just

| | |
|---|---|
| Created | `crates/horch/src/cmd/{skillscmd,marketplacecmd}.rs`; just recipes `skills`, `skills-install`, `skills-update`, `skills-doctor`, `marketplace-refresh` |
| Commands | `horch skills [list] [--phase] [--json]` (legacy output exact), `show <id>`, `install <source>[@rev] [--path]`, `update [<id>]`, `doctor`; `horch marketplace list\|refresh` |
| Tests | `mkt_09_legacy_skills_flags_output_unchanged`, `mkt_09_cli_list_show_json`, `mkt_08_e2e_install_update_repeatable`, `mkt_08_runtime_needs_no_network` (spawn with `HORCH_GIT_BIN=/nonexistent` after install) |
| Gate | `just gate` + mkt_08_e2e |
| Do not change | `horch skills` and `horch skills --json` output (A0 golden) |

### A12 Cleanup

| | |
|---|---|
| Deleted | every re-export shim listed in A1 to A11; the `ledger.rs` facade |
| Kept | `LedgerRecordV1`, Brief v1 defaults, the `tier` key |
| Changed | `#![warn(unreachable_pub)]`; `pub(crate)` default; lib.rs module table; README architecture section; design-doc statuses |
| Tests | `arc_25_no_shim_modules` |
| Gate | full gate + every per-phase e2e |
| Do not change | on-disk formats |

---

## 6. Compatibility

| Artifact | Rule | Test |
|---|---|---|
| Ledgers | `LedgerRecordV1` reads every legacy shape (bash-era, pre-effort, pr14-substituted, orchestrator). New keys are optional and skip-if-empty. `status` stays `working`/`done`; typed status goes in `state`. The `tier` key stays. Same path `<state_root>/<slug>.json`. | `arc_17_*` |
| Briefs | v1 (no `schema`) reads as `schema: 1`; `claude_bin`/`codex_bin` fold into `BinOverrides`. v2 writers keep the v1 keys. | `arc_07_brief_v1_readable` |
| Frontmatter | `deny_unknown_fields` stays. Every PR #14 teammate and a pre-#12 file without `fallbacks` parse. `agent: opencode` stays the spelling. | `arc_08_legacy_frontmatter_corpus_parses`, `arc_03_harness_kind_serde_compat` |
| Offline skills | bundled skills are compiled in and versioned `bundled+<digest12>`; a locked git skill runs with no network once materialized. | `skl_01_*`, `mkt_08_offline_reinstall_from_lock`, `mkt_08_runtime_needs_no_network` |
| Decision JSON | `DecisionDto` renders today's `Decision` bytes. | `arc_12_decisions_match_baseline` |
| CLI | `route`/`spawn` exit codes and flags; `horch skills` output. | `bal_04`, `mkt_09_legacy_skills_flags_output_unchanged` |

`SPEC-TODO(Spec A §13)`: the compatibility list verbatim.

---

## 7. Fault points (`HORCH_FAULT`)

`runtime::fault::Faults` parses `HORCH_FAULT` once in bootstrap. `abort-*`
points exit the process at that point. `fail-*` points make the step return
its error. After each fault, `resume` must leave no phantom-live record.

| Point | Where | Expected after the fault |
|---|---|---|
| `abort-after-execution-insert` | service, after insert(Planned) | the record is not live (`arc_16_crash_after_insert_not_live`) |
| `abort-after-brief` | service, after the brief write | the record is not live; the role is free after unregister on next spawn |
| `abort-after-pane-split` | service, after split | the record is not live; the pane is closed by the next `horch tile`/cleanup |
| `fail-pane-split` | service split | `LaunchFailed{Split}`, role unregistered (`arc_16_split_failure_launch_failed`, `arc_16_e2e_fail_split`) |
| `fail-run` | service run | `LaunchFailed{Run}`, pane closed, role unregistered (`arc_16_run_failure_closes_pane`) |
| `abort-after-materialize-before-lock` | marketplace installer | no lock entry; staging empty; reinstall succeeds (`mkt_05_install_is_transactional`) |
| `abort-after-append`, `after-append` | telemetry collect (existing) | unchanged |

The master plan spells the spawn points `abort-after-execution-insert|brief|pane-split`
and `fail-pane-split|run`. This design expands them to full names as above.
The dataset fault points are in the dataset design §7.

---

## Appendix A: Spec A (verbatim)

PENDING: the orchestrator inserts the operator's Spec A text here.
