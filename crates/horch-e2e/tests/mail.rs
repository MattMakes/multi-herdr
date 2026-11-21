use std::fs::File;
use std::path::Path;
use std::process::Output;

use horch_core::messaging::mail::Mail;
use horch_e2e::harness::{wait_for_or, Harness};

fn text(output: &Output) -> String {
    format!(
        "status: {}\nstdout: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

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
    let digest = horch_core::messaging::delivery::MAIL_LINE;
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
    let id = Mail::under(&harness.state, "w1").pending().unwrap()[0]
        .id
        .short();

    let inbox = harness.run(&["inbox"]);
    assert!(inbox.status.success(), "{}", text(&inbox));
    assert!(String::from_utf8_lossy(&inbox.stdout)
        .contains(&format!("[sonnet-1 #{id}] DONE: do not lose this")));
    wait_for_or(
        "the courier to exit",
        || (!courier_holds(&lock_path)?).then_some(()),
        || format!("calls: {:?}", harness.calls_of("herdr")),
    );
}

#[test]
fn trf_04_courier_stays_while_mail_is_pending_and_the_pane_is_gone() {
    let harness = fleet();
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
    std::thread::sleep(std::time::Duration::from_millis(1000));
    assert_eq!(courier_holds(&lock_path), Some(true), "the courier stays");
    let pending = std::fs::read_dir(harness.state.join("mail/w1/pending")).unwrap();
    assert_eq!(pending.count(), 1, "the message stays pending");

    let inbox = harness.run(&["inbox"]);
    assert!(inbox.status.success(), "{}", text(&inbox));
    wait_for_or(
        "the courier to exit",
        || (!courier_holds(&lock_path)?).then_some(()),
        || format!("calls: {:?}", harness.calls_of("herdr")),
    );
}
