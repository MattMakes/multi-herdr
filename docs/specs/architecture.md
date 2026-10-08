# Design: the architecture refactor (Spec A, phases A0 to A12)

| | |
|---|---|
| Status | Implemented. Phases A0 to A12 landed. |
| Date | 2026-10-02 |
| Author | The fleet (unit `a0-designs`), from the operator's master plan |
| Companion | [`dataset-competition.md`](dataset-competition.md) (Spec B, phases B1 to B6) |
| Base | branch `arch-refactor-dataset`, off `combine-open-prs` @ `575c2c2` |

Requirement tables in section 3 are machine-read by
`scripts/check-req-coverage.sh`. Do not change their header row. Do not
define an ID in any other table. The operator's original Spec A prose is
not kept (Appendix A). The finish run (2026-10-04) closed every spec-todo
marker: each such place now states the implemented, tested behaviour as the
spec and cites its code and test. Wave 2 (2026-10-06) replaced the last 2
`PENDING:` appendix placeholders. The gate step `no_spec_todo` fails on
either marker in a tracked doc.

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
| OD4 | Clef and Laya are **inert** in this branch. Both speak the System One API: `POST /v1/systemone` with typed `choice`/`score`/`noul` questions, returning a probability per option. Clef is Cloudflare's 27B/9B decision model; Laya is Convai's 421M local model, fine-tuned on `{state, questions, answers}`. We ship the seam, record eligible sets, planner propensities and `teacher: none`, export System-One/Laya-shaped rows, and add a **data-readiness report**. There is no HTTP client and no API keys. Clef is enabled only once readiness says the local data justifies it, and Laya later still. |
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
- Every phase follows the junior checklist (Spec A §16). A commit body quotes
  the lines it checked, unchanged, and a line that does not apply gets
  `(n/a: <reason>)`. The Spec A §16 checklist items are:
  1. `just gate` is green on this commit
     (`HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate` prints
     `GATE GREEN`, `scripts/phase-gate.sh`).
  2. Every ID of this phase has a test
     (`scripts/check-req-coverage.sh --phase <P>` exits 0).
  3. Every new test name starts with its lowercase requirement ID
     (`ARC-02` → `arc_02_...`).
  4. No golden prompt changed; any prose change is a named sanctioned block.
  5. No serialization golden re-blessed; a format change bumped its schema
     version.
  6. No oracle under `crates/*/tests/oracles/` regenerated. This item
     applied during phases A0 to A12. After A12, a plan that changes a
     teammate's launch or skills re-blesses the oracle files it names, in
     their own commit, as `docs/testing-and-gates.md` ("HORCH_BLESS") says.
  7. No existing test changed to make it pass, unless the unit plan says so.
  8. No new crate outside the allowlist (`scripts/check-deps.sh` and
     `nfr_05`, `nfr_06`, `nfr_09`, `nfr_11` pass).
  9. No new `std::env` read outside `runtime/` and the binary's bootstrap
     (`arc_05_no_ambient_env_in_core`).
  10. No `ANTHROPIC_API_KEY` reaches a child process (`FORBIDDEN_ENV`
      strips it).
  11. Tests are hermetic: no network, no real harness binary, no herdr
      server, no file outside a temp dir; real `git` only on temp repos.
  12. No re-export shim module (`arc_25_no_shim_modules`). Phases A1 to A11
      kept a shim for every moved module; A12 removed them all.
  13. Old ledgers, old briefs and old teammate frontmatter still load.
  14. New `pub mod` lines in `crates/horch-core/src/lib.rs` are in
      alphabetical order; no other line changed.
  15. The diff was re-read adversarially; the unit report lists gotchas.

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
| Pane | — | herdr pane id | `PaneId` in records and plans, `&str` at the workspace port (§4.7); new optional `pane_id` on records |
| Skill | candidate features | `skills.rs` Bundle | `SkillId`, `SkillVersion`, `ResolvedSkillRef` |
| Competition | **Round** | — | `competition::Round` (`RoundId`); an Experiment groups rounds |
| Candidate | Candidate | — | a planned `SpawnRequest` plus an anonymous label |
| Evaluation | Judgment | — | `evaluation::Judgment`, a versioned label |
| RoutingProvenance | eligible set / teacher | `via`, `substitution_reason` | `routing::{RoutingProvenance, EligibleEntry}`, `teacher::*` |
| worktree isolation | WorktreeManager | — | `vcs::worktree`, as execution infrastructure (`SpawnRequest.workdir`) |

Roster types added after this refactor: `roster::Requirement` (a host tool
such as `godot`), `Teammate.requires` (the requirements `horch doctor`
checks) and `Teammate.skills_when` (skills a project file pattern adds). See
[`godot.md`](godot.md) (GDW-01 to GDW-03).

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
    heartbeat.rs                  the liveness scheme of both background jobs (judge, compaction): beat, read, liveness
    compaction/{window,policy,job,jobfile}.rs   context policy (W1): window, policy pure; job through ports; jobfile file I/O
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
docs/specs/architecture.md          (ARC, MKT, SKL)
docs/specs/dataset-competition.md   (MEA, PRE, CMP, JDG, PRO, EXP, SEC, NFR-06..11)
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

Spec A §3 states these module rules. The list is the master plan's list of
8 rules plus 6 rules that phases A4 to B1 added. Every rule is a
source-scan test. A scan reads `.rs` files as text and fails with the file
and line of each violation. Unless the row says otherwise, a scan skips
comment lines and `#[cfg(test)]` modules (`tests/arch_scan.rs:code_lines`).
Every scan in this table fails on a planted violation.

| Rule | Enforced by (test) | Phase | What the scan checks |
|---|---|---|---|
| roster never calls Herdr | `arc_08_roster_never_calls_herdr` (`horch-core/src/roster/tests.rs`) | A3 | `roster/*.rs` except `tests.rs` (at least 9 files, whole text) has no `herdr`, `Herdr`, `std::process`, `std::env::var` |
| routing never launches | `arc_13_routing_never_launches` (`horch-core/tests/routing.rs`) | A5 | the 6 files of `routing/` except `quota_probe.rs` and `snapshot.rs`, before `#[cfg(test)]`, have no `std::process`, `Command::new`, `std::fs`, `herdr`, `launch::`, `std::env::var` |
| planning is pure | `arc_15_plan_is_pure` (`horch-core/src/execution/plan.rs`) | A6 | `execution/plan.rs` before `#[cfg(test)]` has no `std::fs`, `std::process`, `Command::new`, `std::env`, `workspace::`, `herdr`, `Utc::now`, `clock::now`; the shell passes `now` in `PlanInputs` |
| telemetry observes and does not route | `arc_23_telemetry_never_routes` (`horch-core/tests/arch_scan.rs`) | A6 | `telemetry/**` has no `routing::balance`, `routing::decision`, `routing::eligible`, `RoutingDecision`, `decide(`; it may read `routing::{quota, policy, quota_probe, snapshot}` |
| the marketplace knows nothing about teammates | `mkt_01_no_core_dependency` (`horch-marketplace/tests/marketplace.rs`) | A8 | `horch-marketplace/Cargo.toml` has no `horch-core`; no file in `src/` contains `horch_core` or `teammate` (case-insensitive, whole text) |
| the CLI holds no policy | `arc_22_no_error_string_matching` and `arc_10_harness_match_only_in_harness` (`horch-core/tests/arch_scan.rs`) | A6 / A12 | `horch` and `horch-core` do not match on error text (`.to_string().contains(`, `format!("{…}").contains(`, `.contains("error`; 1 allowed entry, `telemetry/readers.rs` `FreeUsageLimitError`), and `horch/src/**` has no `HarnessKind::`/`Agent::` match arm |
| prompts hold no execution policy | `arc_10_prompts_hold_no_execution_policy` (`horch-core/tests/arch_scan.rs`) | A6 | `prompts.rs` has no `HarnessKind`, `BalanceMode`, `ExecutionStatus`, `routing::` |
| domain code never calls `std::env` | `arc_05_no_ambient_env_in_core` (`horch-core/tests/arch_scan.rs`) | A2 | `horch-core/src/**` outside `runtime/` has no `std::env::var`, `env::var_os`, `set_var`, `remove_var`, `std::env::current_dir`, `std::env::temp_dir` (`std::env::consts` is allowed) |
| harness match only in harness | `arc_10_harness_match_only_in_harness` (`horch-core/tests/arch_scan.rs`) | A4 | `HarnessKind::`/`Agent::` match arms and `matches!` appear only in `horch-core/src/harness/**` and `roster/validation.rs`; a `==`/`!=` comparison is not a match |
| skills core has no harness flags | `skl_05_core_skill_model_has_no_harness_flags` (`horch-core/tests/arch_scan.rs`) | A10 | `skills.rs` and `skills/**` (whole text, tests and comments included) have no `HarnessKind`, `Agent::`, `--plugin-dir`, `--skill`, `OPENCODE_CONFIG_CONTENT`, `CODEX_HOME` |
| source dispatch localized | `mkt_10_source_dispatch_localized` (`horch-marketplace/tests/marketplace.rs`) | A8 | `SkillSource::`/`ResolvedOrigin::` patterns (and `Self::{Bundled,Local,Git}` in `model.rs`) appear only in `resolver.rs` and `source.rs` |
| tile/balance pure | `arc_27_tile_balance_pure` (`horch-core/src/workspace/mod.rs`) | A7 | `workspace/{layout,tile,balance}.rs` have no `std::process`, `Command::new`, `Herdr`, `WorkspaceClient`, `crate::workspace::herdr`, `std::env` |
| competition domain free of adapters | `cmp_02_domain_has_no_adapter_imports` (`horch-core/tests/measure.rs`), `cmp_02_planner_files_have_no_adapter_imports` (`horch-core/tests/competition_planner.rs`), `cmp_02_evaluation_domain_has_no_adapter_imports` (`horch-core/tests/arch_scan.rs`) | B1 / B3 / B4 | `competition/{model,state,planner,diversity,budget}.rs`, `measure/{event,projection,worker_run}.rs` and `evaluation/{winner,parser}.rs` have no `std::process`, `herdr`, `workspace::`, `launch::`, `vcs::`, `std::env`, `std::fs` (and no `Command`, except in the planner files) |
| shims removed | `arc_25_no_shim_modules` (`horch-core/tests/arch_scan.rs`) | A12 | none of the 18 removed shim files exists; in the 4 crates no file only re-exports, and every `pub use` names the file's own child module (1 allowed entry: `skills/catalog.rs` re-exports `horch_marketplace as marketplace`) |
| compaction rules are pure | `ctx_07_compaction_pure_modules_do_no_io` (`horch-core/tests/arch_scan.rs`) | CTX | `compaction/{window,policy}.rs` have no `std::fs`, `std::process`, `std::env`, `Command`, `herdr`, `workspace::`, `launch::` |

