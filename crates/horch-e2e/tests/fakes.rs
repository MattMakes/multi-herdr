//! Smoke tests for the fakes themselves: each fake answers the calls horch
//! makes, in the shape horch parses. These are not requirement tests.

use std::path::Path;
use std::process::{Command, Output};

use horch_e2e::harness::Harness;
use serde_json::Value;

/// Run one fake by the name of the program it stands in for, in the sealed
/// environment of `h`.
fn fake(h: &Harness, name: &str, args: &[&str]) -> Output {
    let exe = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    let mut cmd = Command::new(h.bin.join(exe));
    cmd.args(args);
    h.seal(&mut cmd);
    cmd.output().expect("running a fake")
}

fn json(o: &Output) -> Value {
    serde_json::from_slice(&o.stdout)
        .unwrap_or_else(|e| panic!("not JSON ({e}): {}", String::from_utf8_lossy(&o.stdout)))
}

fn pane_ids(h: &Harness) -> Vec<String> {
    let listed = json(&fake(h, "herdr", &["pane", "list"]));
    listed["result"]["panes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["pane_id"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn fake_herdr_split_adds_pane_and_close_removes_it() {
    let h = Harness::new("fakes-split");
    let created = json(&fake(&h, "herdr", &["workspace", "create", "--label", "t"]));
    let root = created["result"]["root_pane"]["pane_id"].as_str().unwrap();
    assert_eq!(pane_ids(&h), [root]);

    let split = json(&fake(
        &h,
        "herdr",
        &["pane", "split", root, "--direction", "right", "--no-focus"],
    ));
    let new = split["result"]["pane"]["pane_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(new, root);
    assert_eq!(
        split["result"]["pane"]["workspace_id"],
        created["result"]["root_pane"]["workspace_id"]
    );
    assert_eq!(
        split["result"]["pane"]["tab_id"],
        created["result"]["root_pane"]["tab_id"]
    );
    assert_eq!(pane_ids(&h), [root, new.as_str()]);
    assert!(fake(&h, "herdr", &["pane", "get", &new]).status.success());

    let closed = fake(&h, "herdr", &["pane", "close", &new]);
    assert!(closed.status.success());
    assert_eq!(pane_ids(&h), [root]);
    assert!(!fake(&h, "herdr", &["pane", "get", &new]).status.success());
    assert!(!fake(&h, "herdr", &["pane", "close", &new]).status.success());

    // Both calls stay violations, so existing assertions keep their meaning.
    let violations = h.violations();
    assert!(
        violations.iter().any(|v| v.contains("pane split")),
        "{violations:?}"
    );
    assert!(
        violations.iter().any(|v| v.contains("pane close")),
        "{violations:?}"
    );
}

#[test]
fn fake_herdr_fail_split_and_fail_run() {
    let mut h = Harness::new("fakes-fail");
    let created = json(&fake(&h, "herdr", &["workspace", "create", "--label", "t"]));
    let root = created["result"]["root_pane"]["pane_id"]
        .as_str()
        .unwrap()
        .to_string();

    // A list: `fail_split` combines with other scenarios.
    h.set("HORCH_FAKE_SCENARIO", "exec,fail_split");
    let split = fake(
        &h,
        "herdr",
        &["pane", "split", &root, "--direction", "right"],
    );
    assert_eq!(split.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&split.stderr).contains("fail_split"));
    assert_eq!(pane_ids(&h), [root.as_str()], "state unchanged");
    // `pane run` is not affected by `fail_split`.
    assert!(fake(&h, "herdr", &["pane", "run", &root, "true"])
        .status
        .success());

    h.set("HORCH_FAKE_SCENARIO", "fail_run");
    let marker = h.tmp.join("ran");
    let command = format!("touch {}", marker.display());
    let run = fake(&h, "herdr", &["pane", "run", &root, &command]);
    assert_eq!(run.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&run.stderr).contains("fail_run"));
    assert_eq!(pane_ids(&h), [root.as_str()], "state unchanged");
    // And `fail_run` does not stop a split.
    assert!(fake(&h, "herdr", &["pane", "split", &root])
        .status
        .success());
    assert!(!marker.exists(), "a failed run starts no command");
}

