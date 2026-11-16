use std::collections::HashMap;
use std::time::Instant;

use serde_json::{json, Value};

use super::cdp::Event;

#[derive(Debug, Default)]
pub struct PageTracker {
    pub session: String,
    pub target_id: String,
    pub main_frame: String,
    pub loading: bool,
    pub activity: u64,
    pub navigation_status: Option<u16>,
    pub navigation_kind: String,
    pub console: Vec<Value>,
    pub exceptions: Vec<Value>,
    pub downloads: Vec<Value>,
    pub drag_data: Option<Value>,
    pub ended: bool,
    pub crashed: bool,
    pub soft_since: Option<Instant>,
    current_loader: String,
    statuses: HashMap<String, u16>,
}

impl PageTracker {
    pub fn new(session: &str, target_id: &str) -> PageTracker {
        PageTracker {
            session: session.to_string(),
            target_id: target_id.to_string(),
            navigation_kind: "load".into(),
            ..PageTracker::default()
        }
    }

    pub fn expect_navigation(&mut self) {
        self.loading = true;
        self.soft_since = Some(Instant::now());
    }

    pub fn finish_loading(&mut self) {
        self.loading = false;
        self.soft_since = None;
    }

    pub fn handle(&mut self, event: &Event) {
        let params = &event.params;
        let ours = event.session.as_deref() == Some(self.session.as_str());
        let text = |pointer: &str| params.pointer(pointer).and_then(Value::as_str);
        match event.method.as_str() {
            "Browser.downloadWillBegin" => {
                self.downloads.push(json!({
                    "url": text("/url").unwrap_or_default(),
                    "suggested_filename": text("/suggestedFilename").unwrap_or_default(),
                    "state": "in_progress",
                    "path": null,
                    "guid": text("/guid").unwrap_or_default(),
                }));
                self.finish_loading();
                self.activity += 1;
            }
            "Browser.downloadProgress" => {
                let guid = text("/guid").unwrap_or_default();
                if let Some(download) = self
                    .downloads
                    .iter_mut()
                    .find(|download| download["guid"] == guid)
                {
                    let state = match text("/state") {
                        Some("completed") => "completed",
                        Some("canceled") => "canceled",
                        _ => "in_progress",
                    };
                    download["state"] = json!(state);
                    if let Some(path) = text("/filePath") {
                        download["path"] = json!(path);
                    }
                }
            }
            "Target.detachedFromTarget" => {
                if text("/sessionId") == Some(self.session.as_str()) {
                    self.ended = true;
                }
            }
            "Target.targetDestroyed" => {
                if text("/targetId") == Some(self.target_id.as_str()) {
                    self.ended = true;
                }
            }
            "Target.targetCrashed" => {
                if text("/targetId") == Some(self.target_id.as_str()) {
                    self.crashed = true;
                }
            }
            _ if !ours => {}
            "Inspector.targetCrashed" => self.crashed = true,
            "Inspector.detached" => self.ended = true,
            "Page.frameStartedLoading" => {
                if text("/frameId") == Some(self.main_frame.as_str()) {
                    self.loading = true;
                    self.soft_since = None;
                    self.activity += 1;
                }
            }
            "Page.frameRequestedNavigation" => {
                if text("/frameId") == Some(self.main_frame.as_str())
                    && text("/disposition") == Some("currentTab")
                    && !self.loading
                {
                    self.expect_navigation();
                    self.activity += 1;
                }
            }
            "Page.frameStoppedLoading" => {
                if text("/frameId") == Some(self.main_frame.as_str()) {
                    self.finish_loading();
                    self.activity += 1;
                }
            }
            "Page.frameNavigated" => {
                let frame = &params["frame"];
                if frame.get("parentId").is_some() {
                    return;
                }
                if let Some(id) = frame.get("id").and_then(Value::as_str) {
                    self.main_frame = id.to_string();
                }
                let loader = frame
                    .get("loaderId")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let mime = frame
                    .get("mimeType")
                    .and_then(Value::as_str)
                    .unwrap_or("text/html");
                let restored = text("/type") == Some("BackForwardCacheRestore");
                if restored {
                    self.finish_loading();
                }
                self.navigation_kind = if restored {
                    "bfcache".into()
                } else if mime == "text/html" || mime == "application/xhtml+xml" {
                    "load".into()
                } else {
                    "non_document".into()
                };
                self.current_loader = loader.to_string();
                self.navigation_status = self.statuses.get(loader).copied();
                self.console.clear();
                self.exceptions.clear();
                self.activity += 1;
            }
            "Page.navigatedWithinDocument" => {
                if text("/frameId") == Some(self.main_frame.as_str()) {
                    self.navigation_kind = "same_document".into();
                    self.finish_loading();
                    self.activity += 1;
                }
            }
            "Network.responseReceived" => {
                if text("/type") != Some("Document")
                    || text("/frameId") != Some(self.main_frame.as_str())
                {
                    return;
                }
                let url = text("/response/url").unwrap_or_default().to_ascii_lowercase();
                if !(url.starts_with("http://") || url.starts_with("https://")) {
                    return;
                }
                let Some(status) = params
                    .pointer("/response/status")
                    .and_then(Value::as_f64)
                    .map(|status| status as u16)
                else {
                    return;
                };
                let loader = text("/loaderId").unwrap_or_default().to_string();
                if loader == self.current_loader {
                    self.navigation_status = Some(status);
                }
                self.statuses.insert(loader, status);
            }
            "Network.webSocketFrameReceived" | "Network.eventSourceMessageReceived" => {
                self.activity += 1;
            }
            "Runtime.consoleAPICalled" => {
                let level = match text("/type") {
                    Some("error") | Some("assert") => "error",
                    Some("warning") => "warning",
                    _ => return,
                };
                let message = params
                    .get("args")
                    .and_then(Value::as_array)
                    .map(|args| {
                        args.iter()
                            .map(describe_remote_object)
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .unwrap_or_default();
                let frame = params.pointer("/stackTrace/callFrames/0");
                self.console.push(json!({
                    "level": level,
                    "text": message,
                    "url": frame.and_then(|frame| frame.get("url")).and_then(Value::as_str).unwrap_or_default(),
                    "line": frame.and_then(|frame| frame.get("lineNumber")).and_then(Value::as_u64).map_or(0, |line| line + 1),
                }));
                self.activity += 1;
            }
            "Runtime.exceptionThrown" => {
                self.exceptions
                    .push(describe_exception(&params["exceptionDetails"]));
                self.activity += 1;
            }
            "Input.dragIntercepted" => {
                self.drag_data = params.get("data").cloned();
            }
            _ => {}
        }
    }

    pub fn downloads_json(&self) -> Value {
        Value::Array(
            self.downloads
                .iter()
                .map(|download| {
                    let mut download = download.clone();
                    if let Some(object) = download.as_object_mut() {
                        object.remove("guid");
                    }
                    download
                })
                .collect(),
        )
    }
}

fn describe_remote_object(object: &Value) -> String {
    if let Some(value) = object.get("value") {
        return match value {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        };
    }
    if let Some(value) = object.get("unserializableValue").and_then(Value::as_str) {
        return value.to_string();
    }
    if let Some(description) = object.get("description").and_then(Value::as_str) {
        return description.to_string();
    }
    object
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("undefined")
        .to_string()
}

pub fn describe_exception(details: &Value) -> Value {
    let text = details
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let kind = if text.contains("(in promise)") {
        "rejection"
    } else {
        "exception"
    };
    let exception = details.get("exception");
    let description = exception
        .and_then(|exception| exception.get("description"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| exception.map(describe_remote_object))
        .unwrap_or_else(|| text.to_string());
    let mut lines = description.lines();
    let message = lines.next().unwrap_or_default().to_string();
    let mut stack: Vec<String> = lines.map(str::to_string).collect();
    if stack.is_empty() {
        if let Some(frames) = details
            .pointer("/stackTrace/callFrames")
            .and_then(Value::as_array)
        {
            stack = frames
                .iter()
                .map(|frame| {
                    format!(
                        "    at {} ({}:{}:{})",
                        frame
                            .get("functionName")
                            .and_then(Value::as_str)
                            .filter(|name| !name.is_empty())
                            .unwrap_or("<anonymous>"),
                        frame.get("url").and_then(Value::as_str).unwrap_or_default(),
                        frame.get("lineNumber").and_then(Value::as_u64).unwrap_or(0) + 1,
                        frame.get("columnNumber").and_then(Value::as_u64).unwrap_or(0) + 1,
                    )
                })
                .collect();
        }
    }
    let first_frame = details.pointer("/stackTrace/callFrames/0");
    let url = details
        .get("url")
        .and_then(Value::as_str)
        .filter(|url| !url.is_empty())
        .or_else(|| first_frame.and_then(|frame| frame.get("url")).and_then(Value::as_str))
        .unwrap_or_default();
    json!({
        "kind": kind,
        "message": message,
        "url": url,
        "line": details.get("lineNumber").and_then(Value::as_u64).unwrap_or(0) + 1,
        "column": details.get("columnNumber").and_then(Value::as_u64).unwrap_or(0) + 1,
        "stack": stack.join("\n"),
    })
}
