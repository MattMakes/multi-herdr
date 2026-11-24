# 17: Correct godot-optimization CPU and draw-call claims

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus` · **Parallel:** yes
**Kind:** docs
**Check:** `cargo test -p horch-core --test godot_rust_support -- grs_27 grs_28 grs_29 grs_30 grs_31 grs_32 grs_33 grs_34 && python3 scripts/godot/xref_check.py && python3 scripts/godot/gdscript_blocks_check.py --strict-own skills/copied.json skills/godot-optimization/`
**Own:** `skills/godot-optimization/SKILL.md`, `skills/godot-optimization/references/cpu-bottlenecks.md`, `skills/godot-optimization/references/draw-calls.md`, `crates/horch-core/tests/godot_rust_support/optimization_cpu_draw.rs`

Spec: `docs/specs/godot-rust-support.md` (Implementation Decisions 7, corrections).
Waits for: 01 (Scaffold the godot_rust_support test binary). Wave 2.

## Goal

godot-optimization stops teaching wrong CPU, batching, material and LOD rules.

## Requirements

- **GRS-27**: `skills/godot-optimization/references/cpu-bottlenecks.md` (lines 13 and 82-97) and `skills/godot-optimization/SKILL.md` (line 145) no longer say a temporary `Vector2`/`Vector3` constructor allocates or causes a garbage-collector pause. They say small vectors are stored inline in `Variant`, GDScript has no .NET garbage collector, and `Array`, `Dictionary`, `String` and managed C# objects allocate in different ways. Tests: grs_27_cpu_bottlenecks_says_vectors_are_inline_values_not_gc_allocations.
- **GRS-28**: `skills/godot-optimization/references/cpu-bottlenecks.md` (lines 176-185) no longer says typed arrays skip per-element type checks. It says a typed `Array` write calls the type validator, typing gives safety and lets the compiler specialize nearby expressions, and packed arrays give contiguous value storage. Tests: grs_28_cpu_bottlenecks_says_typed_array_writes_are_validated.
- **GRS-29**: `skills/godot-optimization/references/cpu-bottlenecks.md` (line 119) no longer says `String` comparison allocates temporaries. It says the comparison scans without a new string, and `StringName == StringName` is cheaper because it compares interned pointers. Tests: grs_29_cpu_bottlenecks_says_string_comparison_does_not_allocate.
- **GRS-30**: `skills/godot-optimization/SKILL.md` (line 133) no longer tells the reader to move heavy `_process()` work to `_physics_process()` because it runs less often. It says to keep simulation in physics and give delay-tolerant work an explicit cadence or budget, scheduled on accumulated `delta` or a timer, not on a frame count. Tests: grs_30_optimization_uses_explicit_update_tiers_not_physics_throttling.
- **GRS-31**: `skills/godot-optimization/SKILL.md` (lines 98, 132 and 164) and `skills/godot-optimization/references/draw-calls.md` (lines 11-20) no longer prescribe `CanvasGroup` as the 2D batcher. They say Godot 4.7 batches compatible 2D work automatically in every renderer, and `CanvasGroup` is a backbuffer-compositing node that can add cost. Tests: grs_31_optimization_does_not_prescribe_canvas_group_for_batching.
- **GRS-32**: `skills/godot-optimization/references/draw-calls.md` (lines 33-40 and 54-61) no longer labels a `duplicate()`d `ShaderMaterial` as shared. It uses `instance uniform` with `set_instance_shader_parameter()` and duplicates only when an instance uniform cannot hold the difference. Tests: grs_32_draw_calls_uses_instance_uniforms_not_duplicated_materials.
- **GRS-33**: `skills/godot-optimization/references/draw-calls.md` (line 65) no longer says `GeometryInstance3D.gi_mode = BAKE_STATIC` lets the engine merge static geometry. It says that property opts geometry into GI, and static merging is done in the asset pipeline or with an HLOD mesh. Tests: grs_33_draw_calls_says_gi_mode_does_not_merge_meshes.
- **GRS-34**: `skills/godot-optimization/references/draw-calls.md` (lines 168-198) no longer uses `VisibleOnScreenNotifier3D` and a per-frame script for distance LOD. It uses imported mesh LOD and `GeometryInstance3D` visibility ranges with HLOD dependencies and hysteresis. Tests: grs_34_draw_calls_uses_mesh_lod_and_visibility_ranges.

## Steps

1. Read the matching sections of the 5 HFR reports for each row.
2. Write 1 `grs_<nn>_` test per row first.
3. Change only the named claim. Do not restructure the file. The test checks the old text is gone and the new claim is present, not line numbers.
4. Keep every changed GDScript block valid for `gdscript_blocks_check.py`. Check its arguments with `--help` first.
5. Name each test with its `grs_<nn>_` prefix. The test names in the spec's Requirement Table are the target names.
6. Run `horch ledger finish`. When every step passes, run `horch ledger done "<summary>"`.

## Done when

The Check passes for all 8 rows.
