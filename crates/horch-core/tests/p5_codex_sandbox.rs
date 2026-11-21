#![cfg(unix)]

use std::path::{Path, PathBuf};

use horch_core::fleetd::worker_profiles::domain::operator_boundary::{Act, OperatorPaths};
use horch_core::harness::codex::{operator_boundary_on, CodexFacts, OPERATOR_BOUNDARY_SINCE};
use horch_core::harness::{CommandSpec, HarnessKind, LaunchEnv, PrepareRequest, Session};
use horch_core::roster::{ExecDecision, Roster, Teammate};
use horch_core::runtime::{MapEnv, RuntimeContext};

struct Fleet {
    root: tempfile::TempDir,
    paths: OperatorPaths,
    fleet_root: PathBuf,
    task_dir: PathBuf,
    worktree: PathBuf,
}

fn fleet() -> Fleet {
    let root = tempfile::tempdir().unwrap();
    let fleet_root = root.path().join("fleet");
    let task_dir = fleet_root.join("tasks/T-1/attempts/A1");
    let worktree = fleet_root.join("work/T-1");
    for dir in [&task_dir, &worktree, &root.path().join("home/.codex")] {
        std::fs::create_dir_all(dir).unwrap();
    }
    Fleet {
        paths: OperatorPaths::of(&fleet_root),
        root,
        fleet_root,
        task_dir,
        worktree,
    }
}

impl Fleet {
    fn ctx(&self) -> RuntimeContext {
        let env = MapEnv::new(self.root.path())
            .with("HOME", &self.root.path().join("home").to_string_lossy())
            .with(
                "HORCH_STATE_DIR",
                &self.root.path().join("state").to_string_lossy(),
            );
        RuntimeContext::from_env(&env).unwrap()
    }

    fn argv(&self, teammate: &Teammate) -> Vec<String> {
        let ctx = self.ctx();
        let codex = HarnessKind::Codex.adapter();
        let prepared = codex
            .prepare(
                &ctx,
                &PrepareRequest {
                    role: "codex-1",
                    exec_rules: Roster::builtin().unwrap().exec_rules(),
                    skills: None,
                    compact_window: None,
                    teammate,
                    model: "gpt-5.6-sol",
                    workdir: &self.worktree,
                    shell_env: &[],
                },
            )
            .unwrap();
        let mut teammate = teammate.clone();
        teammate.args.extend(prepared.extra_args.iter().cloned());
        let mut env = LaunchEnv::from_context(&ctx);
        env.fleet_root = Some(self.fleet_root.clone());
        env.task_dir = Some(self.task_dir.clone());
        env.worktree = Some(self.worktree.clone());
        let cmd = codex
            .command(
                &env,
                &CommandSpec {
                    teammate: &teammate,
                    session: Session::Unmanaged,
                    prompt: "do the task",
                    model_override: None,
                },
            )
            .unwrap();
        cmd.get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }
}

fn last_pair(argv: &[String], key: &str) -> Option<toml::Value> {
    argv.windows(2)
        .filter(|w| w[0] == "-c")
        .filter_map(|w| w[1].split_once('='))
        .rfind(|(k, _)| k.trim() == key)
        .map(|(_, v)| {
            let table: toml::Table =
                toml::from_str(&format!("v = {v}")).unwrap_or_else(|e| panic!("{key}={v}: {e}"));
            table["v"].clone()
        })
}

