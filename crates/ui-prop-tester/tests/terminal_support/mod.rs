#![allow(dead_code)]

use std::time::{Duration, Instant};

use serde_json::{json, Value};
use ui_prop_tester::{Driver, DriverError, Observation, TerminalConfig};

pub const POLL_LIMIT: Duration = Duration::from_secs(30);

pub fn shell_config(script: &str) -> TerminalConfig {
    TerminalConfig {
        program: "/bin/sh".into(),
        args: vec!["-c".into(), script.into()],
        columns: 40,
        rows: 10,
        scrollback_lines_max: 100,
        quiescence_timeout_ms: 20,
    }
}

pub fn shell_arguments(script: &str) -> Vec<String> {
    vec!["/bin/sh".into(), "-c".into(), script.into()]
}

pub struct Session {
    pub driver: Box<dyn Driver>,
    pub last: Observation,
}

impl Session {
    pub fn start(config: &TerminalConfig) -> Session {
        let mut driver = ui_prop_tester::terminal::open(config).expect("open the terminal driver");
        let last = driver.start().expect("start the terminal driver");
        Session { driver, last }
    }

    pub fn shell(script: &str) -> Session {
        Session::start(&shell_config(script))
    }

    pub fn apply(&mut self, action: Value) -> &Observation {
        self.last = self.driver.apply(&action).expect("apply a terminal action");
        &self.last
    }

    pub fn try_apply(&mut self, action: Value) -> Result<Observation, DriverError> {
        let observation = self.driver.apply(&action)?;
        self.last = observation.clone();
        Ok(observation)
    }

    pub fn write(&mut self, text: &str) -> &Observation {
        self.apply(json!({ "kind": "write", "text": text }))
    }

    pub fn state(&self) -> &Value {
        &self.last.state
    }

    pub fn until(&mut self, mut condition: impl FnMut(&Observation) -> bool) -> &Observation {
        let began = Instant::now();
        while !condition(&self.last) {
            assert!(
                began.elapsed() < POLL_LIMIT,
                "condition not met; last state: {}",
                self.last.state
            );
            self.last = self
                .driver
                .apply(&json!({ "kind": "scroll_to_bottom" }))
                .expect("observe the terminal");
        }
        &self.last
    }

    pub fn until_line(&mut self, text: &str) -> &Observation {
        self.until(|observation| lines(&observation.state).iter().any(|line| line == text))
    }

    pub fn until_ended(&mut self) -> &Observation {
        self.until(|observation| observation.ended)
    }

    pub fn shutdown(mut self) {
        self.driver
            .shutdown()
            .expect("shut the terminal driver down");
    }
}

pub fn lines(state: &Value) -> Vec<String> {
    state["lines"]
        .as_array()
        .expect("lines array")
        .iter()
        .map(|line| line.as_str().expect("line string").to_string())
        .collect()
}

pub fn cell(state: &Value, row: u64, column: u64) -> Value {
    state["cells"]
        .as_array()
        .expect("cells array")
        .iter()
        .find(|cell| cell["row"] == row && cell["column"] == column)
        .cloned()
        .unwrap_or(Value::Null)
}

pub fn process_exists(pid: i32) -> bool {
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_ok()
}

pub fn wait_for_process_gone(pid: i32) -> bool {
    let began = Instant::now();
    while process_exists(pid) {
        if began.elapsed() > POLL_LIMIT {
            return false;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    true
}

pub fn pid_after(state: &Value, prefix: &str) -> Option<i32> {
    lines(state)
        .iter()
        .find_map(|line| line.strip_prefix(prefix)?.trim().parse().ok())
}
