//! The local relay between worker messages and an orchestrator agent session.
//!
//! The relay is an MCP server on stdin and stdout. After MCP initialization it
//! also accepts one-line messages on a workspace-specific Unix socket.
//! Off Unix there is no relay: `send` reports it as not delivered.
#![cfg_attr(not(unix), allow(dead_code, unused_imports))]

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
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

/// The longest wait for one relay client to send or receive a complete line.
pub const CONNECTION_TIMEOUT: Duration = Duration::from_secs(5);

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
    pub connection_timeout: Duration,
}

/// Why a relay did not confirm one message.
#[derive(Debug)]
pub enum SendError {
    /// The relay did not receive and deliver the complete message.
    NotDelivered(anyhow::Error),
    /// The relay received the message, but its acknowledgement was not received.
    Unconfirmed(anyhow::Error),
}

impl std::fmt::Display for SendError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotDelivered(error) | Self::Unconfirmed(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for SendError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NotDelivered(error) | Self::Unconfirmed(error) => error.source(),
        }
    }
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
pub fn send(
    socket: &Path,
    from: &str,
    text: &str,
    timeout: Duration,
) -> std::result::Result<(), SendError> {
    request(socket, &json!({"from": from, "text": text}), timeout)
}

/// True when a relay answers a ping within one second.
pub fn is_live(socket: &Path) -> bool {
    request(socket, &json!({"ping": true}), Duration::from_secs(1)).is_ok()
}

/// The relay needs Unix sockets, so off Unix no relay is ever reachable.
#[cfg(not(unix))]
fn request(socket: &Path, _: &Value, _: Duration) -> std::result::Result<(), SendError> {
    Err(SendError::NotDelivered(anyhow!(
        "channel relay {} needs Unix sockets, which this platform lacks",
        socket.display()
    )))
}

/// `timeout` is 1 deadline for the whole request: the connect, every write
/// and the read of the answer each get only the time that is left.
#[cfg(unix)]
fn request(socket: &Path, value: &Value, timeout: Duration) -> std::result::Result<(), SendError> {
    let deadline = Instant::now() + timeout;
    let mut stream = connect_by(socket, deadline)
        .with_context(|| format!("connect to channel relay {}", socket.display()))
        .map_err(SendError::NotDelivered)?;
    let mut line = serde_json::to_vec(value)
        .context("encode channel request")
        .map_err(SendError::NotDelivered)?;
    line.push(b'\n');
    write_by(&mut stream, &line, deadline)
        .context("write channel request")
        .map_err(SendError::NotDelivered)?;

    let answer = read_line_by(&mut stream, deadline)
        .context("read channel relay answer")
        .map_err(SendError::Unconfirmed)?;
    let answer = answer.trim_end_matches(['\r', '\n']);
    if answer == "ok" {
        Ok(())
    } else if answer.is_empty() {
        Err(SendError::Unconfirmed(anyhow!(
            "channel relay closed without an answer"
        )))
    } else {
        Err(SendError::NotDelivered(anyhow!(answer.to_owned())))
    }
}

/// The time left until `deadline`, or a timed-out error once it passed.
fn time_left(deadline: Instant) -> std::io::Result<Duration> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return Err(std::io::ErrorKind::TimedOut.into());
    }
    Ok(left)
}

/// `UnixStream::connect` has no timeout, and it can block while the relay's
/// backlog is full. Connect a nonblocking socket instead, and wait for it
/// only until `deadline`. No helper thread outlives the request.
#[cfg(unix)]
fn connect_by(socket: &Path, deadline: Instant) -> std::io::Result<UnixStream> {
    use std::os::unix::io::AsRawFd;

    let address = unix_address(socket)?;
    let stream = new_nonblocking_socket()?;
    let length = std::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t;
    // SAFETY: `address` is a valid sockaddr_un of `length` bytes.
    let connected = unsafe {
        libc::connect(
            stream.as_raw_fd(),
            (&address as *const libc::sockaddr_un).cast(),
            length,
        )
    };
    if connected != 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::EINPROGRESS) {
            return Err(error);
        }
        wait_ready(&stream, libc::POLLOUT, deadline)?;
        if let Some(error) = stream.take_error()? {
            return Err(error);
        }
    }
    Ok(stream)
}

