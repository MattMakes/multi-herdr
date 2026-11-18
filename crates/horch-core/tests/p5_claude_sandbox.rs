//! The operator boundary of the Claude Code adapter (SR-8, FM-32, RR-5;
//! ticket 35 of fleetd-p5): the settings of a sandboxed worker deny reading
//! the operator token, connecting to the command channel and writing into
//! the run folder and the ledger, and allow writes only in the task folder
//! and the worktree.
//!
//! The real `claude` never runs here. The tests read the `--settings` text
//! of the built command, and the attack test runs a child under the
//! platform sandbox with the same rules (ticket 06).

use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::fleetd::worker_profiles::domain::operator_boundary::{Act, OperatorPaths};
use horch_core::harness::claude::{operator_boundary, operator_boundary_on};
use horch_core::harness::{CommandSpec, HarnessKind, LaunchEnv, Session};
use horch_core::roster::Teammate;
use horch_core::runtime::{MapEnv, RuntimeContext};
use serde_json::{json, Value};

/// A test fleet in `/tmp` (a socket path has a short limit): a home, a
/// state root, the ledger folder with the fleet root in it (the default
/// layout), the run folder with the token and the request folder, a task
/// folder and a worktree.
struct Fleet {
    root: tempfile::TempDir,
    paths: OperatorPaths,
    fleet_root: PathBuf,
    ledger_db: PathBuf,
    task_dir: PathBuf,
    worktree: PathBuf,
}

fn fleet() -> Fleet {
    use std::os::unix::fs::DirBuilderExt;
    let root = tempfile::tempdir_in("/tmp").unwrap();
    let ledger_dir = root.path().join("ledger");
    let fleet_root = ledger_dir.join("fleet");
    let paths = OperatorPaths::of(&fleet_root);
    let task_dir = fleet_root.join("tasks/T-1/attempts/A1");
    let worktree = fleet_root.join("work/T-1");
    for dir in [&task_dir, &worktree, &root.path().join("bin")] {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&paths.run)
        .unwrap();
    std::fs::create_dir(&paths.requests).unwrap();
    std::fs::write(&paths.token, "secret\n").unwrap();
    Fleet {
        ledger_db: ledger_dir.join("ledger.sqlite3"),
        root,
        paths,
        fleet_root,
        task_dir,
        worktree,
    }
}

impl Fleet {
    /// The launch environment of a worker of this fleet. `state` is the
    /// state root. The `PATH` has stub Linux sandbox tools, so the builder
    /// runs on Linux too.
    fn env(&self, state: &Path) -> LaunchEnv {
        let home = self.root.path().join("home");
        let bin = self.root.path().join("bin");
        for tool in ["bwrap", "socat"] {
            let path = bin.join(tool);
            std::fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
            std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o755))
                .unwrap();
        }
        let s = |p: &Path| p.to_string_lossy().into_owned();
        let env = MapEnv::new(&home)
            .with("HOME", &s(&home))
            .with("PATH", &s(&bin))
            .with("HORCH_STATE_DIR", &s(state))
            .with("HORCH_LEDGER_DB", &s(&self.ledger_db))
            .with("FLEET_ROOT", &s(&self.fleet_root))
            .with("HERDR_SOCKET_PATH", &s(&home.join("herdr.sock")))
            .with(
                "HORCH_CLAUDE_MANAGED_SETTINGS",
                &s(&home.join("no-managed-settings.json")),
            );
        let mut env = LaunchEnv::from_context(&RuntimeContext::from_env(&env).unwrap());
        env.task_dir = Some(self.task_dir.clone());
        env.worktree = Some(self.worktree.clone());
        env
    }

    /// The state root outside the fleet root.
    fn state(&self) -> PathBuf {
        self.root.path().join("state")
    }

    /// The `--settings` object of a worker launch with `block` as its
    /// `sandbox:` block.
    fn settings(&self, block: Value, state: &Path) -> Value {
        let t = worker(block);
        let cmd = HarnessKind::Claude
            .adapter()
            .build_command(
                &self.env(state),
                &CommandSpec {
                    teammate: &t,
                    session: Session::Fresh("id"),
                    prompt: "go",
                    model_override: None,
                },
            )
            .unwrap();
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let i = args.iter().position(|a| a == "--settings").unwrap();
        serde_json::from_str(&args[i + 1]).unwrap()
    }
}

