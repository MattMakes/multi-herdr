use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::driver::{Action, DriverKind};

pub const MAGIC: &[u8; 4] = b"UIPT";
pub const FORMAT_VERSION: u32 = 1;

const RECORD_HEADER: u8 = 1;
const RECORD_STATE: u8 = 2;
const RECORD_SCREENSHOT: u8 = 3;
const RECORD_SUMMARY: u8 = 4;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpecificationRecord {
    pub path: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraceHeader {
    pub tool_version: String,
    pub driver: DriverKind,
    pub seed: u64,
    pub target: Value,
    pub specification: Option<SpecificationRecord>,
    pub started_unix_ms: u64,
    pub coverage_symbols: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Violation {
    pub property: String,
    pub state_index: u64,
    pub path: String,
    pub message: Option<String>,
    pub cells: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraceState {
    pub index: u64,
    pub timestamp_ms: u64,
    pub action: Option<Action>,
    pub state: Value,
    pub cells: Value,
    pub violations: Vec<Violation>,
    #[serde(skip)]
    pub screenshot: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TraceSummary {
    pub outcome: String,
    pub states: u64,
    pub violations: Vec<Violation>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Trace {
    pub header: TraceHeader,
    pub states: Vec<TraceState>,
    pub summary: Option<TraceSummary>,
}

impl Trace {
    pub fn actions(&self) -> Vec<Action> {
        self.states
            .iter()
            .filter_map(|state| state.action.clone())
            .collect()
    }

    pub fn violated_properties(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        let from_states = self.states.iter().flat_map(|state| state.violations.iter());
        let from_summary = self
            .summary
            .iter()
            .flat_map(|summary| summary.violations.iter());
        for violation in from_states.chain(from_summary) {
            if !names.contains(&violation.property) {
                names.push(violation.property.clone());
            }
        }
        names
    }
}

pub struct TraceWriter<W: Write> {
    sink: W,
}

impl<W: Write> TraceWriter<W> {
    pub fn new(mut sink: W, header: &TraceHeader) -> io::Result<Self> {
        sink.write_all(MAGIC)?;
        sink.write_all(&FORMAT_VERSION.to_le_bytes())?;
        let mut writer = Self { sink };
        writer.record(RECORD_HEADER, &encode(header)?)?;
        Ok(writer)
    }

    pub fn state(&mut self, state: &TraceState) -> io::Result<()> {
        self.record(RECORD_STATE, &encode(state)?)?;
        if let Some(png) = &state.screenshot {
            self.record(RECORD_SCREENSHOT, png)?;
        }
        Ok(())
    }

    pub fn finish(mut self, summary: &TraceSummary) -> io::Result<W> {
        self.record(RECORD_SUMMARY, &encode(summary)?)?;
        self.sink.flush()?;
        Ok(self.sink)
    }

    fn record(&mut self, tag: u8, payload: &[u8]) -> io::Result<()> {
        self.sink.write_all(&[tag])?;
        self.sink.write_all(&(payload.len() as u64).to_le_bytes())?;
        self.sink.write_all(payload)
    }
}

fn encode<T: Serialize>(value: &T) -> io::Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn decode<T: for<'de> Deserialize<'de>>(payload: &[u8]) -> io::Result<T> {
    serde_json::from_slice(payload).map_err(|error| invalid(format!("bad trace record: {error}")))
}

fn invalid(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

pub fn write_trace<W: Write>(sink: W, trace: &Trace) -> io::Result<W> {
    let mut writer = TraceWriter::new(sink, &trace.header)?;
    for state in &trace.states {
        writer.state(state)?;
    }
    match &trace.summary {
        Some(summary) => writer.finish(summary),
        None => {
            writer.sink.flush()?;
            Ok(writer.sink)
        }
    }
}

pub fn write_trace_file(path: &Path, trace: &Trace) -> io::Result<()> {
    write_trace(BufWriter::new(File::create(path)?), trace).map(|_| ())
}

pub fn read_trace_file(path: &Path) -> io::Result<Trace> {
    read_trace(BufReader::new(File::open(path)?))
}

pub fn read_trace<R: Read>(mut source: R) -> io::Result<Trace> {
    let mut magic = [0u8; 4];
    source
        .read_exact(&mut magic)
        .map_err(|_| invalid("not a ui-prop-tester trace: the file is too short".into()))?;
    if &magic != MAGIC {
        return Err(invalid(
            "not a ui-prop-tester trace: bad magic bytes".into(),
        ));
    }
    let mut version = [0u8; 4];
    source.read_exact(&mut version)?;
    let version = u32::from_le_bytes(version);
    if version != FORMAT_VERSION {
        return Err(invalid(format!(
            "trace format version {version} is not supported; this build reads version {FORMAT_VERSION}"
        )));
    }
    let mut header: Option<TraceHeader> = None;
    let mut states: Vec<TraceState> = Vec::new();
    let mut summary = None;
    loop {
        let mut tag = [0u8; 1];
        match source.read_exact(&mut tag) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(error) => return Err(error),
        }
        let mut length = [0u8; 8];
        source.read_exact(&mut length)?;
        let length = usize::try_from(u64::from_le_bytes(length))
            .map_err(|_| invalid("trace record is too large".into()))?;
        let mut payload = vec![0u8; length];
        source.read_exact(&mut payload)?;
        match tag[0] {
            RECORD_HEADER => header = Some(decode(&payload)?),
            RECORD_STATE => states.push(decode(&payload)?),
            RECORD_SCREENSHOT => match states.last_mut() {
                Some(state) => state.screenshot = Some(payload),
                None => return Err(invalid("screenshot record before any state".into())),
            },
            RECORD_SUMMARY => summary = Some(decode(&payload)?),
            other => return Err(invalid(format!("unknown trace record tag {other}"))),
        }
    }
    let header = header.ok_or_else(|| invalid("trace has no header record".into()))?;
    Ok(Trace {
        header,
        states,
        summary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> Trace {
        let violation = Violation {
            property: "noErrors".into(),
            state_index: 1,
            path: "always > now".into(),
            message: None,
            cells: json!({"errors": 1}),
        };
        Trace {
            header: TraceHeader {
                tool_version: "0.0.0".into(),
                driver: DriverKind::Browser,
                seed: 99,
                target: json!({"origin": "http://localhost:1/"}),
                specification: Some(SpecificationRecord {
                    path: "spec.js".into(),
                    source: "export const a = 1;".into(),
                }),
                started_unix_ms: 5,
                coverage_symbols: None,
            },
            states: vec![
                TraceState {
                    index: 0,
                    timestamp_ms: 0,
                    action: None,
                    state: json!({"url": "a"}),
                    cells: json!({}),
                    violations: vec![],
                    screenshot: Some(vec![1, 2, 3]),
                },
                TraceState {
                    index: 1,
                    timestamp_ms: 12,
                    action: Some(json!({"kind": "click"})),
                    state: json!({"url": "b"}),
                    cells: json!({"errors": 1}),
                    violations: vec![violation.clone()],
                    screenshot: None,
                },
            ],
            summary: Some(TraceSummary {
                outcome: "violation".into(),
                states: 2,
                violations: vec![violation],
                message: None,
            }),
        }
    }

    #[test]
    fn a_trace_round_trips_with_screenshots() {
        let trace = sample();
        let bytes = write_trace(Vec::new(), &trace).unwrap();
        assert_eq!(read_trace(bytes.as_slice()).unwrap(), trace);
    }

    #[test]
    fn a_trace_without_a_summary_still_reads() {
        let mut trace = sample();
        trace.summary = None;
        let bytes = write_trace(Vec::new(), &trace).unwrap();
        assert_eq!(read_trace(bytes.as_slice()).unwrap(), trace);
    }

    #[test]
    fn foreign_bytes_and_other_versions_are_refused() {
        assert!(read_trace(&b"nope"[..]).is_err());
        let mut bytes = write_trace(Vec::new(), &sample()).unwrap();
        bytes[4] = 9;
        let error = read_trace(bytes.as_slice()).unwrap_err().to_string();
        assert!(error.contains("version 9"), "{error}");
    }

    #[test]
    fn actions_and_violated_properties_are_listed_in_order() {
        let trace = sample();
        assert_eq!(trace.actions(), vec![json!({"kind": "click"})]);
        assert_eq!(trace.violated_properties(), vec!["noErrors".to_string()]);
    }
}
