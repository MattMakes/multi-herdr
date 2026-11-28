use std::path::Path;

fn skill() -> String {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/godot-multithreading/SKILL.md");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn assert_has(text: &str, needle: &str) {
    assert!(text.contains(needle), "missing: {needle}");
}

fn assert_lacks(text: &str, needle: &str) {
    assert!(!text.contains(needle), "old claim still present: {needle}");
}

#[test]
fn grs_40_multithreading_says_separate_server_threads_are_not_automatically_faster() {
    let text = skill();
    assert_lacks(
        &text,
        "Servers handle thousands of thread-driven instances well.",
    );
    assert_has(&text, "not automatically faster");
    assert_has(&text, "restricted to physics processing");
    assert_has(&text, "Jolt separate-thread support is experimental in 4.7");
    assert_has(&text, "`Separate` thread model has known bugs");
}

#[test]
fn grs_41_multithreading_separates_cpu_preparation_from_gpu_sync() {
    let text = skill();
    assert_lacks(
        &text,
        "**No GPU work off the main thread** (texture creation, image read/modify)",
    );
    assert_has(&text, "plain `Image` and packed-array data");
    assert_has(
        &text,
        "Creating textures, reading GPU-backed image data or uploading from a worker",
    );
    assert_has(&text, "synchronize with `RenderingServer` and stall");
}

#[test]
fn grs_42_multithreading_stages_scene_integration_after_threaded_load() {
    let text = skill();
    assert_lacks(
        &text,
        "add_child(scene.instantiate())\n            set_process(false)",
    );
    assert_has(
        &text,
        "still run on the main thread after `load_threaded_get()`",
    );
    for step in [
        "`instantiate()`",
        "`_enter_tree`",
        "`_ready`",
        "server registration",
        "pipeline surface preparation",
    ] {
        assert_has(&text, step);
    }
    assert_has(&text, "_integrate_next_batch()");
}
