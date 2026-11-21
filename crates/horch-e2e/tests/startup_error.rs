//! End-to-end: a worker that dies before it registers still shows its error
//! (W13 C4).

use std::process::Output;
use std::time::{Duration, Instant};

use horch_core::execution::records::SessionLedger;
use horch_e2e::harness::{fixtures, wait_for_or, Harness};
use horch_e2e::ledger_seed::{seed_run, seed_task};
use serde_json::Value;

fn text(o: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        o.status,
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn records(h: &Harness) -> Vec<Value> {
    SessionLedger::for_project(&h.state, &h.project.to_string_lossy())
        .read()
        .map(|all| {
            all.into_iter()
                .map(|r| serde_json::to_value(r).unwrap())
                .collect()
        })
        .unwrap_or_default()
}

fn shown(h: &Harness, task: &str) -> String {
    let o = h.run(&["ledger", "show", task, "--json"]);
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn workspace(name: &str, wait_ms: &str) -> Harness {
    let mut h = Harness::new(name).with_git();
    h.set(
        "HORCH_LEDGER_DB",
        h.state.join("ledger.sqlite3").to_string_lossy(),
    );
    h.set("HORCH_NO_WAKE", "1").set("HORCH_NO_WATCHDOG", "1");
    let quota = fixtures().join("quota/all-ok.json");
    h.set("HORCH_QUOTA_FILE", quota.to_string_lossy());
    h.set("HORCH_FAKE_SCENARIO", "exec");
    h.set("HORCH_SPAWN_WAIT_MS", wait_ms);
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
    let workspace = h.fleet_workspace();
    assert_eq!(workspace, "w1");
    h.set("HORCH_WORKSPACE_ID", workspace);
    h
}

fn queue_one(h: &Harness) {
    let ticket = h.project.join("startup-1.md");
    std::fs::write(&ticket, "# Startup\n**Check:** `true`\n").unwrap();
    let run = seed_run(h, "startup", Some(1), "w1:p1");
    seed_task(h, &run, &ticket, "sonnet");
    h.serve_fleetd();
}

fn worker_row(h: &Harness) -> Value {
    serde_json::from_str::<Value>(&shown(h, "T-1"))
        .map_or(Value::Null, |v| v["attempts"][0].clone())
}

fn registered(h: &Harness) -> bool {
    horch_core::messaging::mailbox::Mailbox::under(&h.state, "w1")
        .pane_for("sonnet-1")
        .is_some()
}

fn startup_errors(h: &Harness) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(h.home.join(".local/share/horch/startup-errors"))
        .map(|d| d.filter_map(|e| e.ok()).map(|e| e.path()).collect())
        .unwrap_or_default()
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

#[cfg(unix)]
#[test]
fn spawn_waits_for_a_worker_that_registers_and_prints_only_the_pane() {
    let h = workspace("startup-ok", "20000");
    queue_one(&h);
    wait_for_or(
        "the worker registers",
        || registered(&h).then_some(()),
        || shown(&h, "T-1"),
    );
    let row = worker_row(&h);
    assert_ne!(row["status"], "launch_failed", "{row}");
    assert!(!shown(&h, "T-1").contains("launch_failed"), "{row}");
    assert_eq!(startup_errors(&h), Vec::<std::path::PathBuf>::new());
    stop_fleetd_and_jobs(&h);
}

#[cfg(unix)]
#[test]
fn a_worker_that_registers_after_the_wait_still_works() {
    let mut h = workspace("startup-late", "300");
    let fake = h.bin.join("herdr");
    let gate = h.root.join("gate");
    let wrapper = h.write_warmed(
        "herdr-slow",
        format!(
            "#!/bin/sh\n\
             if [ \"$1\" = warm ]; then exit 0; fi\n\
             if [ \"$1\" = pane ] && [ \"$2\" = run ]; then\n\
             \x20 exec '{fake}' pane run \"$3\" \"i=0; while [ ! -e '{gate}' ] && [ \\$i -lt 600 ]; do /bin/sleep 0.05; i=\\$((i+1)); done; $4\"\n\
             fi\n\
             exec '{fake}' \"$@\"\n",
            fake = fake.display(),
            gate = gate.display()
        )
        .as_bytes(),
    );
    h.set("HORCH_HERDR_BIN", wrapper.to_string_lossy());
    queue_one(&h);
    let waited = Instant::now();
    wait_for_or(
        "the launch of the pane",
        || worker_row(&h)["pane_id"].is_string().then_some(()),
        || shown(&h, "T-1"),
    );
    std::thread::sleep(Duration::from_millis(1000).saturating_sub(waited.elapsed()));
    let row = worker_row(&h);
    assert_eq!(row["status"], "spawning", "{row}");
    assert!(!registered(&h), "{row}");
    assert!(!shown(&h, "T-1").contains("launch_failed"), "{row}");

    std::fs::write(&gate, "").unwrap();
    let row = wait_for_or(
        "the late worker ran to its end",
        || {
            let row = worker_row(&h);
            (row["session_id"].is_string() && row["exit_code"] == 0).then_some(row)
        },
        || format!("{}\n{:?}", shown(&h, "T-1"), records(&h)),
    );
    assert_ne!(row["status"], "launch_failed", "{row}");
    stop_fleetd_and_jobs(&h);
}

#[cfg(unix)]
#[test]
fn spawn_fails_with_the_error_of_a_worker_whose_brief_is_missing() {
    let mut h = workspace("startup-error", "60000");
    let fake = h.bin.join("herdr");
    let wrapper = h.write_warmed(
        "herdr-elsewhere",
        format!(
            "#!/bin/sh\n\
             if [ \"$1\" = warm ]; then exit 0; fi\n\
             if [ \"$1\" = pane ] && [ \"$2\" = run ]; then\n\
             \x20 export HORCH_WORKSPACE_ID=elsewhere\n\
             fi\n\
             exec '{}' \"$@\"\n",
            fake.display()
        )
        .as_bytes(),
    );
    h.set("HORCH_HERDR_BIN", wrapper.to_string_lossy());
    queue_one(&h);

    let row = wait_for_or(
        "the launch fails with the worker's error",
        || {
            let row = worker_row(&h);
            (row["status"] == "launch_failed").then_some(row)
        },
        || shown(&h, "T-1"),
    );
    let reason = row["end_reason"].as_str().unwrap_or_default();
    assert!(reason.starts_with("no brief at "), "{row}");
    assert!(reason.contains("sonnet-1"), "{row}");
    assert!(reason.contains("herdr-orchestration-elsewhere"), "{row}");
    assert!(!reason.contains("has not registered"), "{row}");

    let errors = h.home.join(".local/share/horch/startup-errors");
    assert!(
        errors.is_dir(),
        "the worker wrote under {}",
        errors.display()
    );
    assert_eq!(startup_errors(&h), Vec::<std::path::PathBuf>::new());

    // The record ends as a failed launch, with the worker's error.
    let r = records(&h)
        .into_iter()
        .find(|r| r["role"] == "sonnet-1")
        .expect("a record for sonnet-1");
    assert_eq!(r["status"], "done", "{r}");
    assert_eq!(r["state"]["state"], "launch_failed", "{r}");
    assert_eq!(r["state"]["stage"], "run", "{r}");
    let reason = r["state"]["reason"].as_str().unwrap_or_default();
    assert!(reason.starts_with("no brief at "), "{r}");
    stop_fleetd_and_jobs(&h);
}