fn worker(block: Value) -> Teammate {
    Teammate {
        name: "boxed-worker".into(),
        model: Some("opus".into()),
        sandbox: Some(block.as_object().unwrap().clone()),
        ..Teammate::default()
    }
}

/// The paths of a list of the settings.
fn paths(list: &Value) -> Vec<PathBuf> {
    list.as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_default()
}

/// Whether an entry of `list` is `path` or a folder that holds it.
fn covers(list: &Value, path: &Path) -> bool {
    paths(list).iter().any(|entry| path.starts_with(entry))
}

/// Whether a permission deny rule of `tool` closes `path`, as
/// `Tool(//<path>)` or `Tool(//<folder of path>/**)`.
fn rule_denies(settings: &Value, tool: &str, path: &Path) -> bool {
    let rules = settings["permissions"]["deny"].as_array().unwrap();
    rules.iter().filter_map(Value::as_str).any(|rule| {
        let Some(inner) = rule
            .strip_prefix(&format!("{tool}(/"))
            .and_then(|r| r.strip_suffix(')'))
        else {
            return false;
        };
        match inner.strip_suffix("/**") {
            Some(dir) => path.starts_with(dir),
            None => Path::new(inner) == path,
        }
    })
}

#[test]
fn claude_worker_settings_deny_reading_the_token() {
    let f = fleet();
    // The teammate opens the whole fleet root to read: the deny still wins.
    let block = json!({"filesystem": {"allowRead": [f.fleet_root]}});
    let settings = f.settings(block.clone(), &f.state());
    let fs = &settings["sandbox"]["filesystem"];
    assert!(covers(&fs["denyRead"], &f.paths.token), "{settings}");
    assert!(
        !paths(&fs["allowRead"])
            .iter()
            .any(|p| p.starts_with(&f.paths.run)),
        "an allowRead entry in the run folder re-opens it: {settings}"
    );
    assert!(rule_denies(&settings, "Read", &f.paths.token), "{settings}");
    let boundary = operator_boundary(&worker(block), &f.fleet_root).unwrap();
    assert!(boundary.denies(Act::ReadToken), "{boundary:?}");
}

#[test]
fn claude_worker_settings_never_allow_the_channel_socket() {
    let f = fleet();
    // The state root holds the fleet root, and the teammate asks for the
    // socket, its run folder and every socket.
    let state = f.fleet_root.parent().unwrap().to_path_buf();
    let block = json!({"network": {
        "allowUnixSockets": [f.paths.channel, f.paths.run, "/var/run/other.sock"],
        "allowAllUnixSockets": true
    }});
    let settings = f.settings(block.clone(), &state);
    let net = &settings["sandbox"]["network"];
    assert!(
        !covers(&net["allowUnixSockets"], &f.paths.channel),
        "{settings}"
    );
    assert!(
        paths(&net["allowUnixSockets"]).contains(&PathBuf::from("/var/run/other.sock")),
        "a socket outside the run folder stays: {settings}"
    );
    assert_eq!(net["allowAllUnixSockets"], json!(false), "{settings}");
    // Linux ignores allowUnixSockets: the deny of the read hides the socket.
    assert!(
        covers(
            &settings["sandbox"]["filesystem"]["denyRead"],
            &f.paths.channel
        ),
        "{settings}"
    );
    let boundary = operator_boundary(&worker(block), &f.fleet_root).unwrap();
    assert!(boundary.denies(Act::ConnectChannel), "{boundary:?}");

    // The relay folder under a state root outside the fleet root stays.
    let settings = f.settings(json!({}), &f.state());
    assert!(
        covers(
            &settings["sandbox"]["network"]["allowUnixSockets"],
            &f.state().join("channels/w1.sock")
        ),
        "{settings}"
    );
}

