# A0 baseline

The state of the code before the architecture refactor, and the fixes that
made the base green.

## Environment

| Item | Value |
|---|---|
| Machine | operator's Mac (Darwin 25.5.0) |
| `rustc --version` | rustc 1.96.0 (ac68faa20 2026-05-25) |
| `cargo --version` | cargo 1.96.0 (30a34c682 2026-05-25) |
| `git --version` | git version 2.39.1 |
| `sqlite3 --version` | 3.51.0 2025-06-12 (`/usr/bin/sqlite3`) |
| Flags | `HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1` |

## Base commits

- `575c2c2` (origin/combine-open-prs, OD1) did not build: `crates/horch`
  used `chrono` without declaring it. `8de1f0a` declares it.
- `cargo fmt --all --check` failed on 19 files at `e5122ab`. `9587e24`
  applies `cargo fmt`.
- The rows below are for `9587e24`, untouched, run one command at a time.

## Results on the untouched base (`9587e24`)

| # | Command | Exit | Tests | Wall time |
|---|---|---|---|---|
| 1 | `cargo fmt --all --check` | 0 | - | 0 s |
| 2 | `cargo build --workspace --all-targets` | 0 | - | 8 s |
| 3 | `cargo build --workspace --bins` | 0 | - | 0 s |
| 4 | `cargo test --workspace --no-fail-fast` | 101 | 388 passed, 47 failed, 1 ignored | 43 s |
| 5 | `HORCH_TEAMMATES_DIR=teammates cargo run --quiet --bin horch -- teammates --check` | 0 | roster ok: 33 teammates, 28 offered | 0 s |
| 6 | `./scripts/check-req-coverage.sh` | 0 | 38 IDs, 0 missing | 1 s |
| 7 | `./scripts/check-deps.sh` | 0 | - | 0 s |
| 8 | `./scripts/verify-telemetry-e2e.sh` | 101 | `the_hermetic_story` failed | 3 s |

Build times are warm-cache times. `scripts/phase-gate.sh` and `just gate` did
not exist on the base.

## Failures and fixes

Each root cause has its own commit on `ard/a0-gate`. No test was weakened.

| Failing tests | Root cause | Fix (commit subject) |
|---|---|---|
| 21 in `horch-core` `telemetry_collect` and `telemetry_readers` (`tel_01`..`tel_11`, `expected_md_*`, `a_ledger_mid_write_keeps_its_last_good_copy`) | `tests/common/mod.rs` passed `opencode.sql` to `sqlite3` as an argument. The SQL starts with `--`, so `sqlite3` read it as an option and exited 1. | `A0: Fix baseline opencode fixture build (SQL on stdin)` (also `horch-e2e/tests/e2e.rs`) |
| 20 in `horch-e2e` `e2e.rs` (`bal_04`..`06`, `quo_01`..`07`, `spc_02`, `spc_03`, `spc_05`, `tel_02`, `tel_10`, `tel_11`) | The same `--` argument bug, and `Harness::new` copied `/usr/bin/sqlite3` into the sealed bin dir. macOS kills a copied system binary (exit 137). | `A0: Fix baseline opencode fixture build (SQL on stdin)`, `A0: Fix baseline e2e sqlite3 (symlink, not copy)` |
| `nfr_01_no_real_binaries` | The test ran `/bin/sh -c env` with a sealed PATH that holds only the fakes; `env` was not found (127). | `A0: Fix baseline nfr_01_no_real_binaries` (`/usr/bin/env`) |
| `quota::tests::quo_06_opencode_cooldown` | A Zen refusal hit the 5 h refusal fallback (`refusal_in_force`, no windows) and returned `Exhausted` before the cooldown check. | `A0: Fix baseline quo_06_opencode_cooldown` (skip the refusal check for `POOL_ZEN`) |
| `teammates::spawnable_tests::roster_check_demands_the_subagent_deny_on_every_claude_fleet_pane` | The expected count 11 predates PR #11, which added 8 Claude personas. | `A0: Fix baseline roster_check_demands_...` (11 -> 19) |
| `balance_policy::tests::bal_03_decision_table` | Fixture `quota/claude-tight-codex-close.json` gave codex pace 2.9x, so the pace rule made codex Tight; the test needs codex Ok. | `A0: Fix baseline bal_03_decision_table` (codex `resets_at` -> `2026-09-28T23:00:00Z`; pace rule unchanged) |
| `cmd::teammatescmd::tests::the_template_documents_exactly_the_teammate_fields` | `Teammate.fallbacks` has `skip_serializing_if = Vec::is_empty`, so `Teammate::default()` omitted the key. `_template.md` was right. | `A0: Fix baseline the_template_documents_...` (test serializes a Teammate with 1 fallback) |
| `cmd::telemetry::tests::spc_04_render_every_group_and_window_fits` | At 80 columns the header's final clip cut off `window: <w>`. | `A0: Fix baseline spc_04_render_every_group_and_window_fits` (clip the left part first) |
| `cmd::telemetry::tests::spc_04_render_goldens`, `the_hermetic_story` (and so `verify-telemetry-e2e.sh`) | The telemetry render goldens were missing (known gap). | `A0: Bless telemetry render goldens` |

## Oracles re-frozen after the fixes

The U02 oracles were frozen from the unfixed base. After the fixes,
`HORCH_BLESS=1` for `baseline_oracles` and `baseline_cli` changed 1 file
(`A0: Re-freeze oracles after baseline fixes`):

| Oracle file | Cause |
|---|---|
| `crates/horch-core/tests/oracles/routing/claude-tight-codex-close.json` | The `bal_03` fixture fix. Codex is now `ok` (7d 80%) instead of `tight` (pace 2.9x): 225 pool rows change state and detail; notes and reasons now name codex as ok. No decision kind and no `via` changed. |

The `quo_06` fix changes no oracle: no routing fixture holds a Zen refusal.

## After the fixes

All 8 commands exit 0, and `cargo test --workspace --no-fail-fast` has 0
failures. The fleet pane's `HORCH_*` and `HERDR_*` variables do not change
the result.
