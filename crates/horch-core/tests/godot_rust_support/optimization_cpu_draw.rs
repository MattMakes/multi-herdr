use std::path::Path;

fn read(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../skills/godot-optimization")
        .join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn skill() -> String {
    read("SKILL.md")
}

fn cpu() -> String {
    read("references/cpu-bottlenecks.md")
}

fn draw() -> String {
    read("references/draw-calls.md")
}

fn absent(file: &str, text: &str, old: &str) {
    assert!(!text.contains(old), "{file} still says: {old}");
}

fn present(file: &str, text: &str, new: &str) {
    assert!(text.contains(new), "{file} does not say: {new}");
}

#[test]
fn grs_27_cpu_bottlenecks_says_vectors_are_inline_values_not_gc_allocations() {
    let cpu = cpu();
    absent("cpu-bottlenecks.md", &cpu, "Vector2/3 via constructor");
    absent("cpu-bottlenecks.md", &cpu, "triggers the garbage collector");
    absent("cpu-bottlenecks.md", &cpu, "100 allocations per frame");
    absent("cpu-bottlenecks.md", &cpu, "reuse a variable declared outside the loop");
    present("cpu-bottlenecks.md", &cpu, "stored inline in `Variant`");
    present("cpu-bottlenecks.md", &cpu, "GDScript has no .NET garbage collector");
    present("cpu-bottlenecks.md", &cpu, "managed C# objects");
    let skill = skill();
    absent("SKILL.md", &skill, "or Vector constructors per frame");
    present("SKILL.md", &skill, "stored inline in `Variant`");
}

#[test]
fn grs_28_cpu_bottlenecks_says_typed_array_writes_are_validated() {
    let cpu = cpu();
    absent("cpu-bottlenecks.md", &cpu, "skip per-element type checks");
    absent("cpu-bottlenecks.md", &cpu, "no per-element type check");
    present("cpu-bottlenecks.md", &cpu, "typed `Array` write calls the type validator");
    present("cpu-bottlenecks.md", &cpu, "specialize");
    present("cpu-bottlenecks.md", &cpu, "contiguous value storage");
}

#[test]
fn grs_29_cpu_bottlenecks_says_string_comparison_does_not_allocate() {
    let cpu = cpu();
    absent("cpu-bottlenecks.md", &cpu, "allocates temporaries");
    present("cpu-bottlenecks.md", &cpu, "without building a new string");
    present("cpu-bottlenecks.md", &cpu, "interned pointers");
}

#[test]
fn grs_30_optimization_uses_explicit_update_tiers_not_physics_throttling() {
    let skill = skill();
    absent("SKILL.md", &skill, "Move logic to `_physics_process` (runs less often)");
    present("SKILL.md", &skill, "Keep simulation in `_physics_process`");
    present("SKILL.md", &skill, "explicit cadence or budget");
    present("SKILL.md", &skill, "accumulated `delta` or a `Timer`, not on a frame count");
}

#[test]
fn grs_31_optimization_does_not_prescribe_canvas_group_for_batching() {
    let skill = skill();
    absent("SKILL.md", &skill, "wrap 2D groups sharing a texture in `CanvasGroup`");
    absent("SKILL.md", &skill, "Use `CanvasGroup` for 2D batching");
    absent("SKILL.md", &skill, "are wrapped in `CanvasGroup`");
    absent("SKILL.md", &skill, "CanvasGroup batching constraints");
    present("SKILL.md", &skill, "batches compatible 2D work automatically in every renderer");
    let draw = draw();
    absent("draw-calls.md", &draw, "2D Batching with CanvasGroup");
    absent("draw-calls.md", &draw, "the node type alone enables batching");
    absent("draw-calls.md", &draw, "Godot batches them into a single draw call");
    present("draw-calls.md", &draw, "batches compatible 2D work automatically in every renderer");
    present("draw-calls.md", &draw, "backbuffer-compositing node");
}

#[test]
fn grs_32_draw_calls_uses_instance_uniforms_not_duplicated_materials() {
    let draw = draw();
    absent("draw-calls.md", &draw, "shared_material.duplicate()");
    absent("draw-calls.md", &draw, "SharedMaterial.Duplicate()");
    absent("draw-calls.md", &draw, "All instances share the same material; vary only the instance parameter");
    present("draw-calls.md", &draw, "instance uniform");
    present("draw-calls.md", &draw, "set_instance_shader_parameter(");
    present("draw-calls.md", &draw, "SetInstanceShaderParameter(");
    present("draw-calls.md", &draw, "only when an instance uniform cannot hold the difference");
}

#[test]
fn grs_33_draw_calls_says_gi_mode_does_not_merge_meshes() {
    let draw = draw();
    absent("draw-calls.md", &draw, "to let the engine merge static geometry");
    present("draw-calls.md", &draw, "opts the geometry into global illumination");
    present("draw-calls.md", &draw, "does not merge meshes");
    present("draw-calls.md", &draw, "asset pipeline");
    present("draw-calls.md", &draw, "HLOD mesh");
}

#[test]
fn grs_34_draw_calls_uses_mesh_lod_and_visibility_ranges() {
    let draw = draw();
    absent("draw-calls.md", &draw, "`VisibleOnScreenNotifier3D` distance thresholds");
    absent("draw-calls.md", &draw, "Manually swap mesh at distance");
    absent("draw-calls.md", &draw, "distance_to(_camera.global_position)");
    present("draw-calls.md", &draw, "Generate LODs");
    present("draw-calls.md", &draw, "visibility_range_end");
    present("draw-calls.md", &draw, "visibility_parent");
    present("draw-calls.md", &draw, "hysteresis");
}
