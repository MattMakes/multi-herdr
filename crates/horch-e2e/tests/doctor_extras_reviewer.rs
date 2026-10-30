//! `horch doctor` warns when a code-sweep reviewer cannot run on this host
//! (#158).

use horch_e2e::harness::Harness;

fn text(h: &Harness) -> String {
    let o = h.run(&["doctor"]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

#[test]
fn doctor_warns_when_the_codex_reviewer_has_no_codex() {
    let mut h = Harness::new("extras-reviewer");
    let present = text(&h);
    assert!(
        !present.contains("reviewer codex-reviewer cannot run"),
        "{present}"
    );
    let missing = h.root.join("no-such-codex");
    h.set("HORCH_CODEX_BIN", missing.to_string_lossy());
    let absent = text(&h);
    assert!(
        absent.contains("warning: reviewer codex-reviewer cannot run on this host: the codex binary is not installed."),
        "{absent}"
    );
    assert!(
        !absent.contains("reviewer architect-reviewer cannot run"),
        "{absent}"
    );
}
