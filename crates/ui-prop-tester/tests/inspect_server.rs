#[path = "trace_fixtures.rs"]
mod trace_fixtures;

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use serde_json::Value;
use trace_fixtures::{browser_trace, path_text, run_cli, terminal_trace, write_trace};
use ui_prop_tester::inspect::serve;
use ui_prop_tester::trace::Trace;

struct Server {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<std::io::Result<()>>>,
}

impl Server {
    fn start(trace: Trace) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let handle = thread::spawn(move || serve(listener, trace, "run.trace", &flag));
        Self {
            address,
            stop,
            handle: Some(handle),
        }
    }

    fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let result = self.handle.take().unwrap().join().unwrap();
        assert!(result.is_ok(), "{result:?}");
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

struct Response {
    status: u16,
    head: String,
    body: Vec<u8>,
}

impl Response {
    fn header(&self, name: &str) -> Option<String> {
        self.head.lines().skip(1).find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.trim()
                .eq_ignore_ascii_case(name)
                .then(|| value.trim().to_string())
        })
    }

    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap()
    }
}

fn request(address: SocketAddr, method: &str, path: &str) -> Response {
    let mut stream = TcpStream::connect(address).unwrap();
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).unwrap();
    let split = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("a response head");
    let head = String::from_utf8(bytes[..split].to_vec()).unwrap();
    let body = bytes[split + 4..].to_vec();
    let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    Response { status, head, body }
}

fn get(address: SocketAddr, path: &str) -> Response {
    request(address, "GET", path)
}

#[test]
fn the_index_page_is_the_self_contained_viewer() {
    let server = Server::start(browser_trace());
    let page = get(server.address, "/");
    assert_eq!(page.status, 200);
    assert_eq!(
        page.header("content-type").as_deref(),
        Some("text/html; charset=utf-8")
    );
    let policy = page.header("content-security-policy").unwrap();
    assert!(policy.contains("default-src 'none'"), "{policy}");
    assert!(policy.contains("connect-src 'self'"), "{policy}");
    let html = String::from_utf8(page.body.clone()).unwrap();
    assert_eq!(
        page.header("content-length").unwrap(),
        page.body.len().to_string()
    );
    for needle in [
        "/api/header",
        "/api/states",
        "/api/screenshots/",
        "ArrowLeft",
        "prefers-color-scheme: dark",
    ] {
        assert!(html.contains(needle), "the viewer lacks {needle}");
    }
    for external in ["src=\"http", "href=\"http", "url(", "@import", "<!--", "/*"] {
        assert!(!html.contains(external), "the viewer contains {external}");
    }
    for metric in [
        "js_heap_used",
        "js_heap_total",
        "dom_nodes",
        "event_listeners",
        "layout_objects",
    ] {
        assert!(html.contains(metric), "the viewer does not chart {metric}");
    }
    assert_eq!(get(server.address, "/index.html?x=1").status, 200);
    server.stop();
}

#[test]
fn the_header_endpoint_describes_the_run() {
    let server = Server::start(browser_trace());
    let response = get(server.address, "/api/header");
    assert_eq!(response.status, 200);
    assert_eq!(
        response.header("content-type").as_deref(),
        Some("application/json")
    );
    let value = response.json();
    assert_eq!(value["file"], "run.trace");
    assert_eq!(value["state_count"], 4);
    assert_eq!(value["screenshot_count"], 3);
    assert_eq!(value["header"]["seed"], 42);
    assert_eq!(value["header"]["driver"], "browser");
    assert_eq!(
        value["header"]["target"]["origin"],
        "http://localhost:8080/"
    );
    assert_eq!(value["header"]["specification"]["path"], "specs/app.js");
    assert_eq!(value["summary"]["outcome"], "violation");
    server.stop();
}

#[test]
fn the_states_endpoint_lists_every_state_without_screenshot_bytes() {
    let trace = browser_trace();
    let server = Server::start(trace.clone());
    let response = get(server.address, "/api/states");
    assert_eq!(response.status, 200);
    let states = response.json();
    let states = states.as_array().unwrap();
    assert_eq!(states.len(), 4);
    for (value, state) in states.iter().zip(&trace.states) {
        assert_eq!(value["index"], state.index);
        assert_eq!(value["state"], state.state);
        assert_eq!(value["cells"], state.cells);
        assert_eq!(value["has_screenshot"], state.screenshot.is_some());
        assert!(value.get("screenshot").is_none());
    }
    assert_eq!(states[1]["action"]["target"]["test_id"], "save");
    assert_eq!(
        states[3]["violations"][0]["property"],
        "savedMessageAppears"
    );
    server.stop();
}

