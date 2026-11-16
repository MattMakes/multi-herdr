use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::cdp::CdpError;
use super::BrowserDriver;
use crate::driver::{Action, DriverError, ExtractOutcome, RemoteExtractor};

const EXTRACT_TIMEOUT: Duration = Duration::from_secs(30);

struct KeyDefinition {
    key: String,
    code: String,
    virtual_code: i64,
    text: Option<String>,
}

fn key_definition(name: &str) -> Option<KeyDefinition> {
    let named = |key: &str, code: &str, virtual_code: i64, text: Option<&str>| KeyDefinition {
        key: key.to_string(),
        code: code.to_string(),
        virtual_code,
        text: text.map(str::to_string),
    };
    let definition = match name {
        "Backspace" => named("Backspace", "Backspace", 8, None),
        "Tab" => named("Tab", "Tab", 9, None),
        "Enter" => named("Enter", "Enter", 13, Some("\r")),
        "Escape" => named("Escape", "Escape", 27, None),
        "Space" | " " => named(" ", "Space", 32, Some(" ")),
        "PageUp" => named("PageUp", "PageUp", 33, None),
        "PageDown" => named("PageDown", "PageDown", 34, None),
        "End" => named("End", "End", 35, None),
        "Home" => named("Home", "Home", 36, None),
        "ArrowLeft" => named("ArrowLeft", "ArrowLeft", 37, None),
        "ArrowUp" => named("ArrowUp", "ArrowUp", 38, None),
        "ArrowRight" => named("ArrowRight", "ArrowRight", 39, None),
        "ArrowDown" => named("ArrowDown", "ArrowDown", 40, None),
        "Insert" => named("Insert", "Insert", 45, None),
        "Delete" => named("Delete", "Delete", 46, None),
        other => {
            let mut characters = other.chars();
            let character = characters.next()?;
            if characters.next().is_some() {
                if let Some(number) = other.strip_prefix('F').and_then(|n| n.parse::<i64>().ok()) {
                    if (1..=12).contains(&number) {
                        return Some(named(other, other, 111 + number, None));
                    }
                }
                return None;
            }
            let upper = character.to_ascii_uppercase();
            let (code, virtual_code) = if character.is_ascii_alphabetic() {
                (format!("Key{upper}"), upper as i64)
            } else if character.is_ascii_digit() {
                (format!("Digit{character}"), character as i64)
            } else {
                (String::new(), 0)
            };
            KeyDefinition {
                key: other.to_string(),
                code,
                virtual_code,
                text: Some(other.to_string()),
            }
        }
    };
    Some(definition)
}

fn driver_error(context: &str, error: CdpError) -> DriverError {
    match error {
        CdpError::Timeout(_) => DriverError::Stalled(format!("{context}: {error}")),
        other => DriverError::Failed(format!("{context}: {other}")),
    }
}

fn number(action: &Action, field: &str) -> Result<f64, DriverError> {
    action
        .get(field)
        .and_then(Value::as_f64)
        .ok_or_else(|| DriverError::Failed(format!("the action {action} needs a numeric {field}")))
}

fn point(action: &Action, field: &str) -> Result<(f64, f64), DriverError> {
    let value = action
        .get(field)
        .ok_or_else(|| DriverError::Failed(format!("the action {action} needs a {field}")))?;
    let x = value.get("x").and_then(Value::as_f64);
    let y = value.get("y").and_then(Value::as_f64);
    match (x, y) {
        (Some(x), Some(y)) => Ok((x, y)),
        _ => Err(DriverError::Failed(format!(
            "the {field} of {action} needs numeric x and y"
        ))),
    }
}

