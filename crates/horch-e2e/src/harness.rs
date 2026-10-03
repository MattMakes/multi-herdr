//! A sealed world for one end-to-end test.
//!
//! Each [`Harness`] gets a fresh temp directory holding a fake home, a state
//! root, a temp dir for mailboxes, and a `bin/` of fakes named like the real
//! programs (`claude`, `codex`, `herdr`, `opencode`, `prime-agent`, `pi`,
//! `ollama`). `horch` runs with a cleared environment whose PATH is that
//! `bin/` alone, and every `HORCH_*_BIN` points into it, so no real harness, no
//! real herdr and no file outside the temp dir can be reached (NFR-01).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

use crate::bin_dir;

/// The fakes, by the name of the program each stands in for.
pub const FAKES: [(&str, &str); 7] = [
    ("claude", "fake-claude"),
    ("codex", "fake-codex"),
    ("herdr", "fake-herdr"),
    ("opencode", "fake-opencode"),
    ("pi", "fake-pi"),
    ("prime-agent", "fake-prime"),
    ("ollama", "fake-ollama"),
];

pub struct Harness {
    pub root: PathBuf,
    pub home: PathBuf,
    pub state: PathBuf,
    pub tmp: PathBuf,
    pub bin: PathBuf,
    pub log: PathBuf,
    pub project: PathBuf,
    /// Extra environment for every `horch` this harness runs.
    pub env: BTreeMap<String, String>,
}

