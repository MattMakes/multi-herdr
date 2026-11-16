mod actions;
mod boundary;
mod cdp;
mod cookies;
mod coverage;
mod fingerprint;
mod process;
mod reflex;
mod tracker;

use std::collections::HashMap;
use std::time::{Duration, Instant};

use base64::Engine as _;
use serde_json::{json, Value};
use tempfile::TempDir;

use crate::driver::{
    Action, BrowserConfig, Driver, DriverError, DriverKind, ExtractOutcome, Observation,
    RemoteExtractor, SpecSetup,
};

pub use boundary::{file_url, Boundary, MatchPattern};
pub use fingerprint::match_recorded_click;
pub use process::find_chrome;

use cdp::{Cdp, CdpError};
use coverage::Coverage;
use process::ChromeProcess;
use tracker::PageTracker;

const NAVIGATION_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const SOFT_NAVIGATION_GRACE: Duration = Duration::from_secs(2);
const FRAME_TIMEOUT: Duration = Duration::from_secs(5);
const CAPTURE_SCRIPT: &str = include_str!("capture.js");
const FRAME_SCRIPT: &str = "new Promise((resolve) => { let done = false; const finish = () => { if (!done) { done = true; resolve(true); } }; requestAnimationFrame(() => requestAnimationFrame(finish)); setTimeout(finish, 1000); })";
const DEFAULT_PERMISSIONS: &[&str] = &["localNetworkAccess", "localNetwork", "loopbackNetwork"];

pub fn open(config: &BrowserConfig) -> Result<Box<dyn Driver>, DriverError> {
    Ok(Box::new(BrowserDriver::open(config)?))
}

pub struct BrowserDriver {
    config: BrowserConfig,
    cdp: Cdp,
    process: Option<ChromeProcess>,
    session: String,
    target_id: String,
    created_target: bool,
    origin_url: String,
    boundary: Boundary,
    downloads: Option<TempDir>,
    tracker: PageTracker,
    custom_actions: HashMap<String, String>,
    viewport: (u32, u32),
    started: Instant,
    last_state: Value,
    last_action: Value,
    coverage: Option<Coverage>,
    shut_down: bool,
}

fn setup<E: std::fmt::Display>(context: &str) -> impl Fn(E) -> DriverError + '_ {
    move |error| DriverError::Setup(format!("{context}: {error}"))
}

impl BrowserDriver {
    pub fn open(config: &BrowserConfig) -> Result<BrowserDriver, DriverError> {
        let origin_url = boundary::origin_url(&config.origin).map_err(DriverError::Setup)?;
        let boundary =
            Boundary::new(&origin_url, &config.allow_urls).map_err(DriverError::Setup)?;
        if !(config.device_scale_factor > 0.0) || config.width == 0 || config.height == 0 {
            return Err(DriverError::Setup(
                "the viewport needs a positive width, height and device scale factor".into(),
            ));
        }
        let (process, websocket_url, page_hint) = match &config.remote_debugger {
            Some(endpoint) => {
                let url = process::browser_websocket_url(endpoint).map_err(DriverError::Setup)?;
                match url.split_once("/devtools/page/") {
                    Some((base, page)) => {
                        let http = base.replacen("ws://", "http://", 1);
                        let browser =
                            process::browser_websocket_url(&http).map_err(DriverError::Setup)?;
                        (None, browser, Some(page.to_string()))
                    }
                    None => (None, url, None),
                }
            }
            None => {
                let process = ChromeProcess::launch(config).map_err(DriverError::Setup)?;
                let url = process.websocket_url.clone();
                (Some(process), url, None)
            }
        };
        let cdp = Cdp::connect(&websocket_url, CONNECT_TIMEOUT).map_err(DriverError::Setup)?;
        let mut driver = BrowserDriver {
            config: config.clone(),
            cdp,
            process,
            session: String::new(),
            target_id: String::new(),
            created_target: false,
            origin_url,
            boundary,
            downloads: None,
            tracker: PageTracker::default(),
            custom_actions: HashMap::new(),
            viewport: (config.width, config.height),
            started: Instant::now(),
            last_state: Value::Null,
            last_action: Value::Null,
            coverage: None,
            shut_down: false,
        };
        driver.attach(page_hint)?;
        driver.configure()?;
        Ok(driver)
    }

    fn launched(&self) -> bool {
        self.process.is_some()
    }

