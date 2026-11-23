#![cfg(unix)]

use std::path::PathBuf;
use std::process::Output;

use horch_e2e::harness::{fixtures, wait_for_or, Harness};
use horch_e2e::ledger_seed::{seed_run, seed_task};
use serde_json::Value;

const CARRIED: [&str; 3] = ["HORCH_OPENCODE_BIN", "HORCH_PI_BIN", "HORCH_SQLITE3_BIN"];

fn text(o: &Output) -> String {
    format!(
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn briefs(h: &Harness) -> Vec<PathBuf> {
    std::fs::read_dir(h.state.join("herdr-orchestration-w1"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.to_string_lossy().ends_with(".brief.json"))
        .collect()
}

fn stop_fleetd_and_jobs(h: &Harness) {
    use horch_core::procid::{signal_same, start_time};
    h.stop_fleetd();
    let marks: Vec<String> = [Some(h.root.clone()), std::fs::canonicalize(&h.root).ok()]
        .into_iter()
        .flatten()
        .map(|root| format!("{}/", root.display()))
        .collect();
    let jobs = || -> Vec<(u32, u64)> {
        horch_e2e::process::commands_with_environment()
            .into_iter()
            .filter(|(_, l)| {
                l.contains(" job run ") && marks.iter().any(|m| l.contains(m.as_str()))
            })
            .filter_map(|(pid, _)| Some((pid, start_time(pid)?)))
            .collect()
    };
    let first = std::time::Instant::now();
    horch_e2e::harness::wait_for("the jobs of fleetd to end", || {
        let running = jobs();
        let signal = if first.elapsed() < std::time::Duration::from_secs(10) {
            15
        } else {
            9
        };
        for (pid, started) in &running {
            signal_same(*pid, *started, signal);
        }
        running.is_empty().then_some(())
    });
}

#[test]
fn arc_07_e2e_bin_overrides_reach_worker() {
    let mut h = Harness::new("arc07").with_git();
    h.set(
        "HORCH_LEDGER_DB",
        h.root.join("ledger.sqlite3").to_string_lossy(),
    );
    h.set("HORCH_NO_WAKE", "1").set("HORCH_NO_WATCHDOG", "1");
    let quota = fixtures().join("quota/all-ok.json");
    h.set("HORCH_QUOTA_FILE", quota.to_string_lossy());
    h.set("HORCH_FAKE_SCENARIO", "exec");
    let fakes: Vec<(&str, PathBuf)> = CARRIED
        .iter()
        .map(|key| (*key, h.bin.join(format!("fake-for-{key}"))))
        .collect();
    for (key, path) in &fakes {
        h.set(key, path.to_string_lossy());
    }
    let root = h.fleet_root();
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("fleet.toml"),
        "health_max_load_per_cpu = 1000000.0\npass_period_s = 1\n",
    )
    .unwrap();
    std::fs::write(
        h.project.join("fleet.toml"),
        "[gates]\nfast = \"true\"\nfull = \"true\"\n",
    )
    .unwrap();
    for args in [
        &["add", "fleet.toml"][..],
        &["commit", "--quiet", "--no-gpg-sign", "-m", "gates"],
    ] {
        let o = h.git_cmd(args).output().unwrap();
        assert!(o.status.success(), "git {args:?}: {}", text(&o));
    }
    let fake = h.bin.join("herdr");
    let wrapper = h.write_warmed(
        "herdr-bare-pane",
        format!(
            "#!/bin/sh\n\
             if [ \"$1\" = warm ]; then exit 0; fi\n\
             if [ \"$1\" = pane ] && [ \"$2\" = run ]; then\n\
             \x20 unset {carried}\n\
             fi\n\
             exec '{fake}' \"$@\"\n",
            carried = CARRIED.join(" "),
            fake = fake.display()
        )
        .as_bytes(),
    );
    h.set("HORCH_HERDR_BIN", wrapper.to_string_lossy());
    let workspace = h.fleet_workspace();
    assert_eq!(workspace, "w1");
    h.set("HORCH_WORKSPACE_ID", workspace);

    let ticket = h.project.join("brief-1.md");
    std::fs::write(&ticket, "# Brief\n**Check:** `true`\n").unwrap();
    let run = seed_run(&h, "brief", Some(1), "w1:p1");
    seed_task(&h, &run, &ticket, "sonnet");
    h.serve_fleetd();

    let brief_path = wait_for_or(
        "the brief of the worker",
        || briefs(&h).into_iter().next(),
        || format!("{:?}", h.calls()),
    );
    let brief: Value =
        serde_json::from_str(&std::fs::read_to_string(&brief_path).unwrap()).unwrap();
    assert_eq!(brief["schema"], 2);
    for (key, path) in &fakes {
        let field = match *key {
            "HORCH_OPENCODE_BIN" => "opencode",
            "HORCH_PI_BIN" => "pi",
            _ => "sqlite3",
        };
        assert_eq!(
            brief["bin_overrides"][field].as_str(),
            Some(path.to_string_lossy().as_ref()),
            "{key}"
        );
    }
    assert_eq!(
        brief["claude_bin"].as_str(),
        Some(h.bin.join("claude").to_string_lossy().as_ref()),
        "a v1 worker still finds the claude override"
    );

    let tasks = h.fleet_root().join("tasks").to_string_lossy().into_owned();
    let claude = wait_for_or(
        "the worker launched claude",
        || {
            h.calls_of("claude")
                .into_iter()
                .rev()
                .find(|c| c["argv"].to_string().contains(&tasks))
        },
        || format!("{:?}", h.calls()),
    );
    let keys: Vec<String> = serde_json::from_value(claude["env_keys"].clone()).unwrap();
    for key in CARRIED.iter().chain(&[
        "HORCH_ROLE",
        "HORCH_RECORD_ID",
        "HORCH_WORKSPACE_ID",
        "HORCH_CLAUDE_BIN",
    ]) {
        assert!(keys.iter().any(|k| k == key), "{key} not in {keys:?}");
    }
    assert!(!keys.iter().any(|k| k == "ANTHROPIC_API_KEY"), "{keys:?}");
    assert_eq!(claude["violations"], serde_json::json!([]), "{claude}");
    stop_fleetd_and_jobs(&h);
}
