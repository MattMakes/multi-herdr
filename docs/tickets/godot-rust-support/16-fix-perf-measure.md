# 16: Correct godot-perf-measure and the percentile convention

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus` · **Parallel:** yes
**Kind:** docs
**Check:** `cargo test -p horch-core --test godot_rust_support -- grs_22 grs_23 grs_24 grs_25 grs_26 && python3 scripts/godot/xref_check.py`
**Own:** `skills/godot-perf-measure/SKILL.md`, `skills/godot-perf-measure/references/shader-warm-up.md`, `skills/godot-optimization/references/measuring-performance.md`, `crates/horch-core/tests/godot_rust_support/perf_measure.rs`

Spec: `docs/specs/godot-rust-support.md` (Implementation Decisions 7, corrections).
Waits for: 01 (Scaffold the godot_rust_support test binary). Wave 2.

## Goal

godot-perf-measure and the percentile reference say what the HFR reports verified.

## Requirements

- **GRS-22**: `skills/godot-perf-measure/SKILL.md` (lines 54-56) no longer says saved settings can force V-Sync back on after `--disable-vsync`. It says the flag forces V-Sync off even when the project enables it, only driver-level enforcement can defeat it, and the scratch settings file stays for reproducibility. Tests: grs_22_perf_measure_says_disable_vsync_overrides_project_setting.
- **GRS-23**: `skills/godot-perf-measure/SKILL.md` (lines 48-50 and 122-123) says `Performance.TIME_PROCESS` and `TIME_PHYSICS_PROCESS` are 1-second peak values, not per-frame samples, and that per-frame rows use callback-to-callback `Time.get_ticks_usec()` wall time. Tests: grs_23_perf_measure_labels_time_process_as_one_second_peak.
- **GRS-24**: `skills/godot-perf-measure/SKILL.md` (lines 57-60) no longer names Metal as a driver that returns 0 GPU timers or prescribes MoltenVK for it. It says to detect and report 0 or unavailable GPU timers at run time on any driver. Tests: grs_24_perf_measure_does_not_name_metal_as_zero_gpu_timer_driver.
- **GRS-25**: `skills/godot-perf-measure/references/shader-warm-up.md` (lines 6-24) says Forward+ and Mobile have automatic ubershader and pipeline precompilation since Godot 4.4, the pipeline compilation monitors find the missing path, and only Compatibility needs a first-draw warm-up of every variant. Tests: grs_25_shader_warm_up_scopes_first_draw_warm_up_to_compatibility.
- **GRS-26**: `skills/godot-optimization/references/measuring-performance.md` (lines 105-110) uses nearest-rank percentiles (`ceil(p * n) - 1`), names that convention, and reports 1% low FPS as `1000 / p99_frame_ms` without averaging FPS. Tests: grs_26_measuring_performance_uses_named_nearest_rank_and_one_percent_low.

## Steps

1. Read the matching sections of `ai_docs/reports/godot-hfr/1-script-cpu.md` to `5-native-memory-hitches.md` for each row.
2. Write 1 `grs_<nn>_` test per row first.
3. Change only the named claim. Do not restructure the file. The test checks the old text is gone and the new claim is present, not line numbers.
4. Name each test with its `grs_<nn>_` prefix. The test names in the spec's Requirement Table are the target names.
5. Run `horch ledger finish`. When every step passes, run `horch ledger done "<summary>"`.

## Done when

The Check passes for all 5 rows.