    fn browser_call(&mut self, method: &str, params: Value) -> Result<Value, CdpError> {
        let result = self.cdp.call(None, method, params);
        self.pump();
        result
    }

    fn page_call(&mut self, method: &str, params: Value) -> Result<Value, CdpError> {
        let session = self.session.clone();
        let result = self.cdp.call(Some(&session), method, params);
        self.pump();
        result
    }

    fn page_call_within(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, CdpError> {
        let session = self.session.clone();
        let result = self.cdp.call_within(Some(&session), method, params, timeout);
        self.pump();
        result
    }

    fn pump(&mut self) {
        for event in self.cdp.drain_events() {
            self.tracker.handle(&event);
        }
    }

    fn attach(&mut self, page_hint: Option<String>) -> Result<(), DriverError> {
        let targets = self
            .browser_call("Target.getTargets", json!({}))
            .map_err(setup("listing browser targets"))?;
        let pages: Vec<(String, String)> = targets
            .get("targetInfos")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|info| info.get("type").and_then(Value::as_str) == Some("page"))
            .filter_map(|info| {
                Some((
                    info.get("targetId")?.as_str()?.to_string(),
                    info.get("url")?.as_str()?.to_string(),
                ))
            })
            .collect();
        let external = self.config.remote_debugger.is_some();
        let chosen = if let Some(hint) = page_hint.filter(|_| !self.config.create_target) {
            Some(hint)
        } else if external && self.config.create_target {
            None
        } else if external {
            let inside = pages
                .iter()
                .find(|(_, url)| self.boundary.allows(url))
                .or_else(|| pages.iter().find(|(_, url)| !url.starts_with("devtools://")));
            match inside {
                Some((id, _)) => Some(id.clone()),
                None => {
                    return Err(DriverError::Setup(
                        "the browser has no page to attach to; pass --create-target".into(),
                    ))
                }
            }
        } else {
            pages.first().map(|(id, _)| id.clone())
        };
        let target_id = match chosen {
            Some(id) => id,
            None => {
                let created = self
                    .browser_call("Target.createTarget", json!({"url": "about:blank"}))
                    .map_err(setup("creating a page"))?;
                self.created_target = external;
                created
                    .get("targetId")
                    .and_then(Value::as_str)
                    .ok_or_else(|| DriverError::Setup("the new page has no target id".into()))?
                    .to_string()
            }
        };
        let attached = self
            .browser_call(
                "Target.attachToTarget",
                json!({"targetId": target_id, "flatten": true}),
            )
            .map_err(setup("attaching to the page"))?;
        let session = attached
            .get("sessionId")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError::Setup("attaching gave no session".into()))?
            .to_string();
        self.session = session.clone();
        self.target_id = target_id.clone();
        self.tracker = PageTracker::new(&session, &target_id);
        self.cdp.reflexes.page_session = Some(session);
        self.cdp.reflexes.target_id = Some(target_id);
        self.cdp.reflexes.boundary = Some(self.boundary.clone());
        Ok(())
    }

    fn configure(&mut self) -> Result<(), DriverError> {
        for method in [
            "Page.enable",
            "Runtime.enable",
            "Network.enable",
            "Performance.enable",
            "Debugger.enable",
        ] {
            self.page_call(method, json!({}))
                .map_err(setup("enabling the page domains"))?;
        }
        self.page_call("Page.setLifecycleEventsEnabled", json!({"enabled": true}))
            .map_err(setup("enabling lifecycle events"))?;
        let _ = self.page_call("Page.setInterceptFileChooserDialog", json!({"enabled": true}));
        if self.config.instrument_javascript {
            self.page_call("Profiler.enable", json!({}))
                .map_err(setup("enabling the profiler"))?;
            self.page_call(
                "Profiler.startPreciseCoverage",
                json!({"callCount": true, "detailed": true}),
            )
            .map_err(setup("starting coverage"))?;
            self.coverage = Some(Coverage::default());
        }
        self.apply_viewport()
            .map_err(setup("setting the viewport"))?;
        self.grant_permissions()?;
        if let Some(policy) = self.config.virtual_time_policy.clone() {
            self.page_call("Emulation.setVirtualTimePolicy", json!({"policy": policy}))
                .map_err(setup("setting the virtual time policy"))?;
        }
        if !self.config.headers.is_empty() {
            let headers: serde_json::Map<String, Value> = self
                .config
                .headers
                .iter()
                .map(|(key, value)| (key.clone(), Value::String(value.clone())))
                .collect();
            self.page_call("Network.setExtraHTTPHeaders", json!({"headers": headers}))
                .map_err(setup("setting extra headers"))?;
        }
        for cookie in self.config.cookies.clone() {
            let params =
                cookies::cookie_params(&cookie, &self.origin_url).map_err(DriverError::Setup)?;
            let result = self
                .page_call("Network.setCookie", params)
                .map_err(setup("setting a cookie"))?;
            if result.get("success") == Some(&Value::Bool(false)) {
                return Err(DriverError::Setup(format!(
                    "the browser refused the cookie '{cookie}'"
                )));
            }
        }
        let downloads = tempfile::Builder::new()
            .prefix("ui-prop-tester-downloads-")
            .tempdir()
            .map_err(setup("creating the download directory"))?;
        let download_path = downloads.path().to_string_lossy().to_string();
        self.downloads = Some(downloads);
        self.browser_call(
            "Browser.setDownloadBehavior",
            json!({"behavior": "allow", "downloadPath": download_path, "eventsEnabled": true}),
        )
        .map_err(setup("routing downloads"))?;
        self.page_call(
            "Fetch.enable",
            json!({"patterns": [{"urlPattern": "*", "resourceType": "Document", "requestStage": "Request"}]}),
        )
        .map_err(setup("enabling the navigation boundary"))?;
        let _ = self.browser_call("Target.setDiscoverTargets", json!({"discover": true}));
        let tree = self
            .page_call("Page.getFrameTree", json!({}))
            .map_err(setup("reading the frame tree"))?;
        let main_frame = tree
            .pointer("/frameTree/frame/id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        self.tracker.main_frame = main_frame.clone();
        self.cdp.reflexes.main_frame = Some(main_frame);
        Ok(())
    }

    fn apply_viewport(&mut self) -> Result<Value, CdpError> {
        let (width, height) = self.viewport;
        self.page_call(
            "Emulation.setDeviceMetricsOverride",
            json!({
                "width": width,
                "height": height,
                "deviceScaleFactor": self.config.device_scale_factor,
                "mobile": false,
            }),
        )
    }

    fn grant_permissions(&mut self) -> Result<(), DriverError> {
        let origin = boundary::Url::parse(&self.origin_url)
            .filter(|url| url.scheme == "http" || url.scheme == "https")
            .map(|url| url.origin());
        let with_origin = |permissions: Vec<String>| {
            let mut params = json!({"permissions": permissions});
            if let Some(origin) = &origin {
                params["origin"] = json!(origin);
            }
            params
        };
        for permission in DEFAULT_PERMISSIONS {
            let _ = self.browser_call(
                "Browser.grantPermissions",
                with_origin(vec![permission.to_string()]),
            );
        }
        if !self.config.grant_permissions.is_empty() {
            let names: Vec<String> = self
                .config
                .grant_permissions
                .iter()
                .map(|name| camel_case(name.trim()))
                .collect();
            self.browser_call("Browser.grantPermissions", with_origin(names))
                .map_err(setup("granting permissions"))?;
        }
        Ok(())
    }

    fn evaluate(&mut self, expression: &str, timeout: Duration) -> Result<Value, CdpError> {
        let result = self.page_call_within(
            "Runtime.evaluate",
            json!({
                "expression": expression,
                "returnByValue": true,
                "awaitPromise": true,
                "userGesture": true,
            }),
            timeout,
        )?;
        if let Some(details) = result.get("exceptionDetails") {
            let described = tracker::describe_exception(details);
            return Err(CdpError::Protocol {
                method: "Runtime.evaluate".into(),
                message: format!(
                    "{}\n{}",
                    described["message"].as_str().unwrap_or_default(),
                    described["stack"].as_str().unwrap_or_default()
                ),
            });
        }
        Ok(result.pointer("/result/value").cloned().unwrap_or(Value::Null))
    }

    fn check_alive(&mut self) -> Result<(), DriverError> {
        if self.tracker.crashed {
            return Err(DriverError::Failed("the page crashed".into()));
        }
        if let Some(reason) = self.cdp.closed() {
            if !self.tracker.ended {
                return Err(DriverError::Failed(format!(
                    "lost the browser connection: {reason}"
                )));
            }
        }
        Ok(())
    }

    fn wait_for_navigation(&mut self) -> Result<(), DriverError> {
        let deadline = Instant::now() + NAVIGATION_TIMEOUT;
        let blocked = self.cdp.reflexes.blocked.len();
        while self.tracker.loading && !self.tracker.ended {
            self.check_alive()?;
            if let Some(since) = self.tracker.soft_since {
                if since.elapsed() >= SOFT_NAVIGATION_GRACE
                    || self.cdp.reflexes.blocked.len() > blocked
                {
                    self.tracker.finish_loading();
                    break;
                }
            }
            if Instant::now() >= deadline {
                return Err(DriverError::Stalled(format!(
                    "the page did not finish loading within {} seconds",
                    NAVIGATION_TIMEOUT.as_secs()
                )));
            }
            let slice = (Instant::now() + Duration::from_millis(100)).min(deadline);
            if let Some(event) = self.cdp.next_event(slice) {
                self.tracker.handle(&event);
            }
        }
        Ok(())
    }

    fn frame_barrier(&mut self) -> Result<bool, DriverError> {
        match self.evaluate(FRAME_SCRIPT, FRAME_TIMEOUT) {
            Ok(_) => Ok(true),
            Err(error) if error.is_navigation_race() => Ok(false),
            Err(CdpError::Timeout(_)) => Ok(true),
            Err(CdpError::Closed(reason)) => {
                if self.tracker.ended {
                    Ok(true)
                } else {
                    Err(DriverError::Failed(format!(
                        "lost the browser connection: {reason}"
                    )))
                }
            }
            Err(CdpError::Protocol { .. }) => Ok(true),
        }
    }

    fn settle(&mut self) -> Result<(), DriverError> {
        let mut rounds = 0;
        loop {
            self.pump();
            self.check_alive()?;
            if self.tracker.ended {
                return Ok(());
            }
            self.wait_for_navigation()?;
            let before = self.tracker.activity;
            let stable = self.frame_barrier()?;
            self.pump();
            rounds += 1;
            let quiet = stable && self.tracker.activity == before && !self.tracker.loading;
            if quiet || (rounds >= 10 && !self.tracker.loading) {
                return Ok(());
            }
        }
    }

    fn settle_scroll(&mut self) -> Result<(), DriverError> {
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut last = Value::Null;
        let mut unchanged = 0;
        while Instant::now() < deadline && unchanged < 3 {
            let position = match self.evaluate(
                "new Promise((resolve) => requestAnimationFrame(() => resolve([scrollX, scrollY])))",
                FRAME_TIMEOUT,
            ) {
                Ok(position) => position,
                Err(_) => return Ok(()),
            };
            if position == last {
                unchanged += 1;
            } else {
                unchanged = 0;
                last = position;
            }
        }
        Ok(())
    }

    fn capture(&mut self) -> Result<Observation, DriverError> {
        let timestamp_ms = self.started.elapsed().as_millis() as u64;
        if self.tracker.ended {
            return Ok(self.ended_observation(timestamp_ms));
        }
        let mut page = Value::Null;
        for _ in 0..10 {
            match self.evaluate(CAPTURE_SCRIPT, NAVIGATION_TIMEOUT) {
                Ok(value) => {
                    page = value;
                    break;
                }
                Err(error) if error.is_navigation_race() => self.settle()?,
                Err(CdpError::Timeout(_)) => {
                    return Err(DriverError::Stalled(
                        "the page did not answer the state capture within 30 seconds".into(),
                    ))
                }
                Err(error) => {
                    if self.tracker.ended {
                        return Ok(self.ended_observation(timestamp_ms));
                    }
                    return Err(DriverError::Failed(format!("capturing the page: {error}")));
                }
            }
        }
        if page.is_null() {
            return Err(DriverError::Stalled(
                "the page kept navigating during the state capture".into(),
            ));
        }
        let clickables: Vec<Value> = page
            .get("clickables")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|mut candidate| {
                let object = candidate.as_object_mut()?;
                if let Some(url) = object.remove("url") {
                    let url = url.as_str().unwrap_or_default();
                    let script = url.to_ascii_lowercase().starts_with("javascript:");
                    if !script && !self.boundary.allows(url) {
                        return None;
                    }
                }
                Some(candidate)
            })
            .collect();
        let history = self.history()?;
        let resources = self.resources();
        let coverage = self.coverage_delta();
        let screenshot = self.screenshot();
        self.pump();
        let (width, height) = self.viewport;
        let state = json!({
            "url": page["url"],
            "title": page["title"],
            "mime_type": page["mime_type"],
            "viewport": {
                "width": width,
                "height": height,
                "device_scale_factor": self.config.device_scale_factor,
            },
            "navigation": {
                "status": self.tracker.navigation_status,
                "kind": self.tracker.navigation_kind,
            },
            "history": history,
            "console": self.tracker.console,
            "exceptions": self.tracker.exceptions,
            "resources": resources,
            "coverage": coverage,
            "scroll": page["scroll"],
            "document": page["document"],
            "focused": page["focused"],
            "clickables": clickables,
            "downloads": self.tracker.downloads_json(),
            "last_action": self.last_action,
        });
        self.last_state = state.clone();
        Ok(Observation {
            timestamp_ms,
            state,
            screenshot,
            ended: self.tracker.ended,
        })
    }

    fn ended_observation(&self, timestamp_ms: u64) -> Observation {
        let mut state = self.last_state.clone();
        if let Some(object) = state.as_object_mut() {
            object.insert("last_action".into(), self.last_action.clone());
            object.insert("clickables".into(), json!([]));
        }
        Observation {
            timestamp_ms,
            state,
            screenshot: None,
            ended: true,
        }
    }

    fn history(&mut self) -> Result<Value, DriverError> {
        let history = self
            .page_call("Page.getNavigationHistory", json!({}))
            .map_err(|error| DriverError::Failed(format!("reading history: {error}")))?;
        let entries: Vec<Value> = history
            .get("entries")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|entry| json!({"url": entry["url"], "title": entry["title"]}))
            .collect();
        Ok(json!({
            "index": history.get("currentIndex").cloned().unwrap_or(json!(0)),
            "entries": entries,
        }))
    }

    fn history_entry(&mut self, offset: i64) -> Result<Option<i64>, DriverError> {
        let history = self
            .page_call("Page.getNavigationHistory", json!({}))
            .map_err(|error| DriverError::Failed(format!("reading history: {error}")))?;
        let index = history
            .get("currentIndex")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            + offset;
        if index < 0 {
            return Ok(None);
        }
        Ok(history
            .get("entries")
            .and_then(Value::as_array)
            .and_then(|entries| entries.get(index as usize))
            .and_then(|entry| entry.get("id"))
            .and_then(Value::as_i64))
    }

    fn resources(&mut self) -> Value {
        let metrics = self
            .page_call("Performance.getMetrics", json!({}))
            .unwrap_or(Value::Null);
        let lookup: HashMap<&str, f64> = metrics
            .get("metrics")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|metric| {
                Some((
                    metric.get("name")?.as_str()?,
                    metric.get("value")?.as_f64()?,
                ))
            })
            .collect();
        let read = |name: &str| lookup.get(name).copied().unwrap_or(0.0) as u64;
        json!({
            "js_heap_used": read("JSHeapUsedSize"),
            "js_heap_total": read("JSHeapTotalSize"),
            "dom_nodes": read("Nodes"),
            "event_listeners": read("JSEventListeners"),
            "layout_objects": read("LayoutObjects"),
        })
    }

    fn coverage_delta(&mut self) -> Value {
        if self.coverage.is_none() {
            return Value::Null;
        }
        let taken = self
            .page_call("Profiler.takePreciseCoverage", json!({}))
            .unwrap_or(Value::Null);
        let coverage = self.coverage.as_mut().expect("coverage is on");
        let added = coverage.record(&taken);
        json!({"branches_new": added, "branches_total": coverage.total()})
    }

    fn screenshot(&mut self) -> Option<Vec<u8>> {
        let shot = self
            .page_call_within(
                "Page.captureScreenshot",
                json!({"format": "png"}),
                Duration::from_secs(10),
            )
            .ok()?;
        let data = shot.get("data")?.as_str()?;
        base64::engine::general_purpose::STANDARD.decode(data).ok()
    }

    fn navigate_to_origin(&mut self) -> Result<(), DriverError> {
        let url = self.origin_url.clone();
        let result = self
            .page_call_within("Page.navigate", json!({"url": url}), NAVIGATION_TIMEOUT)
            .map_err(|error| match error {
                CdpError::Timeout(_) => DriverError::Stalled(format!("loading {url} timed out")),
                other => DriverError::Setup(format!("loading {url}: {other}")),
            })?;
        if let Some(error) = result.get("errorText").and_then(Value::as_str) {
            if !error.is_empty() && !error.contains("ERR_ABORTED") {
                return Err(DriverError::Setup(format!("loading {url}: {error}")));
            }
        }
        if result.get("loaderId").is_some() {
            self.tracker.expect_navigation();
        }
        Ok(())
    }

    fn release(&mut self) {
        if self.shut_down {
            return;
        }
        self.shut_down = true;
        if self.launched() {
            let _ = self
                .cdp
                .call_within(None, "Browser.close", json!({}), Duration::from_secs(5));
            self.cdp.close();
            if let Some(process) = self.process.as_mut() {
                process.wait_for_exit(Duration::from_secs(5));
                process.kill();
            }
            self.process = None;
        } else {
            let session = self.session.clone();
            let short = Duration::from_secs(5);
            if self.created_target {
                let _ = self.cdp.call_within(
                    None,
                    "Target.closeTarget",
                    json!({"targetId": self.target_id}),
                    short,
                );
            } else {
                for method in [
                    "Fetch.disable",
                    "Emulation.clearDeviceMetricsOverride",
                    "Debugger.disable",
                ] {
                    let _ = self.cdp.call_within(Some(&session), method, json!({}), short);
                }
                let _ = self.cdp.call_within(
                    None,
                    "Target.detachFromTarget",
                    json!({"sessionId": session}),
                    short,
                );
            }
            let _ = self.cdp.call_within(
                None,
                "Browser.setDownloadBehavior",
                json!({"behavior": "default"}),
                short,
            );
            self.cdp.close();
        }
        if let Some(downloads) = self.downloads.take() {
            let path = downloads.path().to_path_buf();
            if downloads.close().is_err() {
                let _ = std::fs::remove_dir_all(path);
            }
        }
    }
}

