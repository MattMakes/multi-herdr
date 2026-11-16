use std::time::{Duration, Instant};

use nix::errno::Errno;
use serde_json::{json, Value};

use super::emulator::Emulator;
use super::pty::{ExitReport, Pty, ReadOutcome};
use crate::driver::{Action, Driver, DriverError, DriverKind, Observation, TerminalConfig};

const DRAIN_CAP: Duration = Duration::from_millis(1000);
const STALL_SILENCE: Duration = Duration::from_secs(5);
const WRITE_GRACE: Duration = Duration::from_millis(100);
const EXIT_WAIT: Duration = Duration::from_millis(200);
const READ_CHUNK: usize = 65536;
const READ_BURST: usize = 4 * READ_CHUNK;

pub struct TerminalDriver {
    config: TerminalConfig,
    emulator: Emulator,
    pty: Option<Pty>,
    started: Option<Instant>,
    output_closed: bool,
    drained_after_exit: bool,
    dropped_input: Option<Instant>,
    last_action: Option<Value>,
    buffer: Vec<u8>,
}

fn failed(message: impl Into<String>) -> DriverError {
    DriverError::Failed(message.into())
}

fn field_u64(action: &Action, name: &str) -> Result<u64, DriverError> {
    action.get(name).and_then(Value::as_u64).ok_or_else(|| {
        failed(format!(
            "terminal action needs a nonnegative integer {name:?}"
        ))
    })
}

fn dimension(action: &Action, name: &str) -> Result<u16, DriverError> {
    let value = field_u64(action, name)?;
    if value == 0 || value > u16::MAX as u64 {
        return Err(failed(format!(
            "terminal resize {name} must be between 1 and {}",
            u16::MAX
        )));
    }
    Ok(value as u16)
}

fn write_payload(action: &Action) -> Result<Vec<u8>, DriverError> {
    match (action.get("text"), action.get("bytes")) {
        (Some(Value::String(text)), None) => Ok(text.as_bytes().to_vec()),
        (None, Some(Value::Array(values))) => values
            .iter()
            .map(|value| {
                value
                    .as_u64()
                    .filter(|&byte| byte <= 255)
                    .map(|byte| byte as u8)
                    .ok_or_else(|| failed("terminal write bytes must be integers from 0 to 255"))
            })
            .collect(),
        _ => Err(failed(
            "terminal write needs a string \"text\" or an array \"bytes\", not both",
        )),
    }
}

fn exit_json(exit: Option<ExitReport>) -> Value {
    match exit {
        None => Value::Null,
        Some(report) => json!({ "code": report.code, "signal": report.signal }),
    }
}

impl TerminalDriver {
    pub fn new(config: &TerminalConfig) -> Result<TerminalDriver, DriverError> {
        if config.program.is_empty() {
            return Err(DriverError::Setup("the program to run is empty".into()));
        }
        if config.columns == 0 || config.rows == 0 {
            return Err(DriverError::Setup(
                "the terminal needs at least one column and one row".into(),
            ));
        }
        Ok(TerminalDriver {
            config: config.clone(),
            emulator: Emulator::new(config.columns, config.rows, config.scrollback_lines_max),
            pty: None,
            started: None,
            output_closed: false,
            drained_after_exit: false,
            dropped_input: None,
            last_action: None,
            buffer: vec![0; READ_CHUNK],
        })
    }

    fn pty(&self) -> Result<&Pty, DriverError> {
        self.pty
            .as_ref()
            .ok_or_else(|| failed("the terminal driver has not started"))
    }

    fn exit(&mut self) -> Option<ExitReport> {
        self.pty.as_mut().and_then(Pty::exit)
    }

    fn ended(&mut self) -> bool {
        self.exit().is_some() && (self.output_closed || self.drained_after_exit)
    }

    fn read_output(&mut self) -> Result<bool, DriverError> {
        let mut total = 0;
        let mut received = false;
        while total < READ_BURST {
            let pty = self
                .pty
                .as_ref()
                .ok_or_else(|| failed("the terminal driver has not started"))?;
            let outcome = pty.read(&mut self.buffer)?;
            match outcome {
                ReadOutcome::Data(count) => {
                    self.emulator.feed(&self.buffer[..count]);
                    total += count;
                    received = true;
                }
                ReadOutcome::Empty => break,
                ReadOutcome::Closed => {
                    self.output_closed = true;
                    break;
                }
            }
        }
        if received {
            self.dropped_input = None;
        }
        self.send_responses()?;
        Ok(received)
    }

    fn send_responses(&mut self) -> Result<(), DriverError> {
        let responses = self.emulator.take_responses();
        if responses.is_empty() || self.output_closed {
            return Ok(());
        }
        let pty = self.pty()?;
        let mut offset = 0;
        while offset < responses.len() {
            match pty.write(&responses[offset..]) {
                Ok(Some(count)) => offset += count,
                Ok(None) | Err(Errno::EIO) => break,
                Err(error) => {
                    return Err(failed(format!(
                        "cannot answer the program's terminal query: {error}"
                    )))
                }
            }
        }
        Ok(())
    }