/// The socket address of `socket`, or an error when the path does not fit.
#[cfg(unix)]
fn unix_address(socket: &Path) -> std::io::Result<libc::sockaddr_un> {
    use std::os::unix::ffi::OsStrExt;

    // SAFETY: sockaddr_un is plain data; all zeroes is a valid value.
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    let path = socket.as_os_str().as_bytes();
    if path.len() >= address.sun_path.len() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "socket path is too long",
        ));
    }
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    for (to, from) in address.sun_path.iter_mut().zip(path) {
        *to = *from as libc::c_char;
    }
    Ok(address)
}

/// A new Unix stream socket that is close-on-exec and nonblocking.
#[cfg(unix)]
fn new_nonblocking_socket() -> std::io::Result<UnixStream> {
    use std::os::unix::io::{AsRawFd, FromRawFd};

    // SAFETY: plain syscall; a negative answer is checked before use.
    let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `fd` is a new socket that nothing else owns.
    let stream = unsafe { UnixStream::from_raw_fd(fd) };
    // SAFETY: plain syscall on the socket that `stream` owns.
    if unsafe { libc::fcntl(stream.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    stream.set_nonblocking(true)?;
    Ok(stream)
}

/// A blocking socket write keeps going while a slow reader frees space, past
/// any write timeout (macOS). So the stream from [`connect_by`] is
/// nonblocking, and `poll` waits only for the time that is left.
#[cfg(unix)]
fn write_by(stream: &mut UnixStream, mut bytes: &[u8], deadline: Instant) -> std::io::Result<()> {
    while !bytes.is_empty() {
        match stream.write(bytes) {
            Ok(0) => return Err(std::io::ErrorKind::WriteZero.into()),
            Ok(written) => bytes = &bytes[written..],
            Err(error) => retry_after(error, stream, libc::POLLOUT, deadline)?,
        }
    }
    Ok(())
}

/// What a failed nonblocking read or write does next. `Ok(())` means try
/// again: a `WouldBlock` waits for `events`, and an interrupted call tries
/// again only while time is left. Any other error is final.
#[cfg(unix)]
fn retry_after(
    error: std::io::Error,
    stream: &UnixStream,
    events: libc::c_short,
    deadline: Instant,
) -> std::io::Result<()> {
    match error.kind() {
        std::io::ErrorKind::WouldBlock => wait_ready(stream, events, deadline),
        std::io::ErrorKind::Interrupted => time_left(deadline).map(drop),
        _ => Err(error),
    }
}

/// One answer line, up to and with its newline, or what came before EOF.
#[cfg(unix)]
fn read_line_by(stream: &mut UnixStream, deadline: Instant) -> std::io::Result<String> {
    let mut answer = Vec::new();
    let mut buffer = [0; 256];
    while !answer.contains(&b'\n') && answer.len() < MAX_LINE_BYTES {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => answer.extend_from_slice(&buffer[..read]),
            Err(error) => retry_after(error, stream, libc::POLLIN, deadline)?,
        }
    }
    String::from_utf8(answer)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

/// Wait until `stream` is ready for `events`, but not past `deadline`.
/// `Ok(())` means ready: `wait_ready` retries an interrupted poll itself,
/// with the time left.
#[cfg(unix)]
fn wait_ready(
    stream: &UnixStream,
    events: libc::c_short,
    deadline: Instant,
) -> std::io::Result<()> {
    use std::os::unix::io::AsRawFd;
    let mut poll = libc::pollfd {
        fd: stream.as_raw_fd(),
        events,
        revents: 0,
    };
    loop {
        let millis = time_left(deadline)?.as_millis().clamp(1, i32::MAX as u128) as libc::c_int;
        // SAFETY: `poll` points to 1 valid pollfd for the length of the call.
        let ready = unsafe { libc::poll(&mut poll, 1, millis) };
        if ready > 0 {
            return Ok(());
        }
        if ready == 0 {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

/// Run the MCP stdio server until `input` reaches EOF.
pub fn serve<R: BufRead, W: Write + Send + 'static>(
    mut opts: ServeOptions,
    mut input: R,
    output: W,
) -> Result<()> {
    let output = Arc::new(Mutex::new(output));
    // `opts.output` moves into `delivery`; the placeholder is never read.
    let delivery = Arc::new(std::mem::replace(
        &mut opts.output,
        Output::ClaudeNotification,
    ));
    let mut relay = None;

    let result = (|| -> Result<()> {
        let mut line = String::new();
        loop {
            line.clear();
            if input.read_line(&mut line).context("read MCP input")? == 0 {
                return Ok(());
            }
            handle_mcp_line(&line, &opts, &delivery, &output, &mut relay)?;
        }
    })();

    drop(relay);
    result
}

/// Answer one MCP input line, and start the relay on `initialized`.
fn handle_mcp_line<W: Write + Send + 'static>(
    line: &str,
    opts: &ServeOptions,
    delivery: &Arc<Output>,
    output: &Arc<Mutex<W>>,
    relay: &mut Option<Relay>,
) -> Result<()> {
    let message: Value = match serde_json::from_str(line) {
        Ok(message) => message,
        Err(error) => {
            eprintln!("invalid MCP JSON: {error}");
            return Ok(());
        }
    };

    let method = message.get("method").and_then(Value::as_str);
    if method == Some("notifications/initialized") && relay.is_none() {
        *relay = Some(start_relay(
            &opts.socket,
            Arc::clone(delivery),
            Arc::clone(output),
            opts.connection_timeout,
        )?);
    }

    let Some(id) = message.get("id") else {
        return Ok(());
    };
    let response = mcp_response(
        id,
        method,
        &message,
        &opts.instructions,
        &opts.version,
        matches!(&**delivery, Output::ClaudeNotification),
    );
    write_json_line(output, &response)
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

struct SocketPathGuard {
    path: PathBuf,
    armed: bool,
}

impl SocketPathGuard {
    fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            armed: true,
        }
    }

    fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for SocketPathGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(&self.path);
        }
    }
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
    connection_timeout: Duration,
) -> Result<Relay> {
    start_relay_after_bind(socket, delivery, output, connection_timeout, || Ok(()))
}

