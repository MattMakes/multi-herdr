#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use horch_core::messaging::channel;
use horch_core::messaging::mail::Mail;
use horch_e2e::harness::{wait_for, Harness};
use serde_json::Value;
use tempfile::TempDir;

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

#[derive(Clone, Copy)]
enum FakeAnswer {
    Refuse,
    Silent,
}

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
