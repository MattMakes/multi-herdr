#![allow(dead_code)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use ui_prop_tester::fake::{FakeDrivers, FakeScript};
use ui_prop_tester::trace::{read_trace_file, Trace};
use ui_prop_tester::Io;

pub struct Outcome {
    pub code: i32,
    pub out: String,
    pub err: String,
}

impl Outcome {
    pub fn all(&self) -> String {
        format!("{}\n{}", self.out, self.err)
    }
}

pub fn run(drivers: &FakeDrivers, args: &[&str]) -> Outcome {
    let mut out: Vec<u8> = Vec::new();
    let mut err: Vec<u8> = Vec::new();
    let mut argv: Vec<OsString> = vec!["ui-prop-tester".into()];
    argv.extend(args.iter().map(OsString::from));
    let code = {
        let mut io = Io {
            out: &mut out,
            err: &mut err,
        };
        ui_prop_tester::run(argv, &mut io, drivers)
    };
    Outcome {
        code,
        out: String::from_utf8_lossy(&out).into_owned(),
        err: String::from_utf8_lossy(&err).into_owned(),
    }
}

pub fn fake(script: &str) -> FakeDrivers {
    FakeDrivers::new(FakeScript::from_json(script).unwrap())
}

pub fn plain() -> FakeDrivers {
    FakeDrivers::new(FakeScript::default())
}

pub struct Workspace {
    pub dir: tempfile::TempDir,
}

impl Workspace {
    pub fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    pub fn write(&self, name: &str, source: &str) -> PathBuf {
        let path = self.path(name);
        std::fs::write(&path, source).unwrap();
        path
    }

    pub fn spec(&self, body: &str) -> PathBuf {
        let source = format!(
            "import {{ always, eventually, next, now, and, or, not, implies, extract, action, constant, from, integer, float, regex, email, text, chars, oneOf, weighted, actions }} from \"ui-prop-tester\";\nimport {{ step, steps }} from \"ui-prop-tester/fake\";\nexport const screen = extract((s) => s.screen);\nexport const count = extract((s) => s.steps);\nexport const history = extract((s) => s.history);\n{body}\n"
        );
        self.write("spec.js", &source)
    }

    pub fn test(&self, drivers: &FakeDrivers, spec: &Path, extra: &[&str]) -> (Outcome, Trace) {
        let output = self.path("run.trace");
        let _ = std::fs::remove_file(&output);
        let spec = spec.to_string_lossy().into_owned();
        let output_text = output.to_string_lossy().into_owned();
        let mut args = vec![
            "terminal",
            "test",
            "--specification",
            spec.as_str(),
            "--output-path",
            output_text.as_str(),
        ];
        args.extend_from_slice(extra);
        args.push("fake-program");
        let outcome = run(drivers, &args);
        let trace = read_trace_file(&output)
            .unwrap_or_else(|error| panic!("no trace ({error}): {}", outcome.all()));
        (outcome, trace)
    }
}

pub fn names(trace: &Trace) -> Vec<String> {
    trace
        .actions()
        .iter()
        .map(|action| {
            action
                .get("name")
                .and_then(|name| name.as_str())
                .unwrap_or("")
                .to_string()
        })
        .collect()
}

pub const MACHINE: &str = r#"{
  "states": {
    "start": {"value": {"mode": "idle"}},
    "armed": {"value": {"mode": "armed"}},
    "broken": {"value": {"mode": "broken"}},
    "done": {"value": {"mode": "done"}, "ended": true}
  },
  "transitions": [
    {"from": "start", "action": "x", "to": "armed"},
    {"from": "armed", "action": "y", "to": "broken"},
    {"from": "*", "action": "quit", "to": "done"},
    {"from": "*", "action": "reset", "to": "start"}
  ]
}"#;