The CLI rule does not forbid a command from calling a pure core function:
`horch route` calls `routing::decision::decide` to print the decision that
`horch spawn` would make (BAL-06), and the rule is in core. The rule
forbids policy written in the CLI: error-text matching and harness
dispatch.

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
| ARC-08 | Roster split, strict serde | A3 | arc_08_unknown_field_rejected, arc_08_roster_never_calls_herdr, arc_08_overlay_precedence, arc_08_legacy_frontmatter_corpus_parses, nfr_03 |
| ARC-09 | Harness owns command lines | A4 | arc_09_argv_matches_baseline |
| ARC-10 | Capabilities; dispatch-only lifecycle | A4 | arc_10_capabilities_match_legacy_predicates, arc_10_harness_match_only_in_harness, arc_10_prompts_hold_no_execution_policy |
| ARC-11 | Workdir-scoped discovery | A4 | arc_11_codex_discovery_by_workdir, arc_11_opencode_discovery_by_workdir, arc_11_canonical_tmp_paths |
| ARC-12 | Pure routing equals baseline | A5 | arc_12_decisions_match_baseline, bal_03, bal_04, bal_05, bal_06, quo_07 |
| ARC-13 | Eligible set and reasons | A5 | arc_13_exclusion_reasons, arc_13_candidates_equivalent, arc_13_routing_never_launches |
| ARC-14 | Provenance on every execution | A5 | arc_14_provenance_on_spawn_substitute_resume |
| ARC-15 | Pure planning | A6 | arc_15_plan_deterministic, arc_15_plan_table, arc_15_plan_is_pure |
| ARC-16 | Apply order; LaunchFailed; no phantoms | A6 | arc_16_split_failure_launch_failed, arc_16_run_failure_closes_pane, arc_16_e2e_fail_split, arc_16_crash_after_insert_not_live |
| ARC-17 | Legacy DTO and compat | A6 | arc_17_legacy_ledgers_load_and_resume, arc_17_roundtrip_byte_identical, arc_17_old_reader_sees_compat_status |
| ARC-18 | Worker workflow; exit code | A6 | arc_18_worker_startup_order, arc_18_agent_exit_recorded |
| ARC-19 | WorkspaceClient; tiler unchanged | A7 | arc_19_herdr_parsing_contract, moved tile/balance/layout tests |
| ARC-20 | Delivery identical | A7 | arc_20_send_line_prompt_first, arc_20_fallback_waits_tail |
| ARC-21 | done lifecycle order | A7 | arc_21_done_order |
| ARC-22 | Thin CLI; typed errors; exit codes | A6 / A12 | bal_04 (exit 3), arc_22_no_error_string_matching |
| ARC-23 | Telemetry uses executions | A6 | arc_23_telemetry_reads_executions, arc_23_telemetry_never_routes, tel_* |
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
| SKL-09 | `horch skills read` prints a catalog skill's file, read-only; refuses an unknown id or file and an absolute or `..` path | A9 | skl_09_read_prints_skill_md, skl_09_read_prints_a_named_file, skl_09_read_lists_files, skl_09_read_rejects_unknown_and_dotdot |
| SKL-10 | The briefing names the catalog skills that the bundle's texts name but do not hold: count, 3 most-named, deterministic; a hyphenless id counts only in backticks, a link path or qualified | A9 | skl_10_briefing_names_skills_outside_the_bundle, skl_10_briefing_has_no_sentence_without_outside_names, skl_10_whole_word_match_only, skl_10_plain_id_counts_in_backticks, skl_10_plain_id_counts_in_a_link_path, skl_10_plain_id_counts_qualified, skl_10_plain_id_in_prose_does_not_count |
| SKL-11 | A materialized bundle keeps the execute bit of every executable skill file; the skill digest covers it. A marketplace install of a `bundled:<id>` skill also writes each executable file with mode 755. The marketplace `tree_digest` stays mode-blind, because every lock entry pins it and a mode-aware digest would mark each installed skill with an executable file `Tampered` | A9 | skl_11_materialized_bundle_keeps_the_execute_bit, skl_11_digest_covers_the_execute_bit, skl_11_bundled_entry_lists_its_executable_files, skl_11_marketplace_install_keeps_the_execute_bit |
| SKL-12 | The spawn plan and the launch apply 1 support rule: `SkillResolution::plan` checks the agent's skill support when the plan activates any skill, operator and plugin skills included, so the spawn plan rejects what the launch rejects, before a record exists. An `available_skills:` name that this build does not bundle is dropped with 1 `NOTE:` line that names the skill and the fix (rebuild and install horch); an unknown name in `skills:` or a phase list still refuses, and `horch teammates --check` still reports an unknown offered name | A9 | skl_12_spawn_plan_rejects_an_operator_only_teammate_the_launch_rejects, skl_12_codex_on_windows_rejects_an_operator_only_teammate_at_the_plan, skl_12_unknown_available_skill_is_dropped_with_a_note, skl_12_unknown_explicit_skill_still_refuses |

### 3.4 Spec A §17 acceptance criteria → IDs

