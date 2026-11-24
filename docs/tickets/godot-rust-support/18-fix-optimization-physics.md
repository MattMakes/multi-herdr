# 18: Correct godot-optimization physics and pooling claims

<!-- horch-ticket: 1 -->
**Persona:** primary `opus`, fallback `opus` · **Parallel:** yes
**Kind:** docs
**Check:** `cargo test -p horch-core --test godot_rust_support -- grs_35 grs_36 grs_37 grs_38 grs_39 && python3 scripts/godot/xref_check.py`
**Own:** `skills/godot-optimization/SKILL.md`, `skills/godot-optimization/references/physics-tuning.md`, `skills/godot-optimization/references/memory-management.md`, `crates/horch-core/tests/godot_rust_support/optimization_physics.rs`

Spec: `docs/specs/godot-rust-support.md` (Implementation Decisions 7, corrections).
Waits for: 01 (Scaffold the godot_rust_support test binary); 17 (Correct godot-optimization CPU and draw-call claims). Wave 3.

## Goal

godot-optimization says what the HFR reports verified about physics tick rate, shapes, queries and pooling.

## Requirements

- **GRS-35**: `skills/godot-optimization/references/physics-tuning.md` (lines 89-92) says `max_physics_steps_per_frame` trades catch-up for slow motion: at 60 TPS and the default cap of 8, game time slows below 7.5 rendered fps, and a higher TPS without a higher cap raises that threshold. Tests: grs_35_physics_tuning_explains_max_physics_steps_slow_motion.
- **GRS-36**: `skills/godot-optimization/SKILL.md` (line 106) and `skills/godot-optimization/references/physics-tuning.md` (line 108) no longer always prefer `Area2D/3D` to raycasts. They say an Area fits continuous enter/exit tracking, a bounded direct query is often cheaper for sparse or on-demand checks, and both are measured at the real entity count. Tests: grs_36_physics_tuning_compares_areas_and_direct_queries.
- **GRS-37**: `skills/godot-optimization/references/physics-tuning.md` (lines 63-77) says a convex decomposition into many hulls can be slower than 1 static concave shape, primitives suit dynamic bodies, and large static geometry uses a simplified concave mesh or `HeightMapShape3D`. Tests: grs_37_physics_tuning_states_concave_versus_convex_crossover.
- **GRS-38**: `skills/godot-optimization/references/physics-tuning.md` (line 81) says that at 240 rendered fps and 60 TPS most frames have no physics tick, that 240 TPS makes physics and `_physics_process` work about 4 times as frequent, and that physics interpolation exists to avoid that cost. Tests: grs_38_physics_tuning_explains_tick_rate_at_high_frame_rate.
- **GRS-39**: `skills/godot-optimization/references/memory-management.md` (line 136) no longer says each instantiate/free cycle re-runs `_ready()` for a pooled node. It says `_ready()` runs on the first tree entry, re-adding a node needs `Node.request_ready()` for another call, and a pool can cost more than it saves, so the full acquire/reset/release path is measured. Tests: grs_39_memory_management_corrects_ready_on_pooled_nodes.

## Steps

1. Read the matching sections of the 5 HFR reports for each row.
2. Write 1 `grs_<nn>_` test per row first.
3. Change only the named claim. Do not restructure the file. The test checks the old text is gone and the new claim is present, not line numbers.
4. Edit only line 106 of `skills/godot-optimization/SKILL.md` (GRS-36). Ticket 17 owns the other claims in that file.
5. Name each test with its `grs_<nn>_` prefix. The test names in the spec's Requirement Table are the target names.
6. Run `horch ledger finish`. When every step passes, run `horch ledger done "<summary>"`.

## Done when

The Check passes for all 5 rows.
