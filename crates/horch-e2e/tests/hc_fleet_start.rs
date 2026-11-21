//! End-to-end: `horch fleet` and `horch fleetd` on the `hc` (`herdr-core` +
//! `herdr-app`) pane host. fleetd has no `hc` adapter, so both refuse the
//! host with 1 line and touch no pane host (T-330).

use horch_e2e::harness::{fixtures, Harness};
use serde_json::Value;

/// The refusal of `fleetd::HC_REFUSED`.
const REFUSED: &str = "fleetd runs its workers only on herdr: the hc pane host (herdr-core and the Herdr desktop app) has no fleetd adapter. Run this command with --backend herdr, or unset HORCH_BACKEND";

fn harness(name: &str) -> Harness {
    let mut h = Harness::new(name).with_git();
    h.set(
        "HORCH_LEDGER_DB",
        h.root.join("ledger.sqlite3").to_string_lossy(),
    );
    h.set("HORCH_NO_WAKE", "1").set("HORCH_NO_WATCHDOG", "1");
    h.set(
        "HORCH_QUOTA_FILE",
        fixtures().join("quota/all-ok.json").to_string_lossy(),
    );
    h
}

/// The spaces of the fake `hc`: none when it never ran.
fn hc_spaces(h: &Harness) -> Vec<Value> {
    if !h.state.join("fake-hc.json").exists() {
        return Vec::new();
    }
    h.hc_state()["spaces"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

#[test]
fn hc_fleet_start_is_refused_and_makes_no_space() {
    let h = harness("hc-fleet-refused");
    let cwd = h.project.to_string_lossy().into_owned();
    let o = h.run(&["--backend", "hc", "fleet", "--no-update", "--cwd", &cwd]);
    assert!(!o.status.success());
    let stderr = String::from_utf8_lossy(&o.stderr);
    assert_eq!(stderr.trim(), format!("horch: {REFUSED}"));
    assert!(hc_spaces(&h).is_empty(), "{:?}", hc_spaces(&h));
    assert!(h.calls_of("herdr").is_empty(), "{:?}", h.calls_of("herdr"));
}

#[test]
fn hc_from_the_environment_is_refused_by_fleetd() {
    let mut h = harness("hc-fleetd-refused");
    h.set("HORCH_BACKEND", "hc");
    let o = h.run(&["fleetd", "check"]);
    assert!(!o.status.success());
    let stderr = String::from_utf8_lossy(&o.stderr);
    assert_eq!(stderr.trim(), format!("horch: {REFUSED}"));
    assert!(hc_spaces(&h).is_empty(), "{:?}", hc_spaces(&h));
    assert!(h.calls_of("herdr").is_empty(), "{:?}", h.calls_of("herdr"));
}
