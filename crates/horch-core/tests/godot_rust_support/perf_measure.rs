use std::fs;
use std::path::Path;

fn read_repo_file(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn section<'a>(text: &'a str, start: &str, end: &str) -> &'a str {
    let from = text
        .find(start)
        .unwrap_or_else(|| panic!("missing section start {start:?}"));
    let rest = &text[from..];
    let to = rest
        .find(end)
        .unwrap_or_else(|| panic!("missing section end {end:?}"));
    &rest[..to]
}

const PERF_SKILL: &str = "skills/godot-perf-measure/SKILL.md";
const SHADER_WARM_UP: &str = "skills/godot-perf-measure/references/shader-warm-up.md";
const MEASURING: &str = "skills/godot-optimization/references/measuring-performance.md";

#[test]
fn grs_22_perf_measure_says_disable_vsync_overrides_project_setting() {
    let text = read_repo_file(PERF_SKILL);
    let lower = text.to_lowercase();
    assert!(!lower.contains("force vsync back on"));
    assert!(!lower.contains("saved user settings force vsync on"));
    let step = section(&text, "3. **Uncap the frame.**", "4. **");
    assert!(step.contains("--disable-vsync"));
    assert!(step.contains("forces V-Sync off even when the project enables it"));
    assert!(step.contains("driver-level"));
    assert!(step.contains("reproducib"));
}

#[test]
fn grs_23_perf_measure_labels_time_process_as_one_second_peak() {
    let text = read_repo_file(PERF_SKILL);
    let step = section(&text, "1. **Measure milliseconds per part", "2. **");
    let sampler = section(&text, "- **Frame sampler:**", "- **A/B pair runner:**");
    for part in [step, sampler] {
        assert!(part.contains("Performance.TIME_PROCESS"), "{part}");
        assert!(part.contains("TIME_PHYSICS_PROCESS"), "{part}");
        assert!(part.contains("1-second peak"), "{part}");
        assert!(part.contains("callback-to-callback"), "{part}");
        assert!(part.contains("Time.get_ticks_usec()"), "{part}");
    }
}

#[test]
fn grs_24_perf_measure_does_not_name_metal_as_zero_gpu_timer_driver() {
    let text = read_repo_file(PERF_SKILL);
    assert!(!text.contains("MoltenVK"));
    assert!(!text.contains("for example Metal"));
    let step = section(&text, "4. **", "5. **");
    assert!(step.contains("run time"));
    assert!(step.contains("any driver"));
    assert!(step.contains("unavailable"));
}

#[test]
fn grs_25_shader_warm_up_scopes_first_draw_warm_up_to_compatibility() {
    let text = read_repo_file(SHADER_WARM_UP);
    assert!(!text.contains("Measure before you build a warm-up on those versions"));
    assert!(text.contains("Forward+ and Mobile"));
    assert!(text.contains("Godot 4.4"));
    assert!(text.contains("ubershader"));
    assert!(text.contains("pipeline precompilation"));
    assert!(text.contains("pipeline compilation monitors"));
    assert!(text.contains("Only the Compatibility renderer needs a first-draw warm-up"));
}

#[test]
fn grs_26_measuring_performance_uses_named_nearest_rank_and_one_percent_low() {
    let text = read_repo_file(MEASURING);
    assert!(!text.contains("int(n * 0.95)"));
    assert!(!text.contains("int(n * 0.99)"));
    assert!(text.contains("nearest-rank"));
    assert!(text.contains("ceil(p * n) - 1"));
    assert!(text.contains("1000.0 / p99"));
    assert!(text.contains("1% low"));
    assert!(text.contains("Do not average FPS"));
}
