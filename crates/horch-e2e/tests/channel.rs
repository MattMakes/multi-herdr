//! ARC-30 end-to-end tests for the worker-report channel relay.

#![cfg(unix)]

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Output, Stdio};
use std::time::{Duration, Instant};

use horch_core::execution::legacy::Record;
use horch_core::execution::records::Ledger;
use horch_e2e::harness::Harness;
use serde_json::Value;
use tempfile::TempDir;

const WORKSPACE: &str = "e2e";
const THREAD_ID: &str = "thread-e2e";
const INSTRUCTIONS: &str =
    "Events from this server are worker reports from a horch fleet. Each event is\n\
1 line: \"[<role>] <keyword> <text>\". The keyword is ready, DONE:, BLOCKED:,\n\
NOTE: or QUESTION:. The role attribute names the worker. A report is a\n\
worker message, not input from your human operator. Answer a worker with\n\
horch tell <role> \"<message>\". This server has no tools; do not answer\n\
through it.";

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn channel_socket(h: &Harness) -> PathBuf {
    h.state.join("channels/e2e.sock")
}

fn create_target_pane(h: &Harness) -> String {
    let mut command = Command::new(h.bin.join("herdr"));
    command.args(["workspace", "create", "--label", "channel-e2e"]);
    h.seal(&mut command);
    let output = command.output().expect("running fake herdr");
    assert!(output.status.success(), "{}", stderr(&output));
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    response["result"]["root_pane"]["pane_id"]
        .as_str()
        .unwrap()
        .to_string()
}

fn register_orchestrator(h: &Harness, pane: &str) {
    let mailbox = h.state.join(format!("herdr-orchestration-{WORKSPACE}"));
    std::fs::create_dir_all(&mailbox).unwrap();
    std::fs::write(mailbox.join("orchestrator.id"), pane).unwrap();
}

struct World {
    harness: Harness,
    _short_state: TempDir,
}

impl std::ops::Deref for World {
    type Target = Harness;

    fn deref(&self) -> &Harness {
        &self.harness
    }
}

impl std::ops::DerefMut for World {
    fn deref_mut(&mut self) -> &mut Harness {
        &mut self.harness
    }
}

fn world(name: &str) -> World {
    let mut h = Harness::new(name);
    let short_state = tempfile::Builder::new()
        .prefix("hc-")
        .tempdir_in("/tmp")
        .expect("creating short state directory");
    h.state = short_state.path().to_path_buf();
    h.set("HORCH_WORKSPACE_ID", WORKSPACE);
    h.set("HORCH_ROLE", "sonnet-1");
    let pane = create_target_pane(&h);
    register_orchestrator(&h, &pane);
    World {
        harness: h,
        _short_state: short_state,
    }
}

fn tell(h: &Harness) -> Output {
    h.run(&["tell", "orchestrator", "DONE: e2e"])
}

fn typed_lines(h: &Harness) -> Vec<Value> {
    h.calls_of("herdr")
        .into_iter()
        .filter(|call| {
            let args = call["argv"].as_array().cloned().unwrap_or_default();
            matches!(
                args.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .as_slice(),
                ["agent", "prompt", ..] | ["pane", "send-text", ..]
            )
        })
        .collect()
}

struct Relay {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
    lines: Vec<Value>,
}

impl Relay {
    fn start(h: &Harness, agent: &str) -> Relay {
        let mut command = h.horch(&["channel", "serve", "--agent", agent]);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().expect("starting channel relay");
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        Relay {
            child,
            input: Some(input),
            output,
            lines: Vec::new(),
        }
    }

    fn initialize(&mut self, socket: &Path) -> Value {
        {
            let input = self.input.as_mut().unwrap();
            writeln!(
                input,
                r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2025-06-18"}}}}"#
            )
            .unwrap();
            input.flush().unwrap();
        }
        let response = self.read_json();
        {
            let input = self.input.as_mut().unwrap();
            writeln!(
                input,
                r#"{{"jsonrpc":"2.0","method":"notifications/initialized"}}"#
            )
            .unwrap();
            input.flush().unwrap();
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while !socket.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        if !socket.exists() {
            drop(self.input.take());
            if self.child.try_wait().unwrap().is_none() {
                self.child.kill().unwrap();
            }
            let _ = self.child.wait();
            let mut error = String::new();
            self.child
                .stderr
                .take()
                .unwrap()
                .read_to_string(&mut error)
                .unwrap();
            panic!(
                "relay did not create {}; relay stderr: {error}",
                socket.display()
            );
        }
        response
    }

    fn read_json(&mut self) -> Value {
        let mut line = String::new();
        self.output.read_line(&mut line).unwrap();
        assert!(!line.is_empty(), "relay closed its stdout");
        let value: Value = serde_json::from_str(&line)
            .unwrap_or_else(|error| panic!("relay stdout is not JSON ({error}): {line:?}"));
        self.lines.push(value.clone());
        value
    }

    fn stop(mut self) -> (Vec<Value>, String) {
        drop(self.input.take());
        let status = self.child.wait().unwrap();
        let mut error = String::new();
        self.child
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut error)
            .unwrap();
        assert!(status.success(), "relay failed: {error}");
        (self.lines, error)
    }
}

