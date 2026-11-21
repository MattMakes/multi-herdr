use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

pub mod harness;
pub mod hc_fake;
pub mod process;

#[derive(Debug)]
pub struct Call {
    pub fake: &'static str,
    pub call_id: String,
    pub argv: Vec<String>,
    pub stdin: Vec<String>,
    pub violations: Vec<String>,
    pub extra: serde_json::Map<String, Value>,
}

impl Call {
    pub fn start(fake: &'static str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        Call {
            fake,
            call_id: format!("{}-{nanos}", std::process::id()),
            argv: std::env::args().skip(1).collect(),
            stdin: Vec::new(),
            violations: Vec::new(),
            extra: serde_json::Map::new(),
        }
    }

    pub fn violate(&mut self, what: impl Into<String>) {
        self.violations.push(what.into());
    }

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
            "call_id": self.call_id,
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

pub fn scenario() -> String {
    scenarios().remove(0)
}

pub fn scenario_has(name: &str) -> bool {
    scenarios().iter().any(|s| s == name)
}

pub fn hash16(text: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

pub fn opencode_session_id(dir: &Path) -> String {
    let dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    format!("ses_{}", hash16(&dir.to_string_lossy()))
}

pub fn prime_session_id(session_dir: &str) -> String {
    format!("prime_{}", hash16(session_dir))
}

pub fn say(line: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

pub fn hang() -> ! {
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}

pub fn read_log(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

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

pub fn bin_dir() -> PathBuf {
    let exe = std::env::current_exe().expect("current exe");
    bin_dir_from(&exe)
}

fn bin_dir_from(exe: &Path) -> PathBuf {
    let start = exe.parent().expect("exe dir").to_path_buf();
    let fake = format!("fake-herdr{}", std::env::consts::EXE_SUFFIX);
    if let Some(dir) = start.ancestors().find(|dir| dir.join(&fake).is_file()) {
        return dir.to_path_buf();
    }
    let mut dir = start;
    if dir.file_name().and_then(|n| n.to_str()) == Some("deps") {
        dir.pop();
    }
    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bin_dir_finds_binaries_above_the_cargo_1_100_out_directory() {
        let root = tempfile::tempdir().unwrap();
        let profile = root.path().join("target/debug");
        let out = profile.join("build/horch-e2e/hash/out");
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(
            profile.join(format!("fake-herdr{}", std::env::consts::EXE_SUFFIX)),
            "fake",
        )
        .unwrap();

        assert_eq!(bin_dir_from(&out.join("horch-e2e-test")), profile);
    }

    #[test]
    fn bin_dir_keeps_the_older_deps_fallback_before_binaries_exist() {
        let profile = Path::new("target/debug");
        assert_eq!(bin_dir_from(&profile.join("deps/horch-e2e-test")), profile);
    }
}