The criterion texts below are the Spec A §17 acceptance criteria. They
are the master plan's table "Spec A §17 acceptance criteria (verbatim)"
(§4). The tests of
the IDs in each row pin the criterion.

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
/// `$validator: fn(kind: &'static str, value: &str) -> Result<(), IdError>` runs on
/// every constructor path; `kind` is `stringify!($name)`. Doc attributes before
/// `$name` are passed on to the struct.
macro_rules! string_id { ($(#[$meta:meta])* $name:ident, $validator:path) => { /* … */ } }

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
pub(crate) fn mint_v7(at: chrono::DateTime<chrono::Utc>) -> uuid::Uuid;

impl ExecutionId  { pub fn mint(at: chrono::DateTime<chrono::Utc>) -> Self; }
impl EventId      { pub fn mint(at: chrono::DateTime<chrono::Utc>) -> Self; }
impl ExperimentId { pub fn mint(at: chrono::DateTime<chrono::Utc>) -> Self; }
impl RoundId      { pub fn mint(at: chrono::DateTime<chrono::Utc>) -> Self; }
impl JudgmentId   { pub fn mint(at: chrono::DateTime<chrono::Utc>) -> Self; }
impl WorkerId     { pub(crate) fn new_for(workspace: &WorkspaceId, role: &RoleName) -> Self; } // "<ws>:<role>"
/// The last 8 ASCII letters and digits of the id: the `<exp8>` in `mh/exp/<exp8>/...`.
impl ExperimentId { pub fn short(&self) -> String; }
```

Validation rules:

| Type | Rule |
|---|---|
| every id | non-empty; no control character (`char::is_control`) |
| `RoleName` | also rejects `/`, `\` and the substring `..` |
| `WorkerId` | exactly one `:`; both sides non-empty |
| `ExecutionId` | any UUID (v4 or v7), and any legacy non-UUID id such as `rec-o1`, `perf-3` (OD7) |

`PaneId` and `WorkspaceId` are typed only inside horch-core's records and
plans. The workspace port (§4.7) takes and returns `&str` and `String` ids,
so the code converts at the edge:

- `RuntimeContext::from_env` validates `HORCH_WORKSPACE_ID` and
  `HERDR_PANE_ID` into `HerdrEnv` (§4.2).
- `Execution::workspace` and `Execution::pane` (§4.3) and
  `WorkspacePlan::workspace` (§4.5) are typed. `finish_plan` takes a
  `&WorkspaceId`, and `ExecutionService::spawn` makes it from the mailbox's
  workspace id string.
- `WorkspacePlan::from_pane`, `SpawnOutcome::pane` and every
  `WorkspaceClient` argument are strings.
- The competition coordinator (`competition/coordinator.rs`) and the
  measurement events (`measure/event.rs`) use `PaneId` for a candidate's
  pane.

`Digest(pub [u8; 32])` displays as `sha256:<64 hex>`. It lives in
`measure/digest.rs` (B1). See the dataset design §4.

### 4.2 `runtime` (A2)

Spec A §4 groups the runtime context in 6 parts: paths, herdr, bins,
settings, inherited and worker. The code below is the spec
(`horch-core/src/runtime/{context,paths,bins,fault,process}.rs`).
`runtime/machine.rs` (the machine probe, B2) is also in `runtime/`; the
dataset design describes it.
`arc_05_context_from_map_env` (`runtime/context.rs`) pins every field that
an environment sets. `arc_05_no_ambient_env_in_core` pins that only
`runtime/` reads the environment.

```rust
// runtime/context.rs
pub trait EnvSource {
    fn var(&self, key: &str) -> Option<String>;          // raw; the caller decides if "" is unset
    fn var_os(&self, key: &str) -> Option<OsString>;
    fn current_dir(&self) -> Option<PathBuf>;
    fn current_exe(&self) -> Option<PathBuf>;
    fn temp_dir(&self) -> PathBuf;
}
/// The real process environment. The only type in horch-core that reads it.
/// `horch::bootstrap` builds the context from it.
pub struct ProcessEnv;
/// A fixed environment for tests.
#[derive(Debug, Clone, Default)]
pub struct MapEnv {
    pub vars: BTreeMap<String, String>,
    pub cwd: Option<PathBuf>,
    pub exe: Option<PathBuf>,
    pub temp: Option<PathBuf>,                    // else $TMPDIR in vars, else /tmp
}
impl MapEnv {
    pub fn new(cwd: impl Into<PathBuf>) -> Self;
    pub fn with(self, key: &str, value: &str) -> Self;
    pub fn with_exe(self, exe: impl Into<PathBuf>) -> Self;
}

/// Everything horch reads from its environment, read once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeContext {
    pub paths: Paths,
    pub herdr: HerdrEnv,
    pub bins: Bins,
    pub settings: Settings,
    pub inherited: Inherited,
    pub worker: Option<WorkerEnv>,                // None when no HORCH_* worker variable is set
}
impl RuntimeContext {
    pub fn from_env(env: &dyn EnvSource) -> anyhow::Result<RuntimeContext>; // errors on a bad id
    pub fn prepend_own_dir_to_path(&mut self) -> Option<OsString>;
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HerdrEnv {
    pub workspace: Option<WorkspaceId>,   // HORCH_WORKSPACE_ID (the public id, set once a worker registered)
    pub pane: Option<PaneId>,             // HERDR_PANE_ID (herdr's internal id, `p_2`)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bins {
    pub roster_override: Option<PathBuf>, // HORCH_TEAMMATES_DIR
    pub current_exe: Option<PathBuf>,
    pub horch_exe: PathBuf,               // sibling `horch` of current_exe, else `horch` on PATH, else `horch`
    pub harness: HarnessBins,
    pub overrides: BinOverrides,
}
impl Bins { pub fn exe(&self) -> anyhow::Result<PathBuf>; } // current_exe, else an error

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub now: Option<DateTime<Utc>>,       // HORCH_NOW, when it parses
    pub now_unparsable: Option<String>,   // HORCH_NOW, when it is set and does not parse (the clock warns once)
    pub tiling: TilingMode,               // HORCH_TILE: `0`, `false`, `no`, `off` → Disabled
    pub balance_override: Option<String>, // HORCH_BALANCE, raw
    pub quota_file: Option<PathBuf>,      // HORCH_QUOTA_FILE
    pub machine_file: Option<PathBuf>,    // HORCH_MACHINE_FILE (B2)
    pub probe_timeout: Option<Duration>,  // HORCH_PROBE_TIMEOUT_MS
    pub tell_grace: Option<Duration>,     // HORCH_TELL_GRACE_MS
    pub spawn_wait: Option<Duration>,     // HORCH_SPAWN_WAIT_MS (spawn waits for its worker to register)
    pub faults: Faults,                   // HORCH_FAULT
}

/// Variables horch does not own but reads, or passes on to the tools it runs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Inherited {
    pub home_var: Option<OsString>,                // HOME exactly as set
    pub path: Option<OsString>,                    // PATH
    pub pathext: Option<String>,                   // PATHEXT (windows)
    pub blender_path: Option<OsString>,            // BLENDER_PATH
    pub godot_path: Option<OsString>,              // GODOT_PATH
    pub opencode_config_content: Option<String>,   // OPENCODE_CONFIG_CONTENT
    pub codex_home: Option<PathBuf>,               // CODEX_HOME
    pub claude_config_dir: Option<PathBuf>,        // CLAUDE_CONFIG_DIR
    pub claude_code_effort_level: Option<String>,  // CLAUDE_CODE_EFFORT_LEVEL
    pub pi_session_dir: Option<PathBuf>,           // PI_CODING_AGENT_SESSION_DIR
    pub opencode_db: Option<PathBuf>,              // HORCH_OPENCODE_DB
    pub xdg_data_home: Option<PathBuf>,            // XDG_DATA_HOME
    pub local_app_data: Option<PathBuf>,           // LOCALAPPDATA (windows)
    pub hostname: Option<String>,                  // HOSTNAME, else COMPUTERNAME
    pub herdr_session: Option<String>,             // HERDR_SESSION
}
impl Inherited { pub fn from_env(env: &dyn EnvSource) -> Inherited; }

/// What a worker pane's agent and its horch children learn from the
/// transport environment (`messaging::brief::Brief::transport_env`).
/// An empty value counts as unset.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkerEnv {
    pub role: Option<String>,         // HORCH_ROLE
    pub teammate: Option<String>,     // HORCH_TEAMMATE
    pub agent: Option<String>,        // HORCH_AGENT
    pub model: Option<String>,        // HORCH_MODEL
    pub record_id: Option<String>,    // HORCH_RECORD_ID
    pub session_id: Option<String>,   // HORCH_SESSION_ID
    pub resume: Option<String>,       // HORCH_RESUME
    pub task: Option<String>,         // HORCH_TASK
}
impl WorkerEnv { pub fn from_env(env: &dyn EnvSource) -> Option<WorkerEnv>; } // None when every value is unset

// runtime/paths.rs
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub cwd: Option<PathBuf>,             // the current directory at bootstrap
    pub project_dir: Option<PathBuf>,     // $HORCH_PROJECT_DIR, else cwd
    pub state_root: PathBuf,              // $HORCH_STATE_DIR, else ${XDG_STATE_HOME:-$HOME/.local/state}/horch
    pub state_override: Option<PathBuf>,  // the explicit $HORCH_STATE_DIR, passed on to panes
    pub data_root: PathBuf,               // $HORCH_DATA_DIR, else ${XDG_DATA_HOME:-$HOME/.local/share}/horch (marketplace store, OD3)
    pub temp_root: PathBuf,               // EnvSource::temp_dir() at bootstrap; mailboxes live under it
    pub home: PathBuf,                    // $HOME (%USERPROFILE% on windows), else "."
}
impl Paths {
    pub fn from_env(env: &dyn EnvSource) -> Paths;
    pub fn project(&self) -> anyhow::Result<PathBuf>;      // project_dir, else an error
    pub fn current_dir(&self) -> anyhow::Result<PathBuf>;  // cwd, else an error
    pub fn set_state_dir(&mut self, dir: impl Into<PathBuf>); // sets state_root and state_override
}

