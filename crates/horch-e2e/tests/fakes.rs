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

/// A fake's `--version` line. A failure names how the fake ended, so an
/// empty answer under load shows its exit status or signal.
fn version(h: &Harness, name: &str) -> String {
    let out = fake(h, name, &["--version"]);
    assert!(
        out.status.success() && !out.stdout.is_empty(),
        "{name} --version: {}, stdout {:?}, stderr {:?}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
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

/// herdr 0.8.2 has no top-level `wait`; `pane wait-output` matches pane
/// text, and a timeout is exit 1 with the `timeout` error code.
#[cfg(unix)]
#[test]
fn fake_herdr_answers_wait_output_like_herdr_0_8_2() {
    let mut h = Harness::new("fakes-wait");
    h.set("HORCH_FAKE_SCENARIO", "exec,shell");
    let created = json(&fake(&h, "herdr", &["workspace", "create", "--label", "t"]));
    let root = created["result"]["root_pane"]["pane_id"]
        .as_str()
        .unwrap()
        .to_string();

    let old = fake(&h, "herdr", &["wait", "output", &root, "--match", "x"]);
    assert_eq!(old.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&old.stderr).contains("unknown command: wait"));

    // Typed but not entered: the line shows, its output does not.
    let typed = "echo SMOKE_TEST_$((40+2))";
    assert!(fake(&h, "herdr", &["pane", "send-text", &root, typed])
        .status
        .success());
    let read = fake(&h, "herdr", &["pane", "read", &root, "--source", "visible"]);
    assert!(String::from_utf8_lossy(&read.stdout).contains(typed));
    let wait = |needle: &str, ms: &str| {
        fake(
            &h,
            "herdr",
            &[
                "pane",
                "wait-output",
                &root,
                "--match",
                needle,
                "--timeout",
                ms,
            ],
        )
    };
    let missed = wait("SMOKE_TEST_42", "200");
    assert_eq!(missed.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missed.stderr).contains(r#""code":"timeout""#));

    assert!(fake(&h, "herdr", &["pane", "send-keys", &root, "enter"])
        .status
        .success());
    let found = wait("SMOKE_TEST_42", "5000");
    assert!(found.status.success(), "{found:?}");
    assert_eq!(json(&found)["result"]["matched_line"], "SMOKE_TEST_42");

    let gone = fake(
        &h,
        "herdr",
        &["pane", "wait-output", "w99:p9", "--match", "x"],
    );
    assert_eq!(gone.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&gone.stderr).contains("pane_not_found"));
}

#[test]
fn fake_opencode_session_list_matches_cwd() {
    let mut h = Harness::new("fakes-opencode");
    assert_eq!(version(&h, "opencode"), "1.18.33");

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

/// `prime-agent` with `TMPDIR` set, as horch scopes `status` and
/// `shutdown`.
fn fake_in(h: &Harness, tmpdir: &str, args: &[&str]) -> Output {
    let mut cmd = Command::new(h.bin.join("prime-agent"));
    cmd.args(args);
    h.seal(&mut cmd);
    cmd.env("TMPDIR", tmpdir);
    cmd.output().expect("running a fake")
}

#[cfg(unix)]
#[test]
fn fake_prime_creates_session_file() {
    let mut h = Harness::new("fakes-prime");
    assert_eq!(version(&h, "prime-agent"), "0.10.0");
    let base = h.state.join("prime/0a1b2c3d4e5f");
    let sessions = base.join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    let scan = base.join(horch_core::harness::prime::scan_dir_name());
    std::fs::create_dir_all(&scan).unwrap();
    let socket = scan.join("d.sock");
    let tmpdir = format!("{}/", base.display());
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
    let status = |h: &Harness, tmpdir: &str| -> Vec<Value> {
        json(&fake_in(h, tmpdir, &["status", "--json"]))
            .as_array()
            .unwrap()
            .clone()
    };
    assert!(status(&h, &tmpdir).is_empty());

    // `stay`: the launch is the daemon for its socket until it is stopped.
    h.set("HORCH_FAKE_SCENARIO", "stay");
    let mut cmd = Command::new(h.bin.join("prime-agent"));
    cmd.args(launch_args);
    h.seal(&mut cmd);
    let mut child = cmd.spawn().unwrap();
    let started = std::time::Instant::now();
    while status(&h, &tmpdir).is_empty() && started.elapsed().as_secs() < 10 {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let daemons = status(&h, &tmpdir);
    assert_eq!(daemons.len(), 1, "{daemons:?}");
    assert_eq!(daemons[0]["socketPath"], socket_arg.as_str());
    assert_eq!(daemons[0]["pid"], child.id());
    assert!(
        std::os::unix::net::UnixStream::connect(&socket).is_ok(),
        "a stay launch listens on its socket"
    );
    // Another TMPDIR scans another dir: it sees and stops nothing.
    let elsewhere = format!("{}/", h.state.join("prime/other").display());
    assert!(status(&h, &elsewhere).is_empty());
    let out = json(&fake_in(&h, &elsewhere, &["shutdown", "--force", "--json"]));
    assert_eq!(out, serde_json::json!({"stopped": [], "failed": []}));
    // Without --force: refused.
    let refused = fake_in(&h, &tmpdir, &["shutdown", "--json"]);
    assert!(!refused.status.success());
    let out = json(&refused);
    assert_eq!(out["stopped"], serde_json::json!([]));
    assert!(out["failed"][0]["reason"]
        .as_str()
        .unwrap()
        .starts_with("confirmation required"));
    assert_eq!(child.try_wait().unwrap(), None);
    // With --force: this launch stops.
    let stop = fake_in(&h, &tmpdir, &["shutdown", "--force", "--json"]);
    assert!(stop.status.success());
    let out = json(&stop);
    assert_eq!(out["stopped"][0]["socketPath"], socket_arg.as_str());
    assert_eq!(out["failed"], serde_json::json!([]));
    child.wait().unwrap();
    assert!(
        status(&h, &tmpdir).is_empty(),
        "a stopped launch is not listed"
    );
}

/// The options Prime 0.10.0 refuses: "Unknown option", exit 1, and the
/// call is still recorded.
#[test]
fn fake_prime_refuses_the_options_prime_0_10_refuses() {
    let h = Harness::new("fakes-prime-refused");
    for flag in [
        "--no-extensions",
        "-e",
        "--extension",
        "--tools",
        "--no-tools",
        "--exclude-tools",
    ] {
        let out = fake(&h, "prime-agent", &["--model", "m/x", flag, "--", "p"]);
        assert_eq!(out.status.code(), Some(1), "{flag}");
        assert_eq!(
            String::from_utf8_lossy(&out.stderr).trim(),
            format!("Error: Unknown option: {flag}")
        );
    }
    // After `--` it is prompt text.
    let out = fake(&h, "prime-agent", &["--model", "m/x", "--", "-e"]);
    assert!(out.status.success());
    assert_eq!(h.calls_of("prime").len(), 7);
}

/// Git output with each strict ISO UTC date `...T12:00:00Z` written as
/// `...T12:00:00+00:00`. Git 2.50 prints the `Z` form for `%cI` and `%aI`;
/// git 2.39 prints the `+00:00` form. Only a whole word that is a date
/// changes, so a `Z` in a name stays.
fn utc_offset_date(text: &str) -> String {
    let word = |w: &str| match w.strip_suffix('Z') {
        Some(time) if time.len() == 19 && time.as_bytes()[10] == b'T' => format!("{time}+00:00"),
        _ => w.to_string(),
    };
    text.split(' ').map(word).collect::<Vec<_>>().join(" ")
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
        utc_offset_date(&git_out(&h, &["log", "-1", "--format=%an <%ae> %aI"])),
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

/// Whether a process with this pid runs.
#[cfg(unix)]
fn runs(pid: &str) -> bool {
    Command::new("/bin/sh")
        .args(["-c", &format!("kill -0 {} 2>/dev/null", pid.trim())])
        .status()
        .unwrap()
        .success()
}

/// Wait up to 10 s for `file` to hold a pid, and return it.
#[cfg(unix)]
fn pid_in(file: &Path) -> String {
    let started = std::time::Instant::now();
    loop {
        let pid = std::fs::read_to_string(file).unwrap_or_default();
        if !pid.trim().is_empty() {
            return pid.trim().to_string();
        }
        assert!(
            started.elapsed().as_secs() < 10,
            "no pid in {}",
            file.display()
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// A test that ends with a pane open (it failed before `horch done`, or
/// never closed it) leaves nothing running: teardown kills the pane's
/// process group, and the test does not fail for it.
#[cfg(unix)]
#[test]
fn teardown_kills_the_open_panes() {
    let mut h = Harness::new("teardown-pane");
    h.set("HORCH_FAKE_SCENARIO", "exec");
    let created = json(&fake(&h, "herdr", &["workspace", "create", "--label", "t"]));
    let root = created["result"]["root_pane"]["pane_id"]
        .as_str()
        .unwrap()
        .to_string();
    let pidfile = std::env::temp_dir().join(format!("teardown-pane-{}", std::process::id()));
    let command = format!("echo $$ > {}; /bin/sleep 60", pidfile.display());
    assert!(fake(&h, "herdr", &["pane", "run", &root, &command])
        .status
        .success());
    let pid = pid_in(&pidfile);
    let _ = std::fs::remove_file(&pidfile);
    assert!(runs(&pid));
    drop(h);
    assert!(!runs(&pid), "teardown kills the pane command");
}

/// The mail courier redelivers until `horch inbox` acknowledges, so a test
/// that queues orchestrator mail leaves it running. Only its environment
/// names the harness root, not its command line: teardown finds it by its
/// environment, stops it, and the test does not fail for it.
#[cfg(unix)]
#[test]
fn teardown_stops_the_mail_courier_of_its_state() {
    let h = Harness::new("teardown-courier");
    // Orchestrator mail and no orchestrator: the courier waits for one.
    horch_core::messaging::mail::Mail::under(&h.state, "w1")
        .enqueue("sonnet-1", "DONE: x")
        .unwrap();
    // Started as horch starts it: detached (the shell exits) in its own
    // process group, with the harness paths only in its environment.
    let mut cmd = Command::new("/bin/sh");
    h.seal(&mut cmd);
    cmd.args([
        "-c",
        "\"$0\" mail courier --workspace w1 </dev/null >/dev/null 2>&1 &",
    ])
    .arg(Harness::horch_bin());
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    assert!(cmd.status().unwrap().success());
    let lock = h.state.join("mail/w1/courier.lock");
    let started = std::time::Instant::now();
    let pid = loop {
        let holders = Command::new("lsof").arg("-t").arg(&lock).output().unwrap();
        let pid = String::from_utf8_lossy(&holders.stdout).trim().to_string();
        if !pid.is_empty() {
            break pid;
        }
        assert!(
            started.elapsed().as_secs() < 10,
            "the courier did not start"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    };

    drop(h);
    let stopped = !runs(&pid);
    if !stopped {
        // SIGKILL, so this test never leaks the process itself.
        let _ = Command::new("/bin/kill").args(["-9", &pid]).status();
    }
    assert!(stopped, "teardown stops the courier {pid}");
}

/// A process can start after teardown's first SIGTERM: a pane command
/// that starts the mail courier on its way out. Teardown stops it too, and
/// the test does not fail for it.
#[cfg(unix)]
#[test]
fn teardown_stops_a_process_that_starts_after_its_first_signal() {
    let h = Harness::new("teardown-late");
    let warm = "if [ \"$1\" = warm ]; then\n  exit 0\nfi\n";
    // macOS scans a new executable at its first exec, for seconds.
    let late = h.write_warmed(
        "late",
        format!("#!/bin/sh\n{warm}while :; do /bin/sleep 1; done\n").as_bytes(),
    );
    let starter = h.write_warmed(
        "starter",
        format!(
            "#!/bin/sh\n{warm}trap ': > \"$3\"; \"$2\" & exit 0' TERM\n: > \"$1\"\nwhile :; do /bin/sleep 1; done\n"
        )
        .as_bytes(),
    );
    let ready = h.tmp.join("starter.ready");
    // Outside the harness root: teardown removes the root.
    let outside = tempfile::tempdir().unwrap();
    let late_started = outside.path().join("late.started");
    // Not this test's child: the outer shell exits.
    let mut cmd = Command::new("/bin/sh");
    cmd.args([
        "-c",
        "\"$0\" \"$1\" \"$2\" \"$3\" </dev/null >/dev/null 2>&1 &",
    ])
    .arg(&starter)
    .arg(&ready)
    .arg(&late)
    .arg(&late_started);
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    assert!(cmd.status().unwrap().success());
    let started = std::time::Instant::now();
    while !ready.exists() {
        assert!(
            started.elapsed().as_secs() < 10,
            "the starter did not start"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let root = h.root.clone();

    let teardown = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(h)));
    // The late process can be stopped before it runs a line, so it is
    // found by its path, which names this harness.
    let still: Vec<String> = horch_e2e::process::processes()
        .into_iter()
        .filter(|p| p.command.contains(&*late.to_string_lossy()))
        .map(|p| p.pid.to_string())
        .collect();
    for pid in &still {
        // SIGKILL, so this test never leaks the process itself.
        let _ = Command::new("/bin/kill").args(["-9", pid]).status();
    }
    let was_started = late_started.exists();
    let _ = std::fs::remove_dir_all(&root);
    assert!(was_started, "the late process started during teardown");
    assert!(
        still.is_empty(),
        "teardown stops the late process {still:?}"
    );
    if let Err(panic) = teardown {
        let message = panic.downcast_ref::<String>().cloned().unwrap_or_default();
        panic!("a late process that teardown stopped is no leak: {message}");
    }
}

/// A process can leave its group between teardown's look and its SIGTERM:
/// the mail courier, forked into the pane's group, calls `setsid` before
/// it execs. Its pid ran at the first SIGTERM, but the signal missed it.
/// Teardown signals its new group too, and the test does not fail for it.
#[cfg(unix)]
#[test]
fn teardown_stops_a_process_that_leaves_its_group_before_the_signal() {
    let h = Harness::new("teardown-mover");
    // The first SIGTERM moves it to a group of its own, as `setsid` before
    // that SIGTERM would; the next one ends it.
    // macOS scans a new executable at its first exec, for seconds.
    let mover = h.write_warmed(
        "mover",
        b"#!/usr/bin/perl\nuse POSIX ();\nexit 0 if $ARGV[0] eq 'warm';\n\
          $SIG{TERM} = sub { POSIX::setsid(); $SIG{TERM} = 'DEFAULT' };\n\
          open(my $f, '>', $ARGV[0]) or die; print $f $$; close $f;\n\
          sleep 1 while 1;\n",
    );
    let pidfile = h.tmp.join("mover.pid");
    // A member of a group whose leader, the outer shell, exits: `setsid`
    // fails for a group leader.
    let mut cmd = Command::new("/bin/sh");
    cmd.args(["-c", "\"$0\" \"$1\" </dev/null >/dev/null 2>&1 &"])
        .arg(&mover)
        .arg(&pidfile);
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    assert!(cmd.status().unwrap().success());
    let pid = pid_in(&pidfile);

    let teardown = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(h)));
    let stopped = !runs(&pid);
    if !stopped {
        // SIGKILL, so this test never leaks the process itself.
        let _ = Command::new("/bin/kill").args(["-9", &pid]).status();
    }
    assert!(stopped, "teardown stops the process {pid}");
    if let Err(panic) = teardown {
        let message = panic.downcast_ref::<String>().cloned().unwrap_or_default();
        panic!("a process that teardown stopped is no leak: {message}");
    }
}

/// A process that outlives teardown's SIGTERM fails the test, and teardown
/// kills it.
#[cfg(unix)]
#[test]
fn teardown_fails_a_test_that_leaks_a_process() {
    let h = Harness::new("teardown-leak");
    // Named by the harness root, in its own process group, deaf to SIGTERM.
    // macOS scans a new executable at its first exec, for up to 35 s under
    // load: past the 10 s that pid_in waits for the pid.
    let stubborn = h.write_warmed(
        "stubborn",
        b"#!/bin/sh\nif [ \"$1\" = warm ]; then\n  exit 0\nfi\ntrap '' TERM\necho $$ > \"$1\"\nwhile :; do /bin/sleep 1; done\n",
    );
    let pidfile = h.tmp.join("stubborn.pid");
    let mut cmd = Command::new(&stubborn);
    // Not the test's own output: a pipe it held open would stall the run.
    cmd.arg(&pidfile)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let mut child = cmd.spawn().unwrap();
    let pid = pid_in(&pidfile);

    let teardown = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(h)));
    // The process is this test's child: reap it, or it stays a zombie.
    let ended = wait_up_to(&mut child, 2).is_some();
    if !ended {
        // SIGKILL, so this test never leaks the process itself.
        let _ = child.kill();
        let _ = child.wait();
    }
    assert!(ended, "teardown killed pid {pid}");
    let panic = teardown.expect_err("a leaked process fails the test");
    let message = panic.downcast_ref::<String>().cloned().unwrap_or_default();
    assert!(message.contains("the test leaked"), "{message}");
    assert!(message.contains("stubborn"), "{message}");
}

/// Append one record to the fake log, the way a fake does.
fn record(h: &Harness, line: Value) {
    use std::io::Write;
    let mut log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&h.log)
        .unwrap();
    writeln!(log, "{line}").unwrap();
}

/// The operating system reuses a pid under fork load. Two processes of one
/// fake that got the same pid are 2 calls, not 1.
#[test]
fn calls_of_keeps_two_calls_that_share_a_pid() {
    let h = Harness::new("fakes-pid-reuse");
    record(
        &h,
        serde_json::json!({"fake": "claude", "pid": 4242, "call_id": "4242-1", "argv": ["first"]}),
    );
    record(
        &h,
        serde_json::json!({"fake": "claude", "pid": 4242, "call_id": "4242-2", "argv": ["second"]}),
    );
    let calls = h.calls_of("claude");
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert_eq!(calls[0]["argv"][0], "first");
    assert_eq!(calls[1]["argv"][0], "second");
}

/// A fake that logs as it goes writes several lines for one call: the last
/// one wins, and a record with no `call_id` still merges by pid.
#[test]
fn calls_of_merges_the_lines_of_one_call() {
    let h = Harness::new("fakes-call-lines");
    for (id, stdin) in [("7-1", 0), ("7-1", 1)] {
        record(
            &h,
            serde_json::json!({"fake": "claude", "pid": 7, "call_id": id, "stdin": vec!["x"; stdin]}),
        );
    }
    record(&h, serde_json::json!({"fake": "herdr", "pid": 8, "n": 1}));
    record(&h, serde_json::json!({"fake": "herdr", "pid": 8, "n": 2}));
    let claude = h.calls_of("claude");
    assert_eq!(claude.len(), 1);
    assert_eq!(claude[0]["stdin"].as_array().unwrap().len(), 1);
    let herdr = h.calls_of("herdr");
    assert_eq!(herdr.len(), 1);
    assert_eq!(herdr[0]["n"], 2);
}

/// Every flush of one real call carries the same `call_id`, and 2 calls
/// carry 2 ids.
#[test]
fn a_fake_logs_one_call_id_per_process() {
    let h = Harness::new("fakes-call-id");
    version(&h, "claude");
    version(&h, "claude");
    let calls = h.calls_of("claude");
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert_ne!(calls[0]["call_id"], calls[1]["call_id"]);
    assert!(calls[0]["call_id"].is_string(), "{calls:?}");
}

// ─── the Phase 4 keys of the fake agents ────────────────────────────────

/// Unix only: the attempt folder and the worktree come from a git project.
#[cfg(unix)]
mod phase4 {
    use super::*;

    /// A git project with its own ledger: 1 run, 1 task claimed by worker
    /// `opus1`, its worktree, and the `task.yaml` of its attempt under the
    /// fleet root. The fakes find `horch` on `PATH`, as in a pane.
    struct Attempt {
        h: Harness,
        worker: String,
        task_id: String,
        task_yaml: std::path::PathBuf,
        worktree: std::path::PathBuf,
    }

    fn horch_ok(h: &Harness, worker: Option<&str>, args: &[&str]) -> String {
        let mut cmd = h.horch(args);
        if let Some(w) = worker {
            cmd.env("HORCH_WORKER_ID", w);
        }
        let o = cmd.output().unwrap();
        assert!(
            o.status.success(),
            "horch {args:?}: {}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        );
        String::from_utf8_lossy(&o.stdout).into_owned()
    }

    fn git_ok(h: &Harness, dir: &Path, args: &[&str]) -> String {
        let mut cmd = h.git_cmd(args);
        cmd.current_dir(dir);
        let o = cmd.output().unwrap();
        assert!(o.status.success(), "git {args:?}: {o:?}");
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    }

    fn attempt(name: &str) -> Attempt {
        use horch_core::fleetd::task_execution::adapters::task_files::write_attempt_files;
        use horch_core::fleetd::task_execution::domain::task_files::TaskFacts;

        let mut h = Harness::new(name).with_git();
        let db = h.root.join("ledger.sqlite3");
        let fleet = h.root.join("fleet");
        h.set("HORCH_LEDGER_DB", db.to_string_lossy());
        h.set("FLEET_ROOT", fleet.to_string_lossy());
        h.set("HORCH_NO_WAKE", "1");
        h.set("HORCH_NO_WATCHDOG", "1");
        let path = format!("{}:{}", h.bin.display(), horch_e2e::bin_dir().display());
        h.set("PATH", path);
        std::fs::create_dir_all(h.project.join("tickets")).unwrap();
        std::fs::write(h.project.join("tickets/t1.md"), "# T1\n").unwrap();
        git_ok(&h, &h.project, &["add", "tickets/t1.md"]);
        git_ok(&h, &h.project, &["commit", "-q", "-m", "t1"]);
        horch_ok(&h, None, &["ledger", "run", "new", "r1"]);
        horch_ok(
            &h,
            None,
            &[
                "ledger",
                "add",
                "--file",
                "tickets/t1.md",
                "--primary",
                "opus",
            ],
        );
        let worker = horch_ok(
            &h,
            None,
            &[
                "ledger",
                "worker",
                "new",
                "--persona",
                "opus",
                "--role",
                "opus1",
            ],
        )
        .trim()
        .to_string();
        horch_ok(&h, Some(&worker), &["ledger", "claim"]);
        horch_ok(&h, Some(&worker), &["ledger", "worktree", "new", "feat"]);
        let conn = rusqlite::Connection::open(&db).unwrap();
        let (task_id, n): (String, i64) = conn
            .query_row(
                "SELECT task_id, attempt FROM workers WHERE worker_id = ?1",
                [&worker],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        let worktree: String = conn
            .query_row(
                "SELECT path FROM worktrees WHERE task_id = ?1",
                [&task_id],
                |r| r.get(0),
            )
            .unwrap();
        let facts = TaskFacts {
            task: "T1".into(),
            task_id: task_id.clone(),
            run: "r1".into(),
            attempt: format!("A{n}"),
            profile: "opus".into(),
            worktree: worktree.clone(),
            branch: "fleet/r1-T1-feat".into(),
            read_first: "tickets/t1.md".into(),
            goal: "T1".into(),
            check: None,
            kind: None,
            ticket_snapshot: "# T1\n".into(),
            extra: None,
            handoff: None,
        };
        let files = write_attempt_files(&fleet, &facts, &[]).unwrap();
        h.set("HORCH_WORKER_ID", worker.as_str());
        h.set("HORCH_TASK_YAML", files.task_yaml.to_string_lossy());
        Attempt {
            h,
            worker,
            task_id,
            task_yaml: files.task_yaml,
            worktree: worktree.into(),
        }
    }

    impl Attempt {
        fn dir(&self) -> &Path {
            self.task_yaml.parent().unwrap()
        }

        /// Launch the fake `name` as a pane agent of this attempt.
        fn launch(&mut self, name: &str, scenario: &str) -> Output {
            self.h.set("HORCH_FAKE_SCENARIO", scenario);
            let args: &[&str] = match name {
                "agy" => &["--model", "m", "--prompt-interactive", "p"],
                _ => &[],
            };
            let mut cmd = Command::new(self.h.bin.join(name));
            cmd.args(args);
            self.h.seal(&mut cmd);
            cmd.current_dir(&self.worktree);
            let o = cmd.output().unwrap();
            assert!(o.status.success(), "{name}: {o:?}");
            o
        }

        fn head(&self) -> String {
            git_ok(&self.h, &self.worktree, &["rev-parse", "HEAD"])
        }
    }

    #[test]
    fn bound_scenario_writes_entry_json_at_entry() {
        let mut a = attempt("fakes-bound");
        let entry = a.dir().join("entry.json");
        for name in ["claude", "codex", "agy"] {
            let _ = std::fs::remove_file(&entry);
            a.launch(name, "bound");
            let text = std::fs::read_to_string(&entry)
                .unwrap_or_else(|e| panic!("{name}: no {}: {e}", entry.display()));
            let v: Value = serde_json::from_str(&text).unwrap();
            assert_eq!(v["bound"], true, "{name}: {v}");
            assert_eq!(v["task_id"], a.task_id.as_str(), "{name}: {v}");
            assert_eq!(v["worker_id"], a.worker.as_str(), "{name}: {v}");
            assert_eq!(v["task_yaml_exists"], true, "{name}: {v}");
        }
        // An agent whose worker has no task is not bound, and a missing
        // `task.yaml` is reported as missing.
        std::fs::remove_file(&entry).unwrap();
        std::fs::remove_file(&a.task_yaml).unwrap();
        a.h.set("HORCH_WORKER_ID", "w-none");
        a.launch("claude", "bound");
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&entry).unwrap()).unwrap();
        assert_eq!(v["bound"], false, "{v}");
        assert_eq!(v["task_id"], Value::Null, "{v}");
        assert_eq!(v["task_yaml_exists"], false, "{v}");
    }

    #[test]
    fn result_scenario_writes_result_json() {
        let mut a = attempt("fakes-result");
        let before = a.head();
        a.launch("claude", "commit,result");
        let after = a.head();
        assert_ne!(before, after, "the commit scenario made no commit");
        let path = a.dir().join("outbox/result.json");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("no {}: {e}: {:?}", path.display(), a.h.calls_of("claude")));
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["task"], a.task_id.as_str(), "{v}");
        assert_eq!(v["status"], "completed", "{v}");
        assert_eq!(v["commits"], serde_json::json!([after]), "{v}");
        assert_eq!(
            v["files_changed"],
            serde_json::json!(["fake-claude.txt"]),
            "{v}"
        );
    }

    #[test]
    fn noresult_scenario_writes_nothing() {
        let mut a = attempt("fakes-noresult");
        let before = a.head();
        a.launch("codex", "noresult");
        assert_eq!(a.head(), before);
        let mut left: Vec<String> = std::fs::read_dir(a.dir())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, ["inbox", "task.yaml"], "the attempt folder changed");
        // `noresult` wins over `result`: the agent exits with no result.
        a.launch("codex", "result,noresult");
        assert!(!a.dir().join("outbox").exists());
    }

    #[test]
    fn ask_scenario_sends_one_question() {
        let mut a = attempt("fakes-ask");
        a.h.set("HORCH_WORKSPACE_ID", "w1");
        let created = fake(&a.h, "herdr", &["workspace", "create", "--label", "ask"]);
        assert!(created.status.success(), "{created:?}");
        let registered =
            a.h.horch(&["register", "orchestrator"])
                .env("HERDR_PANE_ID", "w1:p1")
                .output()
                .unwrap();
        assert!(registered.status.success(), "{registered:?}");
        a.launch("agy", "ask");
        let calls = a.h.calls_of("antigravity");
        let ask = &calls.last().unwrap()["ask"];
        assert_eq!(ask["exit"], 0, "{calls:?}");
        assert!(
            ask["text"].as_str().unwrap().starts_with("QUESTION: "),
            "{calls:?}"
        );
        assert!(!a.dir().join("outbox/result.json").exists());
    }
}