fn path_str(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn sol() -> Teammate {
    Roster::builtin()
        .unwrap()
        .require("codex-sol")
        .unwrap()
        .clone()
}

fn facts(version: &str, network_proxy: bool) -> CodexFacts {
    CodexFacts {
        version: Some(format!("codex-cli {version}")),
        network_proxy,
    }
}

#[test]
fn codex_worker_rules_have_no_ledger_allow_rule() {
    let roster = Roster::builtin().unwrap();
    let rules = roster.exec_rules();
    let ledger_allows: Vec<&str> = rules
        .iter()
        .filter(|r| r.decision == ExecDecision::Allow)
        .filter(|r| r.pattern.starts_with(r#""horch", "ledger""#))
        .map(|r| r.pattern.as_str())
        .collect();
    assert!(ledger_allows.is_empty(), "{ledger_allows:?}");
    for forbidden in [
        r#""horch", "ledger", "add""#,
        r#""horch", "ledger", "add-map""#,
        r#""horch", "ledger", "reopen""#,
    ] {
        assert!(
            rules
                .iter()
                .any(|r| r.pattern == forbidden && r.decision == ExecDecision::Forbidden),
            "{forbidden} must stay forbidden"
        );
    }
    assert!(rules
        .iter()
        .any(|r| r.pattern == r#""horch", "tell""# && r.decision == ExecDecision::Allow));
}

#[test]
fn codex_writable_roots_are_the_task_folder_and_the_worktree() {
    let f = fleet();
    let argv = f.argv(&sol());
    assert_eq!(
        last_pair(&argv, "default_permissions"),
        Some(toml::Value::String("fleet_write".into())),
        "{argv:?}"
    );
    let roots = last_pair(&argv, "permissions.fleet_write.workspace_roots").expect("roots");
    let mut roots: Vec<&str> = roots
        .as_table()
        .unwrap()
        .iter()
        .filter(|(_, on)| on.as_bool() == Some(true))
        .map(|(path, _)| path.as_str())
        .collect();
    roots.sort_unstable();
    let mut want = vec![path_str(&f.task_dir), path_str(&f.worktree)];
    want.sort_unstable();
    assert_eq!(roots, want, "{argv:?}");

    let fs = last_pair(&argv, "permissions.fleet_write.filesystem").expect("filesystem");
    for (path, access) in fs.as_table().unwrap() {
        if !path.starts_with(':') {
            assert_ne!(access.as_str(), Some("write"), "{path} in {fs}");
        }
    }

    let tmpdir = last_pair(&argv, "shell_environment_policy.set.TMPDIR").expect("TMPDIR");
    let tmpdir = PathBuf::from(tmpdir.as_str().unwrap());
    assert!(tmpdir.starts_with(&f.task_dir), "{tmpdir:?}");
    assert!(tmpdir.is_dir(), "the launch makes {tmpdir:?}");
}

#[test]
fn codex_boundary_denies_the_3_acts_or_is_not_supported() {
    let f = fleet();
    let argv = f.argv(&sol());

    let fs = last_pair(&argv, "permissions.fleet_write.filesystem").unwrap();
    assert_eq!(
        fs.get(path_str(&f.paths.run)).and_then(toml::Value::as_str),
        Some("none"),
        "{fs}"
    );
    assert_eq!(
        last_pair(&argv, "features.network_proxy"),
        Some(toml::Value::Boolean(true))
    );
    let net = last_pair(&argv, "permissions.fleet_write.network").unwrap();
    assert_ne!(
        net.get("dangerously_allow_all_unix_sockets")
            .and_then(toml::Value::as_bool),
        Some(true)
    );
    let sockets = net["unix_sockets"].as_table().unwrap();
    for (path, rule) in sockets {
        if rule.as_str() == Some("allow") {
            assert!(
                !f.paths.channel.starts_with(path),
                "{path} opens the channel"
            );
            assert!(
                Path::new(path).starts_with(&f.task_dir)
                    || Path::new(path).starts_with(&f.worktree),
                "{path}"
            );
        }
    }

    let proven = facts(OPERATOR_BOUNDARY_SINCE, true);
    for os in ["macos", "linux"] {
        let b = operator_boundary_on(os, &proven, &sol(), &f.fleet_root).unwrap();
        for act in Act::ALL {
            assert!(b.denies(act), "{os}: {act} is not denied");
        }
        let newer = operator_boundary_on(os, &facts("0.170.2", true), &sol(), &f.fleet_root);
        assert!(newer.unwrap().holds());
    }

    let refused = |os: &str, facts: &CodexFacts| {
        let err = operator_boundary_on(os, facts, &sol(), &f.fleet_root).unwrap_err();
        assert_eq!(err.adapter, "codex");
        assert_eq!(err.os, os);
        err.reason
    };
    assert!(refused("windows", &proven).contains("Windows"));
    assert!(refused("macos", &facts("0.166.9", true)).contains(OPERATOR_BOUNDARY_SINCE));
    let unknown = CodexFacts {
        version: None,
        network_proxy: true,
    };
    assert!(refused("linux", &unknown).contains(OPERATOR_BOUNDARY_SINCE));
    assert!(refused("macos", &facts("0.167.0", false)).contains("network_proxy"));

    let mut yolo = sol();
    yolo.args.push("--yolo".into());
    let b = operator_boundary_on("macos", &proven, &yolo, &f.fleet_root).unwrap();
    assert_eq!(b.allowed(), Act::ALL.to_vec());

    let mut reopen = sol();
    reopen.args.extend([
        "-c".into(),
        format!(
            "permissions.fleet_write.network.unix_sockets={{{}=\"allow\"}}",
            toml::Value::String(path_str(&f.paths.run))
        ),
    ]);
    assert!(
        operator_boundary_on("macos", &proven, &reopen, &f.fleet_root)
            .unwrap()
            .holds()
    );

    let roster = Roster::builtin().unwrap();
    let seat = roster.require("orchestrator-codex").unwrap();
    let b = operator_boundary_on("macos", &proven, seat, &f.fleet_root).unwrap();
    assert!(!b.holds());
}