// runtime/bins.rs
/// Explicit HORCH_*_BIN values only. They travel in the worker's brief.
/// An empty value is unset. Serde: each key skipped when None.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinOverrides {
    pub claude: Option<PathBuf>,      // HORCH_CLAUDE_BIN
    pub codex: Option<PathBuf>,       // HORCH_CODEX_BIN
    pub opencode: Option<PathBuf>,    // HORCH_OPENCODE_BIN
    pub pi: Option<PathBuf>,          // HORCH_PI_BIN
    pub prime: Option<PathBuf>,       // HORCH_PRIME_BIN
    pub antigravity: Option<PathBuf>, // HORCH_ANTIGRAVITY_BIN
    pub herdr: Option<PathBuf>,       // HORCH_HERDR_BIN
    pub sqlite3: Option<PathBuf>,     // HORCH_SQLITE3_BIN
    pub ollama: Option<PathBuf>,      // HORCH_OLLAMA_BIN
    pub git: Option<PathBuf>,         // HORCH_GIT_BIN
}
impl BinOverrides {
    pub fn from_env(env: &dyn EnvSource) -> BinOverrides;
    pub fn env_pairs(&self) -> Vec<(&'static str, PathBuf)>; // the set values as (HORCH_*_BIN, path)
    pub fn merge(&mut self, other: &BinOverrides);           // a value set in `other` wins
    pub fn is_empty(&self) -> bool;
}
/// The resolved program for each tool: the override, else the default name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessBins {
    pub claude: PathBuf,       // override, else `cpx` when it is on PATH, else `claude`
    pub codex: PathBuf,        // `codex`
    pub opencode: PathBuf,     // `opencode`
    pub pi: PathBuf,           // `pi`
    pub prime: PathBuf,        // `prime-agent`
    pub antigravity: PathBuf,  // `agy`
    pub herdr: PathBuf,        // `herdr`
    pub sqlite3: PathBuf,      // `sqlite3`
    pub ollama: PathBuf,       // `ollama`
    pub git: PathBuf,          // `git`
}
impl HarnessBins { pub fn resolve(o: &BinOverrides, path: Option<&OsStr>, pathext: Option<&str>) -> HarnessBins; }
// One resolver for each tool; pub for claude, codex, herdr, sqlite3 and git, pub(crate) for the others.
pub fn claude_bin(o: &BinOverrides, path: Option<&OsStr>, pathext: Option<&str>) -> PathBuf;
pub fn git_bin(o: &BinOverrides) -> PathBuf;                // and codex_bin, herdr_bin, sqlite3_bin

// runtime/fault.rs
pub const ABORT_EXIT_CODE: i32 = 86;
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Faults(BTreeSet<String>);              // HORCH_FAULT, comma-separated; blank entries ignored
impl Faults {
    pub fn parse(raw: Option<&str>) -> Faults;
    pub fn has(&self, point: &str) -> bool;       // exact match, e.g. "abort-after-execution-insert"
    pub fn is_empty(&self) -> bool;
    pub fn points(&self) -> &BTreeSet<String>;
    pub fn indexed(&self, prefix: &str) -> Option<String>; // "abort-after-worktree:2" → Some("2")
    /// Exits with ABORT_EXIT_CODE and a stderr line when `point` is armed.
    pub fn abort_if(&self, point: &str);
}

// runtime/process.rs
pub fn which(path: Option<&OsStr>, pathext: Option<&str>, name: &str) -> Option<PathBuf>;
pub fn spawn_detached(cmd: &mut Command) -> std::io::Result<Child>;  // setsid on unix
pub fn make_executable(path: &Path) -> std::io::Result<()>;
pub fn on_path_in(paths: &OsStr, dir: &Path) -> bool;
pub(crate) fn inherit_env<K, V>(cmd: &mut Command, vars: impl IntoIterator<Item = (K, V)>); // skips keys the command sets and FORBIDDEN_ENV
pub(crate) fn scrub_child_env(cmd: &mut Command);                     // removes FORBIDDEN_ENV and the git repository variables (REPO_ENV, GIT_CONFIG_KEY_<n>, GIT_CONFIG_VALUE_<n>)
```

`cmp_16_only_judge_and_compaction_jobs_detached` pins the files that call
`spawn_detached` (or `setsid`, `process_group`). `harness/prime.rs` is one
of them: the Prime stop process must outlive the Prime worker's
process-group SIGKILL. horch waits for it, at most 30 s, so it is not a
background job.

The fault abort exit code is 86 (`runtime/fault.rs:ABORT_EXIT_CODE`).
Spec A names no code. 86 is distinct from every exit code that `horch`
(0 to 3, `horch/src/exit.rs`) and `multi-herdr-dataset` (0, 1, 3 to 6,
`horch/src/dataset/mod.rs:exit`) use. `arc_16_crash_after_insert_not_live`
(`horch-e2e/tests/lifecycle.rs`) pins it.

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
    pub(crate) fn legacy_status(&self) -> &'static str; // "working" | "done"
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
impl ExecutionKind { pub(crate) fn legacy_kind(&self) -> &'static str; } // Candidate, Judge → "worker"

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionMode { Fresh(Option<SessionId>), Resume(SessionId) }
impl SessionMode { pub fn id(&self) -> Option<&SessionId>; } // the minted or resumed session id

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "session", content = "id", rename_all = "snake_case")]
pub enum SessionState { Pending, Known(SessionId), Unavailable }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TilingMode { Automatic, Disabled }
impl TilingMode { pub fn from_no_tile(no_tile: bool) -> TilingMode; }

/// The work an execution was given.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: Option<TaskId>,            // ledger `task_id`; set by callers that track tasks (B3)
    pub text: String,                  // ledger `task`; the idle placeholder for a worker with no task
    pub plan: Option<String>,          // ledger `plan`: the slug of the plan path the task text names (`ai_docs/plans/<slug>.md`, local operator scratch, not in git)
}

/// One run of one worker, orchestrator, candidate or judge: the typed view
/// of one ledger record. Each comment names the ledger key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Execution {
    pub id: ExecutionId,                  // record_id
    pub kind: ExecutionKind,              // kind + experiment_id, round_id, label ("judge:<n>" for a judge)
    pub teammate: TeammateName,           // tier
    pub harness: HarnessKind,             // agent
    pub model: String,                    // model
    pub effort: Option<String>,           // effort
    pub phase: Option<Phase>,             // phase
    pub role: RoleName,                   // role
    pub task: Task,                       // task_id, task, plan
    pub status: ExecutionStatus,          // state, else derived from status
    pub typed_status: bool,               // whether the record carries `state` (false before A6)
    pub session: SessionState,            // session_id: null → Pending, "" → Unavailable, id → Known
    pub exit_code: Option<i32>,           // exit_code
    pub project: Option<PathBuf>,         // project
    pub workdir: Option<PathBuf>,         // workdir; None → the agent runs in the project
    pub workspace: Option<WorkspaceId>,   // workspace_id
    pub pane: Option<PaneId>,             // pane_id
    pub skills: Vec<ResolvedSkillRef>,    // skills
    pub routing: Option<RoutingProvenance>, // routing
    pub via: Option<String>,              // via: the fallback teammate whose launch settings ran
    pub substitution_reason: Option<String>, // substitution_reason
    pub history: Vec<HistoryEntry>,       // history
    pub created_at: String,               // created_at, the ledger's text
    pub updated_at: String,               // updated_at, the ledger's text
    pub finished_at: Option<String>,      // finished_at, the ledger's text
}
impl Execution {
    /// `spawn:<round>:<label>` for a candidate; None for every other kind.
    pub fn idempotency_key(&self) -> Option<String>;
}
```

Spec A §4 Execution has the 25 fields above
(`horch-core/src/execution/model.rs:Execution`). Every ledger key has one
place in it, so `execution/store.rs:to_execution` and `from_execution`
lose nothing. `arc_17_execution_conversion_lossless`
(`horch-core/tests/execution_plan.rs`) pins the round trip on every legacy
ledger oracle and on the candidate and judge kinds. Three choices differ
from the first sketch of this design, each for one reason:

- The timestamps stay the ledger's text, not `DateTime<Utc>`, so a record
  that any earlier version wrote keeps its bytes.
- `model` stays a `String`: no earlier version validated the ledger's
  `model`, and a `ModelId` rejects an empty value, which would make such a
  record unreadable.
- There is no `worker: WorkerId` field: the worker id is
  `<workspace>:<role>`, which `workspace` and `role` already give.

**`LedgerRecordV1`** (`execution/legacy.rs`, A6). It is today's `Record`
serde, byte for byte, plus optional skip-if-empty fields.
`pub type Record = LedgerRecordV1;` keeps the old name for callers. Today's `Record`
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
    // A5 wrote `routing` here; it stays here so those ledgers keep their bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")] pub routing: Option<RoutingProvenance>,
    // ── new in A6, optional, skip-if-empty ──
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
}

// execution/store.rs
pub fn to_execution(r: &LedgerRecordV1) -> Result<Execution, LegacyError>;
pub fn from_execution(e: &Execution) -> LedgerRecordV1;
#[derive(Debug)]
pub enum LegacyError {
    Id(IdError),
    Agent(String),                              // `agent` names no HarnessKind
    Kind { record: String, reason: String },    // kind, experiment_id, round_id, label name no kind
}
```

`kind` on disk stays `"worker"` or `"orchestrator"`. A Candidate or Judge
execution writes `kind: "worker"` (legacy) plus `experiment_id`, `round_id`,
`label` and a `state`; the store reconstructs `ExecutionKind` from these.

**The judge attempt key (Spec B).** A judge attempt has no idempotency key
field of its own. The pair `round_id` plus `label: "judge:<attempt>"` is
its key, because the store already finds an attempt again by that pair.

- A Judge record writes `kind: "worker"`, `round_id`, `label: "judge:<n>"`,
  and no `experiment_id`. A `judge:` label without a number does not convert
  (`LegacyError::Kind`).
- `competition/judging.rs:find_judge` looks up `ExecutionKind::Judge { round,
  attempt }` and returns the last match. `judge_execution` creates a record
  only when none exists, so a repeated schedule reuses the record.
- Each judge event carries its own per-attempt key:
  `judge.scheduled|started|failed|completed:<round>:<attempt>`.
- `Execution::idempotency_key` stays `spawn:<round>:<label>` for a Candidate
  and `None` for every other kind. The coordinator maps candidates by that
  key, so a judge key there would mix judges into the candidate map.
- Code: `execution/store.rs:kind_of`, `from_execution`, `JUDGE_LABEL`;
  `execution/model.rs:Execution::idempotency_key`. Tests:
  `crates/horch-core/tests/judging.rs:jdg_09_judge_execution_in_ledger`,
  `jdg_04_resume_after_completed_writes_once`;
  `crates/horch-core/tests/execution_plan.rs:arc_17_execution_conversion_lossless`
  (a Judge round-trips, and its `idempotency_key()` is `None`).

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExclusionReason {
    NotInRoster, Hidden, ReservedTier, TrainsOnInput, PoolBlocked, Unspawnable,
    EffortUnsupported, HarnessUnavailable, AgentNone, ExcludedByConfig, OverBudget,
    Unpriced,                          // no price for the model (the planner applies it)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "verdict", content = "reason", rename_all = "snake_case")]
pub enum Verdict { Eligible, Excluded(ExclusionReason) }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EligibleEntry {
    pub teammate: TeammateName,
    pub harness: HarnessKind,
    pub model: Option<ModelId>,        // None: no model of its own, or not in the roster
    pub effort: Option<String>,
    pub fallback_index: Option<u32>,   // None = the requested teammate; Some(i) = fallbacks[i]
    pub pool: String,
    pub pool_state: quota::State,
    #[serde(flatten)]
    pub verdict: Verdict,
}
#[derive(Debug, Clone, Default)]
pub struct EligibilityFilter {          // B3 config exclusions and budget
    pub excluded: BTreeSet<TeammateName>,
    pub max_cost_microusd: Option<i64>,
    pub estimated_cost_microusd: BTreeMap<TeammateName, i64>,
    pub available_harnesses: Option<Vec<HarnessKind>>,   // a Vec: HarnessKind has no Ord
}
/// The requested teammate, then its fallbacks, in `fallbacks:` order.
/// Same order and same drop rules as the pre-A5 private `candidates()`.
pub fn eligible_fallbacks(req: &Teammate, roster: &Roster, view: &QuotaView) -> Vec<EligibleEntry>;
/// Every roster teammate, sorted by name, each with a verdict.
pub fn roster_eligibility(roster: &Roster, view: &QuotaView, filter: &EligibilityFilter) -> Vec<EligibleEntry>;

// routing/decision.rs
pub struct GateFlags { pub exact: bool, pub force: bool }
pub struct PoolLine { pub pool: String, pub state: String, pub detail: String }

/// The gate's result. Its JSON is the pre-A5 JSON, byte for byte:
/// {"decision":"spawn","teammate":…,"note":…} | {"decision":"substitute","original":…,"via":…,"reason":…}
/// | {"decision":"refuse","teammate":…,"reason":…,"pools":[{pool,state,detail}]}
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "decision", rename_all = "lowercase")]
pub enum Decision {
    Spawn { teammate: String, note: Option<String> },
    Substitute { original: String, via: String, reason: String },
    Refuse { teammate: String, reason: String, pools: Vec<PoolLine> },
}
impl Decision { pub fn line(&self) -> Option<String>; }   // the NOTE / SUBSTITUTED / REFUSED line
/// Pure: no I/O. The time arrives inside the `QuotaView` (`view.now`).
pub fn decide(req: &Teammate, roster: &Roster, view: &QuotaView, mode: BalanceMode, flags: GateFlags) -> Decision;
pub fn resolve(req: &Teammate, roster: &Roster, decision: &Decision) -> Option<Teammate>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingMode {
    Auto, Advise, Off, Exact, Force,
    Pinned,                            // a B3 competition candidate on a fixed teammate
    Resume,                            // `horch spawn --resume`: the record's routing, kept
    Ungated,                           // no gate ran: the `none` agent spends nothing
}
impl RoutingMode { pub fn for_gate(mode: BalanceMode, flags: GateFlags) -> RoutingMode; }

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
// Built by RoutingProvenance::{from_decision, ungated, legacy, resumed}.

/// A `Decision` in typed form. The fields are `String`, so the conversion
/// cannot fail. It has no JSON: the JSON stays `Decision`'s.
#[derive(Debug, Clone, PartialEq)]
pub enum RoutingDecision {
    Spawn { teammate: String },
    Substitute { requested: String, resolved: String, reason: String },
    Refuse { requested: String, reason: String, pools: Vec<PoolLine> },
}
impl From<&Decision> for RoutingDecision { /* … */ }
```

`decide` returns `Decision`, and `Decision` is also the JSON type. The
design had `decide` return `RoutingDecision` and added a separate JSON type.
A5 kept `Decision` as the return type and added `RoutingDecision` as a
`From<&Decision>` view.
`SpawnError::Refused` and `PlanError::Refused` carry a `RoutingDecision` and
the REFUSED `line`.

`routing/snapshot.rs`:

```rust
/// `env.quota_file` wins and is never probed. Else, when `allow_probe` is set,
/// no live collector holds the lock and the last probe is older than
/// `probe_on_demand_age_min`, probe now and write the result (QUO-07).
pub fn obtain(state_root: &Path, now: DateTime<Utc>, policy: &Policy,
              allow_probe: bool, env: &QuotaEnv) -> anyhow::Result<QuotaView>;
pub struct QuotaEnv { pub quota_file: Option<PathBuf>, pub probe_timeout: Option<Duration>,
                      pub temp_root: PathBuf, pub bins: ProbeBins }
impl QuotaEnv { pub fn from_context(ctx: &RuntimeContext) -> QuotaEnv; }
```

`obtain` is the only routing path that probes. The telemetry collector
(`telemetry/collect.rs`) also calls `quota_probe::probe_all`.
`arc_13_routing_never_launches` (`horch-core/tests/routing.rs`) exempts
`quota_probe.rs` and `snapshot.rs` from its scan of `routing/`.

### 4.5 `execution` planning and service (A6)

