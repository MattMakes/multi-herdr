use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};

use crate::cli::InspectArgs;
use crate::driver::DriverKind;
use crate::trace::Trace;
use crate::trace_cmd::load_trace;
use crate::{Io, EXIT_ERROR, EXIT_OK};

pub const VIEWER_HTML: &str = include_str!("inspect/viewer.html");

const REQUEST_HEAD_MAX: usize = 16 * 1024;
const ACCEPT_POLL: Duration = Duration::from_millis(20);
const READ_TIMEOUT: Duration = Duration::from_secs(10);
const WRITE_TIMEOUT: Duration = Duration::from_secs(60);
const CONTENT_SECURITY_POLICY: &str = "default-src 'none'; img-src 'self'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; connect-src 'self'; base-uri 'none'; form-action 'none'";

static INTERRUPTED: AtomicBool = AtomicBool::new(false);

pub fn run(args: &InspectArgs, io: &mut Io<'_>) -> i32 {
    match execute(args, io) {
        Ok(()) => EXIT_OK,
        Err(message) => {
            let _ = writeln!(io.err, "ui-prop-tester: error: {message}");
            EXIT_ERROR
        }
    }
}

fn execute(args: &InspectArgs, io: &mut Io<'_>) -> Result<(), String> {
    let trace = load_trace(&args.trace)?;
    if trace.header.driver != DriverKind::Browser {
        return Err(format!(
            "browser inspect shows browser traces and {} is a terminal trace; draw its screens with `ui-prop-tester trace --screen {}`",
            args.trace.display(),
            args.trace.display()
        ));
    }
    let listener = TcpListener::bind(("127.0.0.1", args.port)).map_err(|error| {
        if error.kind() == io::ErrorKind::AddrInUse {
            format!(
                "port {} on 127.0.0.1 is already in use; stop the other server or choose another port with --port",
                args.port
            )
        } else {
            format!("cannot listen on 127.0.0.1:{}: {error}", args.port)
        }
    })?;
    let address = listener
        .local_addr()
        .map_err(|error| format!("cannot read the listening address: {error}"))?;
    let url = viewer_url(address);
    watch_interrupts();
    let _ = writeln!(
        io.out,
        "ui-prop-tester: trace viewer at {url} (press Ctrl+C to stop)"
    );
    let _ = io.out.flush();
    if !args.no_open {
        if let Err(message) = open_in_browser(&url) {
            let _ = writeln!(
                io.err,
                "ui-prop-tester: warning: could not open a browser ({message}); open {url} yourself"
            );
        }
    }
    let label = trace_label(&args.trace);
    serve(listener, trace, &label, &INTERRUPTED)
        .map_err(|error| format!("the viewer server failed: {error}"))?;
    let _ = writeln!(io.out, "ui-prop-tester: viewer stopped");
    Ok(())
}

pub fn viewer_url(address: SocketAddr) -> String {
    format!("http://{address}/")
}

fn trace_label(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(unix)]
fn watch_interrupts() {
    extern "C" fn on_interrupt(_signal: libc::c_int) {
        INTERRUPTED.store(true, Ordering::SeqCst);
    }
    let handler: extern "C" fn(libc::c_int) = on_interrupt;
    unsafe {
        libc::signal(libc::SIGINT, handler as libc::sighandler_t);
        libc::signal(libc::SIGTERM, handler as libc::sighandler_t);
    }
}

#[cfg(not(unix))]
fn watch_interrupts() {}

fn open_in_browser(url: &str) -> Result<(), String> {
    let mut command = if cfg!(target_os = "macos") {
        Command::new("open")
    } else if cfg!(windows) {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", ""]);
        command
    } else {
        Command::new("xdg-open")
    };
    let program = format!("{:?}", command.get_program());
    let status = command
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| format!("{program} failed to start: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} exited with {status}"))
    }
}

struct Site {
    header: Vec<u8>,
    states: Vec<u8>,
    screenshots: Vec<Option<Vec<u8>>>,
}

impl Site {
    fn new(trace: Trace, label: &str) -> io::Result<Self> {
        let screenshot_count = trace
            .states
            .iter()
            .filter(|state| state.screenshot.is_some())
            .count();
        let header = serde_json::to_vec(&json!({
            "file": label,
            "header": trace.header,
            "summary": trace.summary,
            "state_count": trace.states.len(),
            "screenshot_count": screenshot_count,
        }))
        .map_err(io::Error::other)?;
        let mut states: Vec<Value> = Vec::with_capacity(trace.states.len());
        let mut screenshots = Vec::with_capacity(trace.states.len());
        for mut state in trace.states {
            let screenshot = state.screenshot.take();
            let mut value = serde_json::to_value(&state).map_err(io::Error::other)?;
            if let Value::Object(object) = &mut value {
                object.insert("has_screenshot".into(), Value::Bool(screenshot.is_some()));
            }
            states.push(value);
            screenshots.push(screenshot);
        }
        let states = serde_json::to_vec(&states).map_err(io::Error::other)?;
        Ok(Self {
            header,
            states,
            screenshots,
        })
    }
}

