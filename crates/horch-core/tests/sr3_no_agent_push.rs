//! SR-3 (ticket 22 of fleetd-p4): no agent pushes. Each worker gets a git
//! hooks path whose `pre-push` hook refuses, and the profiles of Claude Code
//! and Codex deny `git push`, `gh pr merge` and `gh release`. They also deny
//! the external messages: `gh issue comment`, `gh pr comment`,
//! `gh pr review` and `gh discussion`. horch's own
//! git calls set `-c core.hooksPath=/dev/null`, and a `-c` value wins over
//! the `GIT_CONFIG_*` values of the environment, so the old finish path
//! still pushes. The other hooks of the repository still run for an agent.
//!
//! These tests run git in temporary repositories with no global or system
//! config. The real `claude` and `codex` never run.

use std::path::Path;

use horch_core::fleetd::worker_profiles::domain::launch_env::{
    git_env, hooks_dir, DENIED_COMMANDS, PRE_PUSH_REFUSAL,
};
use horch_core::harness::{CommandSpec, HarnessKind, LaunchEnv, Session};
use horch_core::roster::Teammate;
use horch_core::runtime::{MapEnv, RuntimeContext};

/// The environment variable that tells the child test where its repository is.
const CHILD_REPO: &str = "SR3_CHILD_REPO";

/// `git <args>` in `dir`, with no global or system config and no git
/// variable of this process. `extra` is added last.
fn git(dir: &Path, args: &[&str], extra: &[(String, String)]) -> std::process::Output {
    let mut cmd = std::process::Command::new("git");
    cmd.current_dir(dir).args(args);
    for (key, _) in std::env::vars() {
        if key.starts_with("GIT_") {
            cmd.env_remove(key);
        }
    }
    cmd.env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .envs(extra.iter().map(|(k, v)| (k, v)));
    cmd.output().unwrap()
}

fn ok(out: &std::process::Output) -> bool {
    out.status.success()
}

fn show(out: &std::process::Output) -> String {
    format!(
        "status {:?}\nstdout: {}\nstderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// A repository `work` with 1 commit on `main` and the bare remote
/// `remote.git` as `origin`, both under `root`.
fn repo_with_remote(root: &Path) -> std::path::PathBuf {
    let work = root.join("work");
    let remote = root.join("remote.git");
    std::fs::create_dir_all(&work).unwrap();
    for (dir, args) in [
        (root, &["init", "-q", "--bare", "remote.git"][..]),
        (&work, &["init", "-q", "-b", "main"]),
        (&work, &["config", "user.name", "t"]),
        (&work, &["config", "user.email", "t@example.com"]),
        (
            &work,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        ),
    ] {
        let out = git(dir, args, &[]);
        assert!(ok(&out), "{args:?}: {}", show(&out));
    }
    std::fs::write(work.join("a.txt"), "a\n").unwrap();
    for args in [&["add", "a.txt"][..], &["commit", "-q", "-m", "first"]] {
        let out = git(&work, args, &[]);
        assert!(ok(&out), "{args:?}: {}", show(&out));
    }
    work
}

/// Whether the remote of `root` has the branch `main`.
fn remote_has_main(root: &Path) -> bool {
    ok(&git(
        &root.join("remote.git"),
        &["rev-parse", "--verify", "-q", "refs/heads/main"],
        &[],
    ))
}

/// The agent hooks of a fleet root under `root`, written as a launch
/// writes them, and the environment of an agent that uses them.
fn agent_hooks(root: &Path) -> Vec<(String, String)> {
    let fleet_root = root.join("fleet");
    let dir = horch_core::harness::launch::write_agent_hooks(&fleet_root).unwrap();
    assert_eq!(dir, hooks_dir(&fleet_root));
    git_env(&fleet_root).to_vec()
}

#[test]
fn agent_push_is_refused_by_the_pre_push_hook() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo_with_remote(tmp.path());
    let env = agent_hooks(tmp.path());

    let out = git(&work, &["push", "origin", "main"], &env);

    assert!(!ok(&out), "{}", show(&out));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains(PRE_PUSH_REFUSAL),
        "{}",
        show(&out)
    );
    assert!(!remote_has_main(tmp.path()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let hook = hooks_dir(&tmp.path().join("fleet")).join("pre-push");
        let mode = std::fs::metadata(hook).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
    }
}

/// The repository's own hooks still run for an agent: its `pre-commit`
/// stops a commit, from `.git/hooks` and from its own `core.hooksPath`.
#[test]
fn a_failing_project_pre_commit_hook_stops_an_agent_commit() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo_with_remote(tmp.path());
    let env = agent_hooks(tmp.path());
    let failing = "#!/bin/sh\necho project pre-commit refuses >&2\nexit 3\n";
    let write_hook = |path: &Path| {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, failing).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    };
    let commit = |name: &str| {
        std::fs::write(work.join(name), "x\n").unwrap();
        assert!(ok(&git(&work, &["add", name], &env)));
        git(&work, &["commit", "-q", "-m", name], &env)
    };

    write_hook(&work.join(".git/hooks/pre-commit"));
    let out = commit("b.txt");
    assert!(!ok(&out), "{}", show(&out));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("project pre-commit refuses"),
        "{}",
        show(&out)
    );

    std::fs::remove_file(work.join(".git/hooks/pre-commit")).unwrap();
    assert!(ok(&commit("c.txt")), "no project hook: the commit passes");

    write_hook(&work.join("own-hooks/pre-commit"));
    assert!(ok(&git(
        &work,
        &["config", "core.hooksPath", "own-hooks"],
        &[]
    )));
    let out = commit("d.txt");
    assert!(!ok(&out), "{}", show(&out));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("project pre-commit refuses"),
        "{}",
        show(&out)
    );
}