    fn deliver(&mut self, bytes: &[u8]) -> Result<(), DriverError> {
        let mut offset = 0;
        let grace_end = Instant::now() + WRITE_GRACE;
        while offset < bytes.len() && !self.output_closed {
            let written = self.pty()?.write(&bytes[offset..]);
            match written {
                Ok(Some(count)) => offset += count,
                Ok(None) => {
                    let now = Instant::now();
                    if now >= grace_end {
                        break;
                    }
                    if self.pty()?.wait_readable(true, grace_end - now)? {
                        self.read_output()?;
                    }
                }
                Err(Errno::EIO) => {
                    self.output_closed = true;
                    return Ok(());
                }
                Err(error) => {
                    return Err(failed(format!(
                        "cannot write to the pseudo-terminal: {error}"
                    )))
                }
            }
        }
        if offset < bytes.len() && !self.output_closed && self.exit().is_none() {
            self.dropped_input = Some(Instant::now());
        }
        Ok(())
    }

    fn wait_for_exit_after_close(&mut self) {
        let deadline = Instant::now() + EXIT_WAIT;
        while self.exit().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn settle(&mut self) -> Result<(), DriverError> {
        let quiet = Duration::from_millis(self.config.quiescence_timeout_ms);
        let cap = Instant::now() + DRAIN_CAP;
        loop {
            self.send_responses()?;
            if self.output_closed {
                self.wait_for_exit_after_close();
                break;
            }
            let now = Instant::now();
            let wait = match self.dropped_input {
                Some(dropped) => {
                    (dropped + STALL_SILENCE).saturating_duration_since(now)
                        + Duration::from_millis(1)
                }
                None => {
                    if now >= cap {
                        break;
                    }
                    quiet.min(cap - now)
                }
            };
            let exited_before = self.exit().is_some();
            if self.pty()?.wait_readable(false, wait)? {
                self.read_output()?;
                continue;
            }
            if exited_before {
                self.drained_after_exit = true;
                break;
            }
            if let Some(dropped) = self.dropped_input {
                if self.exit().is_some() {
                    self.dropped_input = None;
                    continue;
                }
                if dropped.elapsed() > STALL_SILENCE {
                    return Err(DriverError::Stalled(format!(
                        "input to {} was dropped because it was not reading, and it printed nothing for {} seconds",
                        self.config.program,
                        STALL_SILENCE.as_secs()
                    )));
                }
                continue;
            }
            if self.exit().is_some() {
                continue;
            }
            break;
        }
        Ok(())
    }

    fn observe(&mut self) -> Observation {
        let timestamp_ms = self
            .started
            .map(|started| started.elapsed().as_millis() as u64)
            .unwrap_or(0);
        let exit = self.exit();
        let ended = self.ended();
        let mut state = self.emulator.snapshot();
        state.insert("exit".into(), exit_json(exit));
        state.insert(
            "last_action".into(),
            self.last_action.clone().unwrap_or(Value::Null),
        );
        Observation {
            timestamp_ms,
            state: Value::Object(state),
            screenshot: None,
            ended,
        }
    }

    fn perform(&mut self, action: &Action) -> Result<(), DriverError> {
        let kind = action
            .get("kind")
            .and_then(Value::as_str)
            .ok_or_else(|| failed("a terminal action needs a string \"kind\""))?;
        match kind {
            "write" => {
                let bytes = write_payload(action)?;
                self.emulator.scroll_to_bottom();
                self.deliver(&bytes)
            }
            "resize" => {
                let columns = dimension(action, "columns")?;
                let rows = dimension(action, "rows")?;
                self.emulator.resize(columns, rows);
                if self.output_closed {
                    return Ok(());
                }
                self.pty()?.resize(columns, rows)
            }
            "click" => {
                let row = field_u64(action, "row")? + 1;
                let column = field_u64(action, "column")? + 1;
                let sequence = format!("\x1b[<0;{column};{row}M\x1b[<0;{column};{row}m");
                self.deliver(sequence.as_bytes())
            }
            "scroll_to_top" => {
                self.emulator.scroll_to_top();
                Ok(())
            }
            "scroll_to_bottom" => {
                self.emulator.scroll_to_bottom();
                Ok(())
            }
            other => Err(failed(format!("unknown terminal action kind {other:?}"))),
        }
    }
}

impl Driver for TerminalDriver {
    fn kind(&self) -> DriverKind {
        DriverKind::Terminal
    }

    fn start(&mut self) -> Result<Observation, DriverError> {
        if self.pty.is_some() {
            return Err(failed("the terminal driver has already started"));
        }
        self.started = Some(Instant::now());
        self.pty = Some(Pty::spawn(
            &self.config.program,
            &self.config.args,
            self.config.columns,
            self.config.rows,
        )?);
        self.settle()?;
        Ok(self.observe())
    }

    fn apply(&mut self, action: &Action) -> Result<Observation, DriverError> {
        self.pty()?;
        self.last_action = Some(action.clone());
        self.perform(action)?;
        self.settle()?;
        Ok(self.observe())
    }

    fn shutdown(&mut self) -> Result<(), DriverError> {
        if let Some(pty) = self.pty.as_mut() {
            pty.terminate();
        }
        Ok(())
    }
}