#[cfg(not(unix))]
fn start_relay_after_bind<W, F>(
    socket: &Path,
    _: Arc<Output>,
    _: Arc<Mutex<W>>,
    _: Duration,
    _: F,
) -> Result<Relay>
where
    W: Write + Send + 'static,
    F: FnOnce() -> Result<()>,
{
    bail!(
        "channel relay {} needs Unix sockets, which this platform lacks",
        socket.display()
    )
}

#[cfg(unix)]
fn start_relay_after_bind<W, F>(
    socket: &Path,
    delivery: Arc<Output>,
    output: Arc<Mutex<W>>,
    connection_timeout: Duration,
    after_bind: F,
) -> Result<Relay>
where
    W: Write + Send + 'static,
    F: FnOnce() -> Result<()>,
{
    prepare_socket_path(socket)?;

    let listener = UnixListener::bind(socket)
        .with_context(|| format!("bind channel socket {}", socket.display()))?;
    let socket_guard = SocketPathGuard::new(socket);
    after_bind()?;
    fs::set_permissions(socket, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("set channel socket mode on {}", socket.display()))?;
    listener
        .set_nonblocking(true)
        .context("make channel socket nonblocking")?;

    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let accept_thread = thread::spawn(move || {
        accept_loop(
            listener,
            &thread_stop,
            &delivery,
            &output,
            connection_timeout,
        )
    });

    socket_guard.disarm();

    Ok(Relay {
        socket: socket.to_path_buf(),
        stop,
        accept_thread: Some(accept_thread),
    })
}

/// Make the socket directory, and clear a stale socket file. Fails when the
/// path is too long or a live relay already serves it.
#[cfg(unix)]
fn prepare_socket_path(socket: &Path) -> Result<()> {
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
    clear_stale_socket(socket)
}

/// Remove a socket file that no relay serves. Fails when a live one exists.
#[cfg(unix)]
fn clear_stale_socket(socket: &Path) -> Result<()> {
    if !socket.exists() {
        return Ok(());
    }
    if is_live(socket) {
        bail!("a channel relay already serves {}", socket.display());
    }
    fs::remove_file(socket)
        .with_context(|| format!("remove stale channel socket {}", socket.display()))
}

#[cfg(unix)]
fn set_timeouts(stream: &UnixStream, timeout: Duration) -> std::io::Result<()> {
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))
}

/// Accept connections until `stop` is set. Each one gets its own thread.
#[cfg(unix)]
fn accept_loop<W: Write + Send + 'static>(
    listener: UnixListener,
    stop: &AtomicBool,
    delivery: &Arc<Output>,
    output: &Arc<Mutex<W>>,
    connection_timeout: Duration,
) {
    while !stop.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _)) => match set_timeouts(&stream, connection_timeout) {
                Ok(()) => {
                    let delivery = Arc::clone(delivery);
                    let output = Arc::clone(output);
                    thread::spawn(move || handle_connection(stream, &delivery, &output));
                }
                Err(error) => eprintln!("cannot set channel connection timeout: {error}"),
            },
            Err(error) => {
                if error.kind() != std::io::ErrorKind::WouldBlock {
                    eprintln!("channel socket accept failed: {error}");
                }
                thread::sleep(Duration::from_millis(5));
            }
        }
    }
}