/// The fake agents of Phase 7 (fleetd-p7 ticket 16): exec mode with the
/// argv of each exec adapter, the scenario keys `usage_limit`,
/// `long_context`, `exit_without_result` and `trust_prompt`, and the probes
/// of the conformance suite (`version:<text>` and `conformance`).
#[cfg(unix)]
mod phase7 {
    use std::io::{BufRead, BufReader};
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Output, Stdio};

    use horch_core::harness::{antigravity, claude, codex};
    use horch_e2e::harness::Harness;
    use serde_json::{json, Value};

    /// The 3 CLIs with an exec adapter, by the name of their fake.
    const EXEC_CLIS: [&str; 3] = ["claude", "codex", "agy"];

    /// A harness with a `START.md` that the exec argv names.
    fn harness(name: &str, scenario: &str) -> (Harness, PathBuf) {
        let mut h = Harness::new(name);
        h.set("HORCH_FAKE_SCENARIO", scenario);
        let start = h.root.join("attempt/inbox/START.md");
        std::fs::create_dir_all(start.parent().unwrap()).unwrap();
        std::fs::write(&start, "Do task T-1.\n").unwrap();
        (h, start)
    }

    /// The exec argv of `cli`, as the adapters of tickets 13 to 15 build it,
    /// with `extra` before the prompt.
    fn exec_argv(cli: &str, start: &Path, extra: &[&str]) -> Vec<String> {
        let start = start.to_string_lossy().into_owned();
        let mut argv: Vec<String> = match cli {
            "claude" => ["-p", "--output-format", "stream-json", "--verbose"]
                .iter()
                .chain(&["--include-hook-events", "--model", "claude-opus-5-5"])
                .map(|s| s.to_string())
                .collect(),
            "codex" => vec!["exec".into(), "--json".into()],
            "agy" => ["-p", &start, "--output-format", "stream-json"]
                .iter()
                .chain(&["--mode", "auto", "--model", "gemini-3.8-flash"])
                .map(|s| s.to_string())
                .collect(),
            other => panic!("no exec adapter for {other}"),
        };
        argv.extend(extra.iter().map(|s| s.to_string()));
        if cli != "agy" {
            argv.push(start);
        }
        argv
    }

    fn command(h: &Harness, cli: &str, argv: &[String]) -> Command {
        let exe = format!("{cli}{}", std::env::consts::EXE_SUFFIX);
        let mut cmd = Command::new(h.bin.join(exe));
        cmd.args(argv).current_dir(&h.project);
        h.seal(&mut cmd);
        cmd
    }

    fn exec(h: &Harness, cli: &str, start: &Path, extra: &[&str]) -> Output {
        let argv = exec_argv(cli, start, extra);
        command(h, cli, &argv).output().expect("running a fake")
    }

    fn lines(out: &Output) -> Vec<String> {
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::to_owned)
            .collect()
    }

    /// The event name of 1 stream line, read by the parser of the adapter.
    fn event(cli: &str, line: &str) -> String {
        let name = match cli {
            "claude" => format!("{:?}", claude::read_event(line)),
            "codex" => format!("{:?}", codex::read_event(line)),
            _ => format!("{:?}", antigravity::read_event(line)),
        };
        name.split([' ', '(', '{']).next().unwrap().to_owned()
    }

    fn events(cli: &str, out: &Output) -> Vec<String> {
        lines(out).iter().map(|l| event(cli, l)).collect()
    }

    /// The fixture lines of `cli`, as the adapter tickets recorded them.
    fn fixture(cli: &str) -> Vec<Value> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../horch-core/tests/fixtures/streams")
            .join(format!("{cli}-exec.jsonl"));
        std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    /// The stream line of 1 fixture entry: the bare line, with no doc key.
    fn bare(cli: &str, entry: &Value) -> Value {
        if cli == "codex" {
            return entry["line"].clone();
        }
        let mut v = entry.clone();
        let map = v.as_object_mut().unwrap();
        map.remove("_doc");
        map.remove("_expect");
        v
    }

    fn the_call(h: &Harness, cli: &str) -> Value {
        let fake = if cli == "agy" { "antigravity" } else { cli };
        let calls = h.calls_of(fake);
        assert_eq!(calls.len(), 1, "{calls:?}");
        calls[0].clone()
    }

    #[test]
    fn fake_claude_exec_prints_the_fixture_stream() {
        exec_prints_the_fixture_stream("claude");
    }

    #[test]
    fn fake_codex_exec_prints_the_fixture_stream() {
        exec_prints_the_fixture_stream("codex");
    }

    #[test]
    fn fake_agy_exec_prints_the_fixture_stream() {
        exec_prints_the_fixture_stream("agy");
    }

    /// Each printed line is a line of the fixture, the run makes progress,
    /// and it ends with 1 result and exit 0. The fake read `START.md`.
    fn exec_prints_the_fixture_stream(cli: &str) {
        let (h, start) = harness(&format!("p7-exec-{cli}"), "default");
        let out = exec(&h, cli, &start, &[]);
        assert!(out.status.success(), "{out:?}");
        let known: Vec<Value> = fixture(cli).iter().map(|e| bare(cli, e)).collect();
        let printed = lines(&out);
        for line in &printed {
            let v: Value = serde_json::from_str(line).unwrap();
            assert!(known.contains(&v), "{cli}: not a fixture line: {line}");
        }
        let events = events(cli, &out);
        assert!(events.len() >= 3, "{cli}: {events:?}");
        assert_eq!(events.last().unwrap(), "Result", "{cli}: {events:?}");
        assert_eq!(events.iter().filter(|e| *e == "Result").count(), 1);
        assert!(!events.iter().any(|e| e == "UsageLimited"), "{events:?}");
        let call = the_call(&h, cli);
        assert_eq!(call["exec_prompt"], "Do task T-1.\n", "{call}");
        assert_eq!(call["violations"], json!([]), "{call}");
    }

    #[test]
    fn fake_exec_names_a_prompt_that_is_not_a_file() {
        let (h, start) = harness("p7-noprompt", "default");
        std::fs::remove_file(&start).unwrap();
        let out = exec(&h, "claude", &start, &[]);
        assert!(!out.status.success(), "{out:?}");
        let violations = the_call(&h, "claude")["violations"].to_string();
        assert!(violations.contains("START.md"), "{violations}");
    }

    #[test]
    fn fake_codex_usage_limit_prints_the_signal() {
        usage_limit_prints_the_signal("codex");
    }

    #[test]
    fn fake_claude_usage_limit_prints_the_signal() {
        usage_limit_prints_the_signal("claude");
    }

    #[test]
    fn fake_agy_usage_limit_prints_the_signal() {
        usage_limit_prints_the_signal("agy");
    }

    /// The last line is the usage-limit signal of the CLI, the run has no
    /// result, and the fake exits by itself with a failure.
    fn usage_limit_prints_the_signal(cli: &str) {
        let (h, start) = harness(&format!("p7-limit-{cli}"), "usage_limit");
        let out = exec(&h, cli, &start, &[]);
        assert!(!out.status.success(), "{out:?}");
        let events = events(cli, &out);
        assert_eq!(events.last().unwrap(), "UsageLimited", "{cli}: {events:?}");
        assert!(!events.iter().any(|e| e == "Result"), "{cli}: {events:?}");
    }

    #[test]
    fn fake_exec_exit_without_result_prints_no_result() {
        for cli in EXEC_CLIS {
            let (h, start) = harness(&format!("p7-nores-{cli}"), "exit_without_result");
            let out = exec(&h, cli, &start, &[]);
            assert!(out.status.success(), "{cli}: {out:?}");
            let events = events(cli, &out);
            assert!(events.iter().any(|e| e == "Activity"), "{cli}: {events:?}");
            assert!(
                !events.iter().any(|e| e == "Result" || e == "UsageLimited"),
                "{cli}: {events:?}"
            );
        }
    }

    /// Start `cli` in exec mode and read its stdout until `stop` holds for a
    /// line. The child still runs: the caller ends it.
    fn read_until(
        h: &Harness,
        cli: &str,
        start: &Path,
        stop: impl Fn(&str) -> bool,
    ) -> (Child, Vec<String>) {
        let argv = exec_argv(cli, start, &[]);
        let mut child = command(h, cli, &argv)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut seen = Vec::new();
        let mut reader = BufReader::new(child.stdout.take().unwrap());
        loop {
            let mut line = String::new();
            let n = reader.read_line(&mut line).unwrap();
            assert!(n > 0, "{cli}: the fake ended before the line; saw {seen:?}");
            let line = line.trim_end().to_owned();
            seen.push(line.clone());
            if stop(&line) {
                break;
            }
        }
        // The pipe stays open, so a later print of the fake does not fail.
        std::mem::forget(reader);
        (child, seen)
    }

    /// The child has not exited by itself. Then end it.
    fn still_runs_then_end(mut child: Child) {
        assert!(
            child.try_wait().unwrap().is_none(),
            "the fake stopped by itself"
        );
        child.kill().unwrap();
        child.wait().unwrap();
    }

    #[test]
    fn fake_agy_long_context_passes_256000_tokens() {
        let (h, start) = harness("p7-long-agy", "long_context");
        let past = |line: &str| {
            matches!(antigravity::read_event(line),
                antigravity::Event::ContextTokens(n) if n > 256_000)
        };
        let (child, _) = read_until(&h, "agy", &start, past);
        still_runs_then_end(child);
    }

    #[test]
    fn fake_claude_long_context_passes_256000_tokens() {
        let (h, start) = harness("p7-long-claude", "long_context");
        let past = |line: &str| matches!(claude::read_event(line), claude::Event::ContextTokens(n) if n > 256_000);
        let (child, _) = read_until(&h, "claude", &start, past);
        still_runs_then_end(child);
    }

    /// Codex prints its token count only in `turn.completed`.
    #[test]
    fn fake_codex_long_context_passes_256000_tokens() {
        let (h, start) = harness("p7-long-codex", "long_context");
        let past = |line: &str| {
            let v: Value = serde_json::from_str(line).unwrap_or(Value::Null);
            v["usage"]["input_tokens"]
                .as_u64()
                .is_some_and(|n| n > 256_000)
        };
        let (child, _) = read_until(&h, "codex", &start, past);
        still_runs_then_end(child);
    }

    #[test]
    fn fake_exec_trust_prompt_prints_a_dialog_and_waits() {
        for cli in EXEC_CLIS {
            let (h, start) = harness(&format!("p7-trust-{cli}"), "trust_prompt");
            let (child, seen) = read_until(&h, cli, &start, |l| l.contains("trust"));
            assert!(
                seen.iter()
                    .all(|l| serde_json::from_str::<Value>(l).is_err()),
                "{cli}: a dialog comes before any stream line: {seen:?}"
            );
            still_runs_then_end(child);
        }
    }

    #[test]
    fn fake_probe_version_comes_from_the_scenario() {
        for cli in ["claude", "codex", "agy", "opencode", "pi", "prime-agent"] {
            let mut h = Harness::new(&format!("p7-ver-{cli}"));
            h.set("HORCH_FAKE_SCENARIO", "version:9.8.7-probe");
            let exe = format!("{cli}{}", std::env::consts::EXE_SUFFIX);
            let mut cmd = Command::new(h.bin.join(exe));
            cmd.arg("--version");
            h.seal(&mut cmd);
            let out = cmd.output().unwrap();
            assert!(out.status.success(), "{cli}: {out:?}");
            assert_eq!(
                String::from_utf8_lossy(&out.stdout).trim(),
                "9.8.7-probe",
                "{cli}"
            );
        }
    }

    /// A fleet root with the operator token, a listening command channel
    /// and the request folder, and a worktree in it.
    struct Fleet {
        root: PathBuf,
        worktree: PathBuf,
        _channel: std::os::unix::net::UnixListener,
    }

    fn fleet(h: &mut Harness) -> Fleet {
        let root = h.root.join("fleet");
        let run = root.join("run");
        std::fs::create_dir_all(run.join("requests")).unwrap();
        std::fs::write(run.join("operator.token"), "secret-token\n").unwrap();
        let channel = std::os::unix::net::UnixListener::bind(run.join("fleetd.sock")).unwrap();
        let worktree = h.project.clone();
        h.set("FLEET_ROOT", root.to_string_lossy());
        Fleet {
            root,
            worktree,
            _channel: channel,
        }
    }

    /// The `--settings` of a sandboxed Claude Code worker: it writes only in
    /// its worktree, and it cannot read the run folder of the fleet.
    fn claude_settings(f: &Fleet) -> String {
        json!({
            "env": {"CLAUDE_CODE_AUTO_COMPACT_WINDOW": "256000"},
            "sandbox": {
                "enabled": true,
                "filesystem": {
                    "allowWrite": [f.worktree],
                    "denyRead": [f.root.join("run")],
                    "denyWrite": [f.root.join("run")],
                }
            }
        })
        .to_string()
    }

    /// The probe report of 1 conformance run of `cli`, from its result line
    /// and from the log.
    fn report(h: &Harness, cli: &str, out: &Output) -> Value {
        assert!(out.status.success(), "{cli}: {out:?}");
        let call = the_call(h, cli);
        let logged = call["conformance"].clone();
        assert!(logged.is_object(), "{cli}: {call}");
        let text = lines(out)
            .into_iter()
            .find(|l| l.contains("\\\"operator_boundary\\\""))
            .unwrap_or_else(|| panic!("{cli}: no report in the stream: {out:?}"));
        let line: Value = serde_json::from_str(&text).unwrap();
        let inner = [
            &line["result"],
            &line["result"]["response"],
            &line["item"]["text"],
        ]
        .into_iter()
        .find_map(|v| {
            v.as_str()
                .and_then(|s| serde_json::from_str::<Value>(s).ok())
        })
        .unwrap_or_else(|| panic!("{cli}: the report is not JSON: {text}"));
        assert_eq!(inner, logged, "{cli}");
        logged
    }

    #[test]
    fn fake_probe_sandbox_write_outside_is_refused() {
        let (mut h, start) = harness("p7-sbx-write", "conformance");
        let f = fleet(&mut h);
        let settings = claude_settings(&f);
        let out = exec(&h, "claude", &start, &["--settings", &settings]);
        let r = report(&h, "claude", &out);
        let probe = &r["operator_boundary"];
        assert_eq!(probe["sandbox"], "on", "{r}");
        assert!(
            probe["write_outside"]
                .as_str()
                .unwrap()
                .starts_with("refused"),
            "{r}"
        );
        assert!(
            probe["write_inside"]
                .as_str()
                .unwrap()
                .starts_with("allowed"),
            "{r}"
        );
        let outside = h.project.parent().unwrap();
        let left: Vec<_> = std::fs::read_dir(outside)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("horch-probe"))
            .collect();
        assert!(left.is_empty(), "the probe left a file outside: {left:?}");
    }

    #[test]
    fn fake_probe_operator_acts_are_refused() {
        let (mut h, start) = harness("p7-sbx-acts", "conformance");
        let f = fleet(&mut h);
        let settings = claude_settings(&f);
        let out = exec(&h, "claude", &start, &["--settings", &settings]);
        let r = report(&h, "claude", &out);
        for act in ["read_token", "connect_channel", "write_requests"] {
            let result = r["operator_boundary"][act].as_str().unwrap();
            assert!(result.starts_with("refused"), "{act}: {r}");
        }
        let requests = std::fs::read_dir(f.root.join("run/requests"))
            .unwrap()
            .count();
        assert_eq!(requests, 0, "a request file got through");
    }

    /// Codex: the `-c` permissions of its rendered config close the run
    /// folder (`"none"`) and open the worktree.
    #[test]
    fn fake_codex_probe_applies_the_permissions_of_its_config() {
        let (mut h, start) = harness("p7-sbx-codex", "conformance");
        let f = fleet(&mut h);
        let fs = format!(
            "permissions.fleet_write.filesystem={{\":project_roots\"={{\".\"=\"write\"}},\"{}\"=\"none\"}}",
            f.root.join("run").display()
        );
        let roots = format!(
            "permissions.fleet_write.workspace_roots={{\"{}\"=true}}",
            f.worktree.display()
        );
        let out = exec(&h, "codex", &start, &["-c", &fs, "-c", &roots]);
        let r = report(&h, "codex", &out);
        let probe = &r["operator_boundary"];
        assert_eq!(probe["sandbox"], "on", "{r}");
        for act in [
            "write_outside",
            "read_token",
            "connect_channel",
            "write_requests",
        ] {
            assert!(
                probe[act].as_str().unwrap().starts_with("refused"),
                "{act}: {r}"
            );
        }
        assert!(
            probe["write_inside"]
                .as_str()
                .unwrap()
                .starts_with("allowed"),
            "{r}"
        );
    }

    #[test]
    fn fake_probe_reads_back_the_compaction_setting() {
        let (mut h, start) = harness("p7-compact-claude", "conformance");
        let f = fleet(&mut h);
        let settings = claude_settings(&f);
        let out = exec(&h, "claude", &start, &["--settings", &settings]);
        assert_eq!(report(&h, "claude", &out)["compaction"], "256000");

        let (mut h, start) = harness("p7-compact-codex", "conformance");
        fleet(&mut h);
        let out = exec(
            &h,
            "codex",
            &start,
            &["-c", "model_auto_compact_token_limit=256000"],
        );
        assert_eq!(report(&h, "codex", &out)["compaction"], "256000");

        let (mut h, start) = harness("p7-compact-agy", "conformance");
        fleet(&mut h);
        let out = exec(&h, "agy", &start, &[]);
        assert_eq!(report(&h, "agy", &out)["compaction"], Value::Null);
    }

    #[test]
    fn fake_probe_lists_skills_rules_mcp_servers_and_hooks() {
        let (mut h, start) = harness("p7-config-claude", "conformance");
        fleet(&mut h);
        let plugin = h.root.join("plugin");
        std::fs::create_dir_all(plugin.join("skills/tdd")).unwrap();
        let config = h.root.join("claude-home");
        std::fs::create_dir_all(config.join("rules")).unwrap();
        std::fs::write(config.join("rules/no-key.md"), "x").unwrap();
        std::fs::write(config.join("CLAUDE.md"), "x").unwrap();
        h.set("CLAUDE_CONFIG_DIR", config.to_string_lossy());
        let mcp = r#"{"mcpServers":{"horch":{"command":"horch"}}}"#;
        let plugin = plugin.to_string_lossy().into_owned();
        let extra = ["--mcp-config", mcp, "--model", "m", "--plugin-dir", &plugin];
        let out = exec(&h, "claude", &start, &extra);
        let r = report(&h, "claude", &out);
        assert_eq!(r["skills"], json!(["tdd"]), "{r}");
        assert_eq!(r["rules"], json!(["CLAUDE.md", "no-key.md"]), "{r}");
        assert_eq!(r["mcp_servers"], json!(["horch"]), "{r}");
        assert_eq!(r["hooks"], json!(["PreToolUse"]), "{r}");

        let (mut h, start) = harness("p7-config-codex", "conformance");
        fleet(&mut h);
        let home = h.root.join("codex-home");
        std::fs::create_dir_all(home.join("skills/review")).unwrap();
        std::fs::write(home.join("AGENTS.md"), "x").unwrap();
        std::fs::write(
            home.join("config.toml"),
            "[mcp_servers.docs]\ncommand = \"d\"\n",
        )
        .unwrap();
        h.set("CODEX_HOME", home.to_string_lossy());
        let out = exec(
            &h,
            "codex",
            &start,
            &["-c", "mcp_servers.horch.command=\"horch\""],
        );
        let r = report(&h, "codex", &out);
        assert_eq!(r["skills"], json!(["review"]), "{r}");
        assert_eq!(r["rules"], json!(["AGENTS.md"]), "{r}");
        assert_eq!(r["mcp_servers"], json!(["docs", "horch"]), "{r}");
        assert_eq!(r["hooks"], json!([]), "{r}");
    }

    /// OpenCode, pi and Prime render no sandbox rule, so the probe runs each
    /// act with no sandbox and records what it could do (MAP.md Q5).
    #[test]
    fn fake_opencode_pi_prime_probe_the_operator_boundary() {
        for (cli, program) in [
            ("opencode", "opencode"),
            ("pi", "pi"),
            ("prime", "prime-agent"),
        ] {
            let mut h = Harness::new(&format!("p7-ob-{cli}"));
            h.set("HORCH_FAKE_SCENARIO", "conformance");
            let f = fleet(&mut h);
            let exe = format!("{program}{}", std::env::consts::EXE_SUFFIX);
            let mut cmd = Command::new(h.bin.join(exe));
            cmd.current_dir(&h.project);
            h.seal(&mut cmd);
            let out = cmd.output().unwrap();
            let calls = h.calls_of(cli);
            let probe = calls
                .iter()
                .find_map(|c| c.get("conformance").cloned())
                .unwrap_or_else(|| panic!("{cli}: no probe in the log: {calls:?} {out:?}"));
            let probe = &probe["operator_boundary"];
            assert_eq!(probe["sandbox"], "none", "{cli}: {probe}");
            for act in ["read_token", "connect_channel", "write_requests"] {
                assert!(
                    probe[act].as_str().unwrap().starts_with("allowed"),
                    "{cli} {act}: {probe}"
                );
            }
            let left = std::fs::read_dir(f.root.join("run/requests"))
                .unwrap()
                .count();
            assert_eq!(left, 0, "{cli}: the probe left its request file");
        }
    }
}

