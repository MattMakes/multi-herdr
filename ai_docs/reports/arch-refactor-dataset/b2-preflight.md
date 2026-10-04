# U17 b2-preflight: report

Branch `ard/b2-preflight`. Phase B2 (core part). Nothing calls these modules
yet; the `multi-herdr-dataset` binary (a later unit) gathers the facts and
calls `load`, `storage_probe` and `evaluate`.

## Public API

### `horch_core::competition::config`
- `DatasetConfig`, `Strategy { Diverse }`, `BudgetConfig`, `JudgeMode { Auto }`,
  `JudgeConfig`, `GateConfig`, `Caps` as in design §4.11. `Strategy` and
  `JudgeMode` implement `FromStr` (`"diverse"`, `"auto"`) for flag parsing.
- `RunFlags { task, candidates, strategy, budget_usd: Option<String>, judge,
  baseline, promote_to, worktree_root, allow_dirty }`. `None`/`false` keeps
  the file's value.
- `load(project, &RunFlags) -> anyhow::Result<DatasetConfig>`: reads
  `<project>/.multi-herdr/dataset.yaml` (`CONFIG_FILE`) when present. An
  empty file is an empty config. Every key, also inside `budget`, `judge`,
  `caps` and `gates`, rejects unknown fields. Order: file, then flags, then
  defaults, then `validate`.
- `validate(&DatasetConfig)`, `parse_usd_micro(&str) -> Result<i64>`.

### `horch_core::competition::preflight`
- `PreflightCandidate`, `PoolFacts`, `GitFacts`, `StorageProbe`,
  `TokenEstimate`, `PreflightPlan`, `CheckStatus { Pass, Warn, Fail }`
  (serde `pass`/`warn`/`fail`), `CheckResult`, `PreflightReport` (with
  `check(id)`), `REPORT_SCHEMA_VERSION = "1.0.0"`.
- `evaluate(&PreflightPlan, &MachineSnapshot) -> PreflightReport`: pure and
  deterministic. 13 checks, PRE-01..PRE-13 in order.
- `storage_probe(dir) -> StorageProbe`: creates `.preflight-<12hex>.tmp`
  with `create_new` and mode 0600, writes, `sync_all`, renames to `.done`,
  deletes, then `DirLock::acquire(dir, ".preflight-<12hex>", 60 s, 2 s)` and
  release. It never errors and leaves nothing behind.
- Helpers: `parse_git_version`, `footprint_bytes`, and the threshold
  constants below.

## Defaults (config)

| Field | Default |
|---|---|
| `candidates` | 3 (`DEFAULT_CANDIDATES`) |
| `budget.hard_usd_micro` | 0 = no ceiling (PRE-09 fails) |
| `budget.soft_usd_micro` | 80 % of hard, rounded down, unless the file sets it |
| `budget.judge_reserve_usd_micro` | 10 % of hard, rounded down, unless the file sets it |
| `judge` | auto, `opus`, `high`, 900 s, `policy: null` |
| `gates[].required` | true |
| `caps.candidate_deadline_s` | 3600 |
| `caps.max_parallel` | none |
| `caps.disk_headroom_bytes` | 10 GiB |
| `caps.log_cap_bytes` | 262144 |
| `caps.output_cap_bytes` | 1048576 |

Validation: `candidates >= 1`; budgets not negative; with a hard ceiling,
`hard >= soft > 0` and `judge_reserve < hard`; without one (hard 0), soft
and reserve must be 0 too; each gate has a non-blank command and
`timeout_s > 0`; `caps.max_parallel` is not 0.

## Thresholds (preflight), all `SPEC-RESOLVED(Spec B §3)` (closed: ai_docs/designs/2026-10-02-dataset-competition-design.md §3/§4.11, ai_docs/reports/finish/spec-b-preflight.md)