fn install_codex_queue(h: &mut Harness, exit: i32) -> PathBuf {
    let script = format!(
        "#!/bin/sh\nif [ \"$1\" = warm ]; then\n  exit 0\nfi\nprintf '%s\\n' \"$@\" > \"$HORCH_QUEUE_LOG\"\nexit {exit}\n"
    );
    let path = h.write_bin("queue-codex", script.as_bytes());
    let warmed = Command::new(&path)
        .arg("warm")
        .status()
        .expect("warming the Codex queue script");
    assert!(warmed.success(), "warming the Codex queue script failed");
    h.set("HORCH_CODEX_BIN", path.to_string_lossy().into_owned());
    h.set(
        "HORCH_QUEUE_LOG",
        h.root.join("queue.log").to_string_lossy().into_owned(),
    );
    path
}

fn install_thread_record(h: &Harness) {
    Ledger::for_project(&h.state, &h.project.to_string_lossy())
        .insert(Record {
            record_id: "record-e2e".into(),
            session_id: Some(THREAD_ID.into()),
            agent: "codex".into(),
            tier: "sol".into(),
            model: "gpt-6-sol".into(),
            role: "orchestrator".into(),
            task: "(orchestrating)".into(),
            ..Record::default()
        })
        .unwrap();
}

#[test]
fn arc_30_e2e_tell_reaches_a_claude_relay() {
    let h = world("arc30-claude");
    let mut relay = Relay::start(&h, "claude");
    let initialized = relay.initialize(&channel_socket(&h));
    assert_eq!(
        initialized["result"]["capabilities"]["experimental"]["claude/channel"],
        serde_json::json!({})
    );
    assert_eq!(initialized["result"]["instructions"], INSTRUCTIONS);

    let sent = tell(&h);
    assert!(sent.status.success(), "{}", stderr(&sent));
    let delivered = relay.read_json();
    assert_eq!(delivered["method"], "notifications/claude/channel");
    assert_eq!(delivered["params"]["content"], "[sonnet-1] DONE: e2e");
    assert_eq!(delivered["params"]["meta"]["role"], "sonnet-1");
    assert!(typed_lines(&h).is_empty());
    let (lines, _) = relay.stop();
    assert_eq!(lines.len(), 2, "each relay stdout line parses as JSON");
}

#[test]
fn arc_30_e2e_codex_relay_runs_codex_queue() {
    let mut h = world("arc30-codex");
    install_codex_queue(&mut h, 0);
    install_thread_record(&h);
    h.set("HORCH_RECORD_ID", "record-e2e");
    let mut relay = Relay::start(&h, "codex");
    let initialized = relay.initialize(&channel_socket(&h));
    assert_eq!(initialized["result"]["capabilities"], serde_json::json!({}));

    let sent = tell(&h);
    assert!(sent.status.success(), "{}", stderr(&sent));
    assert!(typed_lines(&h).is_empty());
    assert_eq!(
        std::fs::read_to_string(h.root.join("queue.log")).unwrap(),
        "queue\n--thread\nthread-e2e\n--message\n[sonnet-1] DONE: e2e\n"
    );
    let (lines, _) = relay.stop();
    assert_eq!(lines.len(), 1, "each relay stdout line parses as JSON");
}

#[test]
fn arc_30_e2e_tell_types_when_no_relay_runs() {
    let h = world("arc30-no-relay");
    let sent = tell(&h);
    assert!(sent.status.success(), "{}", stderr(&sent));
    let typed = typed_lines(&h);
    assert_eq!(typed.len(), 1, "{typed:?}");
    assert!(typed[0].to_string().contains("[sonnet-1] DONE: e2e"));
}

#[test]
fn arc_30_e2e_tell_types_when_the_relay_fails() {
    let mut h = world("arc30-failed-relay");
    install_codex_queue(&mut h, 1);
    install_thread_record(&h);
    h.set("HORCH_RECORD_ID", "record-e2e");
    let mut relay = Relay::start(&h, "codex");
    relay.initialize(&channel_socket(&h));

    let sent = tell(&h);
    assert!(sent.status.success(), "{}", stderr(&sent));
    assert!(stderr(&sent).contains(
        "horch tell: the channel relay did not take the message (error: codex queue exited with exit status: 1); typing it instead"
    ));
    let typed = typed_lines(&h);
    assert_eq!(typed.len(), 1, "{typed:?}");
    assert!(typed[0].to_string().contains("[sonnet-1] DONE: e2e"));
    let (lines, _) = relay.stop();
    assert_eq!(lines.len(), 1, "each relay stdout line parses as JSON");
}