/// The keys of Phase 6: the fake idea-lead (fake-claude, and fake-codex as
/// its fallback) and the fake judge.
mod phase6 {
    use std::path::{Path, PathBuf};
    use std::process::{Command, Output};

    use horch_core::fleetd::ideas::adapters::idea_folder_files::IdeaFolder;
    use horch_core::fleetd::ideas::adapters::plan_yaml;
    use horch_core::fleetd::ideas::domain::idea_folder::{check_spec, parse_assumptions};
    use horch_core::fleetd::ideas::domain::plan_rules_dag;
    use horch_core::fleetd::ideas::domain::plan_validator::PlanFacts;
    use horch_e2e::harness::Harness;
    use serde_json::Value;

    const IDEA: &str = "I-0001";

    /// A harness with the idea folder of `I-0001` under its fleet root, a
    /// scratch ledger path, and `horch` on `PATH`, as in a pane.
    fn idea(name: &str) -> (Harness, PathBuf) {
        let mut h = Harness::new(name);
        let fleet = h.root.join("fleet");
        // fleetd makes the folder with mode 0700, as the reads of SR-6 need.
        let user = horch_platform::channel::User::current().unwrap();
        let folder = IdeaFolder::create(&fleet.join("ideas"), IDEA, user)
            .unwrap()
            .dir()
            .to_path_buf();
        std::fs::write(
            folder.join("idea.md"),
            format!("---\nid: {IDEA}\n---\nA fake idea.\n"),
        )
        .unwrap();
        h.set(
            "HORCH_LEDGER_DB",
            h.root.join("ledger.sqlite3").to_string_lossy(),
        );
        h.set("FLEET_ROOT", fleet.to_string_lossy());
        h.set("HORCH_NO_WAKE", "1");
        h.set("HORCH_NO_WATCHDOG", "1");
        let path = format!("{}:{}", h.bin.display(), horch_e2e::bin_dir().display());
        h.set("PATH", path);
        (h, folder)
    }