#[test]
fn claude_worker_settings_deny_writing_the_run_folder_and_the_ledger() {
    let f = fleet();
    // The teammate opens the whole ledger folder to write: the deny wins.
    let ledger_dir = f.ledger_db.parent().unwrap().to_path_buf();
    let block = json!({"filesystem": {"allowWrite": [ledger_dir]}});
    let settings = f.settings(block.clone(), &f.state());
    let deny = &settings["sandbox"]["filesystem"]["denyWrite"];
    let ledger = f.ledger_db.to_string_lossy().into_owned();
    let ledger_files =
        ["", "-wal", "-shm", "-journal"].map(|s| PathBuf::from(format!("{ledger}{s}")));
    for path in [&f.paths.run, &f.paths.requests, &f.paths.token]
        .into_iter()
        .chain(&ledger_files)
    {
        assert!(
            covers(deny, path),
            "{} is writable: {settings}",
            path.display()
        );
        assert!(rule_denies(&settings, "Edit", path), "{settings}");
    }
    // The fleet root is in the ledger folder, so a deny of the whole folder
    // would close the task folder and the worktree.
    assert!(!covers(deny, &f.task_dir), "{settings}");
    assert!(!covers(deny, &f.worktree), "{settings}");
    let boundary = operator_boundary(&worker(block), &f.fleet_root).unwrap();
    assert!(boundary.denies(Act::WriteRequests), "{boundary:?}");
}

#[test]
fn claude_worker_writes_only_the_task_folder_and_the_worktree() {
    let f = fleet();
    let settings = f.settings(json!({}), &f.state());
    assert_eq!(
        paths(&settings["sandbox"]["filesystem"]["allowWrite"]),
        [f.task_dir.clone(), f.worktree.clone()],
        "{settings}"
    );
    let boundary = operator_boundary(&worker(json!({})), &f.fleet_root).unwrap();
    assert!(boundary.holds(), "{boundary:?}");
}

#[test]
fn claude_boundary_is_not_supported_on_windows() {
    let t = worker(json!({}));
    let fleet_root = Path::new("/fleet");
    let gap = operator_boundary_on("windows", &t, fleet_root).unwrap_err();
    assert_eq!(gap.adapter, "claude");
    assert_eq!(gap.os, "windows");
    for os in ["macos", "linux"] {
        let boundary = operator_boundary_on(os, &t, fleet_root).unwrap();
        assert!(boundary.holds(), "{os}: {boundary:?}");
    }
}

