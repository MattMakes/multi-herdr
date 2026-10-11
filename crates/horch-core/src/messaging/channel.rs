//! The local relay between worker messages and an orchestrator agent session.
//!
//! The relay is an MCP server on stdin and stdout. After MCP initialization it
//! also accepts one-line messages on a workspace-specific Unix socket.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};

const MAX_LINE_BYTES: usize = 64 * 1024;
const DEFAULT_PROTOCOL_VERSION: &str = "2025-06-18";

/// The longest wait a sender makes for the relay's answer.
pub const SEND_TIMEOUT: Duration = Duration::from_secs(12);

/// The longest wait for one `codex queue` process.
pub const CODEX_QUEUE_TIMEOUT: Duration = Duration::from_secs(10);

/// Where the relay delivers a line.
pub enum Output {
    /// Write a `notifications/claude/channel` notification to MCP stdout.
    ClaudeNotification,
    /// Run `codex queue` for the current orchestrator thread.
    CodexQueue {
        codex_bin: PathBuf,
        thread: Box<dyn Fn() -> Result<String> + Send + Sync>,
    },
}

/// Inputs that do not come from the MCP client.
pub struct ServeOptions {
    pub socket: PathBuf,
    pub output: Output,
    pub instructions: String,
    pub version: String,
}

/// `<state_root>/channels/<file-safe workspace>.sock`.
pub fn socket_path(state_root: &Path, workspace: &str) -> PathBuf {
    state_root
        .join("channels")
        .join(format!("{}.sock", file_safe(workspace)))
}

fn file_safe(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

/// Send one message to a relay and wait for its answer.
pub fn send(socket: &Path, from: &str, text: &str, timeout: Duration) -> Result<()> {
    request(socket, &json!({"from": from, "text": text}), timeout)
}

/// True when a relay answers a ping within one second.
pub fn is_live(socket: &Path) -> bool {
    request(socket, &json!({"ping": true}), Duration::from_secs(1)).is_ok()
}

fn request(socket: &Path, value: &Value, timeout: Duration) -> Result<()> {
    let mut stream = UnixStream::connect(socket)
        .with_context(|| format!("connect to channel relay {}", socket.display()))?;
    stream
        .set_write_timeout(Some(timeout))
        .context("set channel relay write timeout")?;
    stream
        .set_read_timeout(Some(timeout))
        .context("set channel relay read timeout")?;
    serde_json::to_writer(&mut stream, value).context("encode channel request")?;
    stream.write_all(b"\n").context("write channel request")?;
    stream.flush().context("flush channel request")?;

    let mut answer = String::new();
    BufReader::new(stream)
        .read_line(&mut answer)
        .context("read channel relay answer")?;
    let answer = answer.trim_end_matches(['\r', '\n']);
    if answer == "ok" {
        Ok(())
    } else if answer.is_empty() {
        bail!("channel relay closed without an answer")
    } else {
        bail!("{answer}")
    }
}

/// Run the MCP stdio server until `input` reaches EOF.
pub fn serve<R: BufRead, W: Write + Send + 'static>(
    opts: ServeOptions,
    mut input: R,
    output: W,
) -> Result<()> {
    let output = Arc::new(Mutex::new(output));
    let delivery = Arc::new(opts.output);
    let mut relay = None;

    let result = (|| -> Result<()> {
        let mut line = String::new();
        loop {
            line.clear();
            if input.read_line(&mut line).context("read MCP input")? == 0 {
                return Ok(());
            }
            let message: Value = match serde_json::from_str(&line) {
                Ok(message) => message,
                Err(error) => {
                    eprintln!("invalid MCP JSON: {error}");
                    continue;
                }
            };

            let method = message.get("method").and_then(Value::as_str);
            if method == Some("notifications/initialized") && relay.is_none() {
                relay = Some(start_relay(
                    &opts.socket,
                    Arc::clone(&delivery),
                    Arc::clone(&output),
                )?);
            }

            let Some(id) = message.get("id") else {
                continue;
            };
            let response = mcp_response(
                id,
                method,
                &message,
                &opts.instructions,
                &opts.version,
                matches!(&*delivery, Output::ClaudeNotification),
            );
            write_json_line(&output, &response)?;
        }
    })();

    drop(relay);
    result
}

