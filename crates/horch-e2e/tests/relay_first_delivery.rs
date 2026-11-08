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
        writeln!(input, r#"{{"jsonrpc":"2.0","method":"notifications/initialized"}}"#).unwrap();
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
    let id = mail.enqueue("horch", "[horch] NOTE: Tasks can start: T-9").unwrap();
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