/// The old path of a claim attempt pushes through horch's own git calls,
/// which set `-c core.hooksPath=/dev/null`. In the environment of an agent
/// (a worker runs `horch ledger finish` in its shell), that push still
/// works: as a plain git call and through `vcs::network::output`.
#[test]
fn horch_old_path_push_with_dev_null_hooks_still_works() {
    let tmp = tempfile::tempdir().unwrap();
    let work = repo_with_remote(tmp.path());
    let env = agent_hooks(tmp.path());

    let out = git(
        &work,
        &[
            "-c",
            "core.hooksPath=/dev/null",
            "push",
            "-q",
            "origin",
            "main",
        ],
        &env,
    );
    assert!(ok(&out), "{}", show(&out));
    assert!(remote_has_main(tmp.path()));
    assert!(ok(&git(
        &tmp.path().join("remote.git"),
        &["update-ref", "-d", "refs/heads/main"],
        &[]
    )));

    // The child test runs `network::output` with the agent environment as
    // its own process environment.
    let mut child = std::process::Command::new(std::env::current_exe().unwrap());
    child
        .args([
            "--exact",
            "network_push_child",
            "--ignored",
            "--test-threads=1",
        ])
        .env(CHILD_REPO, &work)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .envs(env.iter().map(|(k, v)| (k, v)));
    let out = child.output().unwrap();
    assert!(
        ok(&out) && String::from_utf8_lossy(&out.stdout).contains("1 passed"),
        "{}",
        show(&out)
    );
    assert!(remote_has_main(tmp.path()));
}

#[test]
#[ignore = "run by horch_old_path_push_with_dev_null_hooks_still_works, with the agent environment"]
fn network_push_child() {
    let repo = std::env::var_os(CHILD_REPO).expect(CHILD_REPO);
    assert_eq!(
        std::env::var("GIT_CONFIG_KEY_0").as_deref(),
        Ok("core.hooksPath")
    );
    let out = horch_core::vcs::network::output(
        Path::new("git"),
        Path::new(&repo),
        &["push", "-q", "origin", "main"],
    )
    .unwrap();
    assert_eq!(out.status, 0, "{}", String::from_utf8_lossy(&out.stderr));
}

/// The launch environment of a test fleet under `root`.
fn launch_env(root: &Path) -> LaunchEnv {
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let s = |p: &Path| p.to_string_lossy().into_owned();
    let env = MapEnv::new(&home)
        .with("HOME", &s(&home))
        .with("HORCH_STATE_DIR", &s(&root.join("state")))
        .with("FLEET_ROOT", &s(&root.join("fleet")))
        .with(
            "HORCH_CLAUDE_MANAGED_SETTINGS",
            &s(&home.join("no-managed-settings.json")),
        );
    LaunchEnv::from_context(&RuntimeContext::from_env(&env).unwrap())
}