fn camel_case(name: &str) -> String {
    let mut result = String::new();
    let mut upper = false;
    for character in name.chars() {
        if character == '-' || character == '_' {
            upper = true;
        } else if upper {
            result.extend(character.to_uppercase());
            upper = false;
        } else {
            result.push(character);
        }
    }
    result
}

impl Driver for BrowserDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Browser
    }

    fn prepare(&mut self, setup: &SpecSetup) -> Result<(), DriverError> {
        for action in &setup.custom_actions {
            self.custom_actions
                .insert(action.name.clone(), action.source.clone());
        }
        Ok(())
    }

    fn start(&mut self) -> Result<Observation, DriverError> {
        self.started = Instant::now();
        let current = self
            .page_call("Page.getNavigationHistory", json!({}))
            .ok()
            .and_then(|history| {
                let index = history.get("currentIndex")?.as_u64()? as usize;
                Some(history.get("entries")?.get(index)?.get("url")?.as_str()?.to_string())
            })
            .unwrap_or_default();
        let attach_in_place = self.config.remote_debugger.is_some()
            && !self.created_target
            && self.boundary.allows(&current);
        if !attach_in_place {
            self.navigate_to_origin()?;
        }
        self.settle()?;
        self.capture()
    }

    fn apply(&mut self, action: &Action) -> Result<Observation, DriverError> {
        if self.tracker.ended {
            return Ok(self.ended_observation(self.started.elapsed().as_millis() as u64));
        }
        let kind = action
            .get("kind")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError::Failed(format!("the action {action} has no kind")))?
            .to_string();
        self.perform(&kind, action)?;
        self.settle()?;
        if kind == "scroll" || kind == "scroll_to_top" {
            self.settle_scroll()?;
            self.settle()?;
        }
        self.last_action = action.clone();
        self.capture()
    }

    fn extracts_remotely(&self) -> bool {
        true
    }

    fn extract(&mut self, extractors: &[RemoteExtractor]) -> Result<Vec<ExtractOutcome>, DriverError> {
        self.run_extractors(extractors)
    }

    fn match_recorded(&self, recorded: &Action, offered: &[Action]) -> Option<Action> {
        fingerprint::match_recorded_click(recorded, offered)
    }

    fn coverage_symbols(&mut self) -> Option<Value> {
        self.coverage.as_ref().map(Coverage::symbols)
    }

    fn shutdown(&mut self) -> Result<(), DriverError> {
        self.release();
        Ok(())
    }
}

impl Drop for BrowserDriver {
    fn drop(&mut self) {
        self.release();
    }
}
