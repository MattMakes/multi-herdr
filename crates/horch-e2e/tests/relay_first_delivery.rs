//! Orchestrator mail goes through the channel relay while it is live. horch
//! types into the orchestrator pane only when no relay answers: an idle
//! pane can hold the operator's half-typed prompt.
#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use horch_core::messaging::channel;
use horch_core::messaging::mail::Mail;
use horch_e2e::harness::{fixtures, wait_for, Harness};
use serde_json::Value;
use tempfile::TempDir;

/// A harness whose state dir is short: the relay socket path must stay
/// below the 104-byte Unix socket limit of macOS.
fn short_world(name: &str) -> (Harness, TempDir) {
    let mut h = Harness::new(name);
    let short = tempfile::Builder::new()
        .prefix("hr-")
        .tempdir_in("/tmp")
        .unwrap();
    h.state = short.path().to_path_buf();
    (h, short)
}

fn fake(h: &Harness, args: &[&str]) {
    let mut command = Command::new(h.bin.join("herdr"));
    h.seal(&mut command);
    let output = command.args(args).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Every herdr call that types text into a pane.
fn pane_writes(h: &Harness) -> Vec<Value> {
    h.calls_of("herdr")
        .into_iter()
        .filter(|call| {
            matches!(
                (call["argv"][0].as_str(), call["argv"][1].as_str()),
                (Some("agent"), Some("prompt")) | (Some("pane"), Some("send-text" | "send-keys"))
            )
        })
        .collect()
}

/// A Claude channel relay for `workspace`. A reader thread passes each
/// stdout line on, so a test waits for a line with a bound.
struct Relay {
    child: Child,
    input: Option<ChildStdin>,
    lines: Receiver<Value>,
}

impl Relay {
    fn start(h: &Harness, workspace: &str) -> Relay {
        let mut command = h.horch(&["channel", "serve", "--agent", "claude"]);
        command
            .env("HORCH_WORKSPACE_ID", workspace)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = command.spawn().expect("starting channel relay");
        let mut input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in output.lines().map_while(Result::ok) {
                if let Ok(value) = serde_json::from_str::<Value>(&line) {
                    if tx.send(value).is_err() {
                        return;
                    }
                }
            }
        });
        writeln!(
            input,
            r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2025-06-18"}}}}"#
        )
        .unwrap();
        writeln!(
            input,
            r#"{{"jsonrpc":"2.0","method":"notifications/initialized"}}"#
        )
        .unwrap();
        input.flush().unwrap();
        let mut relay = Relay {
            child,
            input: Some(input),
            lines,
        };
        relay.next_line("the initialize answer");
        let socket = channel::socket_path(&h.state, workspace);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !channel::is_live(&socket) {
            assert!(Instant::now() < deadline, "the relay did not answer a ping");
            std::thread::sleep(Duration::from_millis(10));
        }
        relay
    }

    fn next_line(&mut self, what: &str) -> Value {
        self.lines
            .recv_timeout(Duration::from_secs(20))
            .unwrap_or_else(|_| panic!("the relay printed no line for {what} in 20 s"))
    }

    /// The content of the next channel notification.
    fn next_notice(&mut self, what: &str) -> String {
        loop {
            let line = self.next_line(what);
            if line["method"] == "notifications/claude/channel" {
                return line["params"]["content"].as_str().unwrap().to_string();
            }
        }
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        drop(self.input.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The incident: the orchestrator pane is idle because the operator types a
/// draft, and the relay is live. The courier's redelivery goes through the
/// relay. Nothing is typed into the pane.
#[test]
fn a_live_relay_gets_the_courier_redelivery_and_the_idle_pane_gets_no_text() {
    let (mut h, _short) = short_world("relay-first-courier");
    h.set("HORCH_WORKSPACE_ID", "w1");
    h.set("HORCH_TEST_MAIL_POLL_MS", "20");
    h.set("HORCH_TEST_MAIL_REDELIVERY_MS", "50");
    fake(&h, &["workspace", "create", "--label", "relay-first"]);
    fake(
        &h,
        &[
            "pane",
            "test-state",
            "w1:p1",
            "--text",
            "> half-typed operator prompt",
            "--status",
            "idle",
            "--agent",
            "claude",
        ],
    );
    let mailbox = h.state.join("herdr-orchestration-w1");
    std::fs::create_dir_all(&mailbox).unwrap();
    std::fs::write(mailbox.join("orchestrator.id"), "w1:p1").unwrap();
    let mut relay = Relay::start(&h, "w1");

    // A horch notice has no first push: the courier owes it 1 resend.
    let mail = Mail::under(&h.state, "w1");
    let id = mail
        .enqueue("horch", "[horch] NOTE: Tasks can start: T-9")
        .unwrap();
    let mut courier = h
        .horch(&["mail", "courier", "--workspace", "w1"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let notice = relay.next_notice("the courier redelivery");
    assert_eq!(
        notice,
        format!(
            "[horch #{}] NOTE: Tasks can start: T-9 (redelivery 1: not acknowledged; run horch inbox)",
            id.short()
        )
    );
    assert!(h.run(&["inbox"]).status.success());
    wait_for("the courier exit after the acknowledgement", || {
        courier.try_wait().unwrap()
    });
    assert_eq!(pane_writes(&h), Vec::<Value>::new());
}

/// `horch fleet` with a live relay sends its startup probe through the relay.
/// The relay's answer confirms the delivery: the probe needs no
/// `horch inbox`, and no redelivery of it ever comes. No `agent prompt`.
#[test]
fn a_live_relay_gets_the_fleet_probe_and_the_idle_pane_gets_no_text() {
    let (mut h, _short) = short_world("relay-first-probe");
    // The pane is idle as soon as `pane run` starts the orchestrator.
    h.set("HORCH_FAKE_SCENARIO", "exec,fleet_probe_ack");
    h.set("HORCH_NO_WATCHDOG", "1");
    h.set(
        "HORCH_FAULT",
        "delivery-probe-ms:30000,delivery-probe-call-ms:30000",
    );
    h.set(
        "HORCH_QUOTA_FILE",
        fixtures().join("quota/all-ok.json").to_string_lossy(),
    );
    // The fleet's workspace is the fake herdr's first: w1.
    let mut relay = Relay::start(&h, "w1");

    let out = h.run(&["fleet", "--no-update"]);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{text}");
    assert!(
        text.contains("fleet delivery: confirmed by the channel relay"),
        "{text}"
    );
    assert_eq!(
        relay.next_notice("the fleet probe"),
        "[horch] NOTE: the channel relay delivers your fleet's reports"
    );
    assert!(Mail::under(&h.state, "w1").pending().unwrap().is_empty());
    let probes: Vec<Value> = pane_writes(&h)
        .into_iter()
        .filter(|c| c["argv"][0] == "agent" && c["argv"][1] == "prompt")
        .filter(|c| c["argv"][2].as_str().is_some_and(|p| p.starts_with("w1:")))
        .collect();
    assert_eq!(probes, Vec::<Value>::new(), "{text}");
}

/// How a fake relay answers a message. It answers every ping with `ok`, so
/// `channel::is_live` is true.
#[derive(Clone, Copy)]
enum FakeAnswer {
    /// An error line: `channel::send` gives NotDelivered.
    Refuse,
    /// No answer before the send timeout: `channel::send` gives Unconfirmed.
    Silent,
}

/// A fake relay on the socket of `workspace`. It returns the messages it
/// received. Each connection gets its own thread, so a silent message does
/// not hold a ping.
fn fake_relay(h: &Harness, workspace: &str, answer: FakeAnswer) -> Receiver<String> {
    use std::os::unix::net::UnixListener;

    let socket = channel::socket_path(&h.state, workspace);
    std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
    let listener = UnixListener::bind(&socket).expect("binding the fake relay");
    let (tx, messages) = mpsc::channel();
    std::thread::spawn(move || {
        for stream in listener.incoming().map_while(Result::ok) {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut request = String::new();
                let mut reader = BufReader::new(&stream);
                if reader.read_line(&mut request).is_err() {
                    return;
                }
                let mut stream = &stream;
                if request.contains("\"ping\"") {
                    let _ = stream.write_all(b"ok\n");
                    return;
                }
                let _ = tx.send(request);
                match answer {
                    FakeAnswer::Refuse => {
                        let _ = stream.write_all(b"error: the relay refused\n");
                    }
                    FakeAnswer::Silent => std::thread::sleep(Duration::from_millis(500)),
                }
            });
        }
    });
    assert!(channel::is_live(&socket), "the fake relay answers a ping");
    messages
}

/// A worker world: an idle orchestrator pane w1:p1 that holds an operator
/// draft, registered as the orchestrator of w1.
fn worker_world(name: &str) -> (Harness, TempDir) {
    let (mut h, short) = short_world(name);
    h.set("HORCH_WORKSPACE_ID", "w1");
    h.set("HORCH_TEST_CHANNEL_SEND_TIMEOUT_MS", "100");
    fake(&h, &["workspace", "create", "--label", "relay-first"]);
    fake(
        &h,
        &[
            "pane",
            "test-state",
            "w1:p1",
            "--text",
            "> half-typed operator prompt",
            "--status",
            "idle",
            "--agent",
            "claude",
        ],
    );
    let mailbox = h.state.join("herdr-orchestration-w1");
    std::fs::create_dir_all(&mailbox).unwrap();
    std::fs::write(mailbox.join("orchestrator.id"), "w1:p1").unwrap();
    (h, short)
}

/// `horch tell orchestrator` from worker sonnet-1. It returns the stderr.
fn worker_tell(h: &Harness, text: &str) -> String {
    let out = h
        .horch(&["tell", "orchestrator", text])
        .env("HORCH_ROLE", "sonnet-1")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(out.status.success(), "{stderr}");
    stderr
}

/// The pending orchestrator mail lines of w1. `horch inbox` then reads
/// them, so the courier that the tell started stops.
fn pending_then_read(h: &Harness) -> Vec<String> {
    let lines = Mail::under(&h.state, "w1")
        .pending()
        .unwrap()
        .iter()
        .map(|m| m.text.clone())
        .collect();
    assert!(h.run(&["inbox"]).status.success());
    lines
}

/// Case A: a live relay refuses the tell. Nothing is typed into the idle
/// pane, and the mail stays pending for the courier's relay redelivery.
#[test]
fn a_live_relay_that_refuses_a_tell_gets_no_pane_text_and_the_mail_stays() {
    let (h, _short) = worker_world("relay-first-refused");
    let messages = fake_relay(&h, "w1", FakeAnswer::Refuse);

    let stderr = worker_tell(&h, "DONE: case A");

    assert!(messages
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .contains("DONE: case A"));
    assert!(
        stderr.contains("the courier redelivers it through the relay"),
        "{stderr}"
    );
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    assert_eq!(pane_writes(&h), Vec::<Value>::new());
    assert_eq!(pending_then_read(&h), ["[sonnet-1] DONE: case A"]);
}

/// Case B: a live relay does not confirm the tell. Nothing is typed into
/// the idle pane, and the mail stays pending for the courier.
#[test]
fn a_live_relay_that_does_not_confirm_a_tell_gets_no_pane_text_and_the_mail_stays() {
    let (h, _short) = worker_world("relay-first-unconfirmed");
    let messages = fake_relay(&h, "w1", FakeAnswer::Silent);

    let stderr = worker_tell(&h, "DONE: case B");

    assert!(messages
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .contains("DONE: case B"));
    assert!(
        stderr.contains("the courier redelivers it through the relay"),
        "{stderr}"
    );
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    assert_eq!(pane_writes(&h), Vec::<Value>::new());
    assert_eq!(pending_then_read(&h), ["[sonnet-1] DONE: case B"]);
}
