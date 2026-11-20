//! ARC-14: every record a launch writes says how routing chose it.
//!
//! fleetd launches each attempt since Phase 6 (FR-5), so the launches below
//! are the attempts of 2 seeded tasks. `horch spawn` is retired: no command
//! substitutes a teammate at launch or resumes a record by its id.

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

/// A harness with a git repository, a ledger file below the state folder
/// (the session records and the task ledger share it), a fleet.toml and
/// the herdr workspace `w1`. The quota is `fixture`.
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

/// `horch ledger show <task> --json`, as text.
fn shown(h: &Harness, task: &str) -> String {
    let o = h.run(&["ledger", "show", task, "--json"]);
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// The record that routing `requested` chose a teammate for.
fn record_of(h: &Harness, requested: &str) -> Option<Value> {
    records(h)
        .into_iter()
        .find(|r| r["routing"]["requested"] == requested)
}

/// A launch of fleetd runs its persona exactly (mode `exact`): the gate of
/// the launch refuses a persona whose pool is closed, and a task moves to
/// its fallback persona only when its worker hits a usage limit.
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

    // A normal launch: codex is ok, so codex-sol runs as itself.
    let plain = wait("codex-sol");
    let p = &plain["routing"];
    assert_eq!(p["requested"], "codex-sol", "{plain:#}");
    assert_eq!(p["resolved"], "codex-sol");
    assert_eq!(p["mode"], "exact");
    assert_eq!(p["pool"], "codex");
    assert_eq!(p["pool_state"], "ok");
    assert!(p["fallback_index"].is_null());
    assert!(plain.get("via").is_none(), "no via on a plain launch");

    // A launch of a persona whose pool is closed: claude is exhausted, so
    // researcher is refused with the gate's reason, and no record is
    // written.
    let refusal = "launch_failed: REFUSED: researcher cannot start. \
                   claude exhausted (7d 100%, resets Fri 14:00Z).";
    wait_for_or(
        "the refusal of the researcher launch",
        || shown(&h, "T-2").contains(refusal).then_some(()),
        || shown(&h, "T-2"),
    );
    assert!(record_of(&h, "researcher").is_none(), "{:?}", records(&h));
}
