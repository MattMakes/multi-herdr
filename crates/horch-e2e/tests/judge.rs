//! JDG-01: the judge is headless only, so `horch spawn judge` is refused
//! before anything touches herdr or the ledger.

use horch_e2e::harness::Harness;

#[test]
fn jdg_01_spawn_judge_refused() {
    let mut h = Harness::new("jdg01");
    h.set("HORCH_WORKSPACE_ID", "w1");
    let out = h.run(&["spawn", "judge", "x", "--from-pane", "w1:p1", "--no-tile"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "spawn judge succeeded\n{stderr}");
    assert!(stderr.contains("HEADLESS_ONLY"), "{stderr}");
    assert!(stderr.contains("'judge' is headless only"), "{stderr}");
    assert!(h.calls_of("herdr").is_empty(), "{:?}", h.calls());
    assert!(h.calls_of("claude").is_empty(), "{:?}", h.calls());
    assert!(h.state_files().is_empty(), "{:?}", h.state_files());
}