| Check | Rule | Constant |
|---|---|---|
| PRE-01 | Fail: no toplevel, no base SHA, dirty without `allow_dirty`, `worktree_supported` false, git version unknown or < 2.17, any `namespace_taken` | `MIN_GIT_VERSION = (2, 17)` |
| PRE-02 | need = N × (checkout + build) + artifacts + `caps.disk_headroom_bytes`. Fail need > free; Warn free unknown. N is every candidate, not the first wave. | – |
| PRE-03 | Footprint: claude/codex/opencode/prime 600 MiB, pi 600 MiB + `local_model_bytes`, none 64 MiB. Fail first wave > available; Warn unknown. | `HARNESS_FOOTPRINT_BYTES`, `NONE_FOOTPRINT_BYTES` |
| PRE-04 | CPU bound = cpus / 2. Local (pi) bound: Apple Silicon = mem_available / `local_model_bytes` (min 1; 1 when the model size is 0), Nvidia = GPU count, else 1. Warn: CPU count unknown, or pi without a GPU. | `CPUS_PER_CANDIDATE = 2` |
| PRE-05 | fd bound = open-file limit / 256, process bound = limit / 64. Fail: bound 0. Warn: unknown, or bound < min(N, max_parallel, cpu, memory). | `FDS_PER_CANDIDATE`, `PROCS_PER_CANDIDATE` |
| PRE-06 | Fail when a candidate harness has no resolved version. | – |
| PRE-07 | As PRE-06; Warn when any `PoolFacts.state != "ok"`. | – |
| PRE-08 | `safe_n = max(1, min(N, max_parallel, cpu, memory, fd, process, local))`; unknown bounds drop out. `waves = ceil(N / safe_n)`. Fail N = 0; Warn safe_n < N. | – |
| PRE-09 | Cost = Σ tokens × `nano_per_token(price)` over input, cache read, output, in `NanoUsd`, rounded half-even once. Fail no hard ceiling or cost + reserve > hard; Warn > soft, or a model without a price (left out, named). | `DEFAULT_TOKEN_ESTIMATE = 200k input, 3M cache read, 60k output` |
| PRE-10 | Fail: judge harness version unresolved, or reserve ≤ 0. | – |
| PRE-11 | Fail when any of write, lock, fsync, rename failed. | – |
| PRE-12 | As PRE-06 (placeholder until the binary unit). | – |
| PRE-13 | Fail: herdr unreachable or `horch_exe` None. Warn (never Fail): `worktree_root` None or not under a `trusted_parents` entry (component-wise prefix). | – |

`environment_digest = digest_json({machine, harness_versions, git_version})`.
The budget and the candidates are not part of it. `passed` = no Fail.

On the Mac fixture (18 cpus, 80 GiB available, 10240 files, 8000 processes)
the bounds are cpu 9, fd 40, process 125, so `max_parallel` or N decides.
LA-6 must compare these numbers with Activity Monitor.

## Deviations from design §4.11

- `PreflightCandidate { label, teammate, harness, model, effort }` instead of
  `CandidatePlanned` (U13 builds it in parallel).
- `JudgeConfig.policy` is `serde_json::Value` (default `null`) with the
  comment `// B4: typed once WinnerPolicy lands`.
- `PreflightPlan.pools: Vec<PoolFacts>` instead of `quota: QuotaView`, so
  this unit does not depend on A5.
- `PreflightPlan.harness_versions` is `BTreeMap<String, Option<String>>`
  keyed by `HarnessKind::as_str()`, not `BTreeMap<HarnessKind, _>`.
  `HarnessKind` does not derive `Ord`, and `harness/mod.rs` is not this
  unit's file. A later unit can add `Ord` and switch the key.
- `PreflightPlan` has more fields than the design: `judge_harness`,
  `checkout_bytes`, `build_bytes`, `artifacts_bytes`, `local_model_bytes`,
  `expected_tokens` (per label), `trusted_parents`. The plan names them.
- The config file uses the struct field names (`soft_usd_micro` and so on),
  not dollar decimals. Only `--budget-usd` takes a decimal.
- `JudgeMode` has only `Auto` (`--judge auto`).

## Tests added (19), `crates/horch-core/tests/preflight.rs`

pre_01_git_root, pre_01_base_sha, pre_01_dirty_policy,
pre_01_worktree_support, pre_01_namespace_free, pre_02_disk_budget,
pre_03_memory_footprint, pre_04_local_inference_bound, pre_05_rlimits,
pre_08_safe_n_waves, pre_09_budget_projection_soft_limit,
pre_10_judge_available_and_reserved, pre_11_storage_probe,
pre_13_herdr_and_horch_exe, config_flags_override_file,
config_rejects_unknown_key, config_budget_decimal_exact,
preflight_base_plan_passes_every_check_in_order,
preflight_report_round_trips_and_digest_tracks_the_environment.

Fixtures: `tests/fixtures/dataset/full.yaml`, `unknown-key.yaml`; the
machine fixtures from `tests/fixtures/machine/`.

## Gotchas and notes for later units

- `scripts/check-req-coverage.sh --phase B2` still lists PRE-06, PRE-07,
  PRE-12 and CMP-01 as MISSING. The binary unit owns them.
- The safe N is one uniform wave size. One pi candidate in a round lowers
  the wave size for every candidate to the local-inference bound. A
  scheduler that runs remote candidates beside a local one needs a finer
  model.
- `worktree_root` is not resolved against the project. The binary must pass
  an absolute path, or the PRE-13 trust check warns.
- The read-only part of `pre_11_storage_probe` skips itself when the test
  runs as root (root ignores directory permissions).
- A `--budget-usd` with more than 6 non-zero decimals is an error, not a
  rounding.
- `competition/mod.rs` is new here; U13 also creates it. Keep both sides'
  `pub mod` lines.