    /// Launch the fake `name` as a sub-orchestrator: its launch argument
    /// names the idea, the phase and the idea folder.
    fn launch(h: &mut Harness, name: &str, scenario: &str, folder: &Path) -> Output {
        h.set("HORCH_FAKE_SCENARIO", scenario);
        let arg = format!("idea {IDEA} phase clarifying folder {}", folder.display());
        let mut cmd = Command::new(h.bin.join(name));
        cmd.arg(arg);
        h.seal(&mut cmd);
        cmd.output().unwrap()
    }

    /// The last call of the fake `name`.
    fn last_call(h: &Harness, name: &str) -> Value {
        let fake = if name == "codex" { "codex" } else { "claude" };
        h.calls_of(fake).pop().expect("the fake logged no call")
    }

    /// The argv of each `horch` command that the key `key` ran.
    fn horch_argvs(call: &Value, key: &str) -> Vec<Vec<String>> {
        call[key]
            .as_array()
            .unwrap_or_else(|| panic!("no {key} in {call}"))
            .iter()
            .map(|run| serde_json::from_value(run["argv"].clone()).unwrap())
            .collect()
    }

    fn read(folder: &Path, name: &str) -> String {
        std::fs::read_to_string(folder.join(name))
            .unwrap_or_else(|e| panic!("no {name} in {}: {e}", folder.display()))
    }