```rust
// execution/lifecycle.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportTarget { Orchestrator, None }

// execution/model.rs. Spec A §8 SpawnRequest: these 13 fields. The role is not a field: it is `ExecutionService::spawn`'s `role` argument.
#[derive(Debug, Clone)]
pub struct SpawnRequest {
    pub teammate: Option<TeammateName>,     // None with `resume`: the record names it
    pub resume: Option<String>,             // a record id or session id to resume
    pub task: String,                       // empty for an idle worker
    pub phase: Option<Phase>,
    pub effort: Option<String>,             // this spawn only, over the teammate's (or the record's)
    pub from_pane: Option<String>,          // the pane to split; None splits the caller's pane
    pub direction: Direction,               // Right by default
    pub tiling: TilingMode,
    pub flags: GateFlags,                   // exact: never substitute; force: never refuse
    pub pinned: bool,                       // skip the usage-limit gate; provenance says `pinned` (B3)
    pub workdir: Option<PathBuf>,           // None → the project dir
    pub kind: ExecutionKind,
    pub report_to: ReportTarget,
}
impl SpawnRequest {
    /// Every option at its default: no resume, Right, Automatic, not pinned,
    /// Worker, report to the orchestrator.
    pub fn worker(teammate: Option<TeammateName>, task: impl Into<String>) -> SpawnRequest;
}

/// What the worker will launch.
#[derive(Debug, Clone, PartialEq)]
pub struct LaunchPlan {
    pub teammate: Teammate,                 // resolved (merged with fallback when substituted)
    pub model: String,
    pub session: SessionMode,
    pub task: String,                       // as asked: empty for an idle worker or a resume without a task
}
/// Where the worker's pane goes.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspacePlan {
    pub workspace: Option<WorkspaceId>,     // set by finish_plan
    pub from_pane: Option<String>,
    pub direction: Direction,
    pub tiling: TilingMode,
}
#[derive(Debug, Clone, PartialEq)]
pub struct ExecutionPlan {
    pub execution: Execution,               // status Planned; a resume holds the record as it will be written
    pub worker: Option<WorkerId>,           // set by finish_plan, with the role
    pub launch: LaunchPlan,
    pub skills: SkillActivationPlan,
    pub workspace: WorkspacePlan,
    pub gate_line: Option<String>,          // NOTE/SUBSTITUTED line, printed before the pane id
    pub resumed: bool,                      // the plan reopens an existing record
    pub report_to: ReportTarget,
}

// execution/plan.rs
pub const IDLE_TASK: &str = "(idle - awaiting assignment)";
/// The usage-limit gate's inputs, read by the shell only for a gated spawn.
#[derive(Debug, Clone, Copy)]
pub struct GateInputs<'a> { pub view: &'a QuotaView, pub balance: BalanceMode }
/// Ids the shell minted for this spawn. A resume uses neither.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MintedIds { pub execution: ExecutionId, pub session: SessionId }
/// Inputs gathered by the shell before planning. Pure data.
#[derive(Debug, Clone, Copy)]
pub struct PlanInputs<'a> {
    pub roster: &'a Roster,
    pub catalog: &'a SkillCatalog,          // bundled skills, the marketplace lock and the teammate's operator_skills
    pub gate: Option<GateInputs<'a>>,       // Some when needs_gate says so
    pub existing: Option<&'a LedgerRecordV1>, // the newest record the resume key matches
    pub now: DateTime<Utc>,
    pub ids: &'a MintedIds,
    pub project: &'a Path,
}
/// A fresh, unpinned spawn of a spawnable teammate whose agent is not `none`.
pub fn needs_gate(req: &SpawnRequest, roster: &Roster) -> bool;
/// Pure. Order: the resume rules or the roster lookup, the reserved-tier
/// check on what will launch, the gate (fresh spawns only), effort, skill support.
pub fn plan_launch(req: &SpawnRequest, inputs: &PlanInputs) -> Result<ExecutionPlan, PlanError>;
/// Pure: completes the plan once the role is allocated. A fresh record takes
/// `workspace`; a resumed one keeps its own. Sets `worker` and `workspace.workspace`.
pub fn finish_plan(plan: ExecutionPlan, role: RoleName, workspace: &WorkspaceId) -> ExecutionPlan;

#[derive(Debug)]
pub enum PlanError {
    NothingToSpawn,
    Refused { decision: RoutingDecision, line: String },   // `line`: the REFUSED line for stdout
    UnknownTeammate(String),
    Unspawnable(String),                    // a reserved model tier, or a headless-only teammate
    NotResumable { id: String, reason: String },
    BadEffort(String),
    SkillUnsupported(String),
    MissingGate,                            // a gated spawn planned without GateInputs: a caller bug
    BadId(IdError),
    RoleTaken(String),                      // the role is registered, briefed or planned in this workspace
}

// execution/service.rs
#[derive(Debug)]
pub enum SpawnError {
    Refused { decision: RoutingDecision, line: String },   // CLI prints `line`, exits 3
    Plan(PlanError),                        // nothing was written
    LaunchFailed { id: ExecutionId, stage: LaunchStage, source: anyhow::Error }, // record LaunchFailed, role free
    Store(anyhow::Error),                   // the ledger or the mailbox could not be written before the launch
}
impl std::fmt::Display for SpawnError { /* … */ }
impl std::error::Error for SpawnError {}
impl From<PlanError> for SpawnError { /* PlanError::Refused → SpawnError::Refused; else Plan */ }

pub struct ExecutionService<'a> {
    pub ctx: &'a RuntimeContext,
    pub store: &'a ExecutionStore,
    pub workspace: &'a dyn WorkspaceClient,
    pub mailbox: &'a Mailbox,               // the workspace's roles and briefs
    pub tile: &'a dyn Fn(&str, TilingMode), // lays the grid out around a new pane; called last
}
impl ExecutionService<'_> {
    /// recover abandoned → allocate the role and insert(Planned) under the ledger lock
    /// (finish_plan) → set_skills → brief → split → run → mark_starting(pane) → tile.
    /// `role`: an explicit role; None allocates `<teammate>-<n>`.
    pub fn spawn(&self, plan: ExecutionPlan, role: Option<&str>) -> Result<SpawnOutcome, SpawnError>;
}
#[derive(Debug, Clone)]
pub struct SpawnOutcome { pub plan: ExecutionPlan, pub pane: String }   // the finished plan and its pane

// execution/store.rs
#[derive(Debug, Clone)]
pub struct ExecutionStore { path: PathBuf }   // <state_root>/<slug>.json; writes take the ledger DirLock
impl ExecutionStore {
    pub fn for_project(state_root: impl AsRef<Path>, project: &str) -> Self;
    pub fn open(paths: &Paths, project: &Path) -> Self;
    pub fn open_in(ctx: &RuntimeContext) -> anyhow::Result<Self>;
    pub fn read(&self) -> anyhow::Result<Vec<LedgerRecordV1>>;
    pub fn load(&self) -> anyhow::Result<Vec<Execution>>;
    pub fn get(&self, key: &str) -> anyhow::Result<LedgerRecordV1>;
    pub fn live(&self) -> anyhow::Result<Vec<LedgerRecordV1>>;
    pub fn insert(&self, record: LedgerRecordV1) -> anyhow::Result<()>;
    pub fn insert_execution(&self, e: &Execution) -> anyhow::Result<()>;
    pub fn update<T>(&self, f: impl FnOnce(&mut Vec<LedgerRecordV1>) -> anyhow::Result<T>) -> anyhow::Result<T>;
    pub fn set_state(&self, key: &str, state: ExecutionStatus) -> anyhow::Result<()>;
    pub fn set_skills(&self, key: &str, skills: Vec<ResolvedSkillRef>) -> anyhow::Result<()>;
    pub fn mark_starting(&self, key: &str, pane: &str) -> anyhow::Result<()>;
    pub fn mark_running(&self, key: &str) -> anyhow::Result<()>;
    pub fn end_live(&self, key: &str, state: ExecutionStatus) -> anyhow::Result<ExecutionStatus>;
    pub fn record_exit(&self, key: &str, code: Option<i32>) -> anyhow::Result<()>;
    pub fn recover_abandoned(&self, now: DateTime<Utc>) -> anyhow::Result<Vec<LedgerRecordV1>>;
    pub fn end_if_pane_closed(&self, key: &str, ws: &dyn WorkspaceClient) -> anyhow::Result<bool>;
}
// `key` is a record id or a session id. There is no idempotency lookup: the
// coordinator (`competition/coordinator.rs`) maps the records by
// `Execution::idempotency_key` itself.

// execution/lifecycle.rs
pub trait WorkerSteps {
    fn load_brief(&mut self) -> Result<Brief>;
    fn enter_context(&mut self, brief: &Brief) -> Result<()>;
    fn register(&mut self, brief: &Brief) -> Result<()>;
    fn set_running(&mut self, brief: &Brief) -> Result<()>;
    fn launch(&mut self, brief: &Brief) -> Result<Option<i32>>;   // None: a signal ended the agent
    fn agent_exited(&mut self, brief: &Brief, code: Option<i32>) -> Result<()>;
    fn close_pane(&mut self) -> Result<()> { Ok(()) }              // after a startup failure; PaneWorker closes its pane
}
pub fn run_worker(steps: &mut dyn WorkerSteps) -> anyhow::Result<i32>;   // PaneWorker runs the steps for real
pub fn done(ws: &dyn WorkspaceClient, steps: &dyn DoneSteps, req: &DoneRequest) -> anyhow::Result<()>;
```

