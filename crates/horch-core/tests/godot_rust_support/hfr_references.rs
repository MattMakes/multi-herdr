use std::fs;
use std::path::Path;

fn repo_path(rel: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel)
}

fn read_repo_file(rel: &str) -> String {
    let path = repo_path(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn ranked_techniques(text: &str) -> Vec<&str> {
    let from = text
        .find("\n## Ranked techniques\n")
        .expect("missing ## Ranked techniques");
    let rest = &text[from..];
    let rest = &rest[1..];
    let end = rest[3..].find("\n## ").map_or(rest.len(), |i| i + 3);
    rest[..end].split("\n### ").skip(1).collect()
}

fn dir_bytes(dir: &Path) -> u64 {
    fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read dir {}: {e}", dir.display()))
        .map(|entry| {
            let entry = entry.expect("dir entry");
            let meta = entry.metadata().expect("metadata");
            if meta.is_dir() {
                dir_bytes(&entry.path())
            } else {
                meta.len()
            }
        })
        .sum()
}

const ANGLES: [(&str, &str); 5] = [
    (
        "skills/godot-hfr-hunt/references/script-cpu.md",
        "ai_docs/reports/godot-hfr/1-script-cpu.md",
    ),
    (
        "skills/godot-hfr-hunt/references/render-gpu.md",
        "ai_docs/reports/godot-hfr/2-render-gpu.md",
    ),
    (
        "skills/godot-hfr-hunt/references/physics-servers-threads.md",
        "ai_docs/reports/godot-hfr/3-physics-servers-threads.md",
    ),
    (
        "skills/godot-hfr-hunt/references/measure-harness.md",
        "ai_docs/reports/godot-hfr/4-measure-harness.md",
    ),
    (
        "skills/godot-hfr-hunt/references/native-memory-hitches.md",
        "ai_docs/reports/godot-hfr/5-native-memory-hitches.md",
    ),
];

const LABELS: [&str; 7] = [
    "- **What:**",
    "- **Why/thread:**",
    "- **Measure:**",
    "- **Version:**",
    "- **Source:**",
    "- **Evidence:**",
    "- **Trap:**",
];

#[test]
fn grs_19_hfr_hunt_references_have_one_file_per_angle_with_measure_evidence_and_trap() {
    let mut files: Vec<String> = fs::read_dir(repo_path("skills/godot-hfr-hunt/references"))
        .expect("references dir")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    files.sort();
    assert_eq!(
        files,
        [
            "detection.md",
            "measure-harness.md",
            "native-memory-hitches.md",
            "physics-servers-threads.md",
            "render-gpu.md",
            "script-cpu.md",
        ]
    );

    for (reference, report) in ANGLES {
        let text = read_repo_file(reference);
        assert!(text.starts_with("# "), "{reference} lacks a title");
        assert!(!text.contains("## Corrections"), "{reference} copies corrections");
        assert!(text.contains("4.7.2"), "{reference} lacks the checked version");

        let ours = ranked_techniques(&text);
        let report_text = read_repo_file(report);
        let theirs = ranked_techniques(&report_text);
        assert_eq!(
            ours.len(),
            theirs.len(),
            "{reference} has {} techniques, {report} has {}",
            ours.len(),
            theirs.len()
        );

        for (index, (technique, original)) in ours.iter().zip(&theirs).enumerate() {
            let number = format!("{}. ", index + 1);
            assert!(
                technique.starts_with(&number),
                "{reference} technique {} is not numbered {number:?}",
                index + 1
            );
            let mut last = 0;
            for label in LABELS {
                let at = technique.find(label).unwrap_or_else(|| {
                    panic!("{reference} technique {} lacks {label}", index + 1)
                });
                assert!(
                    at >= last,
                    "{reference} technique {} has {label} out of order",
                    index + 1
                );
                last = at;
            }
            if !original.contains("](https://") {
                continue;
            }
            let source = &technique[technique.find("- **Source:**").expect("source")..];
            let source = &source[..source.find('\n').unwrap_or(source.len())];
            assert!(
                source.contains("](https://"),
                "{reference} technique {} has no source link",
                index + 1
            );
        }
    }

    let total = dir_bytes(&repo_path("skills/godot-hfr-hunt"));
    assert!(total <= 160 * 1024, "skills/godot-hfr-hunt is {total} bytes");
}