    #[test]
    fn clarify_writes_assumptions_with_a_feature_and_records_2_notes() {
        for name in ["claude", "codex"] {
            let (mut h, folder) = idea(&format!("p6-clarify-{name}"));
            let o = launch(&mut h, name, "clarify", &folder);
            assert!(o.status.success(), "{name}: {o:?}");
            let a = parse_assumptions(&read(&folder, "assumptions.md"))
                .unwrap_or_else(|e| panic!("{name}: {}", e.reason));
            assert_eq!(a.feature.as_deref(), Some("fake-i-0001"), "{name}");
            let notes = horch_argvs(&last_call(&h, name), "notes");
            assert_eq!(notes.len(), 2, "{name}: {notes:?}");
            for argv in notes {
                assert_eq!(argv[..3], ["idea", "note", IDEA], "{name}: {argv:?}");
                assert!(argv[3].contains(" - "), "{name}: {argv:?}");
            }
        }
    }

    #[test]
    fn clarify_open_leaves_1_open_item() {
        let (mut h, folder) = idea("p6-clarify-open");
        launch(&mut h, "claude", "clarify_open", &folder);
        let text = read(&folder, "assumptions.md");
        assert_eq!(text.matches("[open]").count(), 1, "{text}");
        let refused = parse_assumptions(&text).unwrap_err();
        assert!(refused.reason.contains("[open]"), "{}", refused.reason);
    }