#[cfg(unix)]
fn handle_connection<W: Write>(mut stream: UnixStream, delivery: &Output, output: &Arc<Mutex<W>>) {
    let answer = match read_socket_request(&stream) {
        Ok(SocketRequest::Ping) => Ok(()),
        Ok(SocketRequest::Message { from, text }) => deliver(delivery, output, &from, &text),
        Err(error) => {
            let answer = match error {
                SocketReadError::Timeout => b"error: timeout\n".as_slice(),
                SocketReadError::BadRequest => b"error: bad request\n".as_slice(),
            };
            let _ = stream.write_all(answer);
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

#[cfg(unix)]
enum SocketRequest {
    Ping,
    Message { from: String, text: String },
}

#[cfg(unix)]
enum SocketReadError {
    Timeout,
    BadRequest,
}

#[cfg(unix)]
fn read_socket_request(stream: &UnixStream) -> std::result::Result<SocketRequest, SocketReadError> {
    let bytes = read_request_line(stream)?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| SocketReadError::BadRequest)?;
    if value.get("ping") == Some(&Value::Bool(true)) {
        Ok(SocketRequest::Ping)
    } else {
        message_request(&value)
    }
}

/// One request line without its line ending, at most `MAX_LINE_BYTES` long.
#[cfg(unix)]
fn read_request_line(stream: &UnixStream) -> std::result::Result<Vec<u8>, SocketReadError> {
    let mut bytes = Vec::new();
    let mut limited = BufReader::new(stream).take((MAX_LINE_BYTES + 2) as u64);
    limited.read_until(b'\n', &mut bytes).map_err(|error| {
        if matches!(
            error.kind(),
            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
        ) {
            SocketReadError::Timeout
        } else {
            SocketReadError::BadRequest
        }
    })?;
    bytes
        .ends_with(b"\n")
        .then_some(())
        .ok_or(SocketReadError::BadRequest)?;
    bytes.pop();
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    (bytes.len() <= MAX_LINE_BYTES)
        .then_some(bytes)
        .ok_or(SocketReadError::BadRequest)
}

#[cfg(unix)]
fn message_request(value: &Value) -> std::result::Result<SocketRequest, SocketReadError> {
    let from = value
        .get("from")
        .and_then(Value::as_str)
        .ok_or(SocketReadError::BadRequest)?;
    let text = value
        .get("text")
        .and_then(Value::as_str)
        .ok_or(SocketReadError::BadRequest)?;
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

#[cfg(unix)]
#[cfg(test)]
mod tests {
    use std::io::{BufReader, Cursor};
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::{UnixListener, UnixStream};
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
            connection_timeout: CONNECTION_TIMEOUT,
        }
    }

    fn start_server(
        output_kind: Output,
    ) -> (PathBuf, UnixStream, SharedWriter, JoinHandle<Result<()>>) {
        start_server_with_timeout(output_kind, CONNECTION_TIMEOUT)
    }

    fn start_server_with_timeout(
        output_kind: Output,
        connection_timeout: Duration,
    ) -> (PathBuf, UnixStream, SharedWriter, JoinHandle<Result<()>>) {
        let socket = socket_path(&temp_root(), "test");
        let (reader, writer) = UnixStream::pair().expect("stdin pair");
        let output = SharedWriter::default();
        let server_output = output.clone();
        let server_socket = socket.clone();
        let handle = thread::spawn(move || {
            let mut options = options(server_socket, output_kind);
            options.connection_timeout = connection_timeout;
            serve(options, BufReader::new(reader), server_output)
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
                "#!/bin/sh\nif [ \"$1\" = warm ]; then\n  exit 0\nfi\nprintf '%s\\n' \"$@\" >> '{}'\n",
                args_file.display()
            ),
        )
        .expect("write codex script");
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).expect("chmod script");
        assert!(Command::new(&script)
            .arg("warm")
            .status()
            .expect("warm codex script")
            .success());
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
        fs::write(
            &script,
            "#!/bin/sh\nif [ \"$1\" = warm ]; then\n  exit 0\nfi\nexit 1\n",
        )
        .expect("write codex script");
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).expect("chmod script");
        assert!(Command::new(&script)
            .arg("warm")
            .status()
            .expect("warm codex script")
            .success());
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
        assert!(matches!(
            send(&socket, "worker", "hello", SEND_TIMEOUT),
            Err(SendError::NotDelivered(_))
        ));
    }

    /// Send `bytes` to the relay, close the write side, and read its answer.
    fn raw_answer(socket: &Path, bytes: &[u8]) -> String {
        let mut stream = UnixStream::connect(socket).expect("connect raw client");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("set client timeout");
        // The relay may answer and close before it has read every byte of an
        // oversized line, so a broken pipe here is not an error.
        let _ = stream.write_all(bytes);
        let _ = stream.shutdown(std::net::Shutdown::Write);
        let mut answer = String::new();
        BufReader::new(stream)
            .read_line(&mut answer)
            .expect("read raw answer");
        answer
    }

    #[test]
    fn a_malformed_socket_request_gets_one_error_answer() {
        let (socket, mut input, output, server) = start_server(Output::ClaudeNotification);
        initialize(&mut input, &socket);
        let cases: [(&str, &[u8]); 5] = [
            ("no newline", b"{\"ping\":true}"),
            ("not json", b"nope\n"),
            ("no from", b"{\"text\":\"hi\"}\n"),
            ("no text", b"{\"from\":\"a\"}\n"),
            ("text not a string", b"{\"from\":\"a\",\"text\":1}\n"),
        ];
        for (name, bytes) in cases {
            assert_eq!(raw_answer(&socket, bytes), "error: bad request\n", "{name}");
        }
        assert!(output.lines().is_empty(), "no case may reach the output");
        drop(input);
        server
            .join()
            .expect("server thread")
            .expect("server result");
    }

    /// Parse `bytes` as one socket request on a blocking socket pair.
    fn parse_request(bytes: Vec<u8>) -> std::result::Result<SocketRequest, SocketReadError> {
        let (reader, mut writer) = UnixStream::pair().expect("socket pair");
        let sender = thread::spawn(move || {
            let _ = writer.write_all(&bytes);
        });
        let parsed = read_socket_request(&reader);
        drop(reader);
        sender.join().expect("sender thread");
        parsed
    }

    #[test]
    fn a_line_over_the_limit_is_a_bad_request_and_one_at_the_limit_is_not() {
        // `len` counts the newline too.
        let line = |len: usize| {
            let mut line = format!("{{\"ping\":true,\"pad\":\"").into_bytes();
            let pad = len - line.len() - 3;
            line.extend(std::iter::repeat(b'x').take(pad));
            line.extend_from_slice(b"\"}\n");
            line
        };
        assert!(matches!(
            parse_request(line(MAX_LINE_BYTES + 2)),
            Err(SocketReadError::BadRequest)
        ));
        assert!(matches!(
            parse_request(line(MAX_LINE_BYTES + 1)),
            Ok(SocketRequest::Ping)
        ));
    }

    #[test]
    fn a_ping_and_a_crlf_message_are_answered_ok() {
        let (socket, mut input, output, server) = start_server(Output::ClaudeNotification);
        initialize(&mut input, &socket);
        assert_eq!(raw_answer(&socket, b"{\"ping\":true}\n"), "ok\n");
        assert!(output.lines().is_empty(), "a ping delivers nothing");
        assert_eq!(
            raw_answer(&socket, b"{\"from\":\"a\",\"text\":\"hi\"}\r\n"),
            "ok\n"
        );
        assert_eq!(output.lines().len(), 1, "the message is delivered once");
        drop(input);
        server
            .join()
            .expect("server thread")
            .expect("server result");
    }

    #[test]
    fn invalid_mcp_json_is_skipped_and_serving_goes_on() {
        let (socket, mut input, _output, server) = start_server(Output::ClaudeNotification);
        input.write_all(b"not json\n").expect("write garbage");
        initialize(&mut input, &socket);
        assert_eq!(raw_answer(&socket, b"{\"ping\":true}\n"), "ok\n");
        drop(input);
        server
            .join()
            .expect("server thread")
            .expect("server result");
    }

    #[test]
    fn arc_30_silent_client_is_dropped_after_the_timeout() {
        let (socket, mut input, _output, server) =
            start_server_with_timeout(Output::ClaudeNotification, Duration::from_millis(200));
        initialize(&mut input, &socket);
        let stream = UnixStream::connect(&socket).expect("connect silent client");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("set client timeout");
        let mut answer = String::new();
        BufReader::new(stream)
            .read_line(&mut answer)
            .expect("read timeout answer");
        assert_eq!(answer, "error: timeout\n");
        drop(input);
        server
            .join()
            .expect("server thread")
            .expect("server result");
    }

    #[test]
    fn arc_30_send_reports_unconfirmed_when_no_answer() {
        let socket = socket_path(&temp_root(), "unconfirmed");
        fs::create_dir_all(socket.parent().expect("socket parent")).expect("create parent");
        let listener = UnixListener::bind(&socket).expect("bind test listener");
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept request");
            let mut request = String::new();
            BufReader::new(stream)
                .read_line(&mut request)
                .expect("read request");
            thread::sleep(Duration::from_millis(200));
            request
        });
        let result = send(&socket, "worker", "hello", Duration::from_millis(50));
        assert!(matches!(result, Err(SendError::Unconfirmed(_))));
        assert!(server
            .join()
            .expect("server thread")
            .contains("\"text\":\"hello\""));
    }

    /// A relay that accepts and then reads `chunk` bytes every `pause`, and
    /// never answers. A `chunk` of 0 never reads at all.
    fn stalled_relay(name: &str, chunk: usize, pause: Duration) -> (PathBuf, JoinHandle<()>) {
        let socket = socket_path(&temp_root(), name);
        fs::create_dir_all(socket.parent().expect("socket parent")).expect("create parent");
        let listener = UnixListener::bind(&socket).expect("bind test listener");
        let server = thread::spawn(move || read_slowly(listener, chunk, pause));
        (socket, server)
    }

    /// The body of [`stalled_relay`]: read until the sender hangs up.
    fn read_slowly(listener: UnixListener, chunk: usize, pause: Duration) {
        let (mut stream, _) = listener.accept().expect("accept request");
        let mut buffer = vec![0; chunk.max(1)];
        loop {
            thread::sleep(pause);
            if chunk == 0 {
                continue;
            }
            if matches!(stream.read(&mut buffer), Ok(0) | Err(_)) {
                return;
            }
        }
    }

    /// #67: a relay that accepts and never reads or answers costs the push
    /// no more than its timeout.
    #[test]
    fn send_to_a_relay_that_never_reads_ends_inside_the_timeout() {
        let (socket, _server) = stalled_relay("never-reads", 0, Duration::from_secs(60));
        let text = "x".repeat(1024 * 1024);
        let started = Instant::now();
        let result = send(&socket, "worker", &text, Duration::from_millis(300));
        let took = started.elapsed();
        assert!(matches!(result, Err(SendError::NotDelivered(_))));
        assert!(took < Duration::from_millis(600), "{took:?}");
    }

    /// #67: the timeout is 1 deadline for the whole request. A relay that
    /// reads a little before each write times out cannot stretch it.
    #[test]
    fn a_slow_reader_cannot_stretch_the_send_past_its_timeout() {
        let (socket, server) = stalled_relay("trickle", 512, Duration::from_millis(50));
        let text = "x".repeat(256 * 1024);
        let started = Instant::now();
        let result = send(&socket, "worker", &text, Duration::from_millis(300));
        let took = started.elapsed();
        assert!(result.is_err());
        assert!(took < Duration::from_millis(700), "{took:?}");
        server.join().expect("server thread");
    }

    #[test]
    fn arc_30_send_reports_not_delivered_on_error_answer() {
        let socket = socket_path(&temp_root(), "rejected");
        fs::create_dir_all(socket.parent().expect("socket parent")).expect("create parent");
        let listener = UnixListener::bind(&socket).expect("bind test listener");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let mut request = String::new();
            BufReader::new(&stream)
                .read_line(&mut request)
                .expect("read request");
            stream
                .write_all(b"error: rejected\n")
                .expect("write answer");
        });
        let result = send(&socket, "worker", "hello", SEND_TIMEOUT);
        assert!(matches!(result, Err(SendError::NotDelivered(_))));
        server.join().expect("server thread");
    }

    #[test]
    fn arc_30_failed_setup_removes_the_socket() {
        let socket = socket_path(&temp_root(), "failed-setup");
        let result = start_relay_after_bind(
            &socket,
            Arc::new(Output::ClaudeNotification),
            Arc::new(Mutex::new(SharedWriter::default())),
            CONNECTION_TIMEOUT,
            || bail!("injected setup failure"),
        );
        let error = match result {
            Ok(_) => panic!("setup must fail"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("injected setup failure"));
        assert!(!socket.exists(), "failed setup left the socket path");
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