pub fn serve(
    listener: TcpListener,
    trace: Trace,
    label: &str,
    stop: &AtomicBool,
) -> io::Result<()> {
    let site = Arc::new(Site::new(trace, label)?);
    listener.set_nonblocking(true)?;
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                let site = Arc::clone(&site);
                thread::spawn(move || {
                    let _ = answer(stream, &site);
                });
            }
            Err(error)
                if error.kind() == io::ErrorKind::WouldBlock
                    || error.kind() == io::ErrorKind::Interrupted =>
            {
                thread::sleep(ACCEPT_POLL);
            }
            Err(error) if error.kind() == io::ErrorKind::ConnectionAborted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

struct Reply {
    status: &'static str,
    content_type: &'static str,
    body: Vec<u8>,
    allow: bool,
}

impl Reply {
    fn ok(content_type: &'static str, body: Vec<u8>) -> Self {
        Self {
            status: "200 OK",
            content_type,
            body,
            allow: false,
        }
    }

    fn text(status: &'static str, message: &str) -> Self {
        Self {
            status,
            content_type: "text/plain; charset=utf-8",
            body: format!("{message}\n").into_bytes(),
            allow: false,
        }
    }
}

fn answer(mut stream: TcpStream, site: &Site) -> io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(READ_TIMEOUT))?;
    stream.set_write_timeout(Some(WRITE_TIMEOUT))?;
    let head = match read_request_head(&mut stream)? {
        Some(head) => head,
        None => return Ok(()),
    };
    let request_line = head.lines().next().unwrap_or_default();
    let mut words = request_line.split_whitespace();
    let (method, target) = match (words.next(), words.next(), words.next()) {
        (Some(method), Some(target), Some(version)) if version.starts_with("HTTP/") => {
            (method, target)
        }
        _ => {
            return send(
                &mut stream,
                Reply::text("400 Bad Request", "bad request"),
                true,
            )
        }
    };
    let include_body = method != "HEAD";
    let reply = if method == "GET" || method == "HEAD" {
        route(site, target)
    } else {
        Reply {
            allow: true,
            ..Reply::text("405 Method Not Allowed", "only GET and HEAD are served")
        }
    };
    send(&mut stream, reply, include_body)
}

fn read_request_head(stream: &mut TcpStream) -> io::Result<Option<String>> {
    let mut buffer: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 2048];
    loop {
        let count = stream.read(&mut chunk)?;
        if count == 0 {
            return Ok(None);
        }
        buffer.extend_from_slice(&chunk[..count]);
        if let Some(end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            buffer.truncate(end);
            return Ok(Some(String::from_utf8_lossy(&buffer).into_owned()));
        }
        if buffer.len() > REQUEST_HEAD_MAX {
            return Ok(Some(String::new()));
        }
    }
}

fn route(site: &Site, target: &str) -> Reply {
    let path = target.split(['?', '#']).next().unwrap_or_default();
    match path {
        "/" | "/index.html" => Reply::ok("text/html; charset=utf-8", VIEWER_HTML.as_bytes().into()),
        "/api/header" => Reply::ok("application/json", site.header.clone()),
        "/api/states" => Reply::ok("application/json", site.states.clone()),
        _ => match path
            .strip_prefix("/api/screenshots/")
            .and_then(|rest| rest.strip_suffix(".png"))
        {
            Some(index) => screenshot(site, index),
            None => Reply::text("404 Not Found", &format!("{path} is not served here")),
        },
    }
}

fn screenshot(site: &Site, index: &str) -> Reply {
    let found = index
        .parse::<usize>()
        .ok()
        .filter(|_| index.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|position| site.screenshots.get(position))
        .and_then(Option::as_ref);
    match found {
        Some(png) => Reply::ok("image/png", png.clone()),
        None => Reply::text(
            "404 Not Found",
            &format!("state {index} has no screenshot in this trace"),
        ),
    }
}

fn send(stream: &mut TcpStream, reply: Reply, include_body: bool) -> io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n",
        reply.status,
        reply.content_type,
        reply.body.len()
    );
    if reply.content_type.starts_with("text/html") {
        head.push_str(&format!(
            "Content-Security-Policy: {CONTENT_SECURITY_POLICY}\r\n"
        ));
    }
    if reply.allow {
        head.push_str("Allow: GET, HEAD\r\n");
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    if include_body {
        stream.write_all(&reply.body)?;
    }
    stream.flush()?;
    let _ = stream.shutdown(std::net::Shutdown::Write);
    let mut drain = [0u8; 512];
    while matches!(stream.read(&mut drain), Ok(count) if count > 0) {}
    Ok(())
}
