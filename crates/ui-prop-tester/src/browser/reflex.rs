use serde_json::{json, Value};

use super::boundary::Boundary;
use super::cdp::{Command, Event};

const KEPT_EVENTS: &[&str] = &[
    "Page.frameStartedLoading",
    "Page.frameStoppedLoading",
    "Page.frameNavigated",
    "Page.navigatedWithinDocument",
    "Page.frameRequestedNavigation",
    "Page.loadEventFired",
    "Page.javascriptDialogOpening",
    "Page.backForwardCacheNotUsed",
    "Network.responseReceived",
    "Network.webSocketFrameReceived",
    "Network.eventSourceMessageReceived",
    "Runtime.consoleAPICalled",
    "Runtime.exceptionThrown",
    "Browser.downloadWillBegin",
    "Browser.downloadProgress",
    "Input.dragIntercepted",
    "Inspector.targetCrashed",
    "Inspector.detached",
    "Target.detachedFromTarget",
    "Target.targetDestroyed",
    "Target.targetCrashed",
];

#[derive(Debug, Default)]
pub struct Reflexes {
    pub page_session: Option<String>,
    pub target_id: Option<String>,
    pub main_frame: Option<String>,
    pub boundary: Option<Boundary>,
    pub blocked: Vec<String>,
}

impl Reflexes {
    pub fn keeps(&self, event: &Event) -> bool {
        KEPT_EVENTS.contains(&event.method.as_str())
    }

    pub fn react(&mut self, event: &Event) -> Vec<Command> {
        let params = &event.params;
        match event.method.as_str() {
            "Page.javascriptDialogOpening" => vec![Command {
                session: event.session.clone(),
                method: "Page.handleJavaScriptDialog",
                params: json!({
                    "accept": true,
                    "promptText": params.get("defaultPrompt").and_then(Value::as_str).unwrap_or(""),
                }),
            }],
            "Debugger.paused" => vec![Command {
                session: event.session.clone(),
                method: "Debugger.resume",
                params: json!({}),
            }],
            "Fetch.requestPaused" => {
                let request_id = params.get("requestId").cloned().unwrap_or(Value::Null);
                let url = params
                    .pointer("/request/url")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let frame = params.get("frameId").and_then(Value::as_str);
                let top_level = params.get("resourceType").and_then(Value::as_str)
                    == Some("Document")
                    && match (&self.main_frame, frame) {
                        (Some(main), Some(frame)) => main == frame,
                        (None, _) => true,
                        (Some(_), None) => false,
                    };
                let allowed = !top_level
                    || self
                        .boundary
                        .as_ref()
                        .is_none_or(|boundary| boundary.allows(url));
                if allowed {
                    vec![Command {
                        session: event.session.clone(),
                        method: "Fetch.continueRequest",
                        params: json!({"requestId": request_id}),
                    }]
                } else {
                    self.blocked.push(url.to_string());
                    vec![Command {
                        session: event.session.clone(),
                        method: "Fetch.failRequest",
                        params: json!({"requestId": request_id, "errorReason": "Aborted"}),
                    }]
                }
            }
            "Target.targetCreated" => {
                let info = params.get("targetInfo").cloned().unwrap_or(Value::Null);
                let opened_by_us = self.target_id.is_some()
                    && info.get("openerId").and_then(Value::as_str) == self.target_id.as_deref();
                let is_page = info.get("type").and_then(Value::as_str) == Some("page");
                match (opened_by_us && is_page, info.get("targetId")) {
                    (true, Some(target)) => vec![Command {
                        session: None,
                        method: "Target.closeTarget",
                        params: json!({"targetId": target}),
                    }],
                    _ => Vec::new(),
                }
            }
            _ => Vec::new(),
        }
    }
}