fn mcp_response(
    id: &Value,
    method: Option<&str>,
    message: &Value,
    instructions: &str,
    version: &str,
    claude_channel: bool,
) -> Value {
    let result = match method {
        Some("initialize") => {
            let protocol = message
                .pointer("/params/protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or(DEFAULT_PROTOCOL_VERSION);
            let capabilities = if claude_channel {
                json!({"experimental": {"claude/channel": {}}})
            } else {
                json!({})
            };
            json!({
                "protocolVersion": protocol,
                "capabilities": capabilities,
                "serverInfo": {"name": "horch", "version": version},
                "instructions": instructions,
            })
        }
        Some("ping") => json!({}),
        Some("tools/list") => json!({"tools": []}),
        Some("resources/list") => json!({"resources": []}),
        Some("resources/templates/list") => json!({"resourceTemplates": []}),
        Some("prompts/list") => json!({"prompts": []}),
        _ => {
            return json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {"code": -32601, "message": "method not found"},
            });
        }
    };
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn write_json_line<W: Write>(output: &Arc<Mutex<W>>, value: &Value) -> Result<()> {
    let mut output = output
        .lock()
        .map_err(|_| anyhow!("MCP output lock is poisoned"))?;
    serde_json::to_writer(&mut *output, value).context("encode MCP output")?;
    output.write_all(b"\n").context("write MCP output")?;
    output.flush().context("flush MCP output")
}

struct Relay {
    socket: PathBuf,
    stop: Arc<AtomicBool>,
    accept_thread: Option<JoinHandle<()>>,
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.accept_thread.take() {
            let _ = handle.join();
        }
        if let Err(error) = fs::remove_file(&self.socket) {
            if error.kind() != std::io::ErrorKind::NotFound {
                eprintln!(
                    "cannot remove channel socket {}: {error}",
                    self.socket.display()
                );
            }
        }
    }
}

fn start_relay<W: Write + Send + 'static>(
    socket: &Path,
    delivery: Arc<Output>,
    output: Arc<Mutex<W>>,
) -> Result<Relay> {
    if socket.as_os_str().as_encoded_bytes().len() >= 104 {
        bail!("channel socket path is too long: {}", socket.display());
    }
    let parent = socket
        .parent()
        .ok_or_else(|| anyhow!("channel socket has no parent: {}", socket.display()))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("create channel directory {}", parent.display()))?;
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
        .with_context(|| format!("set channel directory mode on {}", parent.display()))?;

    if socket.exists() {
        if is_live(socket) {
            bail!("a channel relay already serves {}", socket.display());
        }
        fs::remove_file(socket)
            .with_context(|| format!("remove stale channel socket {}", socket.display()))?;
    }

    let listener = UnixListener::bind(socket)
        .with_context(|| format!("bind channel socket {}", socket.display()))?;
    fs::set_permissions(socket, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("set channel socket mode on {}", socket.display()))?;
    listener
        .set_nonblocking(true)
        .context("make channel socket nonblocking")?;

    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let accept_thread = thread::spawn(move || {
        while !thread_stop.load(Ordering::Acquire) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let delivery = Arc::clone(&delivery);
                    let output = Arc::clone(&output);
                    thread::spawn(move || handle_connection(stream, &delivery, &output));
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                }
                Err(error) => {
                    eprintln!("channel socket accept failed: {error}");
                    thread::sleep(Duration::from_millis(5));
                }
            }
        }
    });

    Ok(Relay {
        socket: socket.to_path_buf(),
        stop,
        accept_thread: Some(accept_thread),
    })
}

