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

/// 16 lowercase hex digits of the FNV-1a 64-bit hash of `text`. Simple and
/// stable, so a test can predict the ids the fakes mint.
pub fn hash16(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// The session id `fake-opencode` reports for a worker running in `dir`:
/// `ses_` and `hash16` of the canonical directory path. Canonical, because the
/// fake sees the path the operating system reports (`/private/var` on macOS).
pub fn opencode_session_id(dir: &Path) -> String {
    let dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    format!("ses_{}", hash16(&dir.to_string_lossy()))
}

/// The session id `fake-prime` mints for the `--session-dir` it is given:
/// `prime_` and `hash16` of the path text exactly as passed. The session file
/// is `<id>.jsonl` in that directory.
pub fn prime_session_id(session_dir: &str) -> String {
    format!("prime_{}", hash16(session_dir))
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

/// The `inspect_skills` scenario's report, written to
/// `$HORCH_FAKE_LOG.skills.json` (the last launch wins): what one launch
/// exposed. `dirs` are the skill directories the fake was pointed at; each
/// is one skill (it holds `SKILL.md`) or a directory of skills. `skills` is
/// every skill id found there, sorted. `extra` adds the fake's flags and env.
/// Written while the agent runs, because horch removes the bundle after it.
pub fn write_skills_report(fake: &str, dirs: &[PathBuf], extra: Value) {
    let Some(log) = std::env::var_os("HORCH_FAKE_LOG") else {
        return;
    };
    let mut skills = Vec::new();
    for dir in dirs {
        if dir.join("SKILL.md").is_file() {
            skills.extend(dir.file_name().map(|n| n.to_string_lossy().into_owned()));
            continue;
        }
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            if entry.path().join("SKILL.md").is_file() {
                skills.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
    }
    skills.sort();
    skills.dedup();
    let mut report = json!({
        "fake": fake,
        "dirs": dirs.iter().map(|d| d.to_string_lossy()).collect::<Vec<_>>(),
        "skills": skills,
    });
    if let (Value::Object(map), Value::Object(more)) = (&mut report, extra) {
        map.extend(more);
    }
    let mut path = log;
    path.push(".skills.json");
    let _ = std::fs::write(path, report.to_string());
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

/// The competition-candidate behavior for this launch, from
/// `$HORCH_FAKE_LOG.candidates.json`, when the file names this process's
/// working directory or its last component (the candidate label, because a
/// candidate runs in `<worktree root>/<label>`):
///
/// ```json
/// {"A": {"write": {"greeting.txt": "hi\n"}, "commit": true, "exit": "done",
///        "usage": {"model": "claude-opus-5-5", "input": 1000, "output": 500}}}
/// ```
///
/// `exit` is one of `done` (run `horch done`, as a real agent would),
/// `crash` (exit 3), `exit0` (exit 0 without `horch done`), `hang` (block
/// until killed) and `vanish` (close the own pane through herdr).
pub fn candidate_spec() -> Option<(String, Value)> {
    let log = std::env::var_os("HORCH_FAKE_LOG")?;
    let mut path = log;
    path.push(".candidates.json");
    let all: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let cwd = std::env::current_dir().ok()?;
    let canonical = std::fs::canonicalize(&cwd).unwrap_or_else(|_| cwd.clone());
    let label = cwd.file_name()?.to_string_lossy().into_owned();
    for key in [
        cwd.to_string_lossy().into_owned(),
        canonical.to_string_lossy().into_owned(),
        label.clone(),
    ] {
        if let Some(spec) = all.get(&key) {
            return Some((label, spec.clone()));
        }
    }
    None
}

/// Act out a candidate: write its files, commit them when asked, record the
/// call, then end the way `spec.exit` says. `usage` writes the harness's
/// transcript for `spec.usage`, when the fake has one to write. Never
/// returns.
pub fn run_candidate(call: &mut Call, label: &str, spec: &Value, usage: impl Fn(&Value)) -> ! {
    if std::env::var_os("ANTHROPIC_API_KEY").is_some() {
        call.violate("ANTHROPIC_API_KEY present in a candidate's environment");
    }
    call.extra.insert("candidate".into(), json!(label));
    let cwd = std::env::current_dir().unwrap_or_default();
    call.extra
        .insert("cwd".into(), json!(cwd.to_string_lossy()));
    if let Some(files) = spec["write"].as_object() {
        for (path, content) in files {
            let file = cwd.join(path);
            if let Some(dir) = file.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&file, content.as_str().unwrap_or_default());
        }
    }
    if !spec["usage"].is_null() {
        usage(&spec["usage"]);
    }
    if spec["commit"].as_bool() == Some(true) {
        if let Some(git) = std::env::var_os("HORCH_GIT_BIN") {
            for args in [
                &["add", "-A"][..],
                &["commit", "--quiet", "--no-gpg-sign", "-m", "candidate work"],
            ] {
                let _ = std::process::Command::new(&git).args(args).status();
            }
        }
    }
    let exit = spec["exit"].as_str().unwrap_or("done").to_string();
    call.extra.insert("exit".into(), json!(exit));
    call.flush();
    match exit.as_str() {
        "crash" => std::process::exit(3),
        "exit0" => std::process::exit(0),
        "hang" => hang(),
        "vanish" => {
            let herdr = std::env::var_os("HORCH_HERDR_BIN").unwrap_or_else(|| "herdr".into());
            let pane = std::env::var("HERDR_PANE_ID").unwrap_or_default();
            let _ = std::process::Command::new(herdr)
                .args(["pane", "close", &pane])
                .status();
            hang()
        }
        _ => {
            // `horch done` closes this pane, which ends this process.
            let ok = std::process::Command::new("horch")
                .args(["done", &format!("candidate {label} finished")])
                .status()
                .is_ok_and(|s| s.success());
            if !ok {
                call.violate("horch done failed");
                call.flush();
                std::process::exit(1);
            }
            hang()
        }
    }
}
