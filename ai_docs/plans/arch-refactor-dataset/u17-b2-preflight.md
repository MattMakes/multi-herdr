# U17 b2-preflight: DatasetConfig and the pure preflight evaluation

Unit slug: `b2-preflight`. Branch: `ard/b2-preflight`. Phase: B2 (core part).
Requirements: PRE-01, PRE-02, PRE-03, PRE-04, PRE-05, PRE-08, PRE-09, PRE-10,
PRE-11, PRE-13. (PRE-06, PRE-07, PRE-12 and CMP-01 need the binary; a later
unit does them.)

## GOAL

`competition/config.rs` loads and validates the dataset config (file plus
CLI-flag overrides), and `competition/preflight.rs` evaluates a plan against
a machine snapshot with a pure function that returns a `PreflightReport`
with one check per PRE id, a safe N, the number of waves and a projected
cost. A small storage probe adapter backs PRE-11. All unit-tested.

## CONTEXT

- Read first: `ai_docs/plans/arch-refactor-dataset/01-fleet-conventions.md`.
- Read in `00-master-plan.md`: OD3, OD5, §3 "B2", §4 PRE rows, §5 LA-6,
  §6 risks 2 and 6.
- The design doc is your main spec:
  `ai_docs/designs/2026-10-02-dataset-competition-design.md` §4.11
  (`DatasetConfig`, `BudgetConfig`, `JudgeConfig`, `GateConfig`, `Caps`,
  `load`, `CheckStatus`, `CheckResult`, `PreflightReport`, `PreflightPlan`,
  `evaluate`) and §3.2 (PRE table). `SPEC-TODO(Spec B §3)` marks the check
  list and thresholds as provisional; keep the marker and choose sensible,
  documented thresholds.
- Merged building blocks: `runtime::machine::{MachineSnapshot, Known, GpuClass}`
  (`machine-teacher.md`), `usage::money::{MicroUsd, NanoUsd, nano_per_token, CostSource}`
  and `usage::builtin_prices` (`b1-primitives.md`), `fsx`, `measure::digest`,
  `ids`, `harness::HarnessKind`.
- Deviations (types built in parallel by other units):
  - `CandidatePlanned` (U13) → use a local `PreflightCandidate { label, teammate: TeammateName, harness: HarnessKind, model: ModelId, effort: Option<String> }`.
  - `WinnerPolicy` (U15) → `JudgeConfig.policy` is
    `#[serde(default)] serde_json::Value` with a `// B4: typed once WinnerPolicy lands` comment.
  - `QuotaView` (A5 moves it to `routing::quota`; the old `quota::QuotaView`
    path still works through a shim) → you may use it, or use a local
    `PoolFacts { pool: String, state: String }` list; prefer the local list so
    you do not depend on A5's in-flight change.
  - `GitFacts`, `StorageProbe`: define them yourself in `preflight.rs`
    (`GitFacts { toplevel: Option<PathBuf>, base_sha: Option<String>, dirty: bool, worktree_supported: bool, namespace_taken: Vec<String>, git_version: Option<String> }`).
- No `std::env` reads; `load` takes the project path and a `RunFlags` struct.

## FILES

own:
- `crates/horch-core/src/competition/{config,preflight}.rs` (new)
- `crates/horch-core/src/competition/mod.rs` (your `pub mod` lines; U13
  creates the file in parallel; expect a trivial conflict)
- `crates/horch-core/src/lib.rs` (add `pub mod competition;` if absent)
- `crates/horch-core/tests/preflight.rs` (new)
- `crates/horch-core/tests/fixtures/dataset/**` (new: sample `dataset.yaml` files)
- `ai_docs/reports/arch-refactor-dataset/b2-preflight.md`

do not touch: every other file.

## STEPS

1. Create the worktree (conventions §2).
2. `competition/config.rs`: the types of design §4.11 (with the deviation
   above), `RunFlags` (every CLI flag of master plan B2 "Command": task,
   candidates, strategy, budget_usd, judge, baseline, promote_to,
   worktree_root, allow_dirty), and `load(project, flags)`:
   read `<project>/.multi-herdr/dataset.yaml` if present
   (`deny_unknown_fields`), apply flags over it, apply defaults, then
   validate: `candidates >= 1`, `hard >= soft > 0`,
   `judge_reserve < hard`, every gate has a non-empty command and a timeout
   > 0, `log_cap_bytes = 262144` and `output_cap_bytes = 1048576` by default.
   `--budget-usd X` sets the hard ceiling in µ$ (exact decimal parse, no
   float rounding: parse the decimal string into µ$ by hand) and the soft
   limit to 80 % of it unless the file sets it.