/// The `permissions.deny` list of the `--settings` overlay of a Claude
/// launch of `teammate`.
fn claude_deny(root: &Path, teammate: &Teammate) -> Vec<String> {
    let cmd = HarnessKind::Claude
        .adapter()
        .build_command(
            &launch_env(root),
            &CommandSpec {
                teammate,
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
    let settings: serde_json::Value = serde_json::from_str(&args[i + 1]).unwrap();
    settings["permissions"]["deny"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn claude_profile_denies_git_push_and_external_messages_by_default() {
    let tmp = tempfile::tempdir().unwrap();
    let worker = Teammate {
        name: "plain-worker".into(),
        model: Some("opus".into()),
        ..Teammate::default()
    };
    let deny = claude_deny(tmp.path(), &worker);
    for rule in [
        "Bash(git push:*)",
        "Bash(gh pr merge:*)",
        "Bash(gh release:*)",
        "Bash(gh issue comment:*)",
        "Bash(gh pr comment:*)",
        "Bash(gh pr review:*)",
        "Bash(gh discussion:*)",
    ] {
        assert!(deny.iter().any(|d| d == rule), "{rule} not in {deny:?}");
    }
    assert_eq!(DENIED_COMMANDS.len(), 7);
}

/// The rendered rules of a Codex worker launch: its private `CODEX_HOME`
/// holds `rules/horch.rules`, and each denied command has a `forbidden`
/// `prefix_rule`. Windows shares 1 rules file and gets no forbidden rule.
#[cfg(unix)]
#[test]
fn codex_profile_forbids_git_push_and_external_messages_by_default() {
    use horch_core::harness::PrepareRequest;
    use horch_core::roster::Roster;

    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("home/.codex")).unwrap();
    let worktree = tmp.path().join("work");
    std::fs::create_dir_all(&worktree).unwrap();
    let s = |p: &Path| p.to_string_lossy().into_owned();
    let ctx = RuntimeContext::from_env(
        &MapEnv::new(tmp.path())
            .with("HOME", &s(&tmp.path().join("home")))
            .with("HORCH_STATE_DIR", &s(&tmp.path().join("state"))),
    )
    .unwrap();
    let roster = Roster::builtin().unwrap();
    let teammate = roster.require("codex-sol").unwrap();
    let prepared = HarnessKind::Codex
        .adapter()
        .prepare(
            &ctx,
            &PrepareRequest {
                role: "codex-1",
                exec_rules: roster.exec_rules(),
                skills: None,
                compact_window: None,
                teammate,
                model: "gpt-5.6-sol",
                workdir: &worktree,
                shell_env: &[],
            },
        )
        .unwrap();
    let home = prepared
        .env
        .iter()
        .find(|(k, _)| k == "CODEX_HOME")
        .map(|(_, v)| std::path::PathBuf::from(v))
        .expect("a private CODEX_HOME");
    let rules = std::fs::read_to_string(home.join("rules/horch.rules")).unwrap();
    prepared.finish();
    for words in DENIED_COMMANDS {
        let pattern = words
            .iter()
            .map(|w| format!("\"{w}\""))
            .collect::<Vec<_>>()
            .join(", ");
        let forbidden = rules.split("prefix_rule(").any(|block| {
            block.contains(&format!("pattern = [{pattern}]"))
                && block.contains("decision = \"forbidden\"")
        });
        assert!(forbidden, "no forbidden rule for [{pattern}] in:\n{rules}");
    }
}

/// An address that no host answers (RFC 5737, TEST-NET-1): a webhook at
/// this address never gets a message, so the test sends none.
const OFF_HOST_WEBHOOK: &str = "http://192.0.2.1:9/hook";
const SANDBOX_HAS_ITS_OWN_LOOPBACK: bool = cfg!(target_os = "linux");

#[cfg(unix)]
fn check_launches(
    root: &Path,
    checks: &[&str],
) -> Vec<Option<horch_core::fleetd::worker_profiles::application::ports::LaunchSpec>> {
    use horch_core::fleetd::fleet_runtime::application::jobs::SandboxProfiles;
    use horch_core::fleetd::worker_profiles::adapters::check_sandbox::WorkerSandboxes;
    use horch_core::taskledger::runs::{self, NewRun};
    use horch_core::taskledger::workers::{self, NewWorker};
    use horch_core::taskledger::{finish::CodexSandbox, schema};

    let mut conn = rusqlite::Connection::open(root.join("ledger.sqlite3")).unwrap();
    schema::migrate_to(&mut conn, schema::VERSION).unwrap();
    let project = root.join("repo");
    std::fs::create_dir_all(&project).unwrap();
    let run = runs::create(
        &conn,
        &project,
        &NewRun {
            slug: "r1".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let worker = workers::insert(
        &conn,
        &NewWorker {
            run_id: run.run_id.clone(),
            persona: "opus".into(),
            harness: Some("claude".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let worker = workers::get(&conn, &worker).unwrap();
    let sandboxes = WorkerSandboxes::on_path(
        CodexSandbox::default(),
        root.join("fleet"),
        std::env::var_os("PATH"),
    );
    checks
        .iter()
        .map(|check| sandboxes.check_launch(&worker, &project, check).unwrap())
        .collect()
}

#[cfg(unix)]
fn run_check(
    spec: &horch_core::fleetd::worker_profiles::application::ports::LaunchSpec,
) -> std::process::Output {
    let mut cmd = std::process::Command::new(&spec.program);
    cmd.args(&spec.args);
    if let Some(dir) = &spec.cwd {
        cmd.current_dir(dir);
    }
    cmd.output().unwrap()
}

#[cfg(unix)]
fn sandbox_loopback_check() -> String {
    "perl -MIO::Socket::INET -e '\
         alarm 20; \
         my $l = IO::Socket::INET->new(Listen => 1, LocalAddr => \"127.0.0.1\", LocalPort => 0) \
           or die \"listen: $!\"; \
         my $p = $l->sockport; \
         my $pid = fork; defined $pid or die \"fork: $!\"; \
         if (!$pid) { exec \"curl\", \"--silent\", \"--show-error\", \"--max-time\", \"5\", \
           \"--data\", \"hello\", \"http://127.0.0.1:$p/\"; die \"exec: $!\" } \
         my $c = $l->accept or die \"accept: $!\"; \
         my $r = \"\"; while ($r !~ /hello/ && sysread($c, my $b, 1024)) { $r .= $b } \
         print \"GOT $r\\n\"; \
         print $c \"HTTP/1.1 200 OK\\r\\nContent-Length: 0\\r\\nConnection: close\\r\\n\\r\\n\"; \
         close $c; waitpid $pid, 0; exit($? >> 8)'"
        .to_string()
}

#[cfg(unix)]
#[test]
fn verifier_check_sends_no_message_outside_the_fleet() {
    use horch_core::fleetd::worker_profiles::adapters::check_sandbox::NETWORK_RULE;
    use std::io::{Read, Write};

    let tmp = tempfile::tempdir().unwrap();
    let curl = |url: &str| format!("curl --silent --show-error --max-time 5 --data hello {url}");

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let host = curl(&format!("http://127.0.0.1:{port}/"));
    let launches = check_launches(
        tmp.path(),
        &[&sandbox_loopback_check(), &host, &curl(OFF_HOST_WEBHOOK)],
    );
    let Some(spec) = &launches[0] else {
        return;
    };
    let out = run_check(spec);
    assert!(ok(&out), "{}", show(&out));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("GOT POST") && stdout.contains("hello"),
        "no request on the loopback of the sandbox: {}",
        show(&out)
    );
    assert!(
        !String::from_utf8_lossy(&out.stderr).contains("SR-3"),
        "a passing check prints no refusal: {}",
        show(&out)
    );

    if SANDBOX_HAS_ITS_OWN_LOOPBACK {
        let out = run_check(launches[1].as_ref().unwrap());
        assert_eq!(out.status.code(), Some(7), "{}", show(&out));
        drop(listener);
    } else {
        let webhook = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let n = stream.read(&mut buf).unwrap();
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            String::from_utf8_lossy(&buf[..n]).into_owned()
        });
        let out = run_check(launches[1].as_ref().unwrap());
        assert!(ok(&out), "{}", show(&out));
        assert!(webhook.join().unwrap().contains("hello"), "no request");
    }

    // A webhook on another host: the check fails with exit 7 (curl could
    // not connect), never with a timeout, and says why.
    let out = run_check(launches[2].as_ref().unwrap());
    assert_eq!(out.status.code(), Some(7), "{}", show(&out));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains(NETWORK_RULE), "{}", show(&out));
    assert!(stderr.contains("SR-3") && stderr.contains("off-host network"));
}