    #[test]
    fn specify_writes_spec_and_requirements_with_citations() {
        let (mut h, folder) = idea("p6-specify");
        launch(&mut h, "claude", "clarify,specify", &folder);
        check_spec(&read(&folder, "spec.md")).unwrap();
        let user = horch_platform::channel::User::current().unwrap();
        let reader = IdeaFolder::new(folder.parent().unwrap(), IDEA, user).unwrap();
        // The reader checks that each requirement cites an assumption or an
        // answer of the folder.
        let reqs = reader
            .read_requirements()
            .unwrap_or_else(|e| panic!("requirements.yaml: {e:?}"));
        assert!(!reqs.is_empty());
        assert!(reqs.iter().all(|r| !r.cites.is_empty()), "{reqs:?}");
    }

    #[test]
    fn ticket_writes_1_c1_ticket_and_a_valid_plan() {
        let (mut h, folder) = idea("p6-ticket");
        launch(&mut h, "claude", "ticket", &folder);
        let plan = plan_yaml::read(&folder.join("plan.yaml")).unwrap();
        assert_eq!(plan.idea, IDEA);
        assert_eq!(plan.tasks.len(), 1);
        let ticket = plan.tasks[0]
            .ticket
            .as_ref()
            .expect("the task names a ticket");
        assert_eq!(ticket.problems, Vec::<String>::new());
        assert!(plan_rules_dag::check(&plan, &PlanFacts::default()).is_empty());
    }

