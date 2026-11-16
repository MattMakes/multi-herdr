#![allow(dead_code)]

pub mod server;

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use serde_json::{json, Value};
use ui_prop_tester::driver::{Action, BrowserConfig, Driver, Observation};

pub use server::{Fixture, Reply};

pub fn chrome() -> Option<PathBuf> {
    if std::env::var_os("CI").is_some()
        && std::env::var("UI_PROP_TESTER_BROWSER_TESTS").as_deref() != Ok("1")
    {
        eprintln!("skipping the browser test: CI is set and UI_PROP_TESTER_BROWSER_TESTS is not 1");
        return None;
    }
    let explicit = std::env::var_os("UI_PROP_TESTER_CHROME").map(PathBuf::from);
    match ui_prop_tester::browser::find_chrome(explicit.as_deref()) {
        Some(path) => Some(path),
        None => {
            eprintln!("skipping the browser test: no Chromium found");
            None
        }
    }
}

pub fn config(origin: &str) -> BrowserConfig {
    BrowserConfig {
        origin: origin.to_string(),
        headless: true,
        no_sandbox: false,
        width: 800,
        height: 600,
        device_scale_factor: 1.0,
        grant_permissions: Vec::new(),
        virtual_time_policy: None,
        headers: Vec::new(),
        cookies: Vec::new(),
        allow_urls: Vec::new(),
        instrument_javascript: false,
        coverage_symbols: None,
        remote_debugger: None,
        create_target: false,
        chrome: None,
    }
}

pub struct Session {
    pub driver: Box<dyn Driver>,
    pub state: Value,
    pub screenshot: Option<Vec<u8>>,
    pub ended: bool,
}

impl Session {
    pub fn open(config: BrowserConfig) -> Option<Session> {
        let chrome = chrome()?;
        let mut config = config;
        if config.chrome.is_none() {
            config.chrome = Some(chrome);
        }
        let mut driver = ui_prop_tester::browser::open(&config)
            .unwrap_or_else(|error| panic!("opening the browser driver: {error}"));
        let first = driver
            .start()
            .unwrap_or_else(|error| panic!("starting the browser driver: {error}"));
        let mut session = Session {
            driver,
            state: Value::Null,
            screenshot: None,
            ended: false,
        };
        session.record(first);
        Some(session)
    }

    pub fn open_with(config: BrowserConfig, setup: &ui_prop_tester::SpecSetup) -> Option<Session> {
        let chrome = chrome()?;
        let mut config = config;
        if config.chrome.is_none() {
            config.chrome = Some(chrome);
        }
        let mut driver = ui_prop_tester::browser::open(&config)
            .unwrap_or_else(|error| panic!("opening the browser driver: {error}"));
        driver.prepare(setup).expect("prepare");
        let first = driver.start().expect("start");
        let mut session = Session {
            driver,
            state: Value::Null,
            screenshot: None,
            ended: false,
        };
        session.record(first);
        Some(session)
    }

    fn record(&mut self, observation: Observation) {
        self.state = observation.state;
        self.screenshot = observation.screenshot;
        self.ended = observation.ended;
    }

    pub fn apply(&mut self, action: Action) -> &Value {
        let observation = self
            .driver
            .apply(&action)
            .unwrap_or_else(|error| panic!("applying {action}: {error}"));
        self.record(observation);
        &self.state
    }

    pub fn try_apply(&mut self, action: Action) -> Result<&Value, ui_prop_tester::DriverError> {
        let observation = self.driver.apply(&action)?;
        self.record(observation);
        Ok(&self.state)
    }

    pub fn clickable(&self, predicate: impl Fn(&Value) -> bool) -> Option<Value> {
        self.state["clickables"]
            .as_array()?
            .iter()
            .find(|candidate| predicate(candidate))
            .cloned()
    }

    pub fn clickable_by_id(&self, id: &str) -> Value {
        self.clickable(|candidate| candidate["target"]["id"] == id)
            .unwrap_or_else(|| {
                panic!(
                    "no clickable with id {id} among {}",
                    self.state["clickables"]
                )
            })
    }

    pub fn has_clickable_id(&self, id: &str) -> bool {
        self.clickable(|candidate| candidate["target"]["id"] == id)
            .is_some()
    }