impl BrowserDriver {
    pub(super) fn perform(&mut self, kind: &str, action: &Action) -> Result<(), DriverError> {
        match kind {
            "click" => {
                let (x, y) = point(action, "point")?;
                self.click(x, y, 1)
            }
            "double_click" => {
                let (x, y) = point(action, "point")?;
                self.click(x, y, 2)
            }
            "type" => {
                let text = action
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or_else(|| DriverError::Failed(format!("{action} needs a text")))?
                    .to_string();
                self.type_text(&text)
            }
            "key" => {
                let name = action
                    .get("key")
                    .and_then(Value::as_str)
                    .ok_or_else(|| DriverError::Failed(format!("{action} needs a key")))?;
                let definition = key_definition(name)
                    .ok_or_else(|| DriverError::Failed(format!("unknown key '{name}'")))?;
                self.press(&definition)
            }
            "scroll" => {
                let dy = number(action, "dy")?;
                let (width, height) = self.viewport;
                self.mouse(json!({
                    "type": "mouseWheel",
                    "x": width as f64 / 2.0,
                    "y": height as f64 / 2.0,
                    "deltaX": 0,
                    "deltaY": dy,
                }))
            }
            "scroll_to_top" => self
                .evaluate(
                    "window.scrollTo({ top: 0, left: 0, behavior: \"instant\" })",
                    Duration::from_secs(10),
                )
                .map(|_| ())
                .or_else(|error| self.tolerate_navigation("scrolling to the top", error)),
            "back" => self.history_step(-1),
            "forward" => self.history_step(1),
            "reload" => {
                self.tracker.expect_navigation();
                self.page_call("Page.reload", json!({}))
                    .map(|_| ())
                    .map_err(|error| driver_error("reloading", error))
            }
            "wait" => {
                let milliseconds = number(action, "ms")?.max(0.0) as u64;
                self.idle(Duration::from_millis(milliseconds));
                Ok(())
            }
            "drag" => self.drag(action),
            "resize" => {
                let width = number(action, "width")?;
                let height = number(action, "height")?;
                if width < 1.0 || height < 1.0 {
                    return Err(DriverError::Failed(format!(
                        "{action} needs a positive width and height"
                    )));
                }
                self.viewport = (width as u32, height as u32);
                self.apply_viewport()
                    .map(|_| ())
                    .map_err(|error| driver_error("resizing the viewport", error))
            }
            "set_files" => self.set_files(action),
            "custom" => self.custom(action),
            other => Err(DriverError::Failed(format!(
                "the browser driver has no action kind '{other}'"
            ))),
        }
    }

    fn tolerate_navigation(&mut self, context: &str, error: CdpError) -> Result<(), DriverError> {
        if error.is_navigation_race() || self.tracker.ended {
            Ok(())
        } else {
            Err(driver_error(context, error))
        }
    }

    fn mouse(&mut self, params: Value) -> Result<(), DriverError> {
        match self.page_call("Input.dispatchMouseEvent", params) {
            Ok(_) => Ok(()),
            Err(error) => self.tolerate_navigation("dispatching a mouse event", error),
        }
    }

    fn click(&mut self, x: f64, y: f64, clicks: u32) -> Result<(), DriverError> {
        self.mouse(json!({"type": "mouseMoved", "x": x, "y": y, "button": "none", "buttons": 0}))?;
        for count in 1..=clicks {
            self.mouse(json!({"type": "mousePressed", "x": x, "y": y, "button": "left", "buttons": 1, "clickCount": count}))?;
            self.mouse(json!({"type": "mouseReleased", "x": x, "y": y, "button": "left", "buttons": 0, "clickCount": count}))?;
        }
        Ok(())
    }

    fn key_event(&mut self, params: Value) -> Result<(), DriverError> {
        match self.page_call("Input.dispatchKeyEvent", params) {
            Ok(_) => Ok(()),
            Err(error) => self.tolerate_navigation("dispatching a key event", error),
        }
    }

    fn press(&mut self, definition: &KeyDefinition) -> Result<(), DriverError> {
        let mut down = json!({
            "type": if definition.text.is_some() { "keyDown" } else { "rawKeyDown" },
            "key": definition.key,
            "code": definition.code,
            "windowsVirtualKeyCode": definition.virtual_code,
            "nativeVirtualKeyCode": definition.virtual_code,
        });
        if let Some(text) = &definition.text {
            down["text"] = json!(text);
            down["unmodifiedText"] = json!(text);
        }
        self.key_event(down)?;
        self.key_event(json!({
            "type": "keyUp",
            "key": definition.key,
            "code": definition.code,
            "windowsVirtualKeyCode": definition.virtual_code,
            "nativeVirtualKeyCode": definition.virtual_code,
        }))
    }

    fn type_text(&mut self, text: &str) -> Result<(), DriverError> {
        for character in text.chars() {
            let definition = match character {
                '\r' | '\n' => key_definition("Enter"),
                '\t' => key_definition("Tab"),
                control if control.is_control() => None,
                other => key_definition(&other.to_string()),
            };
            if let Some(definition) = definition {
                self.press(&definition)?;
            }
        }
        Ok(())
    }

    fn history_step(&mut self, offset: i64) -> Result<(), DriverError> {
        let Some(entry) = self.history_entry(offset)? else {
            return Ok(());
        };
        self.tracker.expect_navigation();
        match self.page_call("Page.navigateToHistoryEntry", json!({"entryId": entry})) {
            Ok(_) => Ok(()),
            Err(error) => self.tolerate_navigation("moving through history", error),
        }
    }