fn exe(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

impl Harness {
    pub fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "horch-e2e-{name}-{}-{}",
            std::process::id(),
            unique()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let h = Harness {
            home: root.join("home"),
            state: root.join("state"),
            tmp: root.join("tmp"),
            bin: root.join("bin"),
            log: root.join("fake.log"),
            project: root.join("work/alpha"),
            root,
            env: BTreeMap::new(),
        };
        for dir in [&h.home, &h.state, &h.tmp, &h.bin, &h.project] {
            std::fs::create_dir_all(dir).expect("creating harness dirs");
        }
        let built = bin_dir();
        for (name, fake) in FAKES {
            let from = built.join(exe(fake));
            assert!(
                from.is_file(),
                "{} is not built; run `cargo build --workspace --bins` first",
                from.display()
            );
            std::fs::copy(&from, h.bin.join(exe(name))).expect("copying a fake");
        }
        // The one real program the tests use, on a fixture database only.
        if let Some(sqlite) = find_on_real_path("sqlite3") {
            let _ = std::fs::copy(sqlite, h.bin.join(exe("sqlite3")));
        }
        h
    }

    /// The `horch` binary under test, built next to the fakes.
    pub fn horch_bin() -> PathBuf {
        let path = bin_dir().join(exe("horch"));
        assert!(
            path.is_file(),
            "{} is not built; run `cargo build --workspace --bins` first",
            path.display()
        );
        path
    }

    pub fn has_sqlite3(&self) -> bool {
        self.bin.join(exe("sqlite3")).is_file()
    }

    /// A `horch` command in the sealed environment.
    pub fn horch(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(Self::horch_bin());
        cmd.args(args);
        self.seal(&mut cmd);
        cmd
    }

    /// Run `horch`, returning its output.
    pub fn run(&self, args: &[&str]) -> Output {
        self.horch(args).output().expect("running horch")
    }

    /// Apply the sealed environment to any command.
    pub fn seal(&self, cmd: &mut Command) {
        cmd.env_clear();
        cmd.current_dir(&self.project);
        let tmp = self.tmp.to_string_lossy().into_owned();
        let mut env: BTreeMap<String, String> = BTreeMap::new();
        env.insert("PATH".into(), self.bin.to_string_lossy().into_owned());
        env.insert("HOME".into(), self.home.to_string_lossy().into_owned());
        env.insert(
            "USERPROFILE".into(),
            self.home.to_string_lossy().into_owned(),
        );
        for key in ["TMPDIR", "TMP", "TEMP"] {
            env.insert(key.into(), tmp.clone());
        }
        env.insert(
            "HORCH_STATE_DIR".into(),
            self.state.to_string_lossy().into_owned(),
        );
        env.insert(
            "HORCH_PROJECT_DIR".into(),
            self.project.to_string_lossy().into_owned(),
        );
        env.insert(
            "HORCH_FAKE_LOG".into(),
            self.log.to_string_lossy().into_owned(),
        );
        env.insert(
            "CODEX_HOME".into(),
            self.home.join(".codex").to_string_lossy().into_owned(),
        );
        env.insert(
            "HORCH_TEAMMATES_DIR".into(),
            repo_root().join("teammates").to_string_lossy().into_owned(),
        );
        for (name, key) in [
            ("claude", "HORCH_CLAUDE_BIN"),
            ("codex", "HORCH_CODEX_BIN"),
            ("herdr", "HORCH_HERDR_BIN"),
            ("opencode", "HORCH_OPENCODE_BIN"),
            ("pi", "HORCH_PI_BIN"),
            ("prime-agent", "HORCH_PRIME_BIN"),
            ("ollama", "HORCH_OLLAMA_BIN"),
            ("sqlite3", "HORCH_SQLITE3_BIN"),
        ] {
            env.insert(
                key.into(),
                self.bin.join(exe(name)).to_string_lossy().into_owned(),
            );
        }
        // Windows needs these to start any process at all.
        for key in ["SYSTEMROOT", "SystemRoot", "WINDIR", "COMSPEC"] {
            if let Ok(v) = std::env::var(key) {
                env.insert(key.into(), v);
            }
        }
        for (k, v) in &self.env {
            env.insert(k.clone(), v.clone());
        }
        cmd.envs(env);
    }

    pub fn set(&mut self, key: &str, value: impl Into<String>) -> &mut Self {
        self.env.insert(key.to_string(), value.into());
        self
    }

    pub fn unset(&mut self, key: &str) -> &mut Self {
        self.env.remove(key);
        self
    }

    /// Every call the fakes recorded so far.
    pub fn calls(&self) -> Vec<Value> {
        crate::read_log(&self.log)
    }

    /// Calls of one fake (`"herdr"`, `"claude"`, ...), one per process: a
    /// fake that logs as it goes writes several lines, and the last is whole.
    pub fn calls_of(&self, fake: &str) -> Vec<Value> {
        let mut out: Vec<Value> = Vec::new();
        for call in self
            .calls()
            .into_iter()
            .filter(|c| c["fake"].as_str() == Some(fake))
        {
            match out.iter_mut().find(|c| c["pid"] == call["pid"]) {
                Some(slot) => *slot = call,
                None => out.push(call),
            }
        }
        out
    }

    pub fn violations(&self) -> Vec<String> {
        crate::violations(&self.log)
    }

    /// Copy a fixture file or directory tree into the harness.
    pub fn copy_in(&self, from: &Path, to: &Path) {
        copy_tree(from, to);
    }

    /// Every file under the harness state dir, for the TEL-11 sentinel scan.
    pub fn state_files(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        walk(&self.state, &mut out);
        out
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        if std::env::var_os("HORCH_E2E_KEEP").is_none() {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}

/// The repository root, from this crate's manifest dir.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root")
        .to_path_buf()
}

/// The telemetry fixture corpus.
pub fn fixtures() -> PathBuf {
    repo_root().join("crates/horch-core/tests/fixtures/telemetry")
}

fn unique() -> u128 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    nanos ^ (N.fetch_add(1, Ordering::Relaxed) as u128)
}

fn find_on_real_path(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|d| d.join(exe(name)))
        .find(|p| p.is_file())
}

pub fn copy_tree(from: &Path, to: &Path) {
    if from.is_dir() {
        std::fs::create_dir_all(to).expect("mkdir");
        for entry in std::fs::read_dir(from).expect("read_dir").flatten() {
            copy_tree(&entry.path(), &to.join(entry.file_name()));
        }
    } else {
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).expect("mkdir");
        }
        std::fs::copy(from, to).unwrap_or_else(|e| panic!("copy {}: {e}", from.display()));
    }
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else {
            out.push(path);
        }
    }
}
