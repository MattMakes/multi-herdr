//! Shared pieces of the fake harness binaries, and the harness the end-to-end
//! tests drive `horch` through.
//!
//! Every fake appends one JSON line per call to `$HORCH_FAKE_LOG`:
//!
//! ```json
//! {"fake":"claude","argv":[...],"env_keys":[...],"stdin":[...],"violations":[...]}
//! ```
//!
//! A test reads that log to prove what horch asked of each program, and that
//! it never asked for anything forbidden. The behavior of a fake is chosen by
//! `$HORCH_FAKE_SCENARIO`, one name or a comma-separated list.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

pub mod harness;

/// One call of a fake, built up while it runs and written when it finishes.
#[derive(Debug)]
pub struct Call {
    pub fake: &'static str,
    pub argv: Vec<String>,
    pub stdin: Vec<String>,
    pub violations: Vec<String>,
    pub extra: serde_json::Map<String, Value>,
}

impl Call {
    /// Start recording a call of `fake`.
    pub fn start(fake: &'static str) -> Self {
        Call {
            fake,
            argv: std::env::args().skip(1).collect(),
            stdin: Vec::new(),
            violations: Vec::new(),
            extra: serde_json::Map::new(),
        }
    }

    pub fn violate(&mut self, what: impl Into<String>) {
        self.violations.push(what.into());
    }

    /// Append this call to `$HORCH_FAKE_LOG`. Written eagerly by the fakes
    /// that may be killed (the probe fakes), so nothing is lost.
    pub fn flush(&self) {
        let Some(path) = std::env::var_os("HORCH_FAKE_LOG") else {
            return;
        };
        let mut env_keys: Vec<String> = std::env::vars_os()
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect();
        env_keys.sort();
        let mut line = json!({
            "fake": self.fake,
            "pid": std::process::id(),
            "argv": self.argv,
            "env_keys": env_keys,
            "stdin": self.stdin,
            "violations": self.violations,
        });
        if let Value::Object(map) = &mut line {
            for (k, v) in &self.extra {
                map.insert(k.clone(), v.clone());
            }
        }
        let text = format!("{line}\n");
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = f.write_all(text.as_bytes());
        }
    }
}

/// Every scenario in `$HORCH_FAKE_SCENARIO`. The variable holds one name or a
/// comma-separated list (`exec,fail_split`). Empty means `["default"]`.
pub fn scenarios() -> Vec<String> {
    let list: Vec<String> = std::env::var("HORCH_FAKE_SCENARIO")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if list.is_empty() {
        vec!["default".into()]
    } else {
        list
    }
}

/// The first scenario in `$HORCH_FAKE_SCENARIO`, or `default`. With a single
/// name this is that name, as before. Use [`scenario_has`] to test one name
/// in a list.
pub fn scenario() -> String {
    scenarios().remove(0)
}

/// True when `$HORCH_FAKE_SCENARIO` lists `name`.
pub fn scenario_has(name: &str) -> bool {
    scenarios().iter().any(|s| s == name)
}

/// Print one line to stdout and flush it: the probes read line by line.
pub fn say(line: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

/// Block until killed. The `hang` scenarios prove horch's timeouts.
pub fn hang() -> ! {
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}

/// Every line of a fake log, parsed.
pub fn read_log(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// Every violation any fake recorded.
pub fn violations(path: &Path) -> Vec<String> {
    read_log(path)
        .iter()
        .flat_map(|call| {
            let fake = call["fake"].as_str().unwrap_or("?").to_string();
            call["violations"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(move |v| format!("{fake}: {}", v.as_str().unwrap_or("?")))
        })
        .collect()
}

/// The directory the fake binaries were built into (next to `horch`).
pub fn bin_dir() -> PathBuf {
    let exe = std::env::current_exe().expect("current exe");
    // target/<profile>/deps/<test>-<hash> -> target/<profile>
    let mut dir = exe.parent().expect("exe dir").to_path_buf();
    if dir.file_name().and_then(|n| n.to_str()) == Some("deps") {
        dir.pop();
    }
    dir
}