3. `competition/preflight.rs`: `PreflightPlan` (with the deviations),
   `evaluate(plan, snapshot) -> PreflightReport`, pure. One `CheckResult`
   per id, in order PRE-01..PRE-13. For PRE-06, PRE-07, PRE-12 (not this
   unit's ids) still emit a check from the plan facts you have
   (`harness_versions` resolved for every candidate harness → Pass; any
   `None` → Fail) so the report is complete; their tests come later.
   Rules (document each threshold in the report; mark `SPEC-TODO(Spec B §3)`):
   - PRE-01 git: Fail when no toplevel, no base SHA, dirty and not
     `allow_dirty`, worktrees unsupported (`git_version` < 2.17), or any
     planned branch name already taken.
   - PRE-02 disk: need = N × (checkout_bytes + build_bytes) + artifacts +
     `caps.disk_headroom_bytes`; plan carries `checkout_bytes` and
     `build_bytes` estimates; Fail when need > `disk_free`; Warn when
     `disk_free` is Unknown.
   - PRE-03 memory: per-candidate footprint by harness (a constant table:
     claude/codex/opencode/prime ~ 600 MiB, pi + local model from
     `local_model_bytes` in the plan); Fail when sum of the first wave >
     `mem_available`; Unknown → Warn.
   - PRE-04 CPU/GPU: local-inference candidates (harness `Pi`) are limited to
     1 concurrent unless `gpu` is AppleSilicon or Nvidia with enough memory;
     report the bound in `measured`.
   - PRE-05 limits: fd soft limit >= 256 × parallel, process limit >= 64 ×
     parallel; Unknown → Warn.
   - PRE-08 safe N: `safe_n = min(N, caps.max_parallel, cpu bound, memory
     bound, fd bound, process bound, local-inference bound)`, at least 1;
     `waves = ceil(N / safe_n)`.
   - PRE-09 budget: projected cost = sum over candidates of expected tokens
     × price (`builtin_prices` + `nano_per_token`; expected tokens come from
     the plan with a default constant), accumulated in `NanoUsd`, rounded
     once; Fail when projected + judge reserve > hard; Warn when > soft;
     Fail when no hard ceiling.
   - PRE-10 judge: Fail when the judge harness version is unresolved or the
     judge reserve is 0.
   - PRE-11 storage: from `StorageProbe { writable, lock_ok, fsync_ok, rename_ok }`.
   - PRE-13: Fail when herdr is unreachable or `horch_exe` is None; Warn
     (never Fail) about workspace-trust prompts when `worktree_root` is not
     under a known trusted parent (the plan carries `trusted_parents`).
   - `environment_digest` = `digest_json` of (machine snapshot, harness
     versions, git version); `passed` = no Fail.
   Add `pub fn storage_probe(dir: &Path) -> StorageProbe` (an adapter: write
   a temp file 0600, `DirLock` acquire/release, fsync, rename, delete).
4. Tests in `crates/horch-core/tests/preflight.rs`, all pure except PRE-11:
   `pre_01_git_root`, `pre_01_base_sha`, `pre_01_dirty_policy`,
   `pre_01_worktree_support`, `pre_01_namespace_free`, `pre_02_disk_budget`,
   `pre_03_memory_footprint`, `pre_04_local_inference_bound`,
   `pre_05_rlimits`, `pre_08_safe_n_waves`, `pre_09_budget_projection_soft_limit`,
   `pre_10_judge_available_and_reserved`, `pre_11_storage_probe` (temp dir;
   also a read-only dir → not writable, `cfg(unix)`), `pre_13_herdr_and_horch_exe`,
   plus `config_flags_override_file`, `config_rejects_unknown_key`,
   `config_budget_decimal_exact` (`--budget-usd 12.345678` → 12 345 678 µ$).
   Use the machine fixtures in `crates/horch-core/tests/fixtures/machine/`.
5. Gate after each step. Commits: `B2: Add dataset config`,
   `B2: Add pure preflight evaluation`, `B2: Add storage probe`, `B2: Add PRE tests`.
6. Write and commit the report (thresholds table, deviations). Follow
   conventions §6.

## DONE WHEN

- The named tests pass. `just gate` is green.

## REPORT

- `horch note` after each commit.
- `horch done` summary: API, thresholds, deviations, SPEC-TODOs.