fn handle_connection<W: Write>(mut stream: UnixStream, delivery: &Output, output: &Arc<Mutex<W>>) {
    let answer = match read_socket_request(&stream) {
        Ok(SocketRequest::Ping) => Ok(()),
        Ok(SocketRequest::Message { from, text }) => deliver(delivery, output, &from, &text),
        Err(()) => {
            let _ = stream.write_all(b"error: bad request\n");
            let _ = stream.flush();
            return;
        }
    };
    let line = match answer {
        Ok(()) => "ok".to_owned(),
        Err(error) => format!("error: {error}"),
    };
    let _ = writeln!(stream, "{line}");
    let _ = stream.flush();
}

enum SocketRequest {
    Ping,
    Message { from: String, text: String },
}

fn read_socket_request(stream: &UnixStream) -> std::result::Result<SocketRequest, ()> {
    let mut bytes = Vec::new();
    let mut limited = BufReader::new(stream).take((MAX_LINE_BYTES + 2) as u64);
    limited.read_until(b'\n', &mut bytes).map_err(|_| ())?;
    if !bytes.ends_with(b"\n") {
        return Err(());
    }
    bytes.pop();
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    if bytes.len() > MAX_LINE_BYTES {
        return Err(());
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| ())?;
    if value.get("ping") == Some(&Value::Bool(true)) {
        return Ok(SocketRequest::Ping);
    }
    let from = value.get("from").and_then(Value::as_str).ok_or(())?;
    let text = value.get("text").and_then(Value::as_str).ok_or(())?;
    Ok(SocketRequest::Message {
        from: from.to_owned(),
        text: text.to_owned(),
    })
}

fn deliver<W: Write>(
    delivery: &Output,
    output: &Arc<Mutex<W>>,
    from: &str,
    text: &str,
) -> Result<()> {
    match delivery {
        Output::ClaudeNotification => write_json_line(
            output,
            &json!({
                "jsonrpc": "2.0",
                "method": "notifications/claude/channel",
                "params": {"content": text, "meta": {"role": from}},
            }),
        ),
        Output::CodexQueue { codex_bin, thread } => {
            let thread_id = thread().context("get Codex thread id")?;
            run_codex_queue(codex_bin, &thread_id, text)
        }
    }
}