Spec A §8 worker startup order (`execution/lifecycle.rs:run_worker`):

1. `load_brief`: find this pane's mailbox and read the brief that
   `horch spawn` wrote. On failure, record nothing (the record is unknown)
   and return the error.
2. `enter_context`: take the brief's project, ledger and binary overrides
   as the process's context.
3. `register`: register the role in the mailbox, so `horch tell` reaches
   the pane. When step 2 or 3 fails, record `Failed(AgentExited{code:
   None})` at once (best effort), then close the pane (`close_pane`, best
   effort), then return the original error.
4. `set_running`: set the record to `Running`. On failure, log it and go
   on: the agent must start even when its bookkeeping cannot be written.
5. `launch`: run the agent through the harness flow and wait for it. On
   failure, record `agent_exited(None)` (best effort) and return the error.
6. `agent_exited(code)`: record how the agent ended. On failure, log it.
7. Return the agent's exit code, or 1 when a signal ended it. `horch
   worker` exits with it.

The worker never calls `done`: the agent runs `horch done`, which closes
the pane. Tests: `arc_18_worker_startup_order` (the order and a launch
failure), `arc_18_register_failure_records_failed`,
`arc_18_enter_context_failure_records_failed` and
`arc_18_agent_exit_recorded` (`horch-core/tests/execution_plan.rs`).

### 4.6 `harness` (A1 enum, A4 trait)

The code is `crates/horch-core/src/harness/mod.rs` and
`harness/capabilities.rs`. A new harness adds 1 module that implements
`Harness`, 1 `HarnessKind` variant (in `ALL`, `as_str`, `binary`, `adapter`,
`capabilities` and `FromStr`), and 1 `Capabilities` row.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HarnessKind {
    Claude,
    Codex,
    #[serde(rename = "opencode")]
    OpenCode,
    Pi,
    Prime,
    Antigravity,
    None,                                // the smoke teammate: no agent CLI
}
impl HarnessKind {
    pub const ALL: &'static [HarnessKind];               // every kind, None included
    pub fn as_str(self) -> &'static str;                 // "claude" … "antigravity", "none"
    pub fn binary(self, bins: &HarnessBins) -> Option<PathBuf>;   // None for `none`
    pub fn adapter(self) -> &'static dyn Harness;
    pub fn capabilities(self) -> &'static Capabilities;
    pub fn model_takes_effort(self, model: &str) -> bool;
}
// Also Display and FromStr (the `as_str` spelling).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillExposure { None, PluginDir, SkillFlag, ConfigPaths, CodexHome }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    pub caller_minted_session: bool,     // horch picks the session id (`--session-id`)
    pub resumes: bool,
    pub effort: &'static [&'static str], // accepted effort names; empty: no effort setting
    pub daemon: bool,
    pub exec_policy: bool,
    pub skill_exposure: SkillExposure,
    pub tool_lists: bool,
    pub tool_denylist: bool,
    pub headless: bool,                  // the B4 headless runner
    pub footprint_bytes: u64,            // resident memory of 1 CLI, before a local model
    pub local_model: bool,               // the model's weights count against memory
}

pub struct PrepareRequest<'a> {
    pub role: &'a str,
    pub exec_rules: &'a [ExecRule],
    pub skills: Option<&'a Bundle>,
}
#[derive(Default)]
pub struct Prepared {                    // hold until the CLI exits, then call finish()
    pub extra_args: Vec<String>,
    pub env: Vec<(String, OsString)>,
    pub sessions_dir: Option<PathBuf>,
    /* private: the cleanup steps */
}
#[derive(Debug, Clone, Copy)]
pub struct CommandSpec<'a> {
    pub teammate: &'a Teammate,
    pub session: Session<'a>,
    pub prompt: &'a str,
    pub model_override: Option<&'a str>,
}

pub trait Harness: Sync {
    fn kind(&self) -> HarnessKind;
    /// The only method without a default besides `kind`.
    fn command(&self, env: &LaunchEnv, spec: &CommandSpec<'_>) -> Result<Command>;

    // Every method below has a default.
    fn capabilities(&self) -> &'static Capabilities;          // self.kind().capabilities()
    fn validate(&self, t: &Teammate) -> Vec<&'static str>;     // generic_validate(self, t)
    fn model_takes_effort(&self, model: &str) -> bool;         // !capabilities().effort.is_empty()
    fn prepare(&self, ctx: &RuntimeContext, req: &PrepareRequest<'_>) -> Result<Prepared>;
    fn resume_prompt_typed(&self) -> bool;
    fn ensure_skills_supported(&self) -> Result<()>;
    fn skill_namespace(&self) -> Option<&'static str>;
    fn expose_skills(&self, teammate: &Teammate, skills: &Bundle, home: Option<&Path>) -> Result<Teammate>;
    fn expose_skills_env(&self, cmd: &mut Command, teammate: &Teammate, skills: &Bundle, inherited: Option<&str>) -> Result<()>;
    /// `command`, then `scrub_child_env` removes FORBIDDEN_ENV and the git
    /// repository variables. No harness overrides it.
    fn build_command(&self, env: &LaunchEnv, spec: &CommandSpec<'_>) -> Result<Command>;
    /// Session ids recorded at or after `since` for `workdir`, newest first.
    fn discover_sessions(&self, ctx: &RuntimeContext, workdir: &Path, since: SystemTime, sessions_dir: Option<&Path>) -> Vec<String>;
}
```

`Result` is `anyhow::Result`, `Command` is `std::process::Command` and
`SystemTime` is `std::time::SystemTime`. `LaunchEnv` and `Session` are in
`harness/launch.rs`. `HarnessKind` has no `Ord` and no `Hash`.

Capabilities per harness (the constants in `harness/capabilities.rs`):

| Harness | caller_minted_session | resumes | effort | daemon | exec_policy | skill_exposure | tool_lists | tool_denylist | headless | local_model |
|---|---|---|---|---|---|---|---|---|---|---|
| claude | yes | yes | low, medium, high, xhigh, max | no | no | PluginDir (`--plugin-dir` + settings overlay) | yes | yes | yes (B4) | no |
| codex | no | yes | none, low, medium, high, xhigh, max | no | yes | CodexHome (a `skills` link in the private `CODEX_HOME`) | no | no | no | no |
| opencode | no | yes | none, minimal, low, medium, high, xhigh, max | no | no | ConfigPaths (`OPENCODE_CONFIG_CONTENT` `skills.paths`) | no | no | no | no |
| pi | yes | yes | off, minimal, low, medium, high, xhigh, max | no | no | SkillFlag (`--skill <dir>`) | yes | yes | no | yes |
| prime | no | yes | off, minimal, low, medium, high, xhigh, max | yes | no | SkillFlag (`--skill <dir>`) | yes | no | no | no |
| antigravity | no | yes | low, medium, high | no | no | None (`agy` reads only shared skill directories) | no | no | no | no |
| none | no | no | (none) | no | no | None | no | no | no | no |

`footprint_bytes` is 600 MiB (`HARNESS_FOOTPRINT_BYTES`) for every harness
except `none`, which is 64 MiB. `arc_10_capabilities_match_legacy_predicates`
checks every column against the pre-A4 predicates and effort table.

### 4.7 `workspace::WorkspaceClient` (A7)

The trait is `crates/horch-core/src/workspace/client.rs`. Pane and workspace
ids are `&str`. The trait does not take the typed ids `PaneId` and
`WorkspaceId` (`ids.rs`): A7a kept the string-based port, as the A7a report
records, and no later phase changed it.

