//! End-to-end: `horch doctor` on `hc` and `herdr` (HCB-13).

use horch_e2e::harness::{fixtures, Harness};

fn world(name: &str) -> Harness {
    let mut h = Harness::new(name).with_git();
    h.set("HORCH_NO_WATCHDOG", "1");
    h.set("HORCH_FAULT", "delivery-probe-ms:500");
    h.set(
        "HORCH_QUOTA_FILE",
        fixtures().join("quota/all-ok.json").to_string_lossy(),
    );
    h
}

fn combined(o: &std::process::Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

#[test]
fn hcb_13_doctor_passes_on_hc_with_both_programs() {
    let h = world("hcb13-ok");
    let o = h.run(&["--backend", "hc", "doctor"]);
    let text = combined(&o);
    assert!(o.status.success(), "{text}");
    assert!(text.contains("hc is installed and reachable."), "{text}");
    assert!(text.contains("also found: herdr (reachable)"), "{text}");
    assert!(
        text.contains(
            "note: on hc, a sandboxed agent can reach the herdr-core socket, which can run programs outside the sandbox"
        ),
        "{text}"
    );
}

#[test]
fn hcb_13_doctor_refuses_an_old_hc() {
    let mut h = world("hcb13-old-hc");
    let old_hc = h.write_warmed(
        "hc",
        b"#!/bin/sh\nif [ \"$1\" = warm ]; then exit 0; fi\necho 'hc 0.1.20'\nexit 0\n",
    );
    h.set("HORCH_HC_BIN", old_hc.to_string_lossy());
    let o = h.run(&["--backend", "hc", "doctor"]);
    let text = combined(&o);
    assert!(!o.status.success(), "{text}");
    assert!(text.contains("0.1.20"), "must name found version: {text}");
    assert!(text.contains("0.1.21"), "must name needed version: {text}");

    // Also verify old herdr-app is refused naming found and needed versions.
    let mut h2 = world("hcb13-old-app");
    let old_app = h2.write_warmed(
        "herdr-app",
        b"#!/bin/sh\nif [ \"$1\" = warm ]; then exit 0; fi\necho 'herdr-app 0.1.15'\nexit 0\n",
    );
    h2.set("HORCH_HERDR_APP_BIN", old_app.to_string_lossy());
    let o2 = h2.run(&["--backend", "hc", "doctor"]);
    let text2 = combined(&o2);
    assert!(!o2.status.success(), "{text2}");
    assert!(text2.contains("0.1.15"), "must name found version: {text2}");
    assert!(
        text2.contains("0.1.16"),
        "must name needed version: {text2}"
    );
}

#[test]
fn hcb_13_doctor_warns_when_the_window_is_closed() {
    let h = world("hcb13-closed");
    h.close_hc_window();
    let o = h.run(&["--backend", "hc", "doctor"]);
    let text = combined(&o);
    assert!(o.status.success(), "{text}");
    assert!(text.contains("hc is installed and reachable."), "{text}");
    assert!(
        text.contains(
            "warning: the Herdr app window is closed: a new fleet cannot start, a running one continues"
        ),
        "{text}"
    );

    // Stopping herdr-core turns doctor into exit 1 with the core message.
    h.stop_hc_core();
    let o_dead = h.run(&["--backend", "hc", "doctor"]);
    let dead_text = combined(&o_dead);
    assert!(!o_dead.status.success(), "{dead_text}");
    assert!(
        dead_text.contains("herdr-core is not running; open the Herdr app"),
        "{dead_text}"
    );
}

#[test]
fn hcb_13_doctor_on_herdr_is_unchanged() {
    let mut h = world("hcb13-herdr");
    // Remove hc from PATH/overrides to verify `not found: hc` as well as `also found: hc`.
    let o_both = h.run(&["--backend", "herdr", "doctor"]);
    let both_text = combined(&o_both);
    assert!(o_both.status.success(), "{both_text}");
    assert!(
        both_text.contains("herdr is installed and reachable."),
        "{both_text}"
    );
    assert!(
        both_text.contains("also found: hc (reachable)"),
        "{both_text}"
    );
    assert!(
        !both_text.contains("can reach the herdr-core socket"),
        "{both_text}"
    );

    h.set("HORCH_HC_BIN", h.root.join("missing-hc").to_string_lossy());
    let o_one = h.run(&["--backend", "herdr", "doctor"]);
    let one_text = combined(&o_one);
    assert!(o_one.status.success(), "{one_text}");
    assert!(one_text.contains("not found: hc"), "{one_text}");
}