/// A child under the platform sandbox, with the rules of the rendered
/// settings: `allowWrite` writable, `denyRead` denied to read (and so to
/// connect), `denyWrite` denied to write. It tries each act and is refused
/// by the OS, and it still writes its task folder.
#[test]
#[cfg(target_os = "macos")]
fn claude_rules_under_the_platform_sandbox_refuse_the_3_acts() {
    use horch_platform::channel::Listener;
    use horch_platform::sandbox::{sandboxed, Denials};

    let f = fleet();
    let settings = f.settings(json!({}), &f.state());
    let fs = &settings["sandbox"]["filesystem"];
    let writable = paths(&fs["allowWrite"]);
    let denied = Denials {
        read: paths(&fs["denyRead"]),
        connect: vec![],
        write: paths(&fs["denyWrite"]),
    };
    let run = |script: &str, arg: &Path| {
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c").arg(script).arg("sh").arg(arg);
        sandboxed(cmd, &writable, &denied)
            .expect("macOS has sandbox-exec")
            .output()
            .unwrap()
    };
    let refused = |out: &std::process::Output| {
        let stderr = String::from_utf8_lossy(&out.stderr);
        !out.status.success() && stderr.contains("Operation not permitted")
    };

    let read = run("cat \"$1\"", &f.paths.token);
    assert!(refused(&read), "{read:?}");
    assert!(!String::from_utf8_lossy(&read.stdout).contains("secret"));

    let _listener = Listener::bind(&f.paths.channel).unwrap();
    let connect = run(
        "exec perl -MIO::Socket::UNIX -e \
         'IO::Socket::UNIX->new(Peer => $ARGV[0]) or do { print STDERR \"$!\\n\"; exit 3 }; exit 0' \
         \"$1\"",
        &f.paths.channel,
    );
    assert_eq!(connect.status.code(), Some(3), "{connect:?}");
    assert!(refused(&connect), "{connect:?}");

    let request = f.paths.requests.join("r.json");
    let write = run("echo '{}' > \"$1\"", &request);
    assert!(refused(&write), "{write:?}");
    assert!(!request.exists());

    let ledger = run("echo x >> \"$1\"", &f.ledger_db);
    assert!(refused(&ledger), "{ledger:?}");
    assert!(!f.ledger_db.exists());

    let progress = f.task_dir.join("outbox-probe");
    let allowed = run("echo ok > \"$1\"", &progress);
    assert!(allowed.status.success(), "{allowed:?}");
    assert_eq!(std::fs::read_to_string(&progress).unwrap(), "ok\n");
}

/// With the default layout (the fleet root in the ledger folder), a child
/// under the platform sandbox with the rendered rules cannot write or
/// replace (rename over) the ledger file or its `-wal`, `-shm` and
/// `-journal` files, even when the teammate opens the ledger folder, and
/// it can write its task folder.
#[test]
#[cfg(target_os = "macos")]
fn claude_worker_cannot_write_or_replace_the_ledger_files() {
    use horch_platform::sandbox::{sandboxed, Denials};

    let f = fleet();
    let ledger_dir = f.ledger_db.parent().unwrap().to_path_buf();
    assert!(f.fleet_root.starts_with(&ledger_dir));
    // The teammate opens the whole ledger folder to write: the deny wins.
    let block = json!({"filesystem": {"allowWrite": [ledger_dir]}});
    let settings = f.settings(block, &f.state());
    let fs = &settings["sandbox"]["filesystem"];
    let writable = paths(&fs["allowWrite"]);
    let denied = Denials {
        read: paths(&fs["denyRead"]),
        connect: vec![],
        write: paths(&fs["denyWrite"]),
    };
    let run = |script: &str, args: &[&Path]| {
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c").arg(script).arg("sh").args(args);
        sandboxed(cmd, &writable, &denied)
            .expect("macOS has sandbox-exec")
            .output()
            .unwrap()
    };
    let ledger = f.ledger_db.to_string_lossy().into_owned();
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let file = PathBuf::from(format!("{ledger}{suffix}"));
        std::fs::write(&file, "ledger\n").unwrap();
        let append = run("echo x >> \"$1\"", &[&file]);
        assert!(!append.status.success(), "{suffix}: {append:?}");
        let new = f.task_dir.join(format!("new{suffix}"));
        let replace = run("echo y > \"$1\" && mv -f \"$1\" \"$2\"", &[&new, &file]);
        assert!(!replace.status.success(), "{suffix}: {replace:?}");
        let remove = run("rm -f \"$1\"", &[&file]);
        assert!(!remove.status.success(), "{suffix}: {remove:?}");
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "ledger\n",
            "{suffix}"
        );
    }
    let probe = f.task_dir.join("probe");
    let allowed = run("echo ok > \"$1\"", &[&probe]);
    assert!(allowed.status.success(), "{allowed:?}");
    assert_eq!(std::fs::read_to_string(&probe).unwrap(), "ok\n");
}
