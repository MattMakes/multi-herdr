//! ARC-14: every record a launch writes says how routing chose it.

#![cfg(unix)]

use horch_core::execution::records::SessionLedger;
use horch_e2e::harness::{fixtures, wait_for_or, Harness};
use horch_e2e::ledger_seed::{seed_run, seed_task};
use serde_json::Value;

fn quota_fixture(name: &str) -> String {
    fixtures()
        .join("quota")
        .join(format!("{name}.json"))
        .to_string_lossy()
        .into_owned()
}

fn gated(name: &str, fixture: &str) -> Harness {
    let mut h = Harness::new(name).with_git();
    h.set(
        "HORCH_LEDGER_DB",
        h.state.join("ledger.sqlite3").to_string_lossy(),
    );
    h.set("HORCH_NO_WAKE", "1").set("HORCH_NO_WATCHDOG", "1");
    h.set("HORCH_NOW", "2026-09-28T18:00:00Z");
    h.set(
        "HORCH_PROBE_TIMEOUT_MS",
        horch_e2e::harness::DEADLINE.as_millis().to_string(),
    );
    h.set("HORCH_QUOTA_FILE", quota_fixture(fixture));
    h.set("HORCH_FAKE_SCENARIO", "exec");
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
        assert!(o.status.success(), "git {args:?}");
    }
    let workspace = h.fleet_workspace();
    assert_eq!(workspace, "w1");
    h.set("HORCH_WORKSPACE_ID", workspace);
    h
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

fn record_of(h: &Harness, requested: &str) -> Option<Value> {
    records(h)
        .into_iter()
        .find(|r| r["routing"]["requested"] == requested)
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
fn arc_14_provenance_on_spawn_substitute_resume() {
    let h = gated("arc14", "claude-exhausted-codex-ok");
    let run = seed_run(&h, "arc14", Some(1), "w1:p1");
    for (n, persona) in ["codex-sol", "researcher"].into_iter().enumerate() {
        let ticket = h.project.join(format!("arc14-{n}.md"));
        std::fs::write(&ticket, format!("# {persona}\n**Check:** `true`\n")).unwrap();
        seed_task(&h, &run, &ticket, persona);
    }
    h.serve_fleetd();
    let wait = |requested: &str| {
        wait_for_or(
            "the record of the launch",
            || record_of(&h, requested),
            || format!("{:?}", records(&h)),
        )
    };

    let plain = wait("codex-sol");
    let p = &plain["routing"];
    assert_eq!(p["requested"], "codex-sol", "{plain:#}");
    assert_eq!(p["resolved"], "codex-sol");
    assert_eq!(p["mode"], "exact");
    assert_eq!(p["pool"], "codex");
    assert_eq!(p["pool_state"], "ok");
    assert!(p["fallback_index"].is_null());
    assert!(plain.get("via").is_none(), "no via on a plain launch");

    let refusal = "launch_failed: REFUSED: researcher cannot start. \
                   claude exhausted (7d 100%, resets Fri 14:00Z).";
    wait_for_or(
        "the refusal of the researcher launch",
        || shown(&h, "T-2").contains(refusal).then_some(()),
        || shown(&h, "T-2"),
    );
    assert!(record_of(&h, "researcher").is_none(), "{:?}", records(&h));
    stop_fleetd_and_jobs(&h);
}