    pub(super) fn idle(&mut self, duration: Duration) {
        let deadline = Instant::now() + duration;
        while Instant::now() < deadline {
            if let Some(event) = self.cdp.next_event(deadline) {
                self.tracker.handle(&event);
            }
            if self.cdp.closed().is_some() {
                break;
            }
        }
    }

    fn drag(&mut self, action: &Action) -> Result<(), DriverError> {
        let (from_x, from_y) = point(action, "from")?;
        let (to_x, to_y) = point(action, "to")?;
        let steps = action
            .get("steps")
            .and_then(Value::as_u64)
            .unwrap_or(10)
            .max(1);
        let delay =
            Duration::from_millis(action.get("delay_ms").and_then(Value::as_u64).unwrap_or(0));
        let _ = self.page_call("Input.setInterceptDrags", json!({"enabled": true}));
        self.tracker.drag_data = None;
        self.mouse(
            json!({"type": "mouseMoved", "x": from_x, "y": from_y, "button": "none", "buttons": 0}),
        )?;
        self.mouse(json!({"type": "mousePressed", "x": from_x, "y": from_y, "button": "left", "buttons": 1, "clickCount": 1}))?;
        let mut entered = false;
        for step in 1..=steps {
            let fraction = step as f64 / steps as f64;
            let x = from_x + (to_x - from_x) * fraction;
            let y = from_y + (to_y - from_y) * fraction;
            match self.tracker.drag_data.clone() {
                Some(data) => {
                    let kind = if entered { "dragOver" } else { "dragEnter" };
                    entered = true;
                    let _ = self.page_call(
                        "Input.dispatchDragEvent",
                        json!({"type": kind, "x": x, "y": y, "data": data}),
                    );
                }
                None => self.mouse(
                    json!({"type": "mouseMoved", "x": x, "y": y, "button": "left", "buttons": 1}),
                )?,
            }
            if !delay.is_zero() {
                self.idle(delay);
            }
        }
        if let Some(data) = self.tracker.drag_data.take() {
            if !entered {
                let _ = self.page_call(
                    "Input.dispatchDragEvent",
                    json!({"type": "dragEnter", "x": to_x, "y": to_y, "data": data}),
                );
            }
            let _ = self.page_call(
                "Input.dispatchDragEvent",
                json!({"type": "drop", "x": to_x, "y": to_y, "data": data}),
            );
        }
        self.mouse(json!({"type": "mouseReleased", "x": to_x, "y": to_y, "button": "left", "buttons": 0, "clickCount": 1}))?;
        let _ = self.page_call("Input.setInterceptDrags", json!({"enabled": false}));
        Ok(())
    }

