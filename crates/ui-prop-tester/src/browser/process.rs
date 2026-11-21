use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::Value;
use tempfile::TempDir;

use crate::driver::BrowserConfig;

const LAUNCH_TIMEOUT: Duration = Duration::from_secs(90);

const INSTALL_PATHS: &[&str] = &[
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
    "/Applications/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing",
    "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
    "/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary",
    "/usr/bin/google-chrome",
    "/usr/bin/google-chrome-stable",
    "/usr/bin/chromium",
    "/usr/bin/chromium-browser",
    "/snap/bin/chromium",
    "/opt/google/chrome/chrome",
    "/usr/bin/microsoft-edge",
    "/usr/bin/microsoft-edge-stable",
    "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
    "C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe",
    "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe",
    "C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe",
];

const PATH_NAMES: &[&str] = &[
    "google-chrome",
    "google-chrome-stable",
    "chromium",
    "chromium-browser",
    "chrome",
    "microsoft-edge",
    "msedge",
];

pub fn find_chrome(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = explicit {
        return Some(path.to_path_buf());
    }
    let home_paths = std::env::var_os("HOME").map(|home| {
        let home = PathBuf::from(home);
        vec![
            home.join("Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
            home.join("Applications/Chromium.app/Contents/MacOS/Chromium"),
        ]
    });
    let installed = INSTALL_PATHS
        .iter()
        .map(PathBuf::from)
        .chain(home_paths.into_iter().flatten())
        .find(|path| path.is_file());
    if installed.is_some() {
        return installed;
    }
    let path_var = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path_var) {
        for name in PATH_NAMES {
            for candidate in [directory.join(name), directory.join(format!("{name}.exe"))] {
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

pub struct ChromeProcess {
    child: Option<Child>,
    profile: Option<TempDir>,
    pub websocket_url: String,
}

impl ChromeProcess {
    pub fn launch(config: &BrowserConfig) -> Result<ChromeProcess, String> {
        let executable = find_chrome(config.chrome.as_deref()).ok_or_else(|| {
            "no Chromium found; install Google Chrome or Chromium, or pass --chrome <FILE>"
                .to_string()
        })?;
        let profile = tempfile::Builder::new()
            .prefix("ui-prop-tester-profile-")
            .tempdir()
            .map_err(|error| format!("cannot create a browser profile directory: {error}"))?;
        let mut command = Command::new(&executable);
        command
            .arg("--remote-debugging-port=0")
            .arg(format!("--user-data-dir={}", profile.path().display()))
            .arg(format!("--window-size={},{}", config.width, config.height))
            .args([
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-background-networking",
                "--disable-background-timer-throttling",
                "--disable-backgrounding-occluded-windows",
                "--disable-renderer-backgrounding",
                "--disable-component-update",
                "--disable-default-apps",
                "--disable-sync",
                "--disable-extensions",
                "--disable-smooth-scrolling",
                // MacAppCodeSignClone: on macOS, Chrome copies its app into a code_sign_clone
                // folder at each start. A Chrome killed during its start leaves that copy on
                // the disk, and macOS records each copy as one more "Google Chrome" in its
                // privacy lists. The switch stops the copy.
                "--disable-features=Translate,MediaRouter,OptimizationHints,AutofillServerCommunication,DialMediaRouteProvider,MacAppCodeSignClone",
                "--metrics-recording-only",
                "--password-store=basic",
                "--use-mock-keychain",
                "--mute-audio",
            ]);
        if config.headless {
            command.arg("--headless");
        }
        if config.no_sandbox {
            command.arg("--no-sandbox");
        }
        command
            .arg("about:blank")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command
            .spawn()
            .map_err(|error| format!("cannot start {}: {error}", executable.display()))?;
        let stderr = child.stderr.take().expect("stderr is piped");
        let (sender, receiver) = mpsc::channel::<String>();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stderr);
            let mut line = String::new();
            let mut sender = Some(sender);
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        if let Some(active) = &sender {
                            if active.send(line.clone()).is_err() {
                                sender = None;
                            }
                        }
                    }
                }
            }
        });
        let mut process = ChromeProcess {
            child: Some(child),
            profile: Some(profile),
            websocket_url: String::new(),
        };
        let deadline = Instant::now() + LAUNCH_TIMEOUT;
        let mut seen = Vec::new();
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match receiver.recv_timeout(remaining) {
                Ok(line) => {
                    if let Some(index) = line.find("ws://") {
                        process.websocket_url = line[index..].trim().to_string();
                        return Ok(process);
                    }
                    if seen.len() < 20 {
                        seen.push(line.trim().to_string());
                    }
                }
                Err(_) => {
                    process.kill();
                    return Err(format!(
                        "{} did not report a debugging address: {}",
                        executable.display(),
                        seen.join(" | ")
                    ));
                }
            }
        }
    }

    pub fn wait_for_exit(&mut self, timeout: Duration) -> bool {
        let Some(child) = self.child.as_mut() else {
            return true;
        };
        let deadline = Instant::now() + timeout;
        loop {
            match child.try_wait() {
                Ok(Some(_)) => return true,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20))
                }
                _ => return false,
            }
        }
    }

    pub fn kill(&mut self) {
        if let Some(mut child) = self.child.take() {
            #[cfg(unix)]
            {
                let group = child.id() as i32;
                unsafe {
                    libc::kill(-group, libc::SIGKILL);
                }
            }
            let _ = child.kill();
            let _ = child.wait();
            #[cfg(unix)]
            {
                let group = child.id() as i32;
                let deadline = Instant::now() + Duration::from_secs(5);
                while Instant::now() < deadline && unsafe { libc::kill(-group, 0) } == 0 {
                    unsafe {
                        libc::kill(-group, libc::SIGKILL);
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        }
        if let Some(profile) = self.profile.take() {
            let path = profile.path().to_path_buf();
            if profile.close().is_err() {
                let _ = std::fs::remove_dir_all(path);
            }
        }
    }
}

impl Drop for ChromeProcess {
    fn drop(&mut self) {
        self.kill();
    }
}

pub fn browser_websocket_url(endpoint: &str) -> Result<String, String> {
    let lowered = endpoint.to_ascii_lowercase();
    if lowered.starts_with("ws://") {
        return Ok(endpoint.to_string());
    }
    if lowered.starts_with("wss://") || lowered.starts_with("https://") {
        return Err(format!(
            "{endpoint}: encrypted debugging endpoints are not supported; use ws:// or http://"
        ));
    }
    let rest = endpoint.strip_prefix("http://").unwrap_or(endpoint);
    let authority = rest.split('/').next().unwrap_or_default();
    if authority.is_empty() {
        return Err(format!("{endpoint} has no host"));
    }
    let body = http_get(authority, "/json/version")?;
    let value: Value = serde_json::from_str(&body)
        .map_err(|error| format!("{endpoint}/json/version is not JSON: {error}"))?;
    value
        .get("webSocketDebuggerUrl")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| format!("{endpoint}/json/version has no webSocketDebuggerUrl"))
}

fn http_get(authority: &str, path: &str) -> Result<String, String> {
    let address = if authority.contains(':') {
        authority.to_string()
    } else {
        format!("{authority}:80")
    };
    let resolved = address
        .to_socket_addrs()
        .map_err(|error| format!("cannot resolve {address}: {error}"))?
        .next()
        .ok_or_else(|| format!("cannot resolve {address}"))?;
    let mut stream = TcpStream::connect_timeout(&resolved, Duration::from_secs(10))
        .map_err(|error| format!("cannot connect to {address}: {error}"))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let request =
        format!("GET {path} HTTP/1.1\r\nHost: {authority}\r\nAccept: application/json\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|error| format!("cannot write to {address}: {error}"))?;
    let response = read_response(&mut stream)
        .map_err(|error| format!("cannot read from {address}: {error}"))?;
    let text = String::from_utf8_lossy(&response);
    let (head, body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| format!("{address} sent no HTTP response"))?;
    let status_ok = head
        .lines()
        .next()
        .is_some_and(|line| line.split_whitespace().nth(1) == Some("200"));
    if !status_ok {
        return Err(format!(
            "{address}{path} answered {}",
            head.lines().next().unwrap_or_default()
        ));
    }
    let start = body.find('{').unwrap_or(0);
    let end = body.rfind('}').map_or(body.len(), |index| index + 1);
    Ok(body[start..end].to_string())
}

pub fn read_response(stream: &mut impl Read) -> std::io::Result<Vec<u8>> {
    let mut response = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Ok(response);
        }
        response.extend_from_slice(&chunk[..read]);
        let Some(end) = response.windows(4).position(|window| window == b"\r\n\r\n") else {
            continue;
        };
        let head = String::from_utf8_lossy(&response[..end]).to_ascii_lowercase();
        let length = head.lines().find_map(|line| {
            line.strip_prefix("content-length:")
                .and_then(|value| value.trim().parse::<usize>().ok())
        });
        if let Some(length) = length {
            if response.len() >= end + 4 + length {
                response.truncate(end + 4 + length);
                return Ok(response);
            }
        }
    }
}