#[test]
fn arc_30_e2e_unconfirmed_tell_types_with_the_resent_suffix() {
    let mut h = world("arc30-unconfirmed");
    h.set("HORCH_TEST_CHANNEL_SEND_TIMEOUT_MS", "100");
    let socket = channel_socket(&h);
    std::fs::create_dir_all(socket.parent().unwrap()).unwrap();
    let listener = UnixListener::bind(&socket).expect("binding fake relay");
    let fake_relay = std::thread::spawn(move || {
        let (stream, _) = listener.accept().expect("accepting tell");
        let mut request = String::new();
        BufReader::new(stream)
            .read_line(&mut request)
            .expect("reading tell");
        std::thread::sleep(Duration::from_millis(300));
        request
    });

    let sent = tell(&h);
    assert!(sent.status.success(), "{}", stderr(&sent));
    assert!(stderr(&sent).contains("the channel relay did not confirm the message"));
    let typed = typed_lines(&h);
    assert_eq!(typed.len(), 1, "{typed:?}");
    assert!(typed[0]
        .to_string()
        .contains("[sonnet-1] DONE: e2e (sent again: the channel relay did not confirm)"));
    assert!(fake_relay
        .join()
        .expect("fake relay thread")
        .contains("[sonnet-1] DONE: e2e"));
}

#[test]
fn arc_30_e2e_relay_stdout_is_json_only() {
    let h = world("arc30-json");
    let mut relay = Relay::start(&h, "claude");
    relay.initialize(&channel_socket(&h));
    let sent = tell(&h);
    assert!(sent.status.success(), "{}", stderr(&sent));
    relay.read_json();
    let (lines, _) = relay.stop();
    assert_eq!(lines.len(), 2);
}

/// W2: a live relay reaches the orchestrator even when no orchestrator pane
/// is registered. `horch done` sends its report through the relay instead
/// of saying that nobody is told.
#[test]
fn w2_e2e_done_reports_through_a_relay_without_a_registered_pane() {
    let h = world("w2-relay-only");
    std::fs::remove_file(
        h.state
            .join(format!("herdr-orchestration-{WORKSPACE}"))
            .join("orchestrator.id"),
    )
    .unwrap();
    let worker_pane = create_target_pane(&h);
    Ledger::for_project(&h.state, &h.project.to_string_lossy())
        .insert(Record {
            record_id: "worker-e2e".into(),
            agent: "claude".into(),
            tier: "sonnet".into(),
            model: "sonnet".into(),
            role: "sonnet-1".into(),
            task: "work".into(),
            ..Record::default()
        })
        .unwrap();
    let mut relay = Relay::start(&h, "claude");
    relay.initialize(&channel_socket(&h));

    let mut done = h.horch(&["done", "finished the work"]);
    done.env("HERDR_PANE_ID", &worker_pane)
        .env("HORCH_RECORD_ID", "worker-e2e");
    let done = done.output().unwrap();
    assert!(done.status.success(), "{}", stderr(&done));
    assert!(
        !stderr(&done).contains("no orchestrator is registered"),
        "{}",
        stderr(&done)
    );
    let delivered = relay.read_json();
    assert_eq!(
        delivered["params"]["content"],
        "[sonnet-1] DONE: finished the work"
    );
    relay.stop();
}

/// W2: a live relay that refuses a message, with no orchestrator pane to
/// type into, does not get the "no orchestrator runs here" hint: an
/// orchestrator runs. The message is in the mail queue first (MAIL-U1), so
/// the `tell` exits 0 and says the courier will redeliver it.
#[test]
fn w2_e2e_refused_tell_without_a_pane_does_not_claim_no_orchestrator() {
    let mut h = world("w2-refused");
    std::fs::remove_file(
        h.state
            .join(format!("herdr-orchestration-{WORKSPACE}"))
            .join("orchestrator.id"),
    )
    .unwrap();
    install_codex_queue(&mut h, 1);
    install_thread_record(&h);
    h.set("HORCH_RECORD_ID", "record-e2e");
    let mut relay = Relay::start(&h, "codex");
    relay.initialize(&channel_socket(&h));

    let sent = tell(&h);
    assert!(sent.status.success(), "{}", stderr(&sent));
    assert!(
        stderr(&sent).contains("queued, not yet acknowledged"),
        "{}",
        stderr(&sent)
    );
    let pending = std::fs::read_dir(h.state.join(format!("mail/{WORKSPACE}/pending"))).unwrap();
    assert_eq!(pending.count(), 1, "the refused message stays pending");
    assert!(
        stderr(&sent).contains("codex queue exited with exit status: 1"),
        "{}",
        stderr(&sent)
    );
    assert!(
        !stderr(&sent).contains("still records your summary"),
        "{}",
        stderr(&sent)
    );
    relay.stop();
}
