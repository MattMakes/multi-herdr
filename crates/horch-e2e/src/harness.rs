//! A sealed world for one end-to-end test.
//!
//! Each [`Harness`] gets a fresh temp directory holding a fake home, a state
//! root, a temp dir for mailboxes, and a `bin/` of fakes named like the real
//! programs (`claude`, `codex`, `herdr`, `opencode`, `prime-agent`, `pi`,
//! `ollama`). `horch` runs with a cleared environment whose PATH is that
//! `bin/` alone, and every `HORCH_*_BIN` points into it, so no real harness, no
//! real herdr and no file outside the temp dir can be reached (NFR-01).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::bin_dir;

/// Run `cmd` once, untimed, and return its output. macOS scans a new
/// executable at its first exec, for seconds: a warm keeps that scan out of
/// the call under test. On Linux a test thread that forks while another
/// writes a script can hold it open for writing, so exec fails with "Text
/// file busy" until that child execs; retry only that error until it does.
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

/// The fakes, by the name of the program each stands in for.
pub const FAKES: [(&str, &str); 8] = [
    ("agy", "fake-antigravity"),
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
    /// Variables the sealed environment sets by default that this harness
    /// leaves out ([`Harness::without`]).
    removed: BTreeSet<String>,
    /// The real `git`, by absolute path, once [`Harness::with_git`] found it.
    git: Option<PathBuf>,
    /// The test fleetd of [`Harness::serve_fleetd`]. The drop stops it
    /// before the teardown looks for leaked processes.
    fleetd: std::cell::RefCell<Option<crate::fleetd::Fleetds>>,
}

/// The commit date of the fixture repo, and of every commit made through the
/// harness environment, so a commit hash is the same in every run.
pub const GIT_DATE: &str = "2026-09-28T12:00:00+00:00";

/// The default `HORCH_FAULT` of the sealed environment: a 100 ms delivery
/// probe budget.
pub const DELIVERY_PROBE_FAULT: &str = "delivery-probe-ms:100";

/// The default machine fixture: each reading of the health guards is known
/// and far from its floor.
const HEALTHY_MACHINE: &str = r#"{"os":"linux","arch":"x86_64","cpus":8,"load_average_1m":1.0,"mem_total_bytes":17179869184,"mem_available_bytes":8589934592,"disk_free_bytes":214748364800,"disk_total_bytes":536870912000,"gpu":"none","max_open_files":1048576,"max_processes":100000}"#;

