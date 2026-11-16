use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::io::ErrorKind;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tungstenite::protocol::WebSocketConfig;
use tungstenite::{Message, WebSocket};

use super::reflex::Reflexes;

pub const CALL_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
pub struct Event {
    pub method: String,
    pub params: Value,
    pub session: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Command {
    pub session: Option<String>,
    pub method: &'static str,
    pub params: Value,
}

#[derive(Debug, Clone)]
pub enum CdpError {
    Protocol { method: String, message: String },
    Timeout(String),
    Closed(String),
}

impl CdpError {
    pub fn is_navigation_race(&self) -> bool {
        match self {
            CdpError::Protocol { message, .. } => {
                let message = message.to_ascii_lowercase();
                message.contains("context was destroyed")
                    || message.contains("cannot find default execution context")
                    || message.contains("cannot find context")
                    || message.contains("inspected target navigated or closed")
                    || message.contains("execution context was destroyed")
            }
            _ => false,
        }
    }
}

impl fmt::Display for CdpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CdpError::Protocol { method, message } => write!(f, "{method}: {message}"),
            CdpError::Timeout(method) => write!(f, "{method}: no reply from the browser in time"),
            CdpError::Closed(message) => write!(f, "browser connection closed: {message}"),
        }
    }
}

pub struct Cdp {
    socket: WebSocket<TcpStream>,
    next_id: u64,
    events: VecDeque<Event>,
    replies: HashMap<u64, Result<Value, CdpError>>,
    pending_methods: HashMap<u64, String>,
    ignored: HashSet<u64>,
    closed: Option<String>,
    pub reflexes: Reflexes,
}

impl Cdp {
    pub fn connect(url: &str, timeout: Duration) -> Result<Cdp, String> {
        let address = socket_address(url)?;
        let resolved = address
            .to_socket_addrs()
            .map_err(|error| format!("cannot resolve {address}: {error}"))?
            .next()
            .ok_or_else(|| format!("cannot resolve {address}"))?;
        let stream = TcpStream::connect_timeout(&resolved, timeout)
            .map_err(|error| format!("cannot connect to {url}: {error}"))?;
        let _ = stream.set_nodelay(true);
        stream
            .set_read_timeout(Some(timeout))
            .map_err(|error| error.to_string())?;
        let config = WebSocketConfig::default()
            .max_message_size(Some(512 << 20))
            .max_frame_size(Some(512 << 20));
        let (socket, _) = tungstenite::client::client_with_config(url, stream, Some(config))
            .map_err(|error| format!("cannot open the debugging socket {url}: {error}"))?;
        Ok(Cdp {
            socket,
            next_id: 1,
            events: VecDeque::new(),
            replies: HashMap::new(),
            pending_methods: HashMap::new(),
            ignored: HashSet::new(),
            closed: None,
            reflexes: Reflexes::default(),
        })
    }

    pub fn call(
        &mut self,
        session: Option<&str>,
        method: &str,
        params: Value,
    ) -> Result<Value, CdpError> {
        self.call_within(session, method, params, CALL_TIMEOUT)
    }

    pub fn call_within(
        &mut self,
        session: Option<&str>,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, CdpError> {
        let id = self.send(session, method, params)?;
        self.pending_methods.insert(id, method.to_string());
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(reply) = self.replies.remove(&id) {
                self.pending_methods.remove(&id);
                return reply;
            }
            if let Some(reason) = &self.closed {
                return Err(CdpError::Closed(reason.clone()));
            }
            let now = Instant::now();
            if now >= deadline {
                self.pending_methods.remove(&id);
                self.ignored.insert(id);
                return Err(CdpError::Timeout(method.to_string()));
            }
            self.read_one(deadline - now);
        }
    }

    pub fn post(&mut self, session: Option<&str>, method: &str, params: Value) {
        if let Ok(id) = self.send(session, method, params) {
            self.ignored.insert(id);
        }
    }

    pub fn next_event(&mut self, deadline: Instant) -> Option<Event> {
        loop {
            if let Some(event) = self.events.pop_front() {
                return Some(event);
            }
            if self.closed.is_some() {
                return None;
            }
            let now = Instant::now();
            if now >= deadline {
                return None;
            }
            self.read_one(deadline - now);
        }
    }

    pub fn drain_events(&mut self) -> Vec<Event> {
        self.read_available();
        self.events.drain(..).collect()
    }

    pub fn closed(&self) -> Option<&str> {
        self.closed.as_deref()
    }

    pub fn close(&mut self) {
        let _ = self.socket.close(None);
        let _ = self.socket.flush();
    }

    fn read_available(&mut self) {
        if self.closed.is_some() {
            return;
        }
        let deadline = Instant::now() + Duration::from_millis(2);
        while Instant::now() < deadline && self.closed.is_none() {
            if !self.read_one(Duration::from_millis(1)) {
                break;
            }
        }
    }

    fn send(&mut self, session: Option<&str>, method: &str, params: Value) -> Result<u64, CdpError> {
        if let Some(reason) = &self.closed {
            return Err(CdpError::Closed(reason.clone()));
        }
        let id = self.next_id;
        self.next_id += 1;
        let mut message = json!({"id": id, "method": method, "params": params});
        if let Some(session) = session {
            message["sessionId"] = Value::String(session.to_string());
        }
        match self.socket.send(Message::text(message.to_string())) {
            Ok(()) => Ok(id),
            Err(error) => {
                let reason = error.to_string();
                self.closed = Some(reason.clone());
                Err(CdpError::Closed(reason))
            }
        }
    }

    fn read_one(&mut self, wait: Duration) -> bool {
        let wait = wait.max(Duration::from_millis(1));
        let _ = self.socket.get_mut().set_read_timeout(Some(wait));
        match self.socket.read() {
            Ok(Message::Text(text)) => {
                self.dispatch(text.as_str());
                true
            }
            Ok(Message::Close(_)) => {
                self.closed = Some("the browser closed the debugging socket".into());
                false
            }
            Ok(_) => true,
            Err(tungstenite::Error::Io(error))
                if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
            {
                false
            }
            Err(error) => {
                self.closed = Some(error.to_string());
                false
            }
        }
    }

    fn dispatch(&mut self, text: &str) {
        let Ok(message) = serde_json::from_str::<Value>(text) else {
            return;
        };
        if let Some(id) = message.get("id").and_then(Value::as_u64) {
            if self.ignored.remove(&id) {
                return;
            }
            let reply = match message.get("error") {
                Some(error) => Err(CdpError::Protocol {
                    method: self.pending_methods.get(&id).cloned().unwrap_or_default(),
                    message: error
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown error")
                        .to_string(),
                }),
                None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
            };
            self.replies.insert(id, reply);
            return;
        }
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            return;
        };
        let event = Event {
            method: method.to_string(),
            params: message.get("params").cloned().unwrap_or(Value::Null),
            session: message
                .get("sessionId")
                .and_then(Value::as_str)
                .map(str::to_string),
        };
        for command in self.reflexes.react(&event) {
            self.post(command.session.as_deref(), command.method, command.params);
        }
        if self.reflexes.keeps(&event) {
            self.events.push_back(event);
        }
    }
}

fn socket_address(url: &str) -> Result<String, String> {
    let rest = url
        .strip_prefix("ws://")
        .ok_or_else(|| format!("{url} is not a ws:// debugging address"))?;
    let authority = rest.split('/').next().unwrap_or_default();
    if authority.is_empty() {
        return Err(format!("{url} has no host"));
    }
    if authority.contains(':') && !authority.ends_with(']') {
        Ok(authority.to_string())
    } else {
        Ok(format!("{authority}:80"))
    }
}
