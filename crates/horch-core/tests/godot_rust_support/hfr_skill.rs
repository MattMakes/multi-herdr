use std::fs;
use std::path::Path;

fn read_repo_file(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn section<'a>(text: &'a str, start: &str, end: &str) -> &'a str {
    let from = text
        .find(start)
        .unwrap_or_else(|| panic!("missing section start {start:?}"));
    let rest = &text[from + start.len()..];
    let to = rest.find(end).unwrap_or(rest.len());
    &rest[..to]
}

fn positions_in_order(text: &str, needles: &[&str]) -> Vec<usize> {
    let positions: Vec<usize> = needles
        .iter()
        .map(|n| {
            text.find(n)
                .unwrap_or_else(|| panic!("missing {n:?} in text"))
        })
        .collect();
    for pair in positions.windows(2) {
        assert!(
            pair[0] < pair[1],
            "out of order: {needles:?} at {positions:?}"
        );
    }
    positions
}

fn rg_commands(detection: &str) -> Vec<String> {
    detection
        .split('`')
        .skip(1)
        .step_by(2)
        .filter(|span| span.starts_with("rg "))
        .map(str::to_string)
        .collect()
}

const SKILL: &str = "skills/godot-hfr-hunt/SKILL.md";
const DETECTION: &str = "skills/godot-hfr-hunt/references/detection.md";
const REPORTS: [&str; 5] = [
    "ai_docs/reports/godot-hfr/1-script-cpu.md",
    "ai_docs/reports/godot-hfr/2-render-gpu.md",
    "ai_docs/reports/godot-hfr/3-physics-servers-threads.md",
    "ai_docs/reports/godot-hfr/4-measure-harness.md",
    "ai_docs/reports/godot-hfr/5-native-memory-hitches.md",
];

#[test]
fn grs_19_hfr_hunt_skill_md_orders_the_five_workflow_steps() {
    let text = read_repo_file(SKILL);
    assert!(text.len() <= 12 * 1024, "SKILL.md is {} bytes", text.len());
    assert!(text.starts_with("---\nname: godot-hfr-hunt\n"));
    assert!(!text.contains("## Corrections"));
    assert!(text.contains("https://docs.godotengine.org/en/4.7/"));
    assert!(text.contains("https://github.com/godotengine/godot/blob/4.7.2-stable/"));

    let steps = [
        "1. **Uncap.**",
        "2. **Run the harness.**",
        "3. **Read the percentiles.**",
        "4. **Classify the hot spot.**",
        "5. **Apply the matching technique.**",
    ];
    positions_in_order(&text, &steps);

    let uncap = section(&text, steps[0], steps[1]);
    for needle in [
        "--disable-vsync",
        "--max-fps 0",
        "Engine.max_fps == 0",
        "OS.low_processor_usage_mode == false",
        "application/run/frame_delay_msec == 0",
        "--fixed-fps",
        "6.9 ms",
        "--headless",
    ] {
        assert!(uncap.contains(needle), "uncap step lacks {needle:?}");
    }

    let harness = section(&text, steps[1], steps[2]);
    assert!(harness.contains("scripts/hfr_bench.py"));
    assert!(harness.contains("references/measure-harness.md"));

    let read = section(&text, steps[2], steps[3]);
    for needle in [
        "p50", "p95", "p99", "worst", "1% low", "5.00 ms", "4.17 ms", "3.33 ms",
    ] {
        assert!(read.contains(needle), "percentile step lacks {needle:?}");
    }

    let classify = section(&text, steps[3], steps[4]);
    let classes = [
        ("Script CPU", "references/script-cpu.md"),
        ("Render thread", "references/render-gpu.md"),
        ("GPU", "references/render-gpu.md"),
        ("Physics", "references/physics-servers-threads.md"),
        ("Spikes", "references/native-memory-hitches.md"),
    ];
    let labels: Vec<String> = classes.iter().map(|(c, _)| format!("**{c}**")).collect();
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    positions_in_order(classify, &labels);
    for (label, (class, reference)) in labels.iter().zip(classes) {
        let row = section(classify, label, "\n");
        assert!(row.contains(reference), "{class} row lacks {reference}");
    }
    assert!(classify.contains("references/detection.md"));
    assert!(classify.contains("escalation ladder"));

    let apply = section(&text, steps[4], "\n## ");
    assert!(apply.contains("hfr_bench.py compare"));
    assert!(apply.contains("`PASS`"));
    assert!(apply.contains("game state"));
}

#[test]
fn grs_21_hfr_detection_reference_keeps_greps_signatures_and_thresholds() {
    let text = read_repo_file(DETECTION);

    let classes = [
        "## Script CPU",
        "## Render thread",
        "## GPU",
        "## Physics",
        "## Spikes",
    ];
    positions_in_order(&text, &classes);

    let mut commands = Vec::new();
    for report in REPORTS {
        let body = read_repo_file(report);
        let found = rg_commands(section(&body, "\n## Detection\n", "\n## "));
        commands.extend(found);
    }
    assert!(commands.len() >= 20, "{} rg commands", commands.len());
    for command in &commands {
        assert!(
            text.contains(command.as_str()),
            "detection.md lacks `{command}`"
        );
    }

    for cap in [
        "application/run/max_fps",
        "application/run/low_processor_mode",
        "application/run/frame_delay_msec",
        "Engine.max_fps",
        "low_processor_usage_mode",
        "window_set_vsync_mode",
        "--fixed-fps",
    ] {
        assert!(text.contains(cap), "cap search lacks {cap:?}");
    }

    for needle in [
        "5% of the budget",
        "0.20 ms at 240 fps",
        "0.17 ms at 300 fps",
        "4.17 ms",
        "3.33 ms",
    ] {
        assert!(text.contains(needle), "thresholds lack {needle:?}");
    }

    let physics = section(&text, "## Physics", "\n## ");
    assert!(physics.contains("PHYSICS_*_COLLISION_PAIRS"));
    assert!(physics.contains("entity count stays flat"));
    assert!(physics.contains("above 60 TPS"));

    let spikes = section(&text, "## Spikes", "\n## ");
    for needle in [
        "`Draw`",
        "`Surface`",
        "gameplay-time",
        "GC event",
        "slow frame",
        "thousands of",
    ] {
        assert!(spikes.contains(needle), "spikes section lacks {needle:?}");
    }

    let render = section(&text, "## Render thread", "\n## GPU");
    for needle in [
        "RENDER_INFO_TYPE_SHADOW",
        "OccluderInstance3D",
        "CanvasGroup",
        "get_frame_setup_time_cpu()",
    ] {
        assert!(render.contains(needle), "render section lacks {needle:?}");
    }
    let gpu = section(&text, "## GPU", "\n## ");
    for needle in [
        "scaling_3d_scale",
        "discard",
        "hint_screen_texture",
        "--gpu-profile",
    ] {
        assert!(gpu.contains(needle), "GPU section lacks {needle:?}");
    }
}
