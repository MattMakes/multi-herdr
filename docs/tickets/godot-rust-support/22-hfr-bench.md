# 22: hfr_bench.py, and wire godot-hfr-hunt to the performance engineer

<!-- horch-ticket: 1 -->
**Persona:** primary `godot-performance-engineer`, fallback `opus` · **Parallel:** yes
**Kind:** feature
**Check:** `python3 -m unittest discover -s scripts/godot/tests -p 'test_hfr_bench.py' && cargo test -p horch-core --test godot_rust_support -- grs_20 grs_43 grs_44 && cargo test -p horch-core --test skills_catalog && cargo test -p horch-core --test godot_wave && python3 scripts/comments/comment_lint.py check`
**Own:** `skills/godot-hfr-hunt/scripts/hfr_bench.py`, `scripts/godot/tests/test_hfr_bench.py`, `crates/horch-core/tests/skills_catalog.rs`, `teammates/godot-performance-engineer.md`, `crates/horch-core/tests/godot_rust_support/hfr_bench.rs`

Spec: `docs/specs/godot-rust-support.md` (Implementation Decisions 7, harness, wiring and skill gate, and Open Question 5).
Waits for: 01 (Scaffold the godot_rust_support test binary); 21 (godot-hfr-hunt: the 4 technique reference files). Wave 7.

## Goal

`hfr_bench.py compare` gives 1 exact verdict and an exit code that stops a slower candidate from landing, and the performance engineer expects the skill.

## Requirements

- **GRS-20**: `skills/godot-hfr-hunt/scripts/hfr_bench.py` (Python 3 standard library, mode `0755`) has `summarize <take.json>` and `compare --baseline --candidate --scenario --out [--pairs 5] [--target-fps 240]` (`--target-fps` an integer from 200 to 300, default 240). Each take runs `<export> --windowed --resolution 1920x1080 --disable-vsync --max-fps 0 -- --hfr-bench=<name> --warmup-seconds=5 --sample-seconds=30 --hfr-out=<path>`. The take JSON (`schema_version: 1`) holds `frame_ms_p50`, `frame_ms_p95`, `frame_ms_p99`, `worst_frame_ms`, `one_percent_low_fps` (`1000 / frame_ms_p99`), `over_3_333_ms`, `over_4_167_ms` and raw frames, with nearest-rank percentiles (`ceil(p * n) - 1`) in ms. `compare` prints exactly 1 of `PASS`, `FRAME_TIME_REGRESSION`, `<target>_FPS_BUDGET_FAILURE`, `INFRASTRUCTURE_INVALID`, where the budget is `1000 / target` ms against the median candidate p99. It reports a regression only with >= 3 valid pairs, >= 4 of 5 positive p99 deltas (all positive when 3 or 4 are valid) and a median p99 delta > `max(0.10 ms, baseline_p99 * 0.05)`. It exits `0` on `PASS`, `1` on a regression or the `--target-fps` budget failure (only the target budget exits `1`), and `2` on `INFRASTRUCTURE_INVALID`. Tests: grs_20_hfr_bench_python_suite_passes; Python: test_hfr_bench_summarize_uses_nearest_rank_percentiles, test_hfr_bench_compare_flags_regression_on_four_of_five_pairs, test_hfr_bench_compare_ignores_delta_below_noise_floor, test_hfr_bench_compare_reports_budget_failure_without_regression, test_hfr_bench_compare_is_infrastructure_invalid_below_three_pairs, test_hfr_bench_runs_fake_export_without_godot.
- **GRS-43**: `teammates/godot-performance-engineer.md` lists `godot-hfr-hunt` in `skills:` and moves `godot-assets-pipeline` to `available_skills:`, so the seat expects 9 skills, or 11 in a `*.csproj` project, within the `GDW-08` cap. Tests: grs_43_performance_engineer_expects_hfr_hunt_within_eleven_skill_cap.
- **GRS-44**: `godot-hfr-hunt` is registered in `skills/copied.json` (`"copied_files": []`), `REPO_ORIGINAL` and `skills/README.md`, `godot-hfr-hunt/scripts/hfr_bench.py` is in `EXEMPT_FILES` with mode `0755`, the skill passes the `godot_skills` gate step (`scripts/godot/tests`, `xref_check.py`, `api_check.py`, `gdscript_blocks_check.py`, `csharp_blocks_check.py`) and `comment_lint.py`, and `scripts/godot/tests/test_hfr_bench.py` runs without the Godot binary. Tests: grs_44_hfr_hunt_is_registered_exempt_and_executable; Python: test_hfr_bench_runs_fake_export_without_godot.

## Steps

1. Write the 6 `test_hfr_bench_*` tests of GRS-20 first. A fake export script writes take files from fixed frame lists, as `test_godot_run.py` does for `godot-run.sh`.
2. Write `hfr_bench.py` (Python 3 standard library, mode `0755`, no comments) with `summarize` and `compare` as the spec says: take file schema, nearest-rank percentiles, pair validity, the regression rule and the exact verdict lines.
3. `--target-fps` takes an integer from 200 to 300, default 240. The budget is `1000 / target` ms against the median candidate p99. Only the target budget exits 1. Print the 240 and 300 fps budget results as information lines.
4. Add `godot-hfr-hunt/scripts/hfr_bench.py` to `EXEMPT_FILES` and assert mode `0755` in `skills_catalog.rs`.
5. Add `godot-hfr-hunt` to `skills:` and move `godot-assets-pipeline` to `available_skills:` in `godot-performance-engineer.md`.
6. Write `grs_20_hfr_bench_python_suite_passes`, `grs_43_performance_engineer_expects_hfr_hunt_within_eleven_skill_cap` and `grs_44_hfr_hunt_is_registered_exempt_and_executable`.
7. Run the `godot_skills` step of `scripts/phase-gate.sh` by hand once and fix what it finds in files you own.
8. Warm every new fake executable once with a `warm` argument before a test runs it (ADR 0111, `crates/horch-core/tests/common/warm.rs`). A submodule that needs it declares `#[path = "../common/warm.rs"] mod warm;` itself.
9. Write no code comments. `python3 scripts/comments/comment_lint.py check` must pass.
10. Name each test with its `grs_<nn>_` prefix. The test names in the spec's Requirement Table are the target names.
11. Run `horch ledger finish`. When every step passes, run `horch ledger done "<summary>"`.

## Done when

The Check passes. `compare` on fake takes with a 4 of 5 positive p99 delta above the noise floor prints `FRAME_TIME_REGRESSION` and exits 1.
