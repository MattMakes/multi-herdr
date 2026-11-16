use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::driver::{
    default_match_recorded, Action, BrowserConfig, CustomAction, Driver, DriverError,
    DriverFactory, DriverKind, ExtractOutcome, Observation, RemoteExtractor, SpecSetup,
    TerminalConfig,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FakeScript {
    pub initial: String,
    pub states: BTreeMap<String, FakeState>,
    pub transitions: Vec<FakeTransition>,
    pub step_ms: u64,
    pub remote: bool,
    pub fail_start: Option<String>,
    pub fail_after_actions: Option<u64>,
    pub stall_after_actions: Option<u64>,
    pub interrupt_after_actions: Option<u64>,
}

impl Default for FakeScript {
    fn default() -> Self {
        Self {
            initial: "start".into(),
            states: BTreeMap::new(),
            transitions: Vec::new(),
            step_ms: 100,
            remote: false,
            fail_start: None,
            fail_after_actions: None,
            stall_after_actions: None,
            interrupt_after_actions: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FakeState {
    pub value: Value,
    pub ended: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FakeTransition {
    pub from: String,
    pub action: String,
    pub to: String,
    #[serde(default)]
    pub ms: Option<u64>,
}

impl FakeScript {
    pub fn from_json(text: &str) -> Result<Self, String> {
        serde_json::from_str(text).map_err(|error| format!("bad fake driver script: {error}"))
    }
}

#[derive(Default)]
pub struct FakeLog {
    pub opened: AtomicU64,
    pub shutdowns: AtomicU64,
    pub prepared: Mutex<Vec<Vec<CustomAction>>>,
    pub applied: Mutex<Vec<Vec<Action>>>,
}

pub struct FakeDrivers {
    pub script: FakeScript,
    pub log: Arc<FakeLog>,
}

impl FakeDrivers {
    pub fn new(script: FakeScript) -> Self {
        Self {
            script,
            log: Arc::new(FakeLog::default()),
        }
    }

    pub fn opened(&self) -> u64 {
        self.log.opened.load(Ordering::SeqCst)
    }

    pub fn shutdowns(&self) -> u64 {
        self.log.shutdowns.load(Ordering::SeqCst)
    }

    fn open(&self, kind: DriverKind, config: Value) -> Box<dyn Driver> {
        let run = self.log.opened.fetch_add(1, Ordering::SeqCst) + 1;
        let slot = {
            let mut applied = self.log.applied.lock().unwrap_or_else(|e| e.into_inner());
            applied.push(Vec::new());
            applied.len() - 1
        };
        Box::new(FakeDriver {
            kind,
            script: self.script.clone(),
            config,
            run,
            slot,
            log: self.log.clone(),
            screen: self.script.initial.clone(),
            history: Vec::new(),
            last_action: Value::Null,
            clock_ms: 0,
            actions: 0,
            evaluator: None,
        })
    }
}

impl DriverFactory for FakeDrivers {
    fn browser(&self, config: &BrowserConfig) -> Result<Box<dyn Driver>, DriverError> {
        let config = serde_json::to_value(config).unwrap_or(Value::Null);
        Ok(self.open(DriverKind::Browser, config))
    }

    fn terminal(&self, config: &TerminalConfig) -> Result<Box<dyn Driver>, DriverError> {
        let config = serde_json::to_value(config).unwrap_or(Value::Null);
        Ok(self.open(DriverKind::Terminal, config))
    }
}

pub struct FakeDriver {
    kind: DriverKind,
    script: FakeScript,
    config: Value,
    run: u64,
    slot: usize,
    log: Arc<FakeLog>,
    screen: String,
    history: Vec<String>,
    last_action: Value,
    clock_ms: u64,
    actions: u64,
    evaluator: Option<boa_engine::Context>,
}

impl FakeDriver {
    fn observe(&self) -> Observation {
        let described = self.script.states.get(&self.screen);
        let mut state = Map::new();
        if let Some(Value::Object(fields)) = described.map(|s| &s.value) {
            state.extend(fields.clone());
        }
        state.insert("screen".into(), json!(self.screen));
        state.insert("steps".into(), json!(self.actions));
        state.insert("history".into(), json!(self.history));
        state.insert("run".into(), json!(self.run));
        state.insert("last_action".into(), self.last_action.clone());
        state.insert("config".into(), self.config.clone());
        Observation {
            timestamp_ms: self.clock_ms,
            state: Value::Object(state),
            screenshot: None,
            ended: described.is_some_and(|s| s.ended),
        }
    }

    fn transition(&self, label: &str) -> Option<&FakeTransition> {
        self.script
            .transitions
            .iter()
            .find(|t| t.from == self.screen && t.action == label)
            .or_else(|| {
                self.script
                    .transitions
                    .iter()
                    .find(|t| t.from == "*" && t.action == label)
            })
    }

    fn evaluate_remote(&mut self, source: &str, state: &Value) -> ExtractOutcome {
        use boa_engine::{Context, Source};
        let context = self.evaluator.get_or_insert_with(Context::default);
        let program = format!(
            "JSON.stringify(({source})(JSON.parse({})) ?? null)",
            serde_json::to_string(&state.to_string()).unwrap_or_default()
        );
        match context.eval(Source::from_bytes(program.as_bytes())) {
            Ok(value) => {
                let text = value
                    .to_string(context)
                    .map(|t| t.to_std_string_escaped())
                    .unwrap_or_else(|_| "null".into());
                ExtractOutcome::Value(serde_json::from_str(&text).unwrap_or(Value::Null))
            }
            Err(error) => {
                let full = error.to_string();
                let message = full.lines().next().unwrap_or("").to_string();
                let stack = full
                    .find("\n    at ")
                    .map(|start| full[start + 1..].to_string())
                    .unwrap_or_else(|| format!("    at {source}"));
                ExtractOutcome::Thrown { message, stack }
            }
        }
    }
}

impl Driver for FakeDriver {
    fn kind(&self) -> DriverKind {
        self.kind
    }

    fn prepare(&mut self, setup: &SpecSetup) -> Result<(), DriverError> {
        self.log
            .prepared
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(setup.custom_actions.clone());
        Ok(())
    }

    fn start(&mut self) -> Result<Observation, DriverError> {
        if let Some(message) = &self.script.fail_start {
            return Err(DriverError::Setup(message.clone()));
        }
        Ok(self.observe())
    }

    fn apply(&mut self, action: &Action) -> Result<Observation, DriverError> {
        self.log.applied.lock().unwrap_or_else(|e| e.into_inner())[self.slot].push(action.clone());
        if self
            .script
            .fail_after_actions
            .is_some_and(|limit| self.actions >= limit)
        {
            return Err(DriverError::Failed(
                "the fake driver was told to fail".into(),
            ));
        }
        if self
            .script
            .stall_after_actions
            .is_some_and(|limit| self.actions >= limit)
        {
            return Err(DriverError::Stalled(
                "the fake target stopped answering".into(),
            ));
        }
        self.actions += 1;
        let label = match action.get("kind").and_then(Value::as_str) {
            Some("step") => action
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            Some(kind) => kind.to_string(),
            None => String::new(),
        };
        let mut elapsed = self.script.step_ms;
        if let Some(transition) = self.transition(&label).cloned() {
            self.screen = transition.to;
            if let Some(ms) = transition.ms {
                elapsed = ms;
            }
        }
        self.clock_ms += elapsed;
        self.history.push(label);
        self.last_action = action.clone();
        if self
            .script
            .interrupt_after_actions
            .is_some_and(|limit| self.actions >= limit)
        {
            crate::engine::raise_interrupt();
        }
        Ok(self.observe())
    }

    fn extracts_remotely(&self) -> bool {
        self.script.remote
    }

    fn extract(
        &mut self,
        extractors: &[RemoteExtractor],
    ) -> Result<Vec<ExtractOutcome>, DriverError> {
        let state = self.observe().state;
        Ok(extractors
            .iter()
            .map(|extractor| self.evaluate_remote(&extractor.source, &state))
            .collect())
    }

    fn match_recorded(&self, recorded: &Action, offered: &[Action]) -> Option<Action> {
        if recorded.get("kind").and_then(Value::as_str) == Some("step") {
            let name = recorded.get("name");
            return offered
                .iter()
                .any(|candidate| {
                    candidate.get("kind").and_then(Value::as_str) == Some("step")
                        && candidate.get("name") == name
                })
                .then(|| recorded.clone());
        }
        default_match_recorded(recorded, offered)
    }

    fn shutdown(&mut self) -> Result<(), DriverError> {
        self.log.shutdowns.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