fn exe(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

impl Harness {
    /// On unix the preferred root is `<ROOT_PARENT>/horch-e2e-<name>-<pid>-<n>`, with
    /// `<name>` cut to its first 12 characters and `<n>` in hex. When the
    /// process cannot write there, `TMPDIR` is used only if Prime's longest
    /// daemon socket path stays below macOS's 104-byte limit.
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
        // A fake that starts slowly under load must not fail the local health
        // check: its deadline is the e2e deadline here, not production's 5 s.
        h.set(
            "HORCH_LOCAL_PROBE_TIMEOUT_MS",
            DEADLINE.as_millis().to_string(),
        );
        // The health guards of `horch spawn` read a healthy machine, so a
        // test does not depend on the load of the machine that runs it. A
        // test of a guard sets its own `HORCH_MACHINE_FILE`.
        let machine = h.root.join("healthy-machine.json");
        std::fs::write(&machine, HEALTHY_MACHINE).expect("writing the machine fixture");
        h.set("HORCH_MACHINE_FILE", machine.to_string_lossy());
        let ledger = h.root.join("ledger").join("ledger.sqlite3");
        h.set("HORCH_LEDGER_DB", ledger.to_string_lossy());
        let built = bin_dir();
        for (name, fake) in FAKES {
            let from = built.join(exe(fake));
            // A link, not a copy: macOS scans every new executable on its
            // first run, and under load many fresh copies at once take
            // seconds to start. A link to the built file is scanned once.
            // So nothing may write through these names: use `write_bin`.
            // On unix a symlink, not a hard link: syspolicyd scans an exec
            // late under load and reads the file by a path it finds from
            // the inode. With hard links that can be another harness's
            // link, already deleted; syspolicyd then kills the process with
            // SIGKILL ("Malware rejection"). The built file is never deleted.
            let to = h.bin.join(exe(name));
            if link_fake(&from, &to).is_err() {
                std::fs::copy(&from, &to).expect("copying a fake");
            }
        }
        // The one real program the tests use, on a fixture database only.
        // A symlink, not a copy: macOS kills a copied system binary (137).
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

    /// Give this harness a real git repository: 2 files and 1 commit on branch
    /// `main`, in the project dir.
    ///
    /// `git` is found by absolute path on the real `PATH` and is never put on
    /// the sealed `PATH`; `HORCH_GIT_BIN` names it. The environment pins
    /// everything that could change a commit hash or read the operator's
    /// config: an empty `GIT_CONFIG_GLOBAL`, `GIT_CONFIG_NOSYSTEM=1`, the
    /// author, the committer and both dates. Real git touches only temp dirs.
    ///
    /// Without `git` the harness is returned as it was and [`Harness::git_bin`]
    /// is `None`, unless `HORCH_REQUIRE_GIT=1`, which makes that a panic.
    ///
    /// The sealed home also records Claude's and Codex's trust in the
    /// project, as the operator does once, so dataset preflight (PRE-14)
    /// passes. [`Harness::untrust_project`] removes it.
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
        // `symbolic-ref` rather than `init -b`: older git has no `-b`.
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

    /// Record Claude's (`~/.claude.json`) and Codex's
    /// (`$CODEX_HOME/config.toml`) trust in the project root, in the sealed
    /// home, by its canonical path as the harnesses record it.
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

    /// Remove the trust [`Harness::trust_project`] recorded.
    pub fn untrust_project(&self) {
        let _ = std::fs::remove_file(self.home.join(".claude.json"));
        let _ = std::fs::remove_file(self.home.join(".codex").join("config.toml"));
    }

    /// The real `git`, by absolute path, after [`Harness::with_git`].
    pub fn git_bin(&self) -> Option<&Path> {
        self.git.as_deref()
    }

    /// A `git` command in the project dir with the pinned environment.
    /// Panics before [`Harness::with_git`].
    pub fn git_cmd(&self, args: &[&str]) -> Command {
        let git = self.git.as_ref().expect("call with_git() first");
        let mut cmd = Command::new(git);
        cmd.args(args);
        self.seal(&mut cmd);
        // `seal` clears the environment already; the scrub keeps it so if a
        // caller adds variables later.
        horch_marketplace::git::scrub_repo_env(&mut cmd);
        cmd
    }

    /// Make the herdr workspace of the fleet of this harness, as `horch
    /// fleet` makes it: its label is the name of the project folder. fleetd
    /// launches each attempt with no recorded pane in the first pane of
    /// that workspace (T-890). Gives the workspace id, for example `w1`.
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

    /// The commit at `HEAD` of the project repo. `None` without git.
    pub fn head_sha(&self) -> Option<String> {
        self.git.as_ref()?;
        let out = self.git_cmd(&["rev-parse", "HEAD"]).output().ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// NFR-01: whether `horch` may start the program at `path`. Allowed are
    /// the files in the fakes dir and the one real `git` that
    /// [`Harness::with_git`] found. Nothing else is.
    pub fn allows_program(&self, path: &Path) -> bool {
        path.starts_with(&self.bin) || self.git.as_deref() == Some(path)
    }

    /// The `horch` binary under test, built next to the fakes.
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

    /// A `horch` command in the sealed environment.
    pub fn horch(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(Self::horch_bin());
        cmd.args(args);
        self.seal(&mut cmd);
        cmd
    }

    /// The session records of the project with slug `slug`, rendered as its
    /// JSON ledger was (TRF-45: no code writes that file now; the records
    /// live in `<state>/ledger.sqlite3`, an old JSON ledger read only).
    pub fn stored_ledger(&self, slug: &str) -> Result<String, String> {
        horch_core::execution::store::ExecutionStore::for_project(&self.state, slug)
            .stored_json()
            .map_err(|e| format!("{e:#}"))
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
        // Fake panes never show an agent, so `horch tell` gives a fresh role
        // no grace here (the grace has its own tests in horch-core).
        env.insert("HORCH_TELL_GRACE_MS".into(), "0".into());
        // Most scenarios run no worker in a pane, so `horch spawn` does not
        // wait for one to register (tests/startup_error.rs sets the wait).
        env.insert("HORCH_SPAWN_WAIT_MS".into(), "0".into());
        // `horch fleet` runs no agent CLI updater: a test asserts the exact
        // calls each fake saw. tests/update.rs uses `allow_updates`.
        env.insert("HORCH_NO_UPDATE".into(), "1".into());
        // The fake herdr never runs the delivery probe's typed command, so
        // the probe of `horch fleet` and `horch doctor` waits its full
        // budget. 100 ms, not production's 5 s. A test of the probe sets its
        // own `HORCH_FAULT`; `unset` brings this default back.
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
        for key in &self.removed {
            env.remove(key);
        }
        for (k, v) in &self.env {
            env.insert(k.clone(), v.clone());
        }
        cmd.envs(env);
    }

    /// The task ledger of the sealed environment: `HORCH_LEDGER_DB`, which
    /// is `<root>/ledger/ledger.sqlite3` by default, else (after
    /// `unset("HORCH_LEDGER_DB")`) `<home>/.horch/ledger.sqlite3`.
    pub fn ledger_db(&self) -> PathBuf {
        self.env
            .get("HORCH_LEDGER_DB")
            .map(PathBuf::from)
            .unwrap_or_else(|| self.home.join(".horch/ledger.sqlite3"))
    }

    /// The fleet root of the sealed environment: `FLEET_ROOT`, else the
    /// folder `fleet` next to [`Harness::ledger_db`].
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

    /// Start the test fleetd of this harness: `horch fleetd` on
    /// [`Harness::ledger_db`], in the sealed environment. It returns when
    /// fleetd serves its channel, so a routed `horch` command gets its
    /// reply. Set the environment that names the ledger first. The drop of
    /// the harness stops it. Its output is in `<root>/fleetd.stdout` and
    /// `<root>/fleetd.stderr`.
    pub fn serve_fleetd(&self) {
        let mut fleetd = self.fleetd.borrow_mut();
        assert!(fleetd.is_none(), "1 test fleetd for each harness");
        let mut fleetds = crate::fleetd::Fleetds::on(self, self.ledger_db());
        fleetds.serve(self, "fleetd");
        *fleetd = Some(fleetds);
    }

    /// Stop the test fleetd of [`Harness::serve_fleetd`] with SIGTERM, and
    /// wait until it is gone. A test that runs its own fleetd or its own
    /// passes after a routed setup calls it.
    pub fn stop_fleetd(&self) {
        let mut fleetds = self
            .fleetd
            .borrow_mut()
            .take()
            .expect("the test fleetd of serve_fleetd");
        fleetds.stop_current();
    }

    /// Run `f` while the test fleetd of [`Harness::serve_fleetd`] is down,
    /// then start it again. A test uses it for a command that needs the
    /// lock of fleetd, such as `horch ledger watch --once`, which runs no
    /// pass while a fleetd holds the lock. A routed command in `f` writes a
    /// request file.
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

    /// Put an executable `name` with `contents` in `bin/` and return its
    /// path. An existing entry is removed first, never written through: it
    /// can be a link to a built fake.
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

    /// [`Harness::write_bin`], then [`warm`] it with the argument `warm`,
    /// which the script must answer with exit 0.
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

    /// Leave `key` out of the sealed environment, also when the harness sets
    /// it by default (`HORCH_CLAUDE_BIN`, `HORCH_NO_UPDATE`, `HORCH_FAULT`). A later
    /// [`Harness::set`] of the same key still wins.
    pub fn without(&mut self, key: &str) -> &mut Self {
        self.env.remove(key);
        self.removed.insert(key.to_string());
        self
    }

    /// Let `horch fleet` run the agent CLI update step: remove the default
    /// `HORCH_NO_UPDATE=1`.
    pub fn allow_updates(&mut self) -> &mut Self {
        self.without("HORCH_NO_UPDATE")
    }

    /// Every call the fakes recorded so far.
    pub fn calls(&self) -> Vec<Value> {
        crate::read_log(&self.log)
    }

    /// Calls of one fake (`"herdr"`, `"claude"`, ...), one per call: a fake
    /// that logs as it goes writes several lines with one `call_id`, and the
    /// last is whole. A record without a `call_id` is keyed by its pid.
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

    /// Move the fleet clock of the ledger `<root>/ledger.sqlite3` forward
    /// by `seconds`, in place of a wait for it. A fleetd pass adds its
    /// awake time to the stored value, so the move stays.
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

#[cfg(unix)]
const UNIX_SOCKET_PATH_LIMIT: usize = 104;

/// Select the harness root after probing its preferred `/tmp` path.
/// Prime launch ids contain 12 hex characters.
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
        let leaked = match reap(&self.root, &self.log, reap_grace()) {
            Ok(leaked) => leaked,
            // A test that passes while its processes cannot be seen can
            // leave one running for hours: fail it, unless it already fails.
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
                reap_grace(),
                leaked.join("\n")
            );
        }
        if std::env::var_os("HORCH_E2E_KEEP").is_none() {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}

/// How long teardown waits for the harness's processes to end after it
/// signals them. A process ends on SIGTERM only when its current syscall
/// returns, and on a loaded host 1 `rename` can block for seconds (T-114).
fn reap_grace() -> std::time::Duration {
    let design = horch_core::fleetd::fleet_runtime::domain::config::FleetConfig::default();
    std::time::Duration::from_secs(design.stop_grace_s) + PS_SCAN_MARGIN
}

const PS_SCAN_MARGIN: std::time::Duration = std::time::Duration::from_secs(5);

/// End every process this harness started, and return the ones that
/// leaked.
///
/// A test that fails before `horch done`, or never closes a pane, leaves
/// fake-herdr `exec` pane commands running: `horch worker` and a `stay`
/// fake run for ever. Teardown sends SIGTERM to the process group of every
/// pane that fake-herdr recorded and that is still the same group (the
/// leader's start time matches, see [`horch_core::procid::is_same_group`]), and
/// to the group of every process whose command line or environment names
/// the harness root (a fake, a pane whose start was never recorded, the
/// detached mail courier, which redelivers until `horch inbox`
/// acknowledges). A process of those groups, or one that names the root,
/// that still runs after `grace` leaked: it gets SIGKILL and is
/// returned. The error is that the processes could not be listed within
/// `grace`.
#[cfg(unix)]
fn reap(root: &Path, log: &Path, grace: std::time::Duration) -> std::io::Result<Vec<String>> {
    use crate::process::{commands_with_environment, signal_groups, try_processes, Process};
    use horch_core::procid::is_same_group;
    let mut state = log.as_os_str().to_owned();
    state.push(".state.json");
    let state: Value = std::fs::read_to_string(&state)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Null);
    // With the separator: one harness root can be a prefix of another's.
    let mut marks = vec![format!("{}/", root.display())];
    if let Ok(canonical) = std::fs::canonicalize(root) {
        marks.push(format!("{}/", canonical.display()));
    }
    let me = std::process::id();
    // SAFETY: plain syscall without arguments.
    let own_group = u32::try_from(unsafe { libc::getpgrp() }).ok();
    let recorded: Vec<u32> = state["pids"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(_, pid)| u32::try_from(pid.as_u64()?).ok())
        .collect();
    let Some(all) =
        process_list_or_sandbox(try_processes, grace, &recorded, own_group, signal_groups)?
    else {
        return Ok(Vec::new());
    };
    let mut groups: Vec<u32> = state["pids"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(pane, pid)| {
            let pid = u32::try_from(pid.as_u64()?).ok()?;
            let started = state["started"][pane.as_str()].as_u64()?;
            is_same_group(pid, started).then_some(pid)
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
    // A group id stays ours while the group has members: no other process
    // can be given it.
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
    // The pids that ran when the first SIGTERM went out. With its group:
    // every process, in the group it was in, that has had one since.
    let first: Vec<u32> = all.iter().map(|p| p.pid).collect();
    let mut signalled: Vec<(u32, u32)> = all.iter().map(|p| (p.pid, p.pgid)).collect();
    loop {
        let left = left(&groups)?;
        if left.is_empty() {
            return Ok(Vec::new());
        }
        // A process can start after a SIGTERM went out, even in a group
        // that got it: a pane command that starts the mail courier on its
        // way out. A process can also leave its group before the SIGTERM
        // reaches it: the courier, forked into the pane's group, starts its
        // own session before it execs. Its new group gets SIGTERM too.
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
fn reap(_root: &Path, _log: &Path, _grace: std::time::Duration) -> std::io::Result<Vec<String>> {
    Ok(Vec::new())
}

/// The file in each harness root that names the test process that owns it:
/// `<pid> <start time>`.
const OWNER_FILE: &str = ".horch-e2e-owner";

/// `TMPDIR`, or the platform temp dir.
fn tmpdir() -> PathBuf {
    std::env::var_os("TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

/// The preferred parent of a harness root. On Linux it is not `/tmp`: the
/// sandbox (`bwrap`) mounts a new tmpfs on `/tmp`, so the "Sandbox
/// boundary" write next to a work folder under `/tmp` lands in that tmpfs
/// and is not refused, and each claude and codex profile is unhealthy.
#[cfg(target_os = "linux")]
const ROOT_PARENT: &str = "/var/tmp";
#[cfg(all(unix, not(target_os = "linux")))]
const ROOT_PARENT: &str = "/tmp";

/// The dirs that hold harness roots.
fn harness_parents() -> Vec<PathBuf> {
    #[cfg(unix)]
    return vec![PathBuf::from(ROOT_PARENT), tmpdir()];
    #[cfg(not(unix))]
    return vec![std::env::temp_dir()];
}

/// Record this test process as the owner of `root`.
fn write_owner(root: &Path) {
    let pid = std::process::id();
    let started = horch_core::procid::start_time(pid).unwrap_or(0);
    let _ = std::fs::write(root.join(OWNER_FILE), format!("{pid} {started}"));
}

/// Stop the processes of every harness root in `parent` whose owner test
/// process has ended. A test binary that is killed before teardown leaves
/// its processes: the detached mail courier redelivers for hours (audit
/// Q3). Teardown's [`reap`] finds them by the root in their environment.
/// The owner file is then removed, so each root is stopped once. The dir
/// stays: `HORCH_E2E_KEEP` can have kept it. Returns the processes that
/// outlived SIGTERM.
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

/// What `list` returns, tried again every 50 ms while it fails, for at most
/// `grace`. A `ps` that cannot fork on a loaded machine must not read as
/// "no process runs": teardown would then signal nothing, report no leak,
/// and leave a process running for hours.
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

/// List processes for the full leak scan. A sandbox can deny `ps`; then
/// signal the pane groups fake-herdr recorded and skip only the leak scan.
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

/// Build `horch` and every fake 1 time in each test process, before the
/// first use. `cargo test -p horch-e2e` does not build them, so without this
/// a test runs an old binary from an earlier build (T-291, T-294). The build
/// runs as the test process itself, outside the sealed environment. Many
/// test processes can build at the same time: cargo's own lock on the
/// target folder is the only lock. When nothing changed, cargo only checks.
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

/// The variables cargo sets for a test process, apart from `CARGO_PKG_*`.
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

/// The profile folder of the test binary `exe`: `<target>/<profile>` or
/// `<target>/<triple>/<profile>`, where cargo also puts `horch` and the
/// fakes. cargo 1.100 runs a test from
/// `<profile>/build/<package>/<hash>/out/`, an older cargo from
/// `<profile>/deps/` (T-59).
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

/// The cargo command that builds `horch` and the fakes into `built`, with
/// the profile, the target triple and the target folder of this test
/// process. `built` is `<root>/<profile>` for a host build, and
/// `<root>/<triple>/<profile>` for a build with `--target <triple>` or
/// `CARGO_BUILD_TARGET`. `triple` is the target of this test process.
fn build_command(built: &Path, triple: &str) -> Command {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut cmd = Command::new(cargo);
    // cargo sets these for the test process. A build that sees them has a
    // different fingerprint, and cargo builds `ring` and `horch` again.
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

/// The size of each built fake when this test process first saw it.
static BUILT_SIZES: std::sync::OnceLock<Vec<(PathBuf, u64)>> = std::sync::OnceLock::new();

/// Every built fake exists, is not empty, and has the size it had when this
/// test process started. The harness links them into `bin/`, so a test
/// that writes through a linked name changes the built file itself; this
/// fails that test instead of letting later tests run a broken fake.
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

/// Run every built fake once, untimed, before the first test uses it.
///
/// macOS scans a new executable at its first exec (syspolicyd `GK
/// performScan` and an XProtect pass). The scans run one at a time, about
/// 0.2 s each, so with 30 new executables at once the last waits 5.5 s
/// (T-24). The harness links the built fakes into `bin/`, so the first call
/// of a fake inside a test would carry that wait into a timed probe. The
/// first harness of a test process pays the scan here, and the others wait
/// on the same [`std::sync::Once`]. The exit status does not matter: a fake
/// answers an unknown argument at once and writes no log without
/// `HORCH_FAKE_LOG`.
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

/// The one deadline of every e2e wait. A passing run never reaches it, so it
/// is generous: under the parallel full suite on a 4-CPU Linux VM, one step
/// of a debug build can take 10 times as long as it takes alone.
pub const DEADLINE: Duration = Duration::from_secs(120);

/// Poll `f` until it returns a value, for at most [`DEADLINE`].
pub fn wait_for<T>(what: &str, f: impl FnMut() -> Option<T>) -> T {
    wait_for_or(what, f, String::new)
}

/// [`wait_for`], with `context` in the panic message.
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

/// Link a built fake into a harness `bin/`: a symlink on unix, a hard link
/// elsewhere.
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
        build_command, built_dir_of, cargo_profile, choose_harness_root, listed,
        process_list_or_sandbox, reap, reap_grace, stop_stale_harnesses, OWNER_FILE,
    };

    /// T-842: the harness builds `horch` and the fakes before their first
    /// use, so no binary is older than a source file it is built from.
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

    /// T-842 F1: a test built with `--target <triple>` or with
    /// `CARGO_BUILD_TARGET` runs from `<root>/<triple>/<profile>`. The
    /// nested build uses the same triple and the true target root.
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
        // A host build of the same triple keeps the host layout.
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

    /// A `ps` that failed to fork once is asked again, so teardown still
    /// sees the processes.
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

    /// A `ps` that never runs is an error after the grace, never an empty
    /// list that reads as "nothing runs".
    #[test]
    fn a_list_that_keeps_failing_is_an_error_after_the_grace() {
        let got = listed(
            || Err::<Vec<u32>, _>(std::io::Error::other("fork failed")),
            Duration::from_millis(200),
        );
        assert_eq!(got.unwrap_err().to_string(), "fork failed");
    }

    /// A detached mail courier with pending orchestrator mail in
    /// `root/state`, started as horch starts it: in its own process group,
    /// with the root only in its environment. Returns its pid, read from
    /// its lock.
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
        // Never the test's own group: teardown does not signal that.
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
        assert!(cmd.status().unwrap().success());
        let lock = state.join("mail/w1/courier.lock");
        super::wait_for("the courier to hold its lock", || {
            let out = std::process::Command::new("lsof")
                .arg("-t")
                .arg(&lock)
                .output()
                .ok()?;
            String::from_utf8_lossy(&out.stdout).trim().parse().ok()
        })
    }

    /// Q3: a harness root whose owner test process has ended gets its
    /// processes stopped; a root whose owner runs is left alone. Both
    /// owners are this process's pid. Only the start time differs, so no
    /// pid can be reused by another process during the test.
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