fn run_codex_queue(codex_bin: &Path, thread_id: &str, text: &str) -> Result<()> {
    let mut child = Command::new(codex_bin)
        .args(["queue", "--thread", thread_id, "--message", text])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("run {} queue", codex_bin.display()))?;
    let deadline = Instant::now() + CODEX_QUEUE_TIMEOUT;
    loop {
        if let Some(status) = child.try_wait().context("wait for codex queue")? {
            if status.success() {
                return Ok(());
            }
            bail!("codex queue exited with {status}");
        }
        if Instant::now() >= deadline {
            child.kill().context("kill timed out codex queue")?;
            let _ = child.wait();
            bail!(
                "codex queue timed out after {} s",
                CODEX_QUEUE_TIMEOUT.as_secs()
            );
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, Cursor};
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixStream;
    use std::sync::{Arc, Mutex};

    use super::*;

    #[derive(Clone, Default)]
    struct SharedWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("writer lock").extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl SharedWriter {
        fn lines(&self) -> Vec<String> {
            String::from_utf8(self.0.lock().expect("writer lock").clone())
                .expect("UTF-8 output")
                .lines()
                .map(str::to_owned)
                .collect()
        }
    }

    fn temp_root() -> PathBuf {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let path = std::env::temp_dir().join(format!("hc-{}", &id[..8]));
        fs::create_dir(&path).expect("create test directory");
        path
    }

    fn options(socket: PathBuf, output: Output) -> ServeOptions {
        ServeOptions {
            socket,
            output,
            instructions: "channel instructions".to_owned(),
            version: "1.2.3".to_owned(),
        }
    }

    fn start_server(
        output_kind: Output,
    ) -> (PathBuf, UnixStream, SharedWriter, JoinHandle<Result<()>>) {
        let socket = socket_path(&temp_root(), "test");
        let (reader, writer) = UnixStream::pair().expect("stdin pair");
        let output = SharedWriter::default();
        let server_output = output.clone();
        let server_socket = socket.clone();
        let handle = thread::spawn(move || {
            serve(
                options(server_socket, output_kind),
                BufReader::new(reader),
                server_output,
            )
        });
        (socket, writer, output, handle)
    }

    fn initialize(input: &mut UnixStream, socket: &Path) {
        input
            .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
            .expect("write initialized");
        input.flush().expect("flush initialized");
        let deadline = Instant::now() + Duration::from_secs(2);
        while !is_live(socket) {
            assert!(Instant::now() < deadline, "socket was not created");
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn arc_30_socket_path_is_file_safe() {
        assert_eq!(
            socket_path(Path::new("/tmp/root"), "w4W:p1"),
            Path::new("/tmp/root/channels/w4W_p1.sock")
        );
    }

    #[test]
    fn arc_30_initialize_declares_the_claude_channel_capability() {
        let output = SharedWriter::default();
        serve(
            options(PathBuf::from("unused"), Output::ClaudeNotification),
            Cursor::new(
                b"{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"test-version\"}}\n",
            ),
            output.clone(),
        )
        .expect("serve initialize");
        let response: Value = serde_json::from_str(&output.lines()[0]).expect("JSON response");
        assert_eq!(response["result"]["protocolVersion"], "test-version");
        assert_eq!(response["result"]["instructions"], "channel instructions");
        assert_eq!(response["result"]["serverInfo"]["name"], "horch");
        assert_eq!(response["result"]["serverInfo"]["version"], "1.2.3");
        assert_eq!(
            response["result"]["capabilities"]["experimental"]["claude/channel"],
            json!({})
        );
    }

    #[test]
    fn arc_30_codex_output_declares_no_channel_capability() {
        let output = SharedWriter::default();
        serve(
            options(
                PathBuf::from("unused"),
                Output::CodexQueue {
                    codex_bin: PathBuf::from("codex"),
                    thread: Box::new(|| Ok("T1".to_owned())),
                },
            ),
            Cursor::new(b"{\"id\":1,\"method\":\"initialize\"}\n"),
            output.clone(),
        )
        .expect("serve initialize");
        let response: Value = serde_json::from_str(&output.lines()[0]).expect("JSON response");
        assert_eq!(
            response["result"]["protocolVersion"],
            DEFAULT_PROTOCOL_VERSION
        );
        assert_eq!(response["result"]["capabilities"], json!({}));
    }

    #[test]
    fn arc_30_unknown_request_gets_method_not_found() {
        let output = SharedWriter::default();
        serve(
            options(PathBuf::from("unused"), Output::ClaudeNotification),
            Cursor::new(
                b"{\"id\":2,\"method\":\"unknown\"}\n{\"method\":\"unknown-notification\"}\n",
            ),
            output.clone(),
        )
        .expect("serve requests");
        let lines = output.lines();
        assert_eq!(lines.len(), 1);
        let response: Value = serde_json::from_str(&lines[0]).expect("JSON response");
        assert_eq!(response["error"]["code"], -32601);
        assert_eq!(response["error"]["message"], "method not found");
    }

    #[test]
    fn arc_30_socket_binds_only_after_initialized() {
        let (socket, mut input, _output, server) = start_server(Output::ClaudeNotification);
        thread::sleep(Duration::from_millis(30));
        assert!(!socket.exists());
        initialize(&mut input, &socket);
        assert!(is_live(&socket));
        drop(input);
        server
            .join()
            .expect("server thread")
            .expect("server result");
    }

    #[test]
    fn arc_30_message_becomes_one_claude_notification_line() {
        let (socket, mut input, output, server) = start_server(Output::ClaudeNotification);
        initialize(&mut input, &socket);
        send(&socket, "worker-1", "hello", SEND_TIMEOUT).expect("send message");
        drop(input);
        server
            .join()
            .expect("server thread")
            .expect("server result");
        assert_eq!(
            output.lines(),
            vec![
                r#"{"jsonrpc":"2.0","method":"notifications/claude/channel","params":{"content":"hello","meta":{"role":"worker-1"}}}"#
            ]
        );
    }

    #[test]
    fn arc_30_concurrent_senders_each_produce_one_whole_line() {
        let (socket, mut input, output, server) = start_server(Output::ClaudeNotification);
        initialize(&mut input, &socket);
        let senders: Vec<_> = (0..8)
            .map(|index| {
                let socket = socket.clone();
                thread::spawn(move || {
                    send(
                        &socket,
                        &format!("worker-{index}"),
                        &format!("message-{index}"),
                        SEND_TIMEOUT,
                    )
                })
            })
            .collect();
        for sender in senders {
            sender.join().expect("sender thread").expect("send result");
        }
        drop(input);
        server
            .join()
            .expect("server thread")
            .expect("server result");
        let lines = output.lines();
        assert_eq!(lines.len(), 8);
        for line in lines {
            let value: Value = serde_json::from_str(&line).expect("whole JSON line");
            assert_eq!(value["method"], "notifications/claude/channel");
        }
    }

    #[test]
    fn arc_30_stale_socket_file_is_replaced() {
        let (socket, mut input, _output, server) = start_server(Output::ClaudeNotification);
        fs::create_dir_all(socket.parent().expect("socket parent")).expect("create parent");
        fs::write(&socket, b"stale").expect("write stale socket");
        initialize(&mut input, &socket);
        assert!(is_live(&socket));
        drop(input);
        server
            .join()
            .expect("server thread")
            .expect("server result");
    }

    #[test]
    fn arc_30_live_relay_is_not_replaced() {
        let (socket, mut input, _output, server) = start_server(Output::ClaudeNotification);
        initialize(&mut input, &socket);
        let result = serve(
            options(socket.clone(), Output::ClaudeNotification),
            Cursor::new(b"{\"method\":\"notifications/initialized\"}\n"),
            SharedWriter::default(),
        );
        let error = result.expect_err("second relay must fail").to_string();
        assert!(error.contains(&format!(
            "a channel relay already serves {}",
            socket.display()
        )));
        assert!(is_live(&socket));
        drop(input);
        server
            .join()
            .expect("server thread")
            .expect("server result");
    }

    #[test]
    fn arc_30_codex_output_runs_codex_queue() {
        let root = temp_root();
        let args_file = root.join("args");
        let script = root.join("codex");
        fs::write(
            &script,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" >> '{}'\n",
                args_file.display()
            ),
        )
        .expect("write codex script");
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).expect("chmod script");
        let (socket, mut input, _output, server) = start_server(Output::CodexQueue {
            codex_bin: script,
            thread: Box::new(|| Ok("T1".to_owned())),
        });
        initialize(&mut input, &socket);
        send(&socket, "worker", "hello codex", SEND_TIMEOUT).expect("send message");
        drop(input);
        server
            .join()
            .expect("server thread")
            .expect("server result");
        assert_eq!(
            fs::read_to_string(args_file).expect("read args"),
            "queue\n--thread\nT1\n--message\nhello codex\n"
        );
    }

    #[test]
    fn arc_30_codex_failure_answers_error() {
        let root = temp_root();
        let script = root.join("codex");
        fs::write(&script, "#!/bin/sh\nexit 1\n").expect("write codex script");
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).expect("chmod script");
        let (socket, mut input, _output, server) = start_server(Output::CodexQueue {
            codex_bin: script,
            thread: Box::new(|| Ok("T1".to_owned())),
        });
        initialize(&mut input, &socket);
        let error = send(&socket, "worker", "hello", SEND_TIMEOUT).expect_err("send must fail");
        assert!(error.to_string().contains("error: codex queue exited"));
        drop(input);
        server
            .join()
            .expect("server thread")
            .expect("server result");
    }

    #[test]
    fn arc_30_send_to_missing_socket_is_err() {
        let socket = socket_path(&temp_root(), "missing");
        assert!(send(&socket, "worker", "hello", SEND_TIMEOUT).is_err());
    }

    #[test]
    fn arc_30_eof_removes_the_socket() {
        let (socket, mut input, _output, server) = start_server(Output::ClaudeNotification);
        initialize(&mut input, &socket);
        assert!(socket.exists());
        drop(input);
        server
            .join()
            .expect("server thread")
            .expect("server result");
        assert!(!socket.exists());
    }
}
