//! MAIL-U1 durable delivery through the detached courier.

use std::fs::File;
use std::path::Path;
use std::process::Output;

use horch_e2e::harness::{wait_for_or, Harness};

fn text(output: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// Whether a courier holds the lock at `path`; `None` before the file exists.
/// A probe that takes the lock unlocks it before the close: a process that
/// another test thread spawns at the close holds a copy of the file until it
/// execs, the flock stays held while any copy is open, and a courier that
/// starts then exits at once.
fn courier_holds(path: &Path) -> Option<bool> {
    let lock = File::options().read(true).write(true).open(path).ok()?;
    if lock.try_lock().is_err() {
        return Some(true);
    }
    let _ = lock.unlock();
    Some(false)
}

fn fleet() -> Harness {
    let mut harness = Harness::new("mail-courier");
    harness.set("HORCH_WORKSPACE_ID", "w1");
    harness.set("HORCH_FAKE_SCENARIO", "mail_drop");
    harness.set("HORCH_TEST_MAIL_POLL_MS", "20");
    harness.set("HORCH_TEST_MAIL_REDELIVERY_MS", "50");
    let mut create = std::process::Command::new(harness.bin.join("herdr"));
    harness.seal(&mut create);
    let output = create
        .args(["workspace", "create", "--label", "mail-courier"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", text(&output));
    let mailbox = harness.state.join("herdr-orchestration-w1");
    std::fs::create_dir_all(&mailbox).unwrap();
    std::fs::write(mailbox.join("orchestrator.id"), "w1:p1").unwrap();
    harness
}

/// The first push is lost (`mail_drop`). The courier then names the unread
/// DONE in 1 digest line, without its text, until `horch inbox` acks it.
#[test]
fn mail_u1_courier_digests_unread_mail_to_an_idle_orchestrator_until_inbox_acks() {
    let harness = fleet();
    let mut tell = harness.horch(&["tell", "orchestrator", "DONE: do not lose this"]);
    tell.env("HERDR_PANE_ID", "w1:p2")
        .env("HORCH_ROLE", "sonnet-1")
        .env("HORCH_RECORD_ID", "r1");
    let sent = tell.output().unwrap();
    assert!(sent.status.success(), "{}", text(&sent));

    let lock_path = harness.state.join("mail/w1/courier.lock");
    wait_for_or(
        "the courier lock",
        || courier_holds(&lock_path)?.then_some(()),
        || format!("calls: {:?}", harness.calls_of("herdr")),
    );
    let digest = "[horch] NOTE: 1 unread message (sonnet-1 DONE). Run horch inbox.";
    wait_for_or(
        "the digest line",
        || {
            harness
                .calls_of("herdr")
                .iter()
                .any(|call| call["argv"].get(3).and_then(|a| a.as_str()) == Some(digest))
                .then_some(())
        },
        || format!("calls: {:?}", harness.calls_of("herdr")),
    );

    let inbox = harness.run(&["inbox"]);
    assert!(inbox.status.success(), "{}", text(&inbox));
    assert!(String::from_utf8_lossy(&inbox.stdout).contains("[sonnet-1] DONE: do not lose this"));
    wait_for_or(
        "the courier to exit",
        || (!courier_holds(&lock_path)?).then_some(()),
        || format!("calls: {:?}", harness.calls_of("herdr")),
    );
}

/// A pane id file can outlive its pane. The courier counts a pane herdr does
/// not know as no orchestrator, so it exits after the limit instead of
/// polling forever; the mail stays pending.
#[test]
fn mail_u1_courier_exits_when_the_orchestrator_pane_is_gone() {
    let mut harness = fleet();
    harness.set("HORCH_TEST_MAIL_NO_ORCHESTRATOR_MS", "1000");
    let mailbox = harness.state.join("herdr-orchestration-w1");
    std::fs::write(mailbox.join("orchestrator.id"), "w1:p9").unwrap();
    let mut tell = harness.horch(&["tell", "orchestrator", "NOTE: still here"]);
    tell.env("HERDR_PANE_ID", "w1:p2")
        .env("HORCH_ROLE", "sonnet-1")
        .env("HORCH_RECORD_ID", "r1");
    let _ = tell.output().unwrap();

    let lock_path = harness.state.join("mail/w1/courier.lock");
    wait_for_or(
        "the courier lock",
        || courier_holds(&lock_path)?.then_some(()),
        || format!("calls: {:?}", harness.calls_of("herdr")),
    );
    wait_for_or(
        "the courier to exit",
        || (!courier_holds(&lock_path)?).then_some(()),
        || format!("calls: {:?}", harness.calls_of("herdr")),
    );
    let pending = std::fs::read_dir(harness.state.join("mail/w1/pending")).unwrap();
    assert_eq!(pending.count(), 1, "the message stays pending");
}
