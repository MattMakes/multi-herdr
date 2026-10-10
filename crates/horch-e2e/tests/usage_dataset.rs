//! End to end: `horch cost` sees every candidate and the judge of a dataset
//! round (EXP-07).
//!
//! Candidates and the judge are ordinary executions in the project's ledger
//! (ARC-24), so the telemetry readers price them from their transcripts like
//! any worker. The fakes write a Claude transcript for each candidate and
//! for the judge.

use std::path::PathBuf;
use std::process::{Command, Output};

use horch_e2e::bin_dir;
use horch_e2e::harness::Harness;
use serde_json::{json, Value};

/// Two Claude candidates: the codex fake writes no token counts.
const KEEP: [&str; 2] = ["sonnet", "opus"];

fn dataset_bin() -> PathBuf {
    let path = bin_dir().join(format!(
        "multi-herdr-dataset{}",
        std::env::consts::EXE_SUFFIX
    ));
    assert!(
        path.is_file(),
        "{} is not built; run `cargo build --workspace --bins` first",
        path.display()
    );
    path
}

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// Every teammate in the repo roster except [`KEEP`].
fn exclude() -> Vec<Value> {
    let dir = horch_e2e::harness::repo_root().join("teammates");
    let mut out: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.strip_suffix(".md").map(str::to_string)
        })
        .filter(|n| n != "README" && !n.starts_with('_') && !KEEP.contains(&n.as_str()))
        .collect();
    out.sort();
    out.into_iter().map(Value::from).collect()
}

/// `<log><suffix>`: where the fakes read their scripts.
fn script(h: &Harness, suffix: &str) -> PathBuf {
    let mut file = h.log.clone().into_os_string();
    file.push(suffix);
    file.into()
}

#[test]
fn exp_07_candidate_and_judge_in_usage() {
    let mut h = Harness::new("exp07").with_git();
    if h.git_bin().is_none() {
        return;
    }
    let machine = h.root.join("machine.json");
    std::fs::write(
        &machine,
        json!({
            "os": "macos", "arch": "arm64", "cpus": 18,
            "mem_total_bytes": 137_438_953_472u64,
            "mem_available_bytes": 85_899_345_920u64,
            "disk_free_bytes": 500_000_000_000u64,
            "disk_total_bytes": 994_662_584_320u64,
            "gpu": "apple_silicon", "max_open_files": 10240, "max_processes": 8000
        })
        .to_string(),
    )
    .unwrap();
    h.set("HORCH_MACHINE_FILE", machine.to_string_lossy());
    h.set("HORCH_FAKE_SCENARIO", "exec");
    h.set(
        "HORCH_QUOTA_FILE",
        horch_e2e::harness::fixtures()
            .join("quota/all-ok.json")
            .to_string_lossy(),
    );
    let dir = h.project.join(".multi-herdr");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("dataset.yaml"),
        format!(
            "exclude: {}\ncaps:\n  candidate_deadline_s: 60\n",
            Value::from(exclude())
        ),
    )
    .unwrap();
    for args in [
        &["add", "-A"][..],
        &["commit", "--quiet", "--no-gpg-sign", "-m", "dataset config"],
    ] {
        let out = h.git_cmd(args).output().unwrap();
        assert!(out.status.success(), "{}", text(&out));
    }
    let candidate = |file: &str, input: u64| {
        json!({"write": {file: "hello\n"}, "commit": true, "exit": "done",
               "usage": {"model": "claude-sonnet-5", "input": input, "output": 500}})
    };
    std::fs::write(
        script(&h, ".candidates.json"),
        json!({"A": candidate("a.txt", 1000), "B": candidate("b.txt", 2000)}).to_string(),
    )
    .unwrap();
    std::fs::write(
        script(&h, ".judge.json"),
        json!({"attempts": ["valid"],
               "usage": {"model": "claude-opus-5-5", "input": 3000, "output": 700}})
        .to_string(),
    )
    .unwrap();

    let mut cmd = Command::new(dataset_bin());
    cmd.args([
        "run",
        "add a greeting",
        "--candidates",
        "2",
        "--budget-usd",
        "100",
    ]);
    h.seal(&mut cmd);
    let out = cmd.output().unwrap();
    assert_eq!(out.status.code(), Some(0), "{}", text(&out));

    // A filter makes 1 report on stdout; this one covers every call.
    let out = h.run(&["cost", "--json", "--since", "2000-01-01"]);
    assert!(out.status.success(), "{}", text(&out));
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["not_priced"], json!([]), "{report:#}");
    let rows = report["rows"].as_array().unwrap();
    let row = |pred: &dyn Fn(&Value) -> bool| -> Value {
        rows.iter()
            .find(|r| pred(r))
            .cloned()
            .unwrap_or_else(|| panic!("no row: {report:#}"))
    };
    for (label, input) in [("A", 1000), ("B", 2000)] {
        let r = row(&|r| r["role"] == format!("candidate-{label}"));
        assert_eq!(r["tokens"]["input"], input, "{r}");
        assert_eq!(r["tokens"]["output"], 500, "{r}");
        assert!(r["cost"].as_f64().unwrap() > 0.0, "{r}");
    }
    // One judge run: a second one (a valid answer misread as Lost or
    // Malformed) is a real second session, and `cost` prices both (F4).
    let judge_runs = std::fs::read_to_string(script(&h, ".judge.count")).unwrap();
    assert_eq!(judge_runs.trim(), "1", "{report:#}");
    let judge = row(&|r| r["teammate"] == "judge");
    assert_eq!(judge["tokens"]["input"], 3000, "{judge}");
    assert!(judge["cost"].as_f64().unwrap() > 0.0, "{judge}");
    assert_eq!(report["total"]["sessions"], 3, "{report:#}");
    assert!(h.violations().is_empty(), "{:?}", h.violations());
    unlock(&h.state);
}

/// The judge bundle dirs are 0500; make them writable again so the harness
/// can delete its temp dir.
#[cfg(unix)]
fn unlock(dir: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let _ = std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700));
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push(e.path());
            }
        }
    }
}

#[cfg(not(unix))]
fn unlock(_dir: &std::path::Path) {}
