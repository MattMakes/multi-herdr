use std::path::Path;

fn read(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../skills/godot-optimization")
        .join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn physics() -> String {
    read("references/physics-tuning.md")
}

fn absent(file: &str, text: &str, old: &str) {
    assert!(!text.contains(old), "{file} still says: {old}");
}

fn present(file: &str, text: &str, new: &str) {
    assert!(text.contains(new), "{file} does not say: {new}");
}

#[test]
fn grs_35_physics_tuning_explains_max_physics_steps_slow_motion() {
    let p = physics();
    absent("physics-tuning.md", &p, "to prevent the spiral-of-death");
    present("physics-tuning.md", &p, "trades catch-up for slow motion");
    present("physics-tuning.md", &p, "default cap of 8");
    present("physics-tuning.md", &p, "below 7.5 rendered fps");
    present("physics-tuning.md", &p, "without a higher cap raises that threshold");
}

#[test]
fn grs_36_physics_tuning_compares_areas_and_direct_queries() {
    let p = physics();
    absent("physics-tuning.md", &p, "is cheaper than running a raycast or shape query every frame");
    absent("physics-tuning.md", &p, "PREFER Area2D");
    present("physics-tuning.md", &p, "continuous enter/exit tracking");
    present("physics-tuning.md", &p, "bounded direct query is often cheaper for sparse or on-demand checks");
    present("physics-tuning.md", &p, "real entity count");
    let skill = read("SKILL.md");
    absent("SKILL.md", &skill, "prefer `Area2D/3D` over per-frame raycasts");
    present("SKILL.md", &skill, "continuous enter/exit tracking");
    present("SKILL.md", &skill, "bounded direct query");
    present("SKILL.md", &skill, "real entity count");
}

#[test]
fn grs_37_physics_tuning_states_concave_versus_convex_crossover() {
    let p = physics();
    absent("physics-tuning.md", &p, "are extremely expensive");
    present("physics-tuning.md", &p, "convex decomposition into many hulls can be slower than 1 static concave shape");
    present("physics-tuning.md", &p, "Primitives suit dynamic bodies");
    present("physics-tuning.md", &p, "simplified concave mesh or `HeightMapShape3D`");
}

#[test]
fn grs_38_physics_tuning_explains_tick_rate_at_high_frame_rate() {
    let p = physics();
    present("physics-tuning.md", &p, "At 240 rendered fps and 60 TPS, most frames have no physics tick");
    present("physics-tuning.md", &p, "about 4 times as frequent");
    present("physics-tuning.md", &p, "Physics interpolation exists to avoid that cost");
}

#[test]
fn grs_39_memory_management_corrects_ready_on_pooled_nodes() {
    let m = read("references/memory-management.md");
    absent("memory-management.md", &m, "re-runs `_ready()`");
    present("memory-management.md", &m, "`_ready()` runs on the first tree entry");
    present("memory-management.md", &m, "`Node.request_ready()`");
    present("memory-management.md", &m, "can cost more than it saves");
    present("memory-management.md", &m, "full acquire/reset/release path");
}