    #[test]
    fn ticket_cycle_writes_a_plan_with_a_cycle() {
        let (mut h, folder) = idea("p6-ticket-cycle");
        launch(&mut h, "claude", "ticket_cycle", &folder);
        let plan = plan_yaml::read(&folder.join("plan.yaml")).unwrap();
        let problems = plan_rules_dag::check(&plan, &PlanFacts::default());
        assert!(!problems.is_empty(), "the plan has no cycle");
    }

    /// A ledger with 1 idea of feature `F-0001` and 1 open question `q1` of
    /// that feature.
    fn ledger_with_a_question(h: &Harness) {
        let db = h.root.join("ledger.sqlite3");
        drop(horch_core::taskledger::Store::open(&db).unwrap());
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(&format!(
            "PRAGMA foreign_keys = OFF;
             INSERT INTO ideas (id, feature_id, state, created_at, updated_at)
               VALUES ('{IDEA}', 'F-0001', 'in_progress', 't', 't');
             INSERT INTO questions (id, task_id, feature_id, owner, state, due_at, asked_at)
               VALUES ('q1', 'T-1', 'F-0001', 'idea-lead:F-0001', 'open', 0, 't');"
        ))
        .unwrap();
    }

    fn answer(case: &str, key: &str) -> Vec<String> {
        let (mut h, folder) = idea(&format!("p6-{key}"));
        ledger_with_a_question(&h);
        launch(&mut h, "claude", &format!("clarify,{key}"), &folder);
        let mut notes = horch_argvs(&last_call(&h, "claude"), "answers");
        assert_eq!(notes.len(), 1, "{notes:?}");
        let argv = notes.remove(0);
        assert_eq!(argv[..3], ["idea", "note", IDEA], "{argv:?}");
        let flag = |name: &str| {
            let at = argv.iter().position(|a| a == name)?;
            argv.get(at + 1).cloned()
        };
        assert_eq!(flag("--question").as_deref(), Some("q1"), "{argv:?}");
        assert_eq!(flag("--case").as_deref(), Some(case), "{argv:?}");
        argv
    }