#[cfg(unix)]
#[test]
fn fake_herdr_close_kills_the_exec_process() {
    let mut h = Harness::new("fakes-kill");
    h.set("HORCH_FAKE_SCENARIO", "exec");
    let created = json(&fake(&h, "herdr", &["workspace", "create", "--label", "t"]));
    let root = created["result"]["root_pane"]["pane_id"]
        .as_str()
        .unwrap()
        .to_string();
    let pidfile = h.tmp.join("pid");
    let command = format!("echo $$ > {}; /bin/sleep 60", pidfile.display());
    assert!(fake(&h, "herdr", &["pane", "run", &root, &command])
        .status
        .success());
    let started = std::time::Instant::now();
    while std::fs::read_to_string(&pidfile)
        .unwrap_or_default()
        .trim()
        .is_empty()
        && started.elapsed().as_secs() < 10
    {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let pid = std::fs::read_to_string(&pidfile).expect("the pane command started");
    let alive = |pid: &str| {
        Command::new("/bin/sh")
            .args(["-c", &format!("kill -0 {}", pid.trim())])
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success()
    };
    assert!(alive(&pid));
    assert!(fake(&h, "herdr", &["pane", "close", &root])
        .status
        .success());
    let started = std::time::Instant::now();
    while alive(&pid) && started.elapsed().as_secs() < 10 {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(!alive(&pid), "pane close kills the pane process");
}

#[test]
fn fake_opencode_session_list_matches_cwd() {
    let mut h = Harness::new("fakes-opencode");
    assert_eq!(
        String::from_utf8_lossy(&fake(&h, "opencode", &["--version"]).stdout).trim(),
        "1.18.33"
    );

    // The same call horch makes, through the library that parses it.
    let listed = fake(&h, "opencode", &["session", "list", "--format", "json"]);
    assert!(listed.status.success());
    let text = String::from_utf8_lossy(&listed.stdout).into_owned();
    let records: Vec<Value> = serde_json::from_str(&text).unwrap();
    assert_eq!(records.len(), 1);
    let id = horch_e2e::opencode_session_id(&h.project);
    assert!(id.starts_with("ses_") && id.len() == 4 + 16, "{id}");
    assert_eq!(records[0]["id"], id.as_str());
    let dir = records[0]["directory"].as_str().unwrap();
    assert_eq!(
        std::fs::canonicalize(dir).unwrap(),
        std::fs::canonicalize(&h.project).unwrap()
    );
    // The fields `horch_core::harness::opencode::parse_sessions` reads: `created` is
    // milliseconds since the epoch and is recent, so the "after launch" filter
    // keeps it.
    let created = records[0]["created"].as_u64().unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    assert!(
        created <= now && now - created < 60_000,
        "{created} vs {now}"
    );

    // A launch is recorded and exits 0 at once; a resume names its session.
    let launch = fake(
        &h,
        "opencode",
        &[
            "--model",
            "opencode/m",
            "--session",
            "ses_old",
            "--prompt",
            "go",
        ],
    );
    assert!(launch.status.success());
    let calls = h.calls_of("opencode");
    let last = calls.last().unwrap();
    assert_eq!(last["model"], "opencode/m");
    assert_eq!(last["resumed"], "ses_old");
    assert_eq!(last["session_id"], id.as_str());

    // `stay` keeps a launch alive until it is killed.
    h.set("HORCH_FAKE_SCENARIO", "stay");
    let mut cmd = Command::new(h.bin.join("opencode"));
    cmd.args(["--model", "opencode/m", "--prompt", "go"]);
    h.seal(&mut cmd);
    let mut child = cmd.spawn().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(child.try_wait().unwrap().is_none(), "stay keeps running");
    child.kill().unwrap();
    child.wait().unwrap();
}

#[test]
fn fake_opencode_writes_transcript_rows_when_asked() {
    let mut h = Harness::new("fakes-opencode-db");
    let works = h.has_sqlite3()
        && Command::new(h.bin.join("sqlite3"))
            .arg("-version")
            .output()
            .is_ok_and(|o| o.status.success());
    if !works {
        assert!(
            std::env::var_os("HORCH_REQUIRE_SQLITE").is_none(),
            "HORCH_REQUIRE_SQLITE is set but the harness sqlite3 does not run"
        );
        return;
    }
    h.set("HORCH_FAKE_TRANSCRIPTS", "1");
    let launch = fake(&h, "opencode", &["--model", "opencode/m", "--prompt", "go"]);
    assert!(launch.status.success());
    let db = h.home.join(".local/share/opencode/opencode.db");
    assert!(db.is_file(), "no database at {}", db.display());
    let id = horch_e2e::opencode_session_id(&h.project);
    let mut cmd = Command::new(h.bin.join("sqlite3"));
    cmd.arg("-readonly").arg("-json").arg(&db).arg(format!(
        "SELECT count(*) AS n FROM message WHERE session_id = '{id}';"
    ));
    let out = cmd.output().unwrap();
    let rows: Vec<Value> = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(rows[0]["n"], 1);
}

#[test]
fn fake_prime_creates_session_file() {
    let mut h = Harness::new("fakes-prime");
    assert_eq!(
        String::from_utf8_lossy(&fake(&h, "prime-agent", &["--version"]).stdout).trim(),
        "0.9.4"
    );
    let base = h.state.join("prime/prime-1-abc");
    let sessions = base.join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    let socket = base.join("d.sock");
    let (socket_arg, sessions_arg) = (
        socket.to_string_lossy().into_owned(),
        sessions.to_string_lossy().into_owned(),
    );
    let launch_args = [
        "--model",
        "anthropic/claude-opus-5-5",
        "--thinking",
        "high",
        "--daemon-socket",
        &socket_arg,
        "--session-dir",
        &sessions_arg,
        "--",
        "do the work",
    ];

    h.set("HORCH_FAKE_TRANSCRIPTS", "1");
    let launch = fake(&h, "prime-agent", &launch_args);
    assert!(launch.status.success());
    assert!(socket.is_file(), "the socket path exists");
    let id = horch_e2e::prime_session_id(&sessions_arg);
    assert!(id.starts_with("prime_") && id.len() == 6 + 16, "{id}");
    // `find_session` takes the newest `.jsonl` in the directory.
    let files: Vec<_> = std::fs::read_dir(&sessions)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    assert_eq!(files, [sessions.join(format!("{id}.jsonl"))]);
    let text = std::fs::read_to_string(&files[0]).unwrap();
    let lines: Vec<Value> = text
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines[0]["type"], "session");
    assert_eq!(lines[0]["id"], id.as_str());
    assert_eq!(lines[1]["message"]["role"], "assistant");
    let call = h.calls_of("prime").pop().unwrap();
    assert_eq!(call["socket"], socket_arg.as_str());
    assert_eq!(call["session_id"], id.as_str());
    assert_eq!(call["model"], "anthropic/claude-opus-5-5");

    // A resume reuses the file it is given and mints no second one.
    let resume_file = files[0].to_string_lossy().into_owned();
    let mut resume_args = launch_args.to_vec();
    resume_args.splice(8..8, ["--resume", &resume_file]);
    assert!(fake(&h, "prime-agent", &resume_args).status.success());
    assert_eq!(std::fs::read_dir(&sessions).unwrap().count(), 1);
    assert_eq!(
        h.calls_of("prime").pop().unwrap()["resumed"],
        resume_file.as_str()
    );

    // With no launch alive, `status --json` lists no daemon.
    let status = |h: &Harness| -> Vec<Value> {
        json(&fake(h, "prime-agent", &["status", "--json"]))
            .as_array()
            .unwrap()
            .clone()
    };
    assert!(status(&h).is_empty());

    // `stay`: the launch is the daemon for its socket until it is killed.
    h.set("HORCH_FAKE_SCENARIO", "stay");
    let mut cmd = Command::new(h.bin.join("prime-agent"));
    cmd.args(launch_args);
    h.seal(&mut cmd);
    let mut child = cmd.spawn().unwrap();
    let started = std::time::Instant::now();
    while status(&h).is_empty() && started.elapsed().as_secs() < 10 {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let daemons = status(&h);
    assert_eq!(daemons.len(), 1, "{daemons:?}");
    assert_eq!(daemons[0]["socketPath"], socket_arg.as_str());
    assert_eq!(daemons[0]["pid"], child.id());
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(status(&h).is_empty(), "a killed launch is not listed");
}

#[test]
fn harness_with_git_makes_repo() {
    let h = Harness::new("fakes-git").with_git();
    let Some(git) = h.git_bin().map(|p| p.to_path_buf()) else {
        // `with_git` already refused to go on under HORCH_REQUIRE_GIT=1.
        return;
    };
    assert!(git.is_absolute());
    let git_out = |h: &Harness, args: &[&str]| {
        let out = h.git_cmd(args).output().unwrap();
        assert!(out.status.success(), "git {args:?}");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    // 2 files, 1 commit, branch `main`, a clean tree.
    assert_eq!(git_out(&h, &["rev-parse", "--abbrev-ref", "HEAD"]), "main");
    assert_eq!(git_out(&h, &["rev-list", "--count", "HEAD"]), "1");
    assert_eq!(
        git_out(&h, &["ls-files"]).lines().collect::<Vec<_>>(),
        ["README.md", "notes.txt"]
    );
    assert_eq!(git_out(&h, &["status", "--porcelain"]), "");
    let sha = h.head_sha().unwrap();
    assert_eq!(sha.len(), 40);
    assert_eq!(sha, git_out(&h, &["rev-parse", "HEAD"]));
    assert_eq!(
        git_out(&h, &["log", "-1", "--format=%an <%ae> %aI"]),
        "Horch Fixture <fixture@horch.invalid> 2026-09-28T12:00:00+00:00"
    );

    // Pinned: the same fixture gives the same commit in every harness.
    let other = Harness::new("fakes-git-2").with_git();
    assert_eq!(other.head_sha(), Some(sha));

    // The environment horch sees is pinned and names git by absolute path.
    let mut cmd = Command::new("/bin/sh");
    cmd.arg("-c").arg("/usr/bin/env");
    h.seal(&mut cmd);
    let env = String::from_utf8_lossy(&cmd.output().unwrap().stdout).into_owned();
    let has = |line: String| env.lines().any(|l| l == line);
    assert!(has("GIT_CONFIG_NOSYSTEM=1".into()), "{env}");
    assert!(has(format!("HORCH_GIT_BIN={}", git.display())), "{env}");
    let config = h.root.join("gitconfig");
    assert!(
        has(format!("GIT_CONFIG_GLOBAL={}", config.display())),
        "{env}"
    );
    assert_eq!(std::fs::read_to_string(&config).unwrap(), "");
    assert!(!env.lines().any(|l| l.starts_with("ANTHROPIC_API_KEY=")));

    // NFR-01: the fakes and exactly that one git are allowed; nothing else is,
    // and git is not on the sealed PATH.
    assert!(h.allows_program(&h.bin.join("claude")));
    assert!(h.allows_program(&git));
    assert!(!h.allows_program(Path::new("/usr/bin/sh")));
    assert!(!h.allows_program(&git.with_file_name("git-other")));
    let mut cmd = Command::new("/bin/sh");
    cmd.arg("-c").arg("command -v git");
    h.seal(&mut cmd);
    assert!(String::from_utf8_lossy(&cmd.output().unwrap().stdout)
        .trim()
        .is_empty());
    // Without `with_git`, no git is allowed.
    let plain = Harness::new("fakes-nogit");
    assert!(plain.git_bin().is_none() && plain.head_sha().is_none());
    assert!(!plain.allows_program(&git));
}

/// Start fake herdr `args` in the sealed environment of `h` without waiting.
fn start_herdr(h: &Harness, args: &[&str]) -> std::process::Child {
    let mut cmd = Command::new(h.bin.join(format!("herdr{}", std::env::consts::EXE_SUFFIX)));
    cmd.args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped());
    h.seal(&mut cmd);
    cmd.spawn().expect("starting fake herdr")
}

/// Wait up to `secs` for `child`. `None` when it still runs then.
fn wait_up_to(child: &mut std::process::Child, secs: u64) -> Option<std::process::ExitStatus> {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    while std::time::Instant::now() < until {
        if let Some(status) = child.try_wait().unwrap() {
            return Some(status);
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    None
}

/// The state lock dir of `h`'s fake herdr.
fn state_lock(h: &Harness) -> std::path::PathBuf {
    let mut p = h.log.clone().into_os_string();
    p.push(".state.lock");
    p.into()
}

/// A call made after the test removed its temp dir (a detached `horch tile`
/// can make one) fails at once. It used to wait for the lock for ever.
#[test]
fn fake_herdr_fails_when_the_harness_root_is_gone() {
    let mut h = Harness::new("fakes-gone");
    // The fakes stay reachable; only the log's directory goes.
    let gone = h.root.join("gone");
    h.set("HORCH_FAKE_LOG", gone.join("fake.log").to_string_lossy());
    let mut child = start_herdr(&h, &["pane", "list"]);
    let Some(status) = wait_up_to(&mut child, 10) else {
        let _ = child.kill();
        panic!("fake herdr still waits for a lock in a removed directory");
    };
    assert!(!status.success());
    let mut err = String::new();
    std::io::Read::read_to_string(child.stderr.as_mut().unwrap(), &mut err).unwrap();
    assert!(err.contains("the harness is gone"), "{err}");
}

/// A lock left by a dead call is taken over; a live holder's lock is kept,
/// however long it holds it.
#[cfg(unix)]
#[test]
fn fake_herdr_breaks_only_a_dead_holders_lock() {
    let h = Harness::new("fakes-lock");
    let lock = state_lock(&h);

    let dead = Command::new("/usr/bin/true").spawn().unwrap();
    let dead_pid = dead.id();
    let mut dead = dead;
    dead.wait().unwrap();
    std::fs::create_dir(&lock).unwrap();
    std::fs::write(lock.join("pid"), dead_pid.to_string()).unwrap();
    let mut child = start_herdr(&h, &["pane", "list"]);
    let status = wait_up_to(&mut child, 10).expect("a dead holder's lock is taken over");
    assert!(status.success());
    assert!(!lock.exists(), "the call released the lock");

    std::fs::create_dir(&lock).unwrap();
    std::fs::write(lock.join("pid"), std::process::id().to_string()).unwrap();
    let mut child = start_herdr(&h, &["pane", "list"]);
    assert!(
        wait_up_to(&mut child, 3).is_none(),
        "a live holder's lock is kept"
    );
    std::fs::remove_file(lock.join("pid")).unwrap();
    std::fs::remove_dir(&lock).unwrap();
    let status = wait_up_to(&mut child, 10).expect("the call goes on once the lock is free");
    assert!(status.success());
}