#[test]
fn screenshots_are_served_as_png_bytes() {
    let trace = browser_trace();
    let server = Server::start(trace.clone());
    for index in [0usize, 1, 3] {
        let response = get(server.address, &format!("/api/screenshots/{index}.png"));
        assert_eq!(response.status, 200);
        assert_eq!(
            response.header("content-type").as_deref(),
            Some("image/png")
        );
        assert_eq!(
            &response.body,
            trace.states[index].screenshot.as_ref().unwrap()
        );
        assert!(response.body.starts_with(b"\x89PNG\r\n\x1a\n"));
    }
    server.stop();
}

#[test]
fn unknown_paths_states_and_methods_are_refused() {
    let server = Server::start(browser_trace());
    for path in [
        "/api/screenshots/2.png",
        "/api/screenshots/4.png",
        "/api/screenshots/x.png",
        "/api/screenshots/+1.png",
        "/api/screenshots/1",
        "/nope",
        "/api/state",
    ] {
        let response = get(server.address, path);
        assert_eq!(response.status, 404, "{path}");
        assert!(
            response
                .header("content-type")
                .unwrap()
                .starts_with("text/plain"),
            "{path}"
        );
    }
    let post = request(server.address, "POST", "/api/states");
    assert_eq!(post.status, 405);
    assert_eq!(post.header("allow").as_deref(), Some("GET, HEAD"));
    let head = request(server.address, "HEAD", "/api/states");
    assert_eq!(head.status, 200);
    assert!(head.body.is_empty());
    assert_ne!(head.header("content-length").as_deref(), Some("0"));
    server.stop();
}

#[test]
fn a_garbled_request_gets_a_bad_request_answer() {
    let server = Server::start(browser_trace());
    let mut stream = TcpStream::connect(server.address).unwrap();
    stream.write_all(b"nonsense\r\n\r\n").unwrap();
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).unwrap();
    assert!(
        bytes.starts_with(b"HTTP/1.1 400 "),
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    server.stop();
}

#[test]
fn a_port_in_use_is_a_clear_error() {
    let directory = tempfile::tempdir().unwrap();
    let path = write_trace(directory.path(), "run.trace", &browser_trace());
    let holder = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = holder.local_addr().unwrap().port().to_string();
    let result = run_cli(&[
        "browser",
        "inspect",
        "--no-open",
        "--port",
        &port,
        path_text(&path),
    ]);
    assert_eq!(result.code, 1);
    assert!(
        result.err.starts_with("ui-prop-tester: error: ")
            && result.err.contains(&format!("port {port}"))
            && result.err.contains("already in use"),
        "{}",
        result.err
    );
    assert!(result.out.is_empty(), "{}", result.out);
}

#[test]
fn a_terminal_trace_points_to_trace_screen() {
    let directory = tempfile::tempdir().unwrap();
    let path = write_trace(directory.path(), "tty.trace", &terminal_trace());
    let result = run_cli(&[
        "browser",
        "inspect",
        "--no-open",
        "--port",
        "0",
        path_text(&path),
    ]);
    assert_eq!(result.code, 1);
    assert!(result.err.contains("terminal trace"), "{}", result.err);
    assert!(result.err.contains("trace --screen"), "{}", result.err);
}

#[test]
fn a_missing_trace_is_an_error() {
    let directory = tempfile::tempdir().unwrap();
    let missing = directory.path().join("none.trace");
    let result = run_cli(&["browser", "inspect", "--no-open", path_text(&missing)]);
    assert_eq!(result.code, 1);
    assert!(result.err.contains("does not exist"), "{}", result.err);
}

#[cfg(unix)]
#[test]
fn the_command_serves_until_interrupted_and_exits_zero() {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};

    let binary = env!("CARGO_BIN_EXE_ui-prop-tester");
    let warm = Command::new(binary).arg("--version").output().unwrap();
    assert!(warm.status.success());

    let directory = tempfile::tempdir().unwrap();
    let path = write_trace(directory.path(), "run.trace", &browser_trace());
    let mut child = Command::new(binary)
        .args(["browser", "inspect", "--no-open", "--port", "0"])
        .arg(&path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    stdout.read_line(&mut line).unwrap();
    let url = line
        .split_whitespace()
        .find(|word| word.starts_with("http://127.0.0.1:"))
        .map(str::to_string);
    let Some(url) = url else {
        let output = child.wait_with_output().unwrap();
        panic!(
            "no URL in {line:?}; exit {:?}, stderr {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    };
    let address: SocketAddr = url
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .parse()
        .unwrap();
    let header = get(address, "/api/header");
    assert_eq!(header.status, 200);
    assert_eq!(header.json()["state_count"], 4);
    let pid = i32::try_from(child.id()).unwrap();
    assert_eq!(unsafe { libc::kill(pid, libc::SIGINT) }, 0);
    let status = child.wait().unwrap();
    assert_eq!(status.code(), Some(0));
    let mut rest = String::new();
    stdout.read_to_string(&mut rest).unwrap();
    assert!(rest.contains("viewer stopped"), "{rest}");
}