```rust
pub trait WorkspaceClient {
    fn pane_get(&self, pane: &str) -> Result<Pane>;
    fn pane_list(&self, workspace: &str) -> Result<Vec<Pane>>;
    fn pane_split(&self, from: &str, direction: Direction) -> Result<String>;   // the new pane id
    fn pane_run(&self, pane: &str, command: &str) -> Result<()>;
    fn pane_close(&self, pane: &str) -> Result<()>;
    fn agent_prompt(&self, pane: &str, text: &str) -> Result<()>;
    fn pane_send_text(&self, pane: &str, text: &str) -> Result<()>;
    fn pane_send_keys(&self, pane: &str, keys: &str) -> Result<()>;
    fn pane_read(&self, pane: &str, source: &str) -> Result<String>;
    fn workspace_create(&self, label: &str, cwd: Option<&str>, focus: bool) -> Result<NewWorkspace>;
    fn workspace_close(&self, workspace: &str) -> Result<()>;
    /// True when the herdr server answers. A failed pane call on a reachable
    /// server means the pane is gone.
    fn server_reachable(&self) -> bool;
}

// workspace/herdr.rs: the real client. Wire DTOs stay private to this file.
pub struct Herdr { bin: PathBuf }
impl Herdr { pub fn with_bin(bin: impl Into<PathBuf>) -> Self; }  // the CLI passes ctx.bins.harness.herdr
impl WorkspaceClient for Herdr { /* each method calls the Herdr method of the same name */ }

// workspace/testing.rs: the in-memory client for hermetic tests.
pub struct FakeCall { pub method: &'static str, pub args: Vec<String> }
#[derive(Debug, Default)]
pub struct FakeWorkspace { /* private state */ }
impl FakeWorkspace {
    pub fn new() -> Self;
    pub fn fail_next(&self, method: &'static str, message: &str);   // the next call of `method` fails once
    pub fn set_reachable(&self, reachable: bool);
    pub fn set_screen(&self, pane: &str, text: &str);
    pub fn set_agent_states(&self, pane: &str, states: &[(Option<&str>, Option<&str>)]);
    pub fn calls(&self) -> Vec<FakeCall>;
    pub fn pane_ids(&self) -> Vec<String>;
}
impl WorkspaceClient for FakeWorkspace { /* … */ }
// workspace/model.rs: public Rect, Pane, LayoutPane, Layout, Tab, Workspace, Move,
// Focus, NewWorkspace, Direction.
```

`Result` is `anyhow::Result`.

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
    pub fn bundled() -> anyhow::Result<SkillCatalog>;             // skills/ + skills/copied.json via build.rs
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

Plugin skills in the record (G4, 2026-10-04). A teammate's `plugin_skills:`
names skills of external Claude plugins. They are not catalog skills, and a
plugin skill never activates a catalog skill of the same name (SKL-07). The
ledger still records them, so the launch's skill check (SKL-04) covers them:

- `SkillCatalog::with_host_skills` also calls
  `SkillCatalog::with_plugin_skills`. Every caller that records or launches
  (spawn, `horch fleet`, the competition coordinator, the worker launch)
  and `--check` resolve a teammate's skills through 1 module,
  `skills/resolution.rs` (U7, 2026-10-08): `SkillResolution::read` extends
  the caller's base catalog with this one call, so all of them see the same
  plugin skills. `SkillResolution::plan` plans the activation and applies 1
  support rule: the agent must load skills when the plan activates any
  (SKL-12). `execution/plan.rs` stays pure: the shell reads, the plan plans.
- Id: `<plugin>:<skill>`, the name Claude Code lists the skill under. A
  catalog id has no `:`, so a plugin entry never clashes with one.
- Source: `CatalogSource::Plugin`, labelled `plugin:<plugin>@<marketplace>`
  for an installed plugin and `plugin:<plugin>@inline` for a `plugin_dirs`
  plugin (the ids Claude Code gives them).
- Version: `<plugin version>+<digest12>`. The plugin version comes from the
  `installed_plugins.json` record, else from the manifest; `plugin` when
  neither has one.
- Digest: `horch_marketplace::integrity::tree_digest` of the skill directory
  in the original plugin, the same digest the filtered copy is checked with.
- `plan_activation` puts a named plugin entry in `activated` as Explicit.
  An unnamed plugin entry is in neither list.
- `MaterializedSkills` skips plugin entries: the Claude adapter loads them
  from the filtered plugin copy (`harness/claude_plugins.rs`), and that copy
  must hold the digest the plan pinned, else the launch fails.
- Only a teammate on an agent that loads skills as plugins, without
  `disable_skills`, gets plugin entries. A plugin that does not resolve
  fails the spawn, as it would fail the launch.

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
| Created | `scripts/phase-gate.sh`, `justfile` recipe `gate`, the phase-gate inputs (retired once every phase landed), both design docs, `crates/horch-core/tests/baseline_oracles.rs` + `tests/oracles/**`, `crates/horch/tests/baseline_cli.rs` + `tests/oracles/**`, telemetry render goldens (`crates/horch/tests/golden/telemetry-*.txt`, `crates/horch-e2e/tests/golden/telemetry-e2e.txt`, blessed once in a separate commit) |
| Changed | `scripts/check-req-coverage.sh` (scans all designs, Phase column, `--through`, `--phase`, duplicate check), `scripts/check-deps.sh` and `nfr.rs` (sha2 allowlist) |
| Moved | none |
| Shims | none |
| Oracles | argv/env per teammate × {fresh, resume, unmanaged}; routing decisions: 12 quota fixtures × teammates × {∅, exact, force} × {auto, advise, off}; legacy ledgers (bash-era, pre-effort, pr14-substituted, orchestrator); skills briefings and `horch skills --json`; `sessions` render |
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
| Moved | `policy.rs` → `routing/policy.rs`; `quota.rs` pure part (QuotaView, Assessment, State, pools) → `routing/quota.rs`; `quota.rs` Child (`:618`), probes, `harness_version` → `routing/quota_probe.rs`; `current_view` → `routing/snapshot::obtain` (the probing path of routing; the telemetry collector, `telemetry/collect.rs`, also probes through `quota_probe::probe_all`); `balance_policy.rs` → `routing/{balance,decision,eligible}` (private `candidates()` becomes a filter over `eligible_fallbacks`, same order, same drop rules) |
| Shims | `policy.rs`, `quota.rs`, `balance_policy.rs`: `pub use crate::routing::<x>::*;`. `Decision` stays the gate's return type and its JSON type; `RoutingDecision` is `From<&Decision>` |
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
| Changed | `build.rs` also compiles in `skills/copied.json` (which files of each skill are copied, and which skills are verbatim); bundled versions are `bundled+<digest12>`; lock entries merge in; executions record `skills: [ResolvedSkillRef]`; materialize to `state_root/skill-bundles/<execution_id>/` |
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
| Decision JSON | `Decision` keeps its pre-A5 JSON bytes; `RoutingDecision` is a `From<&Decision>` view with no JSON of its own. | `arc_12_decisions_match_baseline` |
| CLI | `route`/`spawn` exit codes and flags; `horch skills` output. | `bal_04`, `mkt_09_legacy_skills_flags_output_unchanged` |

This table is the compatibility list of Spec A §13. It names 6 artifacts,
and the list is complete: each row gives the rule and the test that pins it.
A rule in this table changes only together with its test. The test files are:

- `arc_17_*`: `crates/horch-core/tests/execution_store.rs` and
  `crates/horch-core/tests/execution_plan.rs`.
- `arc_07_brief_v1_readable`: `crates/horch-core/src/messaging/brief.rs`.
- `arc_08_legacy_frontmatter_corpus_parses`: `crates/horch-core/src/roster/tests.rs`.
- `arc_03_harness_kind_serde_compat`: `crates/horch-core/src/harness/mod.rs`.
- `skl_01_*`: `crates/horch-core/tests/skills_catalog.rs`.
- `mkt_08_offline_reinstall_from_lock`: `crates/horch-marketplace/tests/marketplace.rs`.
- `mkt_08_runtime_needs_no_network` and
  `mkt_09_legacy_skills_flags_output_unchanged`: `crates/horch/tests/skills_cli.rs`.
- `arc_12_decisions_match_baseline`: `crates/horch-core/tests/routing.rs`.
- `bal_04`: `crates/horch-e2e/tests/e2e.rs`.

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

## Appendix A: Spec A source text

This design, with the other documents in `docs/specs/`, is the
authoritative Spec A. The operator's original prose request is not kept.
Where this document quotes it (for example the §17 acceptance criteria in
§3.4), the quote came from the master plan, and the requirement tables
and their tests are the binding form.