    pub fn click_id(&mut self, id: &str) -> &Value {
        let candidate = self.clickable_by_id(id);
        self.apply(click_action(&candidate))
    }

    pub fn title(&self) -> &str {
        self.state["title"].as_str().unwrap_or_default()
    }

    pub fn url(&self) -> &str {
        self.state["url"].as_str().unwrap_or_default()
    }

    pub fn eval(&mut self, source: &str) -> Value {
        let outcomes = self
            .driver
            .extract(&[ui_prop_tester::RemoteExtractor {
                id: 0,
                source: source.to_string(),
            }])
            .expect("extract");
        match outcomes.into_iter().next() {
            Some(ui_prop_tester::ExtractOutcome::Value(value)) => value,
            other => panic!("the extractor {source} gave {other:?}"),
        }
    }

    pub fn wait_until(&mut self, source: &str) -> Value {
        for _ in 0..100 {
            let value = self.eval(source);
            if value != Value::Null && value != Value::Bool(false) {
                return value;
            }
            self.apply(json!({"kind": "wait", "ms": 100}));
        }
        panic!("{source} never held");
    }

    pub fn shutdown(mut self) {
        self.driver.shutdown().expect("shutdown");
    }
}

pub fn click_action(candidate: &Value) -> Value {
    json!({"kind": "click", "target": candidate["target"], "point": candidate["point"]})
}

pub fn page(body: &str) -> Reply {
    Reply::html(format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>fixture</title></head><body>{body}</body></html>"
    ))
}

pub fn temp_html(directory: &Path, name: &str, body: &str) -> PathBuf {
    let path = directory.join(name);
    std::fs::write(
        &path,
        format!("<!doctype html><html><head><meta charset=\"utf-8\"><title>{name}</title></head><body>{body}</body></html>"),
    )
    .expect("write the fixture file");
    path
}

pub struct ExternalChrome {
    child: Child,
    _profile: tempfile::TempDir,
    pub websocket_url: String,
    pub http_url: String,
}

impl ExternalChrome {
    pub fn launch(executable: &Path) -> ExternalChrome {
        let profile = tempfile::tempdir().expect("profile directory");
        let mut command = Command::new(executable);
        command
            .arg("--headless")
            .arg("--remote-debugging-port=0")
            .arg(format!("--user-data-dir={}", profile.path().display()))
            .args(["--no-first-run", "--no-default-browser-check", "--use-mock-keychain", "--password-store=basic"])
            .arg("about:blank")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command.spawn().expect("launch the external browser");
        let stderr = child.stderr.take().expect("stderr");
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                if let Some(index) = line.find("ws://") {
                    let _ = sender.send(line[index..].trim().to_string());
                }
            }
        });
        let websocket_url = receiver
            .recv_timeout(Duration::from_secs(90))
            .expect("the external browser reports its debugging address");
        let authority = websocket_url
            .trim_start_matches("ws://")
            .split('/')
            .next()
            .unwrap_or_default()
            .to_string();
        ExternalChrome {
            child,
            _profile: profile,
            websocket_url,
            http_url: format!("http://{authority}"),
        }
    }

    pub fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    pub fn page_urls(&self) -> Vec<String> {
        let authority = self.http_url.trim_start_matches("http://");
        let mut stream = std::net::TcpStream::connect(authority).expect("connect to the browser");
        use std::io::{Read, Write};
        stream
            .write_all(format!("GET /json/list HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n").as_bytes())
            .expect("request the page list");
        let mut text = String::new();
        stream.read_to_string(&mut text).expect("read the page list");
        let body = text.split_once("\r\n\r\n").map(|(_, body)| body).unwrap_or_default();
        let start = body.find('[').unwrap_or(0);
        let end = body.rfind(']').map_or(body.len(), |index| index + 1);
        let list: Value = serde_json::from_str(&body[start..end]).unwrap_or(json!([]));
        list.as_array()
            .into_iter()
            .flatten()
            .filter(|entry| entry["type"] == "page")
            .filter_map(|entry| entry["url"].as_str().map(str::to_string))
            .collect()
    }
}

impl Drop for ExternalChrome {
    fn drop(&mut self) {
        #[cfg(unix)]
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        std::thread::sleep(Duration::from_millis(200));
    }
}
