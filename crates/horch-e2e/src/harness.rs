use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::bin_dir;

pub fn warm(cmd: &mut Command) -> std::io::Result<Output> {
    for _ in 0..100 {
        match cmd.output() {
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                std::thread::sleep(Duration::from_millis(20));
            }
            done => return done,
        }
    }
    cmd.output()
}

pub const FAKES: [(&str, &str); 10] = [
    ("agy", "fake-antigravity"),
    ("claude", "fake-claude"),
    ("codex", "fake-codex"),
    ("hc", "fake-hc"),
    ("herdr", "fake-herdr"),
    ("herdr-app", "fake-herdr-app"),
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
    pub env: BTreeMap<String, String>,
    removed: BTreeSet<String>,
    git: Option<PathBuf>,
}

pub const GIT_DATE: &str = "2026-09-28T12:00:00+00:00";

pub const DELIVERY_PROBE_FAULT: &str = "delivery-probe-ms:100";

fn exe(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

impl Harness {
    pub fn new(name: &str) -> Self {
        let short: String = name.chars().take(12).collect();
        let dir_name = format!("horch-e2e-{short}-{}-{:x}", std::process::id(), unique());
        #[cfg(unix)]
        let root = {
            let preferred = Path::new("/tmp").join(&dir_name);
            let _ = std::fs::remove_dir_all(&preferred);
            let preferred_writable = std::fs::create_dir_all(&preferred).is_ok();
            let fallback =
                tmpdir().join(format!("h{:x}-{:x}", std::process::id(), unique_sequence()));
            choose_harness_root(&preferred, &fallback, preferred_writable)
        };
        #[cfg(not(unix))]
        let root = std::env::temp_dir().join(dir_name);
        let _ = std::fs::remove_dir_all(&root);
        let mut h = Harness {
            home: root.join("home"),
            state: root.join("state"),
            tmp: root.join("tmp"),
            bin: root.join("bin"),
            log: root.join("fake.log"),
            project: root.join("work/alpha"),
            root,
            env: BTreeMap::new(),
            removed: BTreeSet::new(),
            git: None,
        };
        for dir in [&h.home, &h.state, &h.tmp, &h.bin, &h.project] {
            std::fs::create_dir_all(dir).expect("creating harness dirs");
        }
        for sock in [h.hc_socket(), h.app_socket()] {
            if let Some(parent) = sock.parent() {
                std::fs::create_dir_all(parent).expect("creating fake socket dir");
            }
            std::fs::write(&sock, b"").expect("creating fake socket");
        }
        write_owner(&h.root);
        static STALE: std::sync::Once = std::sync::Once::new();
        STALE.call_once(|| {
            for parent in harness_parents() {
                for line in stop_stale_harnesses(&parent) {
                    eprintln!("warning: stopped a process of an earlier test run: {line}");
                }
            }
        });
        check_built_fakes();
        warm_built_fakes();
        h.set(
            "HORCH_LOCAL_PROBE_TIMEOUT_MS",
            DEADLINE.as_millis().to_string(),
        );
        let built = bin_dir();
        for (name, fake) in FAKES {
            let from = built.join(exe(fake));
            let to = h.bin.join(exe(name));
            if link_fake(&from, &to).is_err() {
                std::fs::copy(&from, &to).expect("copying a fake");
            }
        }
        if let Some(sqlite) = find_on_real_path("sqlite3") {
            #[cfg(unix)]
            let _ = std::os::unix::fs::symlink(sqlite, h.bin.join(exe("sqlite3")));
            #[cfg(not(unix))]
            let _ = std::fs::copy(sqlite, h.bin.join(exe("sqlite3")));
        }
        h
    }

    pub fn with_git(mut self) -> Self {
        let Some(git) = find_on_real_path("git").filter(|p| p.is_absolute()) else {
            assert!(
                std::env::var_os("HORCH_REQUIRE_GIT").is_none_or(|v| v != "1"),
                "HORCH_REQUIRE_GIT=1 is set but git is not on PATH"
            );
            return self;
        };
        let config = self.root.join("gitconfig");
        std::fs::write(&config, "").expect("writing the empty git config");
        let pinned = [
            ("GIT_CONFIG_GLOBAL", config.to_string_lossy().into_owned()),
            ("GIT_CONFIG_NOSYSTEM", "1".into()),
            ("GIT_AUTHOR_NAME", "Horch Fixture".into()),
            ("GIT_AUTHOR_EMAIL", "fixture@horch.invalid".into()),
            ("GIT_COMMITTER_NAME", "Horch Fixture".into()),
            ("GIT_COMMITTER_EMAIL", "fixture@horch.invalid".into()),
            ("GIT_AUTHOR_DATE", GIT_DATE.into()),
            ("GIT_COMMITTER_DATE", GIT_DATE.into()),
            ("GIT_TERMINAL_PROMPT", "0".into()),
            ("HORCH_GIT_BIN", git.to_string_lossy().into_owned()),
        ];
        for (k, v) in pinned {
            self.env.insert(k.into(), v);
        }
        std::fs::write(self.project.join("README.md"), "# alpha\n").expect("writing a fixture");
        std::fs::write(self.project.join("notes.txt"), "one\ntwo\n").expect("writing a fixture");
        self.git = Some(git);
        for args in [
            &["init", "--quiet"][..],
            &["symbolic-ref", "HEAD", "refs/heads/main"],
            &["add", "-A"],
            &["commit", "--quiet", "--no-gpg-sign", "-m", "initial commit"],
        ] {
            let out = self.git_cmd(args).output().expect("running git");
            assert!(
                out.status.success(),
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        self.trust_project();
        self
    }

    pub fn trust_project(&self) {
        let root = self
            .project
            .canonicalize()
            .unwrap_or_else(|_| self.project.clone());
        let root = root.to_string_lossy().into_owned();
        let claude = serde_json::json!({
            "projects": { root.as_str(): { "hasTrustDialogAccepted": true } }
        });
        std::fs::write(self.home.join(".claude.json"), claude.to_string())
            .expect("writing the claude trust fixture");
        let codex = self.home.join(".codex");
        std::fs::create_dir_all(&codex).expect("creating the codex home");
        std::fs::write(
            codex.join("config.toml"),
            format!(
                "[projects.{}]\ntrust_level = \"trusted\"\n",
                serde_json::Value::from(root)
            ),
        )
        .expect("writing the codex trust fixture");
    }

    pub fn untrust_project(&self) {
        let _ = std::fs::remove_file(self.home.join(".claude.json"));
        let _ = std::fs::remove_file(self.home.join(".codex").join("config.toml"));
    }

    pub fn git_bin(&self) -> Option<&Path> {
        self.git.as_deref()
    }

    pub fn git_cmd(&self, args: &[&str]) -> Command {
        let git = self.git.as_ref().expect("call with_git() first");
        let mut cmd = Command::new(git);
        cmd.args(args);
        self.seal(&mut cmd);
        horch_marketplace::git::scrub_repo_env(&mut cmd);
        cmd
    }

    pub fn head_sha(&self) -> Option<String> {
        self.git.as_ref()?;
        let out = self.git_cmd(&["rev-parse", "HEAD"]).output().ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    pub fn allows_program(&self, path: &Path) -> bool {
        path.starts_with(&self.bin) || self.git.as_deref() == Some(path)
    }

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

    pub fn horch(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(Self::horch_bin());
        cmd.args(args);
        self.seal(&mut cmd);
        cmd
    }

    pub fn stored_ledger(&self, slug: &str) -> Result<String, String> {
        horch_core::execution::store::ExecutionStore::for_project(&self.state, slug)
            .stored_json()
            .map_err(|e| format!("{e:#}"))
    }

    pub fn run(&self, args: &[&str]) -> Output {
        self.horch(args).output().expect("running horch")
    }

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
        env.insert("HORCH_TELL_GRACE_MS".into(), "0".into());
        env.insert("HORCH_SPAWN_WAIT_MS".into(), "0".into());
        env.insert("HORCH_NO_UPDATE".into(), "1".into());
        env.insert("HORCH_FAULT".into(), DELIVERY_PROBE_FAULT.into());
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
            ("hc", "HORCH_HC_BIN"),
            ("herdr", "HORCH_HERDR_BIN"),
            ("herdr-app", "HORCH_HERDR_APP_BIN"),
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
        for key in ["SYSTEMROOT", "SystemRoot", "WINDIR", "COMSPEC"] {
            if let Ok(v) = std::env::var(key) {
                env.insert(key.into(), v);
            }
        }
        for key in &self.removed {
            env.remove(key);
        }
        for (k, v) in &self.env {
            env.insert(k.clone(), v.clone());
        }
        cmd.envs(env);
    }

    pub fn write_bin(&self, name: &str, contents: &[u8]) -> PathBuf {
        let path = self.bin.join(name);
        let _ = std::fs::remove_file(&path);
        std::fs::write(&path, contents).expect("writing a harness bin");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .expect("making a harness bin executable");
        }
        path
    }

    pub fn write_warmed(&self, name: &str, contents: &[u8]) -> PathBuf {
        let path = self.write_bin(name, contents);
        let out = warm(Command::new(&path).arg("warm"))
            .unwrap_or_else(|e| panic!("warm {}: {e}", path.display()));
        assert!(out.status.success(), "warming {} failed", path.display());
        path
    }

    pub fn set(&mut self, key: &str, value: impl Into<String>) -> &mut Self {
        self.env.insert(key.to_string(), value.into());
        self
    }

    pub fn unset(&mut self, key: &str) -> &mut Self {
        self.env.remove(key);
        self
    }

    pub fn without(&mut self, key: &str) -> &mut Self {
        self.env.remove(key);
        self.removed.insert(key.to_string());
        self
    }

    pub fn allow_updates(&mut self) -> &mut Self {
        self.without("HORCH_NO_UPDATE")
    }

    pub fn calls(&self) -> Vec<Value> {
        crate::read_log(&self.log)
    }

    pub fn calls_of(&self, fake: &str) -> Vec<Value> {
        let mut out: Vec<Value> = Vec::new();
        for call in self
            .calls()
            .into_iter()
            .filter(|c| c["fake"].as_str() == Some(fake))
        {
            let same = |c: &Value| match (c.get("call_id"), call.get("call_id")) {
                (Some(a), Some(b)) => a == b,
                _ => c["pid"] == call["pid"],
            };
            match out.iter_mut().find(|c| same(c)) {
                Some(slot) => *slot = call,
                None => out.push(call),
            }
        }
        out
    }

    pub fn violations(&self) -> Vec<String> {
        crate::violations(&self.log)
    }

    pub fn copy_in(&self, from: &Path, to: &Path) {
        copy_tree(from, to);
    }

    pub fn state_files(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        walk(&self.state, &mut out);
        out
    }

    pub fn hc_socket(&self) -> PathBuf {
        self.home
            .join(".local/state/herdr-core/default/default.sock")
    }

    pub fn app_socket(&self) -> PathBuf {
        self.home.join(".config/herdr-app/herdr-app.sock")
    }

    pub fn hc_calls(&self) -> Vec<Value> {
        crate::read_log(&crate::hc_fake::log_file_in(&self.state))
    }

    pub fn hc_state(&self) -> Value {
        crate::hc_fake::load_in(&self.state)
    }

    pub fn set_hc_terminal_status(
        &self,
        address: &str,
        status: Option<(&str, &str)>,
        status_ms: Option<u64>,
    ) {
        let _lock = crate::hc_fake::StateLock::acquire_in(&self.state);
        let mut state = crate::hc_fake::load_in(&self.state);
        let term = crate::hc_fake::find_terminal_mut(&mut state, address)
            .unwrap_or_else(|| panic!("no fake-hc terminal {address}"));
        match status {
            Some((app, st)) => {
                term["status"] = serde_json::json!({"app": app, "state": st});
                term["reported"] = serde_json::json!(true);
            }
            None => {
                term["status"] = Value::Null;
                term["reported"] = serde_json::json!(false);
            }
        }
        term["status_ms"] = match status_ms {
            Some(ms) => serde_json::json!(ms),
            None => Value::Null,
        };
        crate::hc_fake::save_in(&self.state, &state);
    }

    pub fn set_hc_terminal_screen(&self, address: &str, text: &str) {
        let _lock = crate::hc_fake::StateLock::acquire_in(&self.state);
        let mut state = crate::hc_fake::load_in(&self.state);
        let term = crate::hc_fake::find_terminal_mut(&mut state, address)
            .unwrap_or_else(|| panic!("no fake-hc terminal {address}"));
        term["test_screen"] = serde_json::json!(text);
        crate::hc_fake::save_in(&self.state, &state);
    }

    pub fn close_hc_window(&self) {
        let _ = std::fs::write(crate::hc_fake::window_closed_in(&self.state), b"1");
        let _ = std::fs::remove_file(self.app_socket());
    }

    pub fn open_hc_window(&self) {
        let _ = std::fs::remove_file(crate::hc_fake::window_closed_in(&self.state));
        if let Some(parent) = self.app_socket().parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(self.app_socket(), b"");
    }

    pub fn stop_hc_core(&self) {
        let _ = std::fs::remove_file(self.hc_socket());
        let _ = std::fs::write(crate::hc_fake::core_stopped_in(&self.state), b"1");
    }
}

#[cfg(unix)]
const UNIX_SOCKET_PATH_LIMIT: usize = 104;

#[cfg(unix)]
fn choose_harness_root(preferred: &Path, fallback: &Path, preferred_writable: bool) -> PathBuf {
    if preferred_writable {
        return preferred.to_path_buf();
    }
    // SAFETY: plain syscall without arguments; it cannot fail.
    let uid = unsafe { libc::getuid() };
    let socket = fallback
        .join("state/prime/ffffffffffff")
        .join(format!("prime-agent-{uid}/d.sock"));
    let socket_len = socket.as_os_str().as_encoded_bytes().len();
    assert!(
        socket_len < UNIX_SOCKET_PATH_LIMIT,
        "the preferred harness path {} is not writable; the TMPDIR harness path {} gives Prime socket path {} ({} bytes), which must stay below {} bytes",
        preferred.display(),
        fallback.display(),
        socket.display(),
        socket_len,
        UNIX_SOCKET_PATH_LIMIT
    );
    fallback.to_path_buf()
}

impl Drop for Harness {
    fn drop(&mut self) {
        let leaked = match reap(&self.root, &self.log) {
            Ok(leaked) => leaked,
            Err(error) if !std::thread::panicking() => panic!(
                "teardown could not list the processes of this test ({error}); a leaked process would go unseen"
            ),
            Err(error) => {
                eprintln!("teardown could not list the processes of this test ({error})");
                Vec::new()
            }
        };
        if !std::thread::panicking() {
            check_built_fakes();
            assert!(
                leaked.is_empty(),
                "the test leaked {} process(es) that outlived teardown by {:?}; \
                 teardown killed them:\n{}",
                leaked.len(),
                REAP_GRACE,
                leaked.join("\n")
            );
        }
        if std::env::var_os("HORCH_E2E_KEEP").is_none() {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}

const REAP_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

#[cfg(unix)]
fn reap(root: &Path, log: &Path) -> std::io::Result<Vec<String>> {
    use crate::process::{commands_with_environment, signal_groups, try_processes, Process};
    use horch_core::procid::is_same_group;
    let mut state = log.as_os_str().to_owned();
    state.push(".state.json");
    let state: Value = std::fs::read_to_string(&state)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Null);
    let hc_state: Value = std::fs::read_to_string(root.join("state/fake-hc.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Null);
    let mut marks = vec![format!("{}/", root.display())];
    if let Ok(canonical) = std::fs::canonicalize(root) {
        marks.push(format!("{}/", canonical.display()));
    }
    let me = std::process::id();
    // SAFETY: plain syscall without arguments.
    let own_group = u32::try_from(unsafe { libc::getpgrp() }).ok();
    let recorded: Vec<u32> = [&state, &hc_state]
        .into_iter()
        .flat_map(|s| s["pids"].as_object().into_iter().flatten())
        .filter_map(|(_, pid)| u32::try_from(pid.as_u64()?).ok())
        .collect();
    let Some(all) = process_list_or_sandbox(
        try_processes,
        REAP_GRACE,
        &recorded,
        own_group,
        signal_groups,
    )?
    else {
        return Ok(Vec::new());
    };
    let mut groups: Vec<u32> = [&state, &hc_state]
        .into_iter()
        .flat_map(|s| {
            s["pids"]
                .as_object()
                .into_iter()
                .flatten()
                .filter_map(move |(pane, pid)| {
                    let pid = u32::try_from(pid.as_u64()?).ok()?;
                    let started = s["started"][pane.as_str()].as_u64()?;
                    is_same_group(pid, started).then_some(pid)
                })
        })
        .collect();
    let names_root = |p: &Process, environments: &std::collections::HashMap<u32, String>| {
        let named = |text: &str| marks.iter().any(|m| text.contains(m.as_str()));
        p.pid != me && (named(&p.command) || environments.get(&p.pid).is_some_and(|e| named(e)))
    };
    let environments = commands_with_environment();
    groups.extend(
        all.iter()
            .filter(|p| names_root(p, &environments))
            .map(|p| p.pgid),
    );
    groups.retain(|g| *g > 1 && Some(*g) != own_group);
    groups.sort_unstable();
    groups.dedup();
    if groups.is_empty() {
        return Ok(Vec::new());
    }
    signal_groups(&groups, "TERM");
    let left = |signalled: &[u32]| -> std::io::Result<Vec<Process>> {
        let environments = commands_with_environment();
        Ok(listed(try_processes, REAP_GRACE)?
            .into_iter()
            .filter(|p| {
                p.pid != me && (signalled.contains(&p.pgid) || names_root(p, &environments))
            })
            .collect())
    };
    let until = std::time::Instant::now() + REAP_GRACE;
    let first: Vec<u32> = all.iter().map(|p| p.pid).collect();
    let mut signalled: Vec<(u32, u32)> = all.iter().map(|p| (p.pid, p.pgid)).collect();
    loop {
        let left = left(&groups)?;
        if left.is_empty() {
            return Ok(Vec::new());
        }
        let late: Vec<&Process> = left
            .iter()
            .filter(|p| !signalled.contains(&(p.pid, p.pgid)))
            .collect();
        let mut late_groups: Vec<u32> = late
            .iter()
            .map(|p| p.pgid)
            .filter(|g| *g > 1 && Some(*g) != own_group)
            .collect();
        late_groups.sort_unstable();
        late_groups.dedup();
        signal_groups(&late_groups, "TERM");
        signalled.extend(late.iter().map(|p| (p.pid, p.pgid)));
        groups.extend(late_groups);
        if std::time::Instant::now() >= until {
            let mut kill: Vec<u32> = left
                .iter()
                .map(|p| p.pgid)
                .filter(|g| *g > 1 && Some(*g) != own_group)
                .collect();
            kill.sort_unstable();
            kill.dedup();
            signal_groups(&kill, "KILL");
            return Ok(left
                .into_iter()
                .map(|p| {
                    let when = if first.contains(&p.pid) {
                        "ran at the first SIGTERM"
                    } else {
                        "started after the first SIGTERM"
                    };
                    format!("{} {} ({when})", p.pid, p.command)
                })
                .collect());
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[cfg(not(unix))]
fn reap(_root: &Path, _log: &Path) -> std::io::Result<Vec<String>> {
    Ok(Vec::new())
}

const OWNER_FILE: &str = ".horch-e2e-owner";

fn tmpdir() -> PathBuf {
    std::env::var_os("TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

fn harness_parents() -> Vec<PathBuf> {
    #[cfg(unix)]
    return vec![PathBuf::from("/tmp"), tmpdir()];
    #[cfg(not(unix))]
    return vec![std::env::temp_dir()];
}

fn write_owner(root: &Path) {
    let pid = std::process::id();
    let started = horch_core::procid::start_time(pid).unwrap_or(0);
    let _ = std::fs::write(root.join(OWNER_FILE), format!("{pid} {started}"));
}

fn stop_stale_harnesses(parent: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(parent) else {
        return Vec::new();
    };
    let mut leaked = Vec::new();
    for root in entries.flatten().map(|e| e.path()) {
        let owner = root.join(OWNER_FILE);
        let Ok(text) = std::fs::read_to_string(&owner) else {
            continue;
        };
        let mut parts = text.split_whitespace().map(str::parse::<u64>);
        let (Some(Ok(pid)), Some(Ok(started))) = (parts.next(), parts.next()) else {
            continue;
        };
        let Ok(pid) = u32::try_from(pid) else {
            continue;
        };
        let started = (started != 0).then_some(started);
        if horch_core::procid::alive(pid, started) {
            continue;
        }
        leaked.extend(reap(&root, &root.join("fake.log")).unwrap_or_default());
        let _ = std::fs::remove_file(&owner);
    }
    leaked
}

#[cfg(unix)]
fn listed<T>(
    list: impl Fn() -> std::io::Result<T>,
    grace: std::time::Duration,
) -> std::io::Result<T> {
    let until = std::time::Instant::now() + grace;
    loop {
        match list() {
            Err(e)
                if e.kind() != std::io::ErrorKind::PermissionDenied
                    && std::time::Instant::now() < until =>
            {
                std::thread::sleep(std::time::Duration::from_millis(50))
            }
            done => return done,
        }
    }
}

#[cfg(unix)]
fn process_list_or_sandbox<T>(
    list: impl Fn() -> std::io::Result<T>,
    grace: std::time::Duration,
    recorded: &[u32],
    own_group: Option<u32>,
    mut signal: impl FnMut(&[u32], &str),
) -> std::io::Result<Option<T>> {
    match listed(list, grace) {
        Ok(all) => Ok(Some(all)),
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            let mut groups: Vec<u32> = recorded
                .iter()
                .copied()
                .filter(|g| *g > 1 && Some(*g) != own_group)
                .collect();
            groups.sort_unstable();
            groups.dedup();
            signal(&groups, "TERM");
            eprintln!(
                "warning: teardown signalled recorded pane groups but skipped the process leak scan in a sandbox: {error}"
            );
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

static BUILT_SIZES: std::sync::OnceLock<Vec<(PathBuf, u64)>> = std::sync::OnceLock::new();

fn check_built_fakes() {
    let built = bin_dir();
    let size = |path: &Path| std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let sizes = BUILT_SIZES.get_or_init(|| {
        FAKES
            .iter()
            .map(|(_, fake)| {
                let path = built.join(exe(fake));
                assert!(
                    path.is_file(),
                    "{} is not built; run `cargo build --workspace --bins` first",
                    path.display()
                );
                let len = size(&path);
                (path, len)
            })
            .collect()
    });
    for (path, len) in sizes {
        let now = size(path);
        assert!(
            now == *len && now > 0,
            "built fake {} changed from {len} to {now} bytes: a test wrote \
             through a linked name in a harness bin/; use Harness::write_bin",
            path.display()
        );
    }
}

fn warm_built_fakes() {
    static WARMED: std::sync::Once = std::sync::Once::new();
    WARMED.call_once(|| {
        let built = bin_dir();
        for (_, fake) in FAKES {
            let path = built.join(exe(fake));
            let out = warm(
                Command::new(&path)
                    .arg("warm")
                    .stdin(std::process::Stdio::null())
                    .env_remove("HORCH_FAKE_LOG"),
            );
            assert!(out.is_ok(), "warming {}: {:?}", path.display(), out.err());
        }
    });
}

pub const DEADLINE: Duration = Duration::from_secs(120);

pub fn wait_for<T>(what: &str, f: impl FnMut() -> Option<T>) -> T {
    wait_for_or(what, f, String::new)
}

pub fn wait_for_or<T>(
    what: &str,
    mut f: impl FnMut() -> Option<T>,
    context: impl Fn() -> String,
) -> T {
    let until = Instant::now() + DEADLINE;
    while Instant::now() < until {
        if let Some(v) = f() {
            return v;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!(
        "timed out after {} s waiting for {what}\n{}",
        DEADLINE.as_secs(),
        context()
    );
}

pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root")
        .to_path_buf()
}

pub fn fixtures() -> PathBuf {
    repo_root().join("crates/horch-core/tests/fixtures/telemetry")
}

fn link_fake(from: &Path, to: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    return std::os::unix::fs::symlink(from, to);
    #[cfg(not(unix))]
    return std::fs::hard_link(from, to);
}

fn unique() -> u128 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    nanos ^ (unique_sequence() as u128)
}

fn unique_sequence() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    N.fetch_add(1, Ordering::Relaxed)
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

#[cfg(all(test, unix))]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use super::{
        choose_harness_root, listed, process_list_or_sandbox, stop_stale_harnesses, OWNER_FILE,
    };

    #[test]
    fn harness_root_prefers_writable_tmp_and_checks_the_tmpdir_socket_path() {
        let preferred = Path::new("/tmp/horch-e2e-test");
        let fallback = Path::new("/short/h1-1");
        assert_eq!(choose_harness_root(preferred, fallback, true), preferred);
        assert_eq!(choose_harness_root(preferred, fallback, false), fallback);

        let long = PathBuf::from(format!("/{}", "x".repeat(104)));
        let panic =
            std::panic::catch_unwind(|| choose_harness_root(preferred, &long, false)).unwrap_err();
        let message = panic
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied())
            .unwrap();
        assert!(
            message.contains(&preferred.display().to_string()),
            "{message}"
        );
        assert!(message.contains(&long.display().to_string()), "{message}");
    }

    #[test]
    fn a_permission_denied_process_scan_signals_recorded_groups_and_passes() {
        let signals = RefCell::new(Vec::new());
        let got = process_list_or_sandbox(
            || Err::<Vec<u32>, _>(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
            Duration::from_secs(5),
            &[9, 7, 9, 1, 5],
            Some(5),
            |groups, signal| {
                signals
                    .borrow_mut()
                    .push((groups.to_vec(), signal.to_string()))
            },
        )
        .unwrap();
        assert!(got.is_none(), "the sandbox skips the leak scan");
        assert_eq!(signals.into_inner(), [(vec![7, 9], "TERM".into())]);
    }

    #[test]
    fn a_list_that_fails_once_is_tried_again() {
        let calls = Cell::new(0);
        let got = listed(
            || {
                calls.set(calls.get() + 1);
                if calls.get() == 1 {
                    Err(std::io::Error::other("fork failed"))
                } else {
                    Ok(vec![7])
                }
            },
            Duration::from_secs(5),
        );
        assert_eq!(got.unwrap(), [7]);
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn a_list_that_keeps_failing_is_an_error_after_the_grace() {
        let got = listed(
            || Err::<Vec<u32>, _>(std::io::Error::other("fork failed")),
            Duration::from_millis(200),
        );
        assert_eq!(got.unwrap_err().to_string(), "fork failed");
    }

    fn courier_in(root: &Path) -> u32 {
        let state = root.join("state");
        horch_core::messaging::mail::Mail::under(&state, "w1")
            .enqueue("sonnet-1", "DONE: x")
            .unwrap();
        let mut cmd = std::process::Command::new("/bin/sh");
        cmd.env_clear()
            .env("HOME", root)
            .env("PATH", root)
            .env("HORCH_STATE_DIR", &state)
            .env("HORCH_HERDR_BIN", root.join("no-herdr"))
            .args([
                "-c",
                "\"$0\" mail courier --workspace w1 </dev/null >/dev/null 2>&1 &",
            ])
            .arg(super::Harness::horch_bin());
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
        assert!(cmd.status().unwrap().success());
        let lock = state.join("mail/w1/courier.lock");
        let pid = super::wait_for("the courier to hold its lock", || {
            let out = std::process::Command::new("lsof")
                .arg("-t")
                .arg(&lock)
                .output()
                .ok()?;
            String::from_utf8_lossy(&out.stdout).trim().parse().ok()
        });
        // SAFETY: getpgid only reads process metadata for the known live pid.
        let group = unsafe { libc::getpgid(pid as libc::pid_t) };
        assert!(group > 1, "courier {pid} has invalid process group {group}");
        std::fs::write(
            root.join("fake.log.state.json"),
            serde_json::json!({ "pids": { "courier": group } }).to_string(),
        )
        .unwrap();
        pid
    }

    #[test]
    fn a_harness_of_an_ended_test_process_has_its_processes_stopped() {
        let parent = super::tmpdir().join(format!("h-stale-{:x}", super::unique()));
        let (stale, live) = (parent.join("stale"), parent.join("live"));
        let pid = std::process::id();
        let started = horch_core::procid::start_time(pid).expect("own start time");
        let stale_pid = courier_in(&stale);
        let live_pid = courier_in(&live);
        std::fs::write(stale.join(OWNER_FILE), format!("{pid} {}", started + 1)).unwrap();
        std::fs::write(live.join(OWNER_FILE), format!("{pid} {started}")).unwrap();

        let leaked = stop_stale_harnesses(&parent);

        super::wait_for("the stale courier to stop", || {
            (!horch_core::fsx::pid_alive(stale_pid)).then_some(())
        });
        let stale_runs = horch_core::fsx::pid_alive(stale_pid);
        let live_runs = horch_core::fsx::pid_alive(live_pid);
        let owner_left = stale.join(OWNER_FILE).exists();
        // SAFETY: plain syscall; the live process is ours to stop.
        unsafe { libc::kill(live_pid as libc::pid_t, libc::SIGKILL) };
        let _ = std::fs::remove_dir_all(&parent);
        assert!(leaked.is_empty(), "the courier ends on SIGTERM: {leaked:?}");
        assert!(!stale_runs, "the stale root's process still runs");
        assert!(
            !owner_left,
            "the stale root is stopped once, so its owner file goes"
        );
        assert!(live_runs, "the live root's process was stopped");
    }
}
