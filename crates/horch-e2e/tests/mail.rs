//! MAIL-U1 durable delivery through the detached courier.

use std::fs::File;
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
fn mail_u1_courier_redelivers_to_an_idle_orchestrator_until_inbox_acks() {
    let harness = fleet();
    let mut tell = harness.horch(&["tell", "orchestrator", "NOTE: do not lose this"]);
    tell.env("HERDR_PANE_ID", "w1:p2")
        .env("HORCH_ROLE", "sonnet-1")
        .env("HORCH_RECORD_ID", "r1");
    let sent = tell.output().unwrap();
    assert!(sent.status.success(), "{}", text(&sent));

    let lock_path = harness.state.join("mail/w1/courier.lock");
    wait_for_or(
        "the courier lock",
        || {
            let lock = File::options()
                .read(true)
                .write(true)
                .open(&lock_path)
                .ok()?;
            lock.try_lock().is_err().then_some(())
        },
        || format!("calls: {:?}", harness.calls_of("herdr")),
    );
    let redelivery = wait_for_or(
        "the redelivery line",
        || {
            harness.calls_of("herdr").into_iter().find_map(|call| {
                let args = call["argv"].as_array()?;
                let line = args.get(3)?.as_str()?;
                // A cold start may miss the first scaled retry slot. The
                // courier skips missed slots instead of replaying a burst.
                let retry = line.split_once("(redelivery ")?.1;
                let attempt = retry
                    .split_once(": not acknowledged; run horch inbox)")?
                    .0
                    .parse::<usize>()
                    .ok()?;
                (attempt > 0).then(|| line.to_string())
            })
        },
        || format!("calls: {:?}", harness.calls_of("herdr")),
    );
    assert!(redelivery.starts_with("[sonnet-1] NOTE: do not lose this"));

    let inbox = harness.run(&["inbox"]);
    assert!(inbox.status.success(), "{}", text(&inbox));
    assert!(String::from_utf8_lossy(&inbox.stdout).contains("[sonnet-1] NOTE: do not lose this"));
    wait_for_or(
        "the courier to exit",
        || {
            let lock = File::options()
                .read(true)
                .write(true)
                .open(&lock_path)
                .ok()?;
            lock.try_lock().ok()?;
            Some(())
        },
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
        || {
            let lock = File::options()
                .read(true)
                .write(true)
                .open(&lock_path)
                .ok()?;
            lock.try_lock().is_err().then_some(())
        },
        || format!("calls: {:?}", harness.calls_of("herdr")),
    );
    wait_for_or(
        "the courier to exit",
        || {
            let lock = File::options()
                .read(true)
                .write(true)
                .open(&lock_path)
                .ok()?;
            lock.try_lock().ok()?;
            Some(())
        },
        || format!("calls: {:?}", harness.calls_of("herdr")),
    );
    let pending = std::fs::read_dir(harness.state.join("mail/w1/pending")).unwrap();
    assert_eq!(pending.count(), 1, "the message stays pending");
}