    fn set_files(&mut self, action: &Action) -> Result<(), DriverError> {
        let selector = action
            .get("selector")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError::Failed(format!("{action} needs a selector")))?;
        let paths = action
            .get("paths")
            .and_then(Value::as_array)
            .ok_or_else(|| DriverError::Failed(format!("{action} needs a list of paths")))?;
        let base = std::env::current_dir().unwrap_or_default();
        let mut files = Vec::new();
        for path in paths {
            let text = path.as_str().ok_or_else(|| {
                DriverError::Failed(format!("{action} has a path that is not text"))
            })?;
            let path = PathBuf::from(text);
            let absolute = if path.is_absolute() {
                path
            } else {
                base.join(path)
            };
            if !absolute.is_file() {
                return Err(DriverError::Failed(format!(
                    "set_files: {} is not a file",
                    absolute.display()
                )));
            }
            files.push(absolute.to_string_lossy().to_string());
        }
        let expression = format!(
            "document.querySelector({})",
            serde_json::to_string(selector).unwrap_or_default()
        );
        let found = self
            .page_call("Runtime.evaluate", json!({"expression": expression}))
            .map_err(|error| driver_error("finding the file input", error))?;
        if found.get("exceptionDetails").is_some() {
            return Err(DriverError::Failed(format!(
                "set_files: '{selector}' is not a valid selector"
            )));
        }
        let object = found
            .pointer("/result/objectId")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                DriverError::Failed(format!("set_files: no element matches '{selector}'"))
            })?
            .to_string();
        let params = json!({"files": files, "objectId": object});
        if self
            .page_call("DOM.setFileInputFiles", params.clone())
            .is_err()
        {
            let _ = self.page_call("DOM.enable", json!({}));
            self.page_call("DOM.setFileInputFiles", params)
                .map_err(|error| driver_error("set_files", error))?;
        }
        Ok(())
    }

    fn custom(&mut self, action: &Action) -> Result<(), DriverError> {
        let name = action
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| DriverError::Failed(format!("{action} needs a name")))?;
        let source = self.custom_actions.get(name).cloned().ok_or_else(|| {
            DriverError::Failed(format!("no custom action named '{name}' is defined"))
        })?;
        let cells = action.get("cells").cloned().unwrap_or(json!({}));
        let args = action.get("args").cloned().unwrap_or(Value::Null);
        let expression = format!(
            "(async () => {{ const handler = ({source}\n); await handler({{ document, window, cells: {cells}, args: {args} }}); return null; }})()"
        );
        let result = self.page_call_within(
            "Runtime.evaluate",
            json!({
                "expression": expression,
                "awaitPromise": true,
                "returnByValue": true,
                "userGesture": true,
            }),
            super::NAVIGATION_TIMEOUT,
        );
        match result {
            Ok(result) => match result.get("exceptionDetails") {
                Some(details) => {
                    let described = super::tracker::describe_exception(details);
                    Err(DriverError::Failed(format!(
                        "custom action '{name}' threw {}\n{}",
                        described["message"].as_str().unwrap_or_default(),
                        described["stack"].as_str().unwrap_or_default()
                    )))
                }
                None => Ok(()),
            },
            Err(error) => self.tolerate_navigation(&format!("custom action '{name}'"), error),
        }
    }

    pub(super) fn run_extractors(
        &mut self,
        extractors: &[RemoteExtractor],
    ) -> Result<Vec<ExtractOutcome>, DriverError> {
        if extractors.is_empty() {
            return Ok(Vec::new());
        }
        let all = extractors
            .iter()
            .map(|extractor| extractor.source.as_str())
            .collect::<Vec<_>>();
        for _ in 0..3 {
            match self.evaluate_extractors(&all)? {
                Some(outcomes) => return Ok(outcomes),
                None => {
                    if self.tracker.loading || self.tracker.ended {
                        self.settle()?;
                        continue;
                    }
                    let mut outcomes = Vec::new();
                    for source in &all {
                        match self.evaluate_extractors(&[source])? {
                            Some(mut single) if single.len() == 1 => {
                                outcomes.push(single.remove(0))
                            }
                            _ => outcomes.push(ExtractOutcome::Thrown {
                                message: "the extractor is not a valid function".into(),
                                stack: String::new(),
                            }),
                        }
                    }
                    return Ok(outcomes);
                }
            }
        }
        Err(DriverError::Failed(
            "the page kept navigating while extractors ran".into(),
        ))
    }

    fn evaluate_extractors(
        &mut self,
        sources: &[&str],
    ) -> Result<Option<Vec<ExtractOutcome>>, DriverError> {
        let base = self.last_state.to_string();
        let calls = sources
            .iter()
            .map(|source| format!("await run(() => ({source}\n))"))
            .collect::<Vec<_>>()
            .join(",\n");
        let expression = format!(
            "(async () => {{\nconst base = {base};\nconst run = async (make) => {{ try {{ const state = Object.assign({{}}, base, {{ document, window }}); let value = make()(state); if (value && typeof value.then === \"function\") value = await value; const text = JSON.stringify(value); return {{ ok: true, value: text === undefined ? null : JSON.parse(text) }}; }} catch (error) {{ return {{ ok: false, message: String(error && error.message !== undefined ? error.message : error), stack: String((error && error.stack) || \"\") }}; }} }};\nreturn [\n{calls}\n];\n}})()"
        );
        let result = self.page_call_within(
            "Runtime.evaluate",
            json!({"expression": expression, "awaitPromise": true, "returnByValue": true}),
            EXTRACT_TIMEOUT,
        );
        let result = match result {
            Ok(result) => result,
            Err(error) if error.is_navigation_race() => return Ok(None),
            Err(error) => return Err(driver_error("running extractors", error)),
        };
        if result.get("exceptionDetails").is_some() {
            return Ok(None);
        }
        let values = result
            .pointer("/result/value")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if values.len() != sources.len() {
            return Ok(None);
        }
        Ok(Some(
            values
                .into_iter()
                .map(|outcome| {
                    if outcome.get("ok") == Some(&Value::Bool(true)) {
                        ExtractOutcome::Value(outcome.get("value").cloned().unwrap_or(Value::Null))
                    } else {
                        ExtractOutcome::Thrown {
                            message: outcome["message"].as_str().unwrap_or_default().to_string(),
                            stack: outcome["stack"].as_str().unwrap_or_default().to_string(),
                        }
                    }
                })
                .collect(),
        ))
    }
}