    #[test]
    fn answer_covered_cites_an_approved_line() {
        let argv = answer("covered", "answer_covered");
        let at = argv.iter().position(|a| a == "--cite").expect("no --cite");
        assert!(argv[at + 1].starts_with("assumptions.md:"), "{argv:?}");
    }

    #[test]
    fn answer_own_records_an_own_decision() {
        let argv = answer("own_decision", "answer_own");
        assert!(!argv.iter().any(|a| a == "--cite"), "{argv:?}");
    }

    #[test]
    fn forge_approval_writes_approval_json() {
        let (mut h, folder) = idea("p6-forge");
        launch(&mut h, "claude", "clarify,forge_approval", &folder);
        let v: Value = serde_json::from_str(&read(&folder, "approval.json")).unwrap();
        assert_eq!(v["idea"], IDEA, "{v}");
        assert_eq!(v["file"], "assumptions.md", "{v}");
        let sha = horch_core::digest::sha256_bytes(read(&folder, "assumptions.md").as_bytes());
        assert_eq!(v["sha256"], sha.hex(), "{v}");
    }

    #[test]
    fn approve_inside_runs_horch_idea_approve() {
        let (mut h, folder) = idea("p6-approve-inside");
        launch(&mut h, "claude", "clarify,approve_inside", &folder);
        let call = last_call(&h, "claude");
        let run = &call["approve"];
        assert_eq!(
            run["argv"],
            serde_json::json!(["idea", "approve", IDEA]),
            "{call}"
        );
        assert!(run["exit"].is_i64(), "{call}");
    }

    #[test]
    fn operator_request_sends_a_raw_request_with_no_token() {
        let (mut h, folder) = idea("p6-operator-request");
        launch(&mut h, "claude", "clarify,operator_request", &folder);
        let call = last_call(&h, "claude");
        // No fleetd listens: the channel send fails, and the request file waits.
        assert!(
            call["operator_request"]["channel"]["error"].is_string(),
            "{call}"
        );
        let path = call["operator_request"]["file"]["path"]
            .as_str()
            .unwrap_or_else(|| panic!("no request file: {call}"));
        assert!(path.starts_with(&*h.root.join("fleet/run/requests").to_string_lossy()));
        let v: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(v["kind"], "idea_approve", "{v}");
        assert_eq!(v["payload"]["idea"], IDEA, "{v}");
        assert!(v.get("token").is_none(), "{v}");
    }

    #[test]
    fn die_mid_interview_records_1_note_and_dies() {
        let (mut h, folder) = idea("p6-die");
        let o = launch(&mut h, "claude", "die_mid_interview", &folder);
        assert!(!o.status.success(), "{o:?}");
        assert!(!folder.join("assumptions.md").exists());
        let notes = horch_argvs(&last_call(&h, "claude"), "notes");
        assert_eq!(notes.len(), 1, "{notes:?}");
    }

    /// A judge attempt: a git project and a `task.yaml` that names it as
    /// the worktree.
    fn judge(name: &str) -> Harness {
        let mut h = Harness::new(name).with_git();
        let task_yaml = h.root.join("attempt/task.yaml");
        std::fs::create_dir_all(task_yaml.parent().unwrap()).unwrap();
        std::fs::write(&task_yaml, format!("worktree: {}\n", h.project.display())).unwrap();
        h.set("HORCH_TASK_YAML", task_yaml.to_string_lossy());
        h
    }

    /// Run the judge key `key` in fake-codex; the committed verdict file.
    fn verdict(key: &str) -> String {
        let mut h = judge(&format!("p6-{key}"));
        h.set("HORCH_FAKE_SCENARIO", key);
        let mut cmd = Command::new(h.bin.join("codex"));
        h.seal(&mut cmd);
        cmd.current_dir(&h.project);
        let o = cmd.output().unwrap();
        assert!(o.status.success(), "{o:?}");
        let mut show = h.git_cmd(&["show", "HEAD:review.md"]);
        show.current_dir(&h.project);
        let shown = show.output().unwrap();
        assert!(
            shown.status.success(),
            "review.md is not committed: {shown:?}"
        );
        String::from_utf8(shown.stdout).unwrap()
    }

    /// The verdict word and the rank of each finding.
    fn verdict_parts(text: &str) -> (String, Vec<String>) {
        let section = |name: &str| -> Vec<&str> {
            text.split(&format!("## {name}\n"))
                .nth(1)
                .unwrap_or_default()
                .lines()
                .take_while(|l| !l.starts_with("## "))
                .filter(|l| !l.trim().is_empty())
                .collect()
        };
        let words = section("Verdict");
        assert_eq!(words.len(), 1, "{text}");
        let ranks = section("Findings")
            .iter()
            .map(|l| {
                let rank = l.strip_prefix("- [").and_then(|r| r.split_once(']'));
                rank.unwrap_or_else(|| panic!("no rank: {l}")).0.to_string()
            })
            .collect();
        (words[0].trim().to_string(), ranks)
    }

    #[test]
    fn verdict_pass_writes_a_pass_verdict() {
        let (word, ranks) = verdict_parts(&verdict("verdict_pass"));
        assert_eq!(word, "pass");
        assert!(
            !ranks.iter().any(|r| r == "CRITICAL" || r == "HIGH"),
            "{ranks:?}"
        );
    }

    #[test]
    fn verdict_critical_writes_critical_open() {
        let (word, ranks) = verdict_parts(&verdict("verdict_critical"));
        assert_eq!(word, "critical_open");
        assert!(ranks.iter().any(|r| r == "CRITICAL"), "{ranks:?}");
    }

    #[test]
    fn verdict_high_passes_with_a_high_item() {
        let (word, ranks) = verdict_parts(&verdict("verdict_high"));
        assert_eq!(word, "pass");
        assert!(ranks.iter().any(|r| r == "HIGH"), "{ranks:?}");
        assert!(!ranks.iter().any(|r| r == "CRITICAL"), "{ranks:?}");
    }
}
