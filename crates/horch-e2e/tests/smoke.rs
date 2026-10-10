//! `horch smoke` against the fake herdr.

use horch_e2e::harness::Harness;

/// `horch smoke messaging` passes when the message runs in pane b. Before the
/// fix it called `herdr wait output`, which herdr 0.8.2 does not have, and
/// reported `SMOKE_TEST_42 not observed` with the text on the pane.
#[cfg(unix)]
#[test]
fn smoke_messaging_sees_the_message_run_in_pane_b() {
    let mut h = Harness::new("smoke-messaging");
    h.set("HORCH_FAKE_SCENARIO", "exec,shell");
    let out = h.run(&["smoke", "messaging"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "{}\nstdout: {stdout}\nstderr: {stderr}",
        out.status
    );
    assert!(stdout.contains("PASS:"), "{stdout}");
    let waits: Vec<_> = h
        .calls_of("herdr")
        .into_iter()
        .filter(|c| c["argv"][1] == "wait-output")
        .collect();
    assert_eq!(waits.len(), 1, "one pane wait-output call");
}
