use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use tungstenite::handshake::derive_accept_key;
use tungstenite::protocol::Role;
use tungstenite::{Message, WebSocket};

#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub query: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }
}

pub enum Reply {
    Page {
        status: u16,
        content_type: String,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    },
    Events {
        events: Vec<String>,
        interval: Duration,
    },
    Socket {
        messages: Vec<String>,
        interval: Duration,
    },
    Hang,
}

impl Reply {
    pub fn html(body: impl Into<String>) -> Reply {
        Reply::with_type(200, "text/html; charset=utf-8", body.into().into_bytes())
    }

    pub fn status(status: u16, body: impl Into<String>) -> Reply {
        Reply::with_type(status, "text/html; charset=utf-8", body.into().into_bytes())
    }

    pub fn with_type(status: u16, content_type: &str, body: Vec<u8>) -> Reply {
        Reply::Page {
            status,
            content_type: content_type.to_string(),
            headers: Vec::new(),
            body,
        }
    }

    pub fn redirect(location: &str) -> Reply {
        Reply::Page {
            status: 302,
            content_type: "text/plain".into(),
            headers: vec![("Location".into(), location.to_string())],
            body: Vec::new(),
        }
    }

    pub fn download(name: &str, body: &str) -> Reply {
        Reply::Page {
            status: 200,
            content_type: "application/octet-stream".into(),
            headers: vec![(
                "Content-Disposition".into(),
                format!("attachment; filename=\"{name}\""),
            )],
            body: body.as_bytes().to_vec(),
        }
    }

    pub fn not_found() -> Reply {
        Reply::status(404, "<!doctype html><title>missing</title><p>not found</p>")
    }
}

type Handler = dyn Fn(&Request) -> Reply + Send + Sync;

pub struct Fixture {
    pub port: u16,
    requests: Arc<Mutex<Vec<Request>>>,
    stopping: Arc<AtomicBool>,
}

impl Fixture {
    pub fn start(handler: impl Fn(&Request) -> Reply + Send + Sync + 'static) -> Fixture {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind the fixture server");
        let port = listener.local_addr().expect("local address").port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stopping = Arc::new(AtomicBool::new(false));
        let handler: Arc<Handler> = Arc::new(handler);
        let log = requests.clone();
        let stop = stopping.clone();
        thread::spawn(move || {
            for stream in listener.incoming() {
                if stop.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                let handler = handler.clone();
                let log = log.clone();
                let stop = stop.clone();
                thread::spawn(move || serve(stream, handler, log, stop));
            }
        });
        Fixture {
            port,
            requests,
            stopping,
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{}", self.port, path)
    }

    pub fn requests(&self) -> Vec<Request> {
        self.requests.lock().expect("request log").clone()
    }

    pub fn requests_to(&self, path: &str) -> Vec<Request> {
        self.requests()
            .into_iter()
            .filter(|request| request.path == path)
            .collect()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
    }
}

fn read_request(reader: &mut BufReader<TcpStream>) -> Option<Request> {
    let mut line = String::new();
    if reader.read_line(&mut line).ok()? == 0 {
        return None;
    }
    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_string();
    let target = parts.next()?.to_string();
    let (path, query) = match target.split_once('?') {
        Some((path, query)) => (path.to_string(), query.to_string()),
        None => (target, String::new()),
    };
    let mut headers = HashMap::new();
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).ok()? == 0 {
            return None;
        }
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((key, value)) = header.split_once(':') {
            headers.insert(key.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    let length: usize = headers
        .get("content-length")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0; length];
    reader.read_exact(&mut body).ok()?;
    Some(Request {
        method,
        path,
        query,
        headers,
        body,
    })
}

fn serve(
    stream: TcpStream,
    handler: Arc<Handler>,
    log: Arc<Mutex<Vec<Request>>>,
    stop: Arc<AtomicBool>,
) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(60)));
    let Ok(writer) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(stream);
    let mut writer = writer;
    while let Some(request) = read_request(&mut reader) {
        log.lock().expect("request log").push(request.clone());
        let upgrade = request
            .header("upgrade")
            .is_some_and(|value| value.eq_ignore_ascii_case("websocket"));
        match handler(&request) {
            Reply::Page {
                status,
                content_type,
                headers,
                body,
            } => {
                let mut head = format!(
                    "HTTP/1.1 {status} {}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\n",
                    reason(status),
                    body.len()
                );
                for (key, value) in headers {
                    head.push_str(&format!("{key}: {value}\r\n"));
                }
                head.push_str("\r\n");
                if writer.write_all(head.as_bytes()).is_err() || writer.write_all(&body).is_err() {
                    return;
                }
                let _ = writer.flush();
            }
            Reply::Events { events, interval } => {
                let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n";
                if writer.write_all(head.as_bytes()).is_err() {
                    return;
                }
                let _ = writer.flush();
                for event in events {
                    thread::sleep(interval);
                    if writer
                        .write_all(format!("data: {event}\n\n").as_bytes())
                        .is_err()
                    {
                        return;
                    }
                    let _ = writer.flush();
                }
                while !stop.load(Ordering::SeqCst) {
                    thread::sleep(Duration::from_millis(100));
                    if writer.write_all(b": idle\n\n").is_err() {
                        return;
                    }
                }
                return;
            }
            Reply::Socket { messages, interval } if upgrade => {
                let key = request.header("sec-websocket-key").unwrap_or_default();
                let accept = derive_accept_key(key.as_bytes());
                let head = format!(
                    "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
                );
                if writer.write_all(head.as_bytes()).is_err() {
                    return;
                }
                let mut socket = WebSocket::from_raw_socket(writer, Role::Server, None);
                for message in messages {
                    thread::sleep(interval);
                    if socket.send(Message::text(message)).is_err() {
                        return;
                    }
                }
                let _ = socket
                    .get_mut()
                    .set_read_timeout(Some(Duration::from_millis(200)));
                while !stop.load(Ordering::SeqCst) {
                    match socket.read() {
                        Ok(Message::Close(_)) => return,
                        Ok(Message::Text(text)) => {
                            let _ = socket.send(Message::text(format!("echo:{}", text.as_str())));
                        }
                        Ok(_) => {}
                        Err(tungstenite::Error::Io(error))
                            if matches!(
                                error.kind(),
                                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                            ) => {}
                        Err(_) => return,
                    }
                }
                return;
            }
            Reply::Socket { .. } => {
                let _ = writer.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n");
            }
            Reply::Hang => {
                while !stop.load(Ordering::SeqCst) {
                    thread::sleep(Duration::from_millis(100));
                }
                return;
            }
        }
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Status",
    }
}
