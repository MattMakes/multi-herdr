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
    fleetd: std::cell::RefCell<Option<crate::fleetd::Fleetds>>,
}

pub const GIT_DATE: &str = "2026-09-28T12:00:00+00:00";

pub const DELIVERY_PROBE_FAULT: &str = "delivery-probe-ms:100";

const HEALTHY_MACHINE: &str = r#"{"os":"linux","arch":"x86_64","cpus":8,"load_average_1m":1.0,"mem_total_bytes":17179869184,"mem_available_bytes":8589934592,"disk_free_bytes":214748364800,"disk_total_bytes":536870912000,"gpu":"none","max_open_files":1048576,"max_processes":100000}"#;

fn exe(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

impl Harness {
    pub fn new(name: &str) -> Self {
        let short: String = name.chars().take(12).collect();
        let dir_name = format!("horch-e2e-{short}-{}-{:x}", std::process::id(), unique());
        #[cfg(unix)]
        let root = {
            let preferred = Path::new(ROOT_PARENT).join(&dir_name);
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
            fleetd: std::cell::RefCell::new(None),
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
        build_once();
        check_built_fakes();
        warm_built_fakes();
        h.set(
            "HORCH_LOCAL_PROBE_TIMEOUT_MS",
            DEADLINE.as_millis().to_string(),
        );
        let machine = h.root.join("healthy-machine.json");
        std::fs::write(&machine, HEALTHY_MACHINE).expect("writing the machine fixture");
        h.set("HORCH_MACHINE_FILE", machine.to_string_lossy());
        let ledger = h.root.join("ledger").join("ledger.sqlite3");
        h.set("HORCH_LEDGER_DB", ledger.to_string_lossy());
        #[cfg(unix)]
        write_no_git(&h.root);
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
        #[cfg(target_os = "linux")]
        {
            const FLEETD_SANDBOX_TOOLS: [&str; 2] = ["bwrap", "socat"];
            let host = h.host_bin();
            std::fs::create_dir_all(&host).expect("creating the host tool dir");
            for tool in FLEETD_SANDBOX_TOOLS {
                if let Some(real) = find_on_real_path(tool) {
                    let _ = std::os::unix::fs::symlink(real, host.join(tool));
                }
            }
        }
        h
    }

    #[cfg(target_os = "linux")]
    fn host_bin(&self) -> PathBuf {
        self.root.join("host-bin")
    }

    fn sealed_path(&self) -> String {
        #[cfg(target_os = "linux")]
        let dirs = vec![self.bin.clone(), self.host_bin()];
        #[cfg(not(target_os = "linux"))]
        let dirs = vec![self.bin.clone()];
        std::env::join_paths(dirs)
            .expect("the harness folders hold no PATH separator")
            .to_string_lossy()
            .into_owned()
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

    pub fn fleet_workspace(&self) -> String {
        let label = self
            .project
            .file_name()
            .expect("the project folder has a name")
            .to_string_lossy()
            .into_owned();
        let mut create = Command::new(self.bin.join("herdr"));
        self.seal(&mut create);
        let o = create
            .args(["workspace", "create", "--label", &label])
            .output()
            .expect("running the fake herdr");
        assert!(
            o.status.success(),
            "herdr workspace create --label {label}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        let reply: serde_json::Value =
            serde_json::from_slice(&o.stdout).expect("the reply of workspace create is JSON");
        reply["result"]["workspace"]["workspace_id"]
            .as_str()
            .expect("the reply names the workspace id")
            .to_string()
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
        build_once();
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
        output_within(&mut self.horch(args), DEADLINE)
    }

    pub fn seal(&self, cmd: &mut Command) {
        cmd.env_clear();
        cmd.current_dir(&self.project);
        let tmp = self.tmp.to_string_lossy().into_owned();
        let mut env: BTreeMap<String, String> = BTreeMap::new();
        env.insert("PATH".into(), self.sealed_path());
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
        #[cfg(unix)]
        env.insert(
            "HORCH_GIT_BIN".into(),
            no_git_path(&self.root).to_string_lossy().into_owned(),
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

    pub fn ledger_db(&self) -> PathBuf {
        self.env
            .get("HORCH_LEDGER_DB")
            .map(PathBuf::from)
            .unwrap_or_else(|| self.home.join(".horch/ledger.sqlite3"))
    }

    pub fn fleet_root(&self) -> PathBuf {
        self.env
            .get("FLEET_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                self.ledger_db()
                    .parent()
                    .expect("a ledger file has a folder")
                    .join("fleet")
            })
    }

    pub fn serve_fleetd(&self) {
        let mut fleetd = self.fleetd.borrow_mut();
        assert!(fleetd.is_none(), "1 test fleetd for each harness");
        let mut fleetds = crate::fleetd::Fleetds::on(self, self.ledger_db());
        fleetds.serve(self, "fleetd");
        *fleetd = Some(fleetds);
    }

    pub fn stop_fleetd(&self) {
        let mut fleetds = self
            .fleetd
            .borrow_mut()
            .take()
            .expect("the test fleetd of serve_fleetd");
        fleetds.stop_current();
    }

    pub fn without_fleetd<T>(&self, f: impl FnOnce() -> T) -> T {
        let mut fleetds = self
            .fleetd
            .borrow_mut()
            .take()
            .expect("the test fleetd of serve_fleetd");
        fleetds.stop_current();
        let out = f();
        fleetds.serve(self, "fleetd");
        *self.fleetd.borrow_mut() = Some(fleetds);
        out
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

    pub fn advance_fleet_clock(&self, seconds: i64) {
        let db = self.root.join("ledger.sqlite3");
        let store = horch_core::taskledger::Store::open(&db).unwrap();
        let moved = store
            .conn()
            .execute(
                "UPDATE fleetd SET fleet_clock_s = fleet_clock_s + ?1 WHERE id = 1",
                [seconds],
            )
            .unwrap();
        assert_eq!(moved, 1, "the ledger has a fleetd row");
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
        drop(self.fleetd.get_mut().take());
        let mut leaked = stop_jobs(&self.ledger_db());
        leaked.extend(match reap(&self.root, &self.log, reap_grace()) {
            Ok(leaked) => leaked,
            Err(error) if !std::thread::panicking() => panic!(
                "teardown could not list the processes of this test ({error}); a leaked process would go unseen"
            ),
            Err(error) => {
                eprintln!("teardown could not list the processes of this test ({error})");
                Vec::new()
            }
        });
        if !std::thread::panicking() {
            check_built_fakes();
            assert!(
                leaked.is_empty(),
                "the test leaked {} process(es) that outlived teardown by {:?}; \
                 teardown killed them:\n{}",
                leaked.len(),
                reap_grace(),
                leaked.join("\n")
            );
        }
        if std::env::var_os("HORCH_E2E_KEEP").is_none() {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}

fn reap_grace() -> std::time::Duration {
    let design = horch_core::fleetd::fleet_runtime::domain::config::FleetConfig::default();
    std::time::Duration::from_secs(design.stop_grace_s) + PS_SCAN_MARGIN
}

const PS_SCAN_MARGIN: std::time::Duration = std::time::Duration::from_secs(5);

#[cfg(unix)]
fn job_leaders(db: &Path) -> Vec<(u32, u64)> {
    if !db.is_file() {
        return Vec::new();
    }
    let read = || -> Result<Vec<(u32, u64)>, String> {
        let store =
            horch_core::taskledger::Store::open_read_only(db).map_err(|e| format!("{e:#}"))?;
        let mut stmt = store
            .conn()
            .prepare(
                "SELECT pid, pid_start FROM jobs WHERE pid IS NOT NULL AND pid_start IS NOT NULL",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
    };
    let mut rows = read().unwrap_or_else(|error| {
        eprintln!(
            "warning: teardown could not read the jobs of {}: {error}",
            db.display()
        );
        Vec::new()
    });
    rows.retain(|&(pid, _)| pid > 1);
    rows.sort_unstable();
    rows.dedup();
    rows
}

#[cfg(unix)]
fn stop_jobs(db: &Path) -> Vec<String> {
    stop_job_groups(&job_leaders(db), &mut || {})
}

#[cfg(unix)]
fn stop_job_groups(leaders: &[(u32, u64)], after_check: &mut dyn FnMut()) -> Vec<String> {
    use horch_platform::signal::{stop_same, Stop};
    let snapshot: Vec<(crate::process::Process, u64)> = leaders
        .iter()
        .map(|&(leader, started)| {
            let mut members: Vec<(u32, u64)> = horch_core::procid::group_members(leader)
                .into_iter()
                .filter(|pid| *pid != leader)
                .filter_map(|pid| Some((pid, horch_platform::process::start_time(pid)?)))
                .collect();
            members.push((leader, started));
            (leader, started, members)
        })
        .filter(|&(leader, started, _)| {
            horch_platform::group::is_group_leader(leader, started).unwrap_or(false)
        })
        .flat_map(|(leader, _, members)| {
            members.into_iter().map(move |(pid, start)| {
                let process = crate::process::Process {
                    pid,
                    pgid: leader,
                    command: String::new(),
                };
                (process, start)
            })
        })
        .collect();
    if snapshot.is_empty() {
        return Vec::new();
    }
    after_check();
    let running = |of: &[(crate::process::Process, u64)]| {
        let same: Vec<&(crate::process::Process, u64)> = of
            .iter()
            .filter(|(p, start)| horch_platform::process::start_time(p.pid) == Some(*start))
            .collect();
        let processes: Vec<crate::process::Process> = same.iter().map(|(p, _)| p.clone()).collect();
        let states = states_of(&processes);
        same.into_iter()
            .filter(|(p, _)| {
                after_grace(states.get(&p.pid).map(String::as_str)) == AfterGrace::Running
            })
            .cloned()
            .collect::<Vec<_>>()
    };
    let wait = |of: &[(crate::process::Process, u64)], limit: std::time::Duration| {
        let until = std::time::Instant::now() + limit;
        loop {
            let left = running(of);
            if left.is_empty() || std::time::Instant::now() >= until {
                return left;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    };
    let signal = |of: &[(crate::process::Process, u64)], how: Stop| {
        for (p, start) in of {
            stop_same(p.pid, *start, how);
        }
    };
    signal(&snapshot, Stop::Term);
    let design = horch_core::fleetd::fleet_runtime::domain::config::FleetConfig::default();
    let left = wait(
        &snapshot,
        std::time::Duration::from_secs(design.stop_grace_s),
    );
    if left.is_empty() {
        return Vec::new();
    }
    signal(&left, Stop::Kill);
    let left = wait(&left, EXIT_LIMIT);
    let commands: std::collections::HashMap<u32, String> = crate::process::processes()
        .into_iter()
        .map(|p| (p.pid, p.command))
        .collect();
    left.into_iter()
        .map(|(p, _)| {
            format!(
                "{} {} (a job process of group {}, alive {EXIT_LIMIT:?} after its SIGKILL)",
                p.pid,
                commands.get(&p.pid).map_or("?", String::as_str),
                p.pgid
            )
        })
        .collect()
}

#[cfg(not(unix))]
fn stop_jobs(_db: &Path) -> Vec<String> {
    Vec::new()
}

#[cfg(unix)]
fn reap(root: &Path, log: &Path, grace: std::time::Duration) -> std::io::Result<Vec<String>> {
    use crate::process::{commands_with_environment, signal_groups, try_processes, Process};
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
    let Some(all) =
        process_list_or_sandbox(try_processes, grace, &recorded, own_group, signal_groups)?
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
                    horch_platform::group::is_group_leader(pid, started)
                        .unwrap_or(false)
                        .then_some(pid)
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
        Ok(listed(try_processes, grace)?
            .into_iter()
            .filter(|p| {
                p.pid != me && (signalled.contains(&p.pgid) || names_root(p, &environments))
            })
            .collect())
    };
    let until = std::time::Instant::now() + grace;
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
            let states = states_of(&left);
            let (exiting, running): (Vec<Process>, Vec<Process>) = left
                .into_iter()
                .filter(|p| after_grace(states.get(&p.pid).map(String::as_str)) != AfterGrace::Gone)
                .partition(|p| {
                    after_grace(states.get(&p.pid).map(String::as_str)) == AfterGrace::InExit
                });
            let mut kill: Vec<u32> = running
                .iter()
                .map(|p| p.pgid)
                .filter(|g| *g > 1 && Some(*g) != own_group)
                .collect();
            kill.sort_unstable();
            kill.dedup();
            signal_groups(&kill, "KILL");
            let mut leaked: Vec<String> = running
                .into_iter()
                .map(|p| {
                    let when = if first.contains(&p.pid) {
                        "ran at the first SIGTERM"
                    } else {
                        "started after the first SIGTERM"
                    };
                    format!("{} {} ({when})", p.pid, p.command)
                })
                .collect();
            leaked.extend(wait_exits(exiting, EXIT_LIMIT, states_of));
            return Ok(leaked);
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[cfg(not(unix))]
fn reap(_root: &Path, _log: &Path, _grace: std::time::Duration) -> std::io::Result<Vec<String>> {
    Ok(Vec::new())
}

#[cfg(unix)]
const EXIT_LIMIT: std::time::Duration = std::time::Duration::from_secs(180);

#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AfterGrace {
    Gone,
    InExit,
    Running,
}

#[cfg(unix)]
fn after_grace(state: Option<&str>) -> AfterGrace {
    match state {
        None => AfterGrace::Gone,
        Some(s) if s.starts_with(['Z', 'X']) || s.contains('E') => AfterGrace::InExit,
        Some(_) => AfterGrace::Running,
    }
}

#[cfg(unix)]
fn states_of(processes: &[crate::process::Process]) -> std::collections::HashMap<u32, String> {
    if processes.is_empty() {
        return Default::default();
    }
    let pids: Vec<String> = processes.iter().map(|p| p.pid.to_string()).collect();
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match std::process::Command::new("/bin/ps")
            .args(["-o", "pid=,stat=", "-p", &pids.join(",")])
            .output()
        {
            Ok(out) => {
                return String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .filter_map(|line| {
                        let mut parts = line.split_whitespace();
                        Some((parts.next()?.parse().ok()?, parts.next()?.to_string()))
                    })
                    .collect()
            }
            Err(_) if std::time::Instant::now() < until => {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(_) => return Default::default(),
        }
    }
}

#[cfg(unix)]
fn wait_exits(
    mut exiting: Vec<crate::process::Process>,
    limit: std::time::Duration,
    states: impl Fn(&[crate::process::Process]) -> std::collections::HashMap<u32, String>,
) -> Vec<String> {
    let start = std::time::Instant::now();
    loop {
        let now = states(&exiting);
        exiting.retain(|p| {
            let still = after_grace(now.get(&p.pid).map(String::as_str)) == AfterGrace::InExit;
            if !still {
                eprintln!(
                    "teardown: {} {} was in its exit at the end of the grace and was gone {:?} later",
                    p.pid,
                    p.command,
                    start.elapsed()
                );
            }
            still
        });
        if exiting.is_empty() {
            return Vec::new();
        }
        if start.elapsed() >= limit {
            return exiting
                .into_iter()
                .map(|p| {
                    format!(
                        "{} {} (in its exit for more than {limit:?})",
                        p.pid, p.command
                    )
                })
                .collect();
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

const OWNER_FILE: &str = ".horch-e2e-owner";

fn tmpdir() -> PathBuf {
    std::env::var_os("TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

#[cfg(target_os = "linux")]
const ROOT_PARENT: &str = "/var/tmp";
#[cfg(all(unix, not(target_os = "linux")))]
const ROOT_PARENT: &str = "/tmp";

fn harness_parents() -> Vec<PathBuf> {
    #[cfg(unix)]
    return vec![PathBuf::from(ROOT_PARENT), tmpdir()];
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
        leaked.extend(reap(&root, &root.join("fake.log"), reap_grace()).unwrap_or_default());
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

fn build_once() {
    static BUILT: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    BUILT.get_or_init(|| {
        let mut cmd = build_command(&bin_dir(), env!("HORCH_E2E_TARGET"));
        let out = cmd
            .output()
            .unwrap_or_else(|e| panic!("cannot start {cmd:?}: {e}"));
        assert!(
            out.status.success(),
            "the build of horch and the fakes failed: {cmd:?}\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    });
}

const CARGO_RUN_VARS: &[&str] = &[
    "CARGO_MANIFEST_DIR",
    "CARGO_MANIFEST_PATH",
    "CARGO_CRATE_NAME",
    "CARGO_BIN_NAME",
    "CARGO_PRIMARY_PACKAGE",
    "CARGO_TARGET_TMPDIR",
    "CARGO_RUSTC_CURRENT_DIR",
    "OUT_DIR",
];

pub(crate) fn built_dir_of(exe: &Path) -> PathBuf {
    let dir = exe.parent().expect("exe dir");
    let name = |p: &Path| p.file_name().and_then(|n| n.to_str()).map(str::to_owned);
    if name(dir).as_deref() == Some("deps") {
        return dir.parent().expect("profile dir").to_path_buf();
    }
    let build = dir.ancestors().nth(3);
    if name(dir).as_deref() == Some("out") && build.and_then(name).as_deref() == Some("build") {
        return build
            .and_then(Path::parent)
            .expect("profile dir")
            .to_path_buf();
    }
    dir.to_path_buf()
}

fn cargo_profile(built: &Path) -> &str {
    match built.file_name().and_then(|n| n.to_str()) {
        Some("debug") | None => "dev",
        Some(profile) => profile,
    }
}

fn build_command(built: &Path, triple: &str) -> Command {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut cmd = Command::new(cargo);
    for (key, _) in std::env::vars_os() {
        let key = key.to_string_lossy();
        if key.starts_with("CARGO_PKG_") || CARGO_RUN_VARS.contains(&key.as_ref()) {
            cmd.env_remove(key.as_ref());
        }
    }
    cmd.current_dir(workspace).args([
        "build",
        "--locked",
        "-p",
        "horch",
        "-p",
        "horch-e2e",
        "--bins",
    ]);
    match cargo_profile(built) {
        "dev" => {}
        "release" => {
            cmd.arg("--release");
        }
        profile => {
            cmd.args(["--profile", profile]);
        }
    }
    let mut root = built.parent();
    if root.and_then(Path::file_name).and_then(|n| n.to_str()) == Some(triple) {
        cmd.args(["--target", triple]);
        root = root.and_then(Path::parent);
    }
    if let Some(root) = root {
        cmd.arg("--target-dir").arg(root);
    }
    cmd
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

pub fn output_within(cmd: &mut Command, limit: Duration) -> Output {
    use std::io::Read;
    use std::sync::{Arc, Mutex};
    let line = format!("{cmd:?}");
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    horch_platform::group::new_group(cmd);
    let began = Instant::now();
    let mut child = cmd
        .spawn()
        .unwrap_or_else(|e| panic!("starting {line}: {e}"));
    let read = |pipe: Option<Box<dyn Read + Send>>| {
        let text = Arc::new(Mutex::new(Vec::new()));
        let into = Arc::clone(&text);
        let reader = std::thread::spawn(move || {
            let Some(mut pipe) = pipe else { return };
            let mut chunk = [0u8; 4096];
            while let Ok(n) = pipe.read(&mut chunk) {
                if n == 0 {
                    return;
                }
                into.lock().unwrap().extend_from_slice(&chunk[..n]);
            }
        });
        (text, reader)
    };
    let (stdout, out_reader) = read(child.stdout.take().map(|p| Box::new(p) as _));
    let (stderr, err_reader) = read(child.stderr.take().map(|p| Box::new(p) as _));
    let mut status = None;
    while began.elapsed() < limit {
        if status.is_none() {
            status = match child.try_wait() {
                Ok(status) => status,
                Err(e) => {
                    horch_platform::group::kill_child_group(&mut child);
                    let _ = child.wait();
                    panic!("waiting for {line}: {e}");
                }
            };
        }
        if let Some(status) =
            status.filter(|_| out_reader.is_finished() && err_reader.is_finished())
        {
            let _ = (out_reader.join(), err_reader.join());
            let status = child.wait().unwrap_or(status);
            let take = |text: Arc<Mutex<Vec<u8>>>| std::mem::take(&mut *text.lock().unwrap());
            return Output {
                status,
                stdout: take(stdout),
                stderr: take(stderr),
            };
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    horch_platform::group::kill_child_group(&mut child);
    let _ = child.wait();
    let state = match status {
        Some(status) => format!("exited with {status}, but its stdout or stderr stayed open"),
        None => "did not exit".to_string(),
    };
    let text = |t: &Arc<Mutex<Vec<u8>>>| String::from_utf8_lossy(&t.lock().unwrap()).into_owned();
    panic!(
        "{line} {state} after {} s; its process group was killed\n--- stdout so far ---\n{}\n--- stderr so far ---\n{}",
        limit.as_secs_f32(),
        text(&stdout),
        text(&stderr)
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

#[cfg(unix)]
const NO_GIT: &str = "#!/bin/sh\n\
[ \"$1\" = warm ] && exit 0\n\
if [ \"$1\" = version ]; then echo 'git version 2.41.0 (the test world has no git)'; exit 0; fi\n\
echo 'the test world has no git: use with_git' >&2\n\
exit 1\n";

#[cfg(unix)]
fn no_git_path(root: &Path) -> PathBuf {
    root.join("no-git").join("git")
}

#[cfg(unix)]
fn write_no_git(root: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let path = no_git_path(root);
    std::fs::create_dir_all(path.parent().unwrap()).expect("creating the no-git dir");
    std::fs::write(&path, NO_GIT).expect("writing the no-git fake");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
        .expect("making the no-git fake executable");
    let out = warm(Command::new(&path).arg("warm"))
        .unwrap_or_else(|e| panic!("warm {}: {e}", path.display()));
    assert!(out.status.success(), "warming {} failed", path.display());
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
        build_command, built_dir_of, cargo_profile, choose_harness_root, listed,
        process_list_or_sandbox, reap, reap_grace, stop_stale_harnesses, OWNER_FILE,
    };

    #[test]
    fn reused_group_after_teardown_check_gets_no_signal() {
        use std::io::{BufRead, BufReader};
        use std::os::unix::process::CommandExt;
        use std::process::{Child, Command, Stdio};
        let ignore_term = |cmd: &mut Command| -> Child {
            cmd.args(["-c", "trap '' TERM; echo ready; exec sleep 60 >/dev/null"])
                .stdout(Stdio::piped());
            let mut child = cmd.spawn().expect("start sh");
            let mut line = String::new();
            BufReader::new(child.stdout.take().unwrap())
                .read_line(&mut line)
                .unwrap();
            assert_eq!(line, "ready\n", "sh ignores TERM");
            child
        };
        let mut cmd = Command::new("sh");
        horch_platform::group::new_group(&mut cmd);
        let mut leader = ignore_term(&mut cmd);
        let pgid = leader.id();
        let started = horch_platform::process::start_time(pgid).expect("sh runs");
        let mut late = None;
        let left = super::stop_job_groups(&[(pgid, started)], &mut || {
            let mut cmd = Command::new("sh");
            cmd.process_group(i32::try_from(pgid).unwrap());
            late = Some(ignore_term(&mut cmd));
        });
        let mut late = late.expect("the hook ran");
        std::thread::sleep(Duration::from_millis(200));
        let lives = late.try_wait().unwrap().is_none();
        let leader_ended = leader.try_wait().unwrap().is_some();

        let _ = late.kill();
        let _ = late.wait();
        let _ = leader.kill();
        let _ = leader.wait();
        assert_eq!(left, Vec::<String>::new(), "the leader ends by the KILL");
        assert!(leader_ended, "the leader gets the KILL");
        assert!(
            lives,
            "the process that is not of the snapshot gets no signal"
        );
    }

    #[cfg(unix)]
    #[test]
    fn after_the_grace_a_process_in_its_exit_is_told_from_a_running_one() {
        use super::{after_grace, AfterGrace};
        assert_eq!(after_grace(None), AfterGrace::Gone);
        for state in ["Z", "Z+", "X", "?E", "UE", "SE", "RE+"] {
            assert_eq!(after_grace(Some(state)), AfterGrace::InExit, "{state}");
        }
        for state in ["S", "Ss", "S+", "R", "R+", "U", "Us", "T", "I", "SN"] {
            assert_eq!(after_grace(Some(state)), AfterGrace::Running, "{state}");
        }
    }

    #[cfg(unix)]
    fn bounded_panic(script: &str, limit: Duration) -> String {
        let mut cmd = std::process::Command::new("sh");
        cmd.args(["-c", script]);
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            super::output_within(&mut cmd, limit)
        }))
        .expect_err("the wait must end with a panic");
        panic.downcast_ref::<String>().cloned().unwrap_or_default()
    }

    #[cfg(unix)]
    #[test]
    fn a_run_within_its_limit_gives_its_output() {
        let mut cmd = std::process::Command::new("sh");
        cmd.args(["-c", "echo out; echo err >&2; exit 3"]);
        let out = super::output_within(&mut cmd, Duration::from_secs(60));
        assert_eq!(out.status.code(), Some(3));
        assert_eq!(out.stdout, b"out\n");
        assert_eq!(out.stderr, b"err\n");
    }

    #[cfg(unix)]
    #[test]
    fn a_pipe_that_a_grandchild_holds_open_ends_the_wait_with_a_panic() {
        let began = std::time::Instant::now();
        let message = bounded_panic("sleep 600 & echo $!", Duration::from_secs(2));
        assert!(began.elapsed() < Duration::from_secs(30), "{message}");
        assert!(
            message.contains("exited with exit status: 0, but its stdout or stderr stayed open"),
            "{message}"
        );
        assert!(message.contains("sleep 600 & echo $!"), "{message}");
        let sleeper: u32 = message
            .split("--- stdout so far ---\n")
            .nth(1)
            .and_then(|rest| rest.lines().next())
            .and_then(|pid| pid.trim().parse().ok())
            .unwrap_or_else(|| panic!("no pid in the stdout so far: {message}"));
        crate::harness::wait_for("the end of the killed grandchild", || {
            (!horch_core::procid::alive(sleeper, None)).then_some(())
        });
    }

    #[cfg(unix)]
    #[test]
    fn a_child_that_never_exits_ends_the_wait_with_a_panic() {
        let message = bounded_panic("echo started; sleep 600", Duration::from_secs(1));
        assert!(message.contains("did not exit after 1 s"), "{message}");
        assert!(
            message.contains("--- stdout so far ---\nstarted\n"),
            "{message}"
        );
    }

    #[cfg(unix)]
    fn process(pid: u32) -> crate::process::Process {
        crate::process::Process {
            pid,
            pgid: pid,
            command: format!("horch job run {pid}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_process_in_its_exit_leaks_only_when_its_exit_outlasts_the_limit() {
        use std::collections::HashMap;
        let polls = Cell::new(0);
        let ends = |_: &[crate::process::Process]| {
            polls.set(polls.get() + 1);
            let mut states = HashMap::new();
            if polls.get() < 3 {
                states.insert(7, "?E".to_string());
                states.insert(8, "Z".to_string());
            } else {
                states.insert(8, "S".to_string());
            }
            states
        };
        let leaked = super::wait_exits(vec![process(7), process(8)], Duration::from_secs(60), ends);
        assert_eq!(leaked, Vec::<String>::new());
        assert_eq!(
            polls.get(),
            3,
            "the wait ends when both are out of their exit"
        );

        let stays = |_: &[crate::process::Process]| HashMap::from([(9, "?E".to_string())]);
        let leaked = super::wait_exits(vec![process(9)], Duration::from_millis(300), stays);
        assert_eq!(leaked.len(), 1, "{leaked:?}");
        assert!(
            leaked[0].starts_with("9 horch job run 9 (in its exit"),
            "{leaked:?}"
        );
    }

    #[test]
    fn built_binaries_are_not_older_than_their_source() {
        let horch = super::Harness::horch_bin();
        let fake = crate::bin_dir().join(super::exe("fake-herdr"));
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let horch_src: &[&str] = &["crates/horch/src", "crates/horch-core/src"];
        let fake_src: &[&str] = &["crates/horch-e2e/src", "crates/horch-core/src"];
        for (bin, dirs) in [(horch, horch_src), (fake, fake_src)] {
            let built = std::fs::metadata(&bin).unwrap().modified().unwrap();
            let mut files = Vec::new();
            for dir in dirs {
                super::walk(&root.join(dir), &mut files);
            }
            let newer: Vec<_> = files
                .iter()
                .filter(|f| f.extension().is_some_and(|e| e == "rs"))
                .filter(|f| std::fs::metadata(f).unwrap().modified().unwrap() > built)
                .collect();
            assert!(
                newer.is_empty(),
                "{} is older than {newer:?}",
                bin.display()
            );
        }
    }

    #[test]
    fn the_build_uses_the_profile_and_target_folder_of_the_test() {
        let args = |built: &str| -> Vec<String> {
            build_command(Path::new(built), "aarch64-apple-darwin")
                .get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect()
        };
        let base = [
            "build",
            "--locked",
            "-p",
            "horch",
            "-p",
            "horch-e2e",
            "--bins",
        ];
        let with = |more: &[&str]| -> Vec<String> {
            base.iter().chain(more).map(|a| a.to_string()).collect()
        };
        assert_eq!(
            args("/w/target/debug"),
            with(&["--target-dir", "/w/target"])
        );
        assert_eq!(
            args("/w/target/release"),
            with(&["--release", "--target-dir", "/w/target"])
        );
        assert_eq!(
            args("/w/target/ci"),
            with(&["--profile", "ci", "--target-dir", "/w/target"])
        );
    }

    #[test]
    fn profile_comes_from_the_target_layout_of_old_and_new_cargo() {
        let profile = |exe: &str| {
            let built = built_dir_of(Path::new(exe));
            (cargo_profile(&built).to_string(), built)
        };
        let triple = "/w/target/x86_64-unknown-linux-gnu";
        for (root, want) in [
            ("/w/target/debug", "dev"),
            ("/w/target/release", "release"),
            ("/w/target/ci", "ci"),
            (&format!("{triple}/debug")[..], "dev"),
        ] {
            for exe in [
                format!("{root}/build/horch-e2e/ce99708c1aeeb00b/out/mail-ce99708c1aeeb00b"),
                format!("{root}/deps/mail-ce99708c1aeeb00b"),
            ] {
                assert_eq!(
                    profile(&exe),
                    (want.to_string(), PathBuf::from(root)),
                    "{exe}"
                );
            }
        }
        let new = "/w/target/debug/build/horch-e2e/ce99708c1aeeb00b/out/mail-ce99708c1aeeb00b";
        let args: Vec<String> =
            build_command(&built_dir_of(Path::new(new)), "aarch64-apple-darwin")
                .get_args()
                .skip(7)
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
        assert_eq!(args, ["--target-dir", "/w/target"]);
    }

    #[test]
    fn the_build_uses_the_target_triple_of_the_test() {
        let triple = "x86_64-unknown-linux-gnu";
        let args = |built: &str| -> Vec<String> {
            build_command(Path::new(built), triple)
                .get_args()
                .skip(7)
                .map(|a| a.to_string_lossy().into_owned())
                .collect()
        };
        assert_eq!(
            args("/w/target/x86_64-unknown-linux-gnu/debug"),
            ["--target", triple, "--target-dir", "/w/target"]
        );
        assert_eq!(
            args("/w/target/x86_64-unknown-linux-gnu/release"),
            ["--release", "--target", triple, "--target-dir", "/w/target"]
        );
        assert_eq!(args("/w/target/debug"), ["--target-dir", "/w/target"]);
    }

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

    fn stopping_in(root: &Path, on_term: &str) -> u32 {
        std::fs::create_dir_all(root).unwrap();
        let ready = root.join("ready");
        let script = format!(
            "$SIG{{TERM}} = {on_term}; open my $f, '>', $ARGV[0]; print $f $$; close $f; sleep 600;"
        );
        let mut cmd = std::process::Command::new("/bin/sh");
        cmd.args(["-c", "perl -e \"$0\" \"$1\" </dev/null >/dev/null 2>&1 &"])
            .arg(script)
            .arg(&ready);
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
        assert!(cmd.status().unwrap().success());
        super::wait_for("the process to set its SIGTERM handler", || {
            std::fs::read_to_string(&ready).ok()?.trim().parse().ok()
        })
    }

    #[test]
    fn a_process_that_stops_within_the_stop_grace_is_not_leaked() {
        let root = super::tmpdir().join(format!("h-slow-{:x}", super::unique()));
        let pid = stopping_in(&root, "sub { select(undef, undef, undef, 8); exit 0 }");
        let started = std::time::Instant::now();

        let leaked = reap(&root, &root.join("fake.log"), reap_grace());

        let took = started.elapsed();
        let runs = horch_core::fsx::pid_alive(pid);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(leaked.unwrap(), Vec::<String>::new());
        assert!(!runs, "teardown returned while the process ran");
        assert!(
            took >= Duration::from_secs(8),
            "the stop took {took:?}: it must be slower than the old 5 s wait"
        );
    }

    fn sleeping_job(ready: &Path) -> u32 {
        let mut cmd = std::process::Command::new("/bin/sh");
        cmd.args([
            "-c",
            "perl -e 'setpgrp(0, 0); open my $f, q(>), $ARGV[0]; print $f $$; close $f; sleep 600' \"$0\" </dev/null >/dev/null 2>&1 &",
        ])
        .arg(ready)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .current_dir("/");
        assert!(cmd.status().unwrap().success());
        super::wait_for("the job process to lead its group", || {
            let pid: u32 = std::fs::read_to_string(ready).ok()?.trim().parse().ok()?;
            let started = horch_core::procid::start_time(pid)?;
            horch_core::procid::is_same_group(pid, started).then_some(pid)
        })
    }

    #[cfg(unix)]
    #[test]
    fn the_teardown_stops_each_job_of_the_world_by_its_recorded_group() {
        use super::{after_grace, states_of, AfterGrace};
        let mut h = super::Harness::new("job-teardown");
        let db = h.root.join("ledger.sqlite3");
        h.set("HORCH_LEDGER_DB", db.to_string_lossy());
        let ready = super::tmpdir().join(format!("h-job-{:x}", super::unique()));
        let pid = sleeping_job(&ready);
        let started = horch_core::procid::start_time(pid).expect("the job process runs");
        let store = horch_core::taskledger::Store::open(&db).unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO jobs (kind, subject, input_key, state, pid, pid_start, time_limit_s,
                     deadline, created_at, started_at)
                 VALUES ('docs_sync', 'herdr 0', '0', 'running', ?1, ?2, 3600, 3600,
                     '2026-11-21T00:00:00Z', '2026-11-21T00:00:00Z')",
                rusqlite::params![pid, started],
            )
            .unwrap();
        drop(store);

        drop(h);

        let job = crate::process::Process {
            pid,
            pgid: pid,
            command: String::new(),
        };
        let state = states_of(std::slice::from_ref(&job));
        let runs = after_grace(state.get(&pid).map(String::as_str)) == AfterGrace::Running
            && horch_core::procid::is_same(pid, started);
        if runs {
            // SAFETY: plain syscall; the group is the one this test started.
            unsafe { libc::kill(-(pid as libc::pid_t), libc::SIGKILL) };
        }
        let _ = std::fs::remove_file(&ready);
        assert!(
            !runs,
            "the job process {pid} of the world still runs after the teardown"
        );
    }

    #[test]
    fn a_process_that_ignores_sigterm_is_killed_and_reported() {
        let root = super::tmpdir().join(format!("h-stuck-{:x}", super::unique()));
        let pid = stopping_in(&root, "'IGNORE'");

        let leaked = reap(&root, &root.join("fake.log"), Duration::from_secs(1)).unwrap();

        super::wait_for("the killed process to end", || {
            (!horch_core::fsx::pid_alive(pid)).then_some(())
        });
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(leaked.len(), 1, "{leaked:?}");
        assert!(
            leaked[0].starts_with(&format!("{pid} ")),
            "the report names the process: {leaked:?}"
        );
    }
}
