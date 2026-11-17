//! The concern service: the service definitions of fleetd, install and
//! restart through the service manager, the systemd watchdog ping and the
//! home of the OS user (PL-2, PL-3, PL-5, FM-12).
//!
//! No test runs `launchctl`, `systemctl` or `schtasks`. Each test passes a
//! stand-in manager: a small script in a temp folder that writes its
//! arguments to a file.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use horch_platform::service::{self, Spec};

/// The variable that tells a child run of this test binary to do the child
/// part of a test.
const CHILD: &str = "HORCH_SERVICE_TEST_CHILD";

#[cfg(unix)]
const EXE: &str = "/opt/horch dir/horch";
#[cfg(windows)]
const EXE: &str = r"C:\horch dir\horch.exe";

#[cfg(unix)]
const LEDGER: &str = "/tmp/a&b <c>/ledger.sqlite3";

/// The spec of the tests. A Windows task carries no env, so the env is empty
/// there (`docs/tickets/fleetd-p1/MAP.md`, Q8: the service mode has the
/// default ledger and no `FLEET_ROOT`).
fn spec(folder: &Path) -> Spec {
    #[cfg(unix)]
    let env = vec![
        ("HORCH_LEDGER_DB".into(), LEDGER.into()),
        ("FLEET_ROOT".into(), "fleet-root".into()),
    ];
    #[cfg(windows)]
    let env = Vec::new();
    Spec {
        label: "horch-fleetd".into(),
        exe: PathBuf::from(EXE),
        env,
        folder: folder.to_path_buf(),
    }
}

/// The text of the definition file `name` of `spec`.
#[cfg(any(target_os = "linux", windows))]
fn definition(spec: &Spec, name: &str) -> String {
    let defs = service::definitions(spec).expect("definitions");
    defs.iter()
        .find(|(path, _)| path == &spec.folder.join(name))
        .map(|(_, text)| text.clone())
        .unwrap_or_else(|| panic!("no definition {name} in {defs:?}"))
}

/// Runs this test binary again for the test `test` only, with `CHILD` set.
fn run_child(test: &str, envs: &[(&str, &std::ffi::OsStr)]) -> Output {
    let out = Command::new(std::env::current_exe().expect("test exe"))
        .args([test, "--exact", "--nocapture", "--test-threads=1"])
        .env(CHILD, "1")
        .envs(envs.iter().copied())
        .output()
        .expect("run the child test");
    assert!(
        out.status.success(),
        "child {test} failed: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

fn is_child() -> bool {
    std::env::var_os(CHILD).is_some()
}

#[cfg(target_os = "linux")]
#[test]
fn linux_unit_has_restart_always_and_watchdogsec_30() {
    let dir = tempfile::tempdir().unwrap();
    let spec = spec(dir.path());
    let defs = service::definitions(&spec).unwrap();
    assert_eq!(defs.len(), 1, "a unit and no timer: {defs:?}");
    let unit = definition(&spec, "horch-fleetd.service");
    let lines: Vec<&str> = unit.lines().map(str::trim).collect();
    for line in [
        "[Service]",
        "Type=notify",
        "Restart=always",
        "WatchdogSec=30",
        r#"ExecStart="/opt/horch dir/horch" fleetd"#,
        r#"Environment="HORCH_LEDGER_DB=/tmp/a&b <c>/ledger.sqlite3""#,
        r#"Environment="FLEET_ROOT=fleet-root""#,
        "[Install]",
        "WantedBy=default.target",
    ] {
        assert!(lines.contains(&line), "{line:?} is not in:\n{unit}");
    }
}

#[cfg(target_os = "linux")]
#[test]
fn linux_unit_escapes_specifiers_and_quotes() {
    let dir = tempfile::tempdir().unwrap();
    let mut spec = spec(dir.path());
    spec.exe = PathBuf::from("/opt/50%\"$x\\/horch");
    spec.env = vec![("K".into(), "a%b\"c\\d$e".into())];
    let unit = definition(&spec, "horch-fleetd.service");
    let lines: Vec<&str> = unit.lines().map(str::trim).collect();
    assert!(
        lines.contains(&r#"ExecStart="/opt/50%%\"$$x\\/horch" fleetd"#),
        "{unit}"
    );
    assert!(lines.contains(&r#"Environment="K=a%%b\"c\\d$e""#), "{unit}");
}

/// The value at `key` of the plist `path`, as `plutil` reads it. `plutil`
/// only reads the file.
#[cfg(target_os = "macos")]
fn plist_value(path: &Path, key: &str) -> String {
    let out = Command::new("plutil")
        .args(["-extract", key, "raw", "-o", "-"])
        .arg(path)
        .output()
        .expect("run plutil");
    assert!(
        out.status.success(),
        "plutil -extract {key}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .unwrap()
        .trim_end()
        .to_string()
}

#[cfg(target_os = "macos")]
#[test]
fn mac_plists_have_keepalive_and_a_30_s_check() {
    let dir = tempfile::tempdir().unwrap();
    let spec = spec(dir.path());
    let defs = service::definitions(&spec).unwrap();
    assert_eq!(defs.len(), 2, "{defs:?}");
    for (path, text) in &defs {
        std::fs::write(path, text).unwrap();
    }
    let agent = dir.path().join("horch-fleetd.plist");
    let check = dir.path().join("horch-fleetd.check.plist");

    assert_eq!(plist_value(&agent, "Label"), "horch-fleetd");
    assert_eq!(plist_value(&agent, "ProgramArguments.0"), EXE);
    assert_eq!(plist_value(&agent, "ProgramArguments.1"), "fleetd");
    assert!(Command::new("plutil")
        .args(["-extract", "ProgramArguments.2", "raw", "-o", "-"])
        .arg(&agent)
        .output()
        .map(|o| !o.status.success())
        .unwrap());
    assert_eq!(plist_value(&agent, "KeepAlive"), "true");
    assert_eq!(plist_value(&agent, "RunAtLoad"), "true");
    assert_eq!(
        plist_value(&agent, "EnvironmentVariables.HORCH_LEDGER_DB"),
        LEDGER
    );
    assert_eq!(
        plist_value(&agent, "EnvironmentVariables.FLEET_ROOT"),
        "fleet-root"
    );

    assert_eq!(plist_value(&check, "Label"), "horch-fleetd.check");
    assert_eq!(plist_value(&check, "ProgramArguments.0"), EXE);
    assert_eq!(plist_value(&check, "ProgramArguments.1"), "fleetd");
    assert_eq!(plist_value(&check, "ProgramArguments.2"), "check");
    assert_eq!(plist_value(&check, "StartInterval"), "30");
    assert_eq!(
        plist_value(&check, "EnvironmentVariables.HORCH_LEDGER_DB"),
        LEDGER
    );
}

/// `xml` with the whitespace between tags removed.
#[cfg(windows)]
fn compact(xml: &str) -> String {
    xml.lines().map(str::trim).collect()
}

#[cfg(windows)]
#[test]
fn windows_tasks_run_fleetd_and_its_check() {
    let dir = tempfile::tempdir().unwrap();
    let spec = spec(dir.path());
    let defs = service::definitions(&spec).unwrap();
    assert_eq!(defs.len(), 2, "2 tasks: {defs:?}");
    let agent = compact(&definition(&spec, "horch-fleetd.xml"));
    let check = compact(&definition(&spec, "horch-fleetd.check.xml"));

    assert!(
        agent.contains("<LogonTrigger><Enabled>true</Enabled></LogonTrigger>"),
        "{agent}"
    );
    assert!(
        agent.contains(
            r"<Exec><Command>C:\horch dir\horch.exe</Command><Arguments>fleetd</Arguments></Exec>"
        ),
        "{agent}"
    );
    assert!(agent.contains("<RestartOnFailure>"), "{agent}");
    assert!(
        agent.contains("<MultipleInstancesPolicy>StopExisting</MultipleInstancesPolicy>"),
        "{agent}"
    );
    assert!(
        agent.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"),
        "{agent}"
    );
    assert!(
        agent.contains("<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>"),
        "{agent}"
    );
    assert!(
        check.contains(r"<Exec><Command>C:\horch dir\horch.exe</Command><Arguments>fleetd check</Arguments></Exec>"),
        "{check}"
    );
}

/// A Windows task carries no environment, so a spec with an env is refused.
#[cfg(windows)]
#[test]
fn windows_tasks_refuse_an_env() {
    let dir = tempfile::tempdir().unwrap();
    let mut spec = spec(dir.path());
    spec.env = vec![("FLEET_ROOT".into(), "fleet-root".into())];
    let err = service::definitions(&spec).expect_err("no env on Windows");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    assert!(err.to_string().contains("environment"), "{err}");
}

/// Task Scheduler refuses a repetition interval below `PT1M`, so the check
/// task has 2 triggers of `PT1M` whose start times are 30 s apart (PL-5).
#[cfg(windows)]
#[test]
fn windows_check_task_has_2_triggers_of_pt1m_30_s_apart() {
    let dir = tempfile::tempdir().unwrap();
    let check = compact(&definition(&spec(dir.path()), "horch-fleetd.check.xml"));
    assert!(!check.contains("PT30S"), "{check}");
    let triggers: Vec<&str> = check
        .split("<TimeTrigger>")
        .skip(1)
        .map(|t| t.split("</TimeTrigger>").next().unwrap())
        .collect();
    assert_eq!(triggers.len(), 2, "{check}");
    let starts: Vec<&str> = triggers
        .into_iter()
        .map(|t| {
            // The schema order of a trigger (`triggerBaseType`, an
            // `xs:sequence`): `Enabled`, `StartBoundary`, `Repetition`.
            let rest = t
                .strip_prefix("<Enabled>true</Enabled><StartBoundary>")
                .unwrap_or_else(|| panic!("Enabled, then StartBoundary: {t}"));
            let (start, rest) = rest.split_once("</StartBoundary>").unwrap();
            assert_eq!(rest, "<Repetition><Interval>PT1M</Interval></Repetition>");
            start
        })
        .collect();
    assert_eq!(starts, ["2000-01-01T00:00:00", "2000-01-01T00:00:30"]);
}

/// A stand-in manager script in `dir`. Each call appends 1 line
/// `<args> | <count of files in folder>` to the log.
#[cfg(unix)]
fn stand_in(dir: &Path, folder: &Path, status: i32) -> (PathBuf, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let log = dir.join("calls.log");
    let script = dir.join("manager");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\necho \"$* | $(ls '{}' | wc -l | tr -d ' ')\" >> '{}'\nexit {status}\n",
            folder.display(),
            log.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    (script, log)
}

#[cfg(windows)]
fn stand_in(dir: &Path, folder: &Path, status: i32) -> (PathBuf, PathBuf) {
    let log = dir.join("calls.log");
    let script = dir.join("manager.cmd");
    std::fs::write(
        &script,
        format!(
            "@echo off\r\nset n=0\r\nfor %%f in (\"{}\\*\") do set /a n+=1\r\necho %* ^| %n%>> \"{}\"\r\nexit /b {status}\r\n",
            folder.display(),
            log.display()
        ),
    )
    .unwrap();
    (script, log)
}

fn calls(log: &Path) -> Vec<String> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_string())
        .collect()
}

#[cfg(target_os = "macos")]
fn uid() -> String {
    let out = Command::new("id").arg("-u").output().unwrap();
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

/// The text of a written definition file: UTF-8, or UTF-16LE with a byte
/// order mark for a Windows task XML.
fn read_definition(path: &Path) -> String {
    let bytes = std::fs::read(path).unwrap();
    #[cfg(windows)]
    {
        assert_eq!(
            &bytes[..2],
            [0xFF, 0xFE],
            "a UTF-16LE BOM: {}",
            path.display()
        );
        let units: Vec<u16> = bytes[2..]
            .chunks(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16(&units).unwrap()
    }
    #[cfg(unix)]
    String::from_utf8(bytes).unwrap()
}

/// The manager calls of `install` on this OS, for the definitions in
/// `folder`.
#[cfg(target_os = "macos")]
fn expected_install_calls(folder: &Path) -> Vec<String> {
    ["horch-fleetd.plist", "horch-fleetd.check.plist"]
        .iter()
        .map(|name| format!("bootstrap gui/{} {}", uid(), folder.join(name).display()))
        .collect()
}

#[cfg(target_os = "linux")]
fn expected_install_calls(_folder: &Path) -> Vec<String> {
    vec![
        "--user daemon-reload".to_string(),
        "--user enable --now horch-fleetd.service".to_string(),
    ]
}

#[cfg(windows)]
fn expected_install_calls(folder: &Path) -> Vec<String> {
    let mut calls: Vec<String> = ["horch-fleetd", "horch-fleetd.check"]
        .iter()
        .map(|name| {
            let xml = folder.join(format!("{name}.xml"));
            format!("/Create /XML {} /TN {name} /F", xml.display())
        })
        .collect();
    calls.push("/Run /TN horch-fleetd".to_string());
    calls
}

#[test]
fn install_writes_the_files_then_calls_the_manager() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("defs");
    let spec = spec(&folder);
    let (manager, log) = stand_in(dir.path(), &folder, 0);

    service::install(&spec, &manager).expect("install");

    let defs = service::definitions(&spec).unwrap();
    for (path, text) in &defs {
        assert_eq!(read_definition(path), *text, "{}", path.display());
    }
    // Each call logs the count of files in the folder: all of them exist.
    let suffix = format!(" | {}", defs.len());
    let args: Vec<String> = calls(&log)
        .iter()
        .map(|c| {
            let args = c.strip_suffix(&suffix);
            // `%*` of the Windows stand-in shows the quotes of a path.
            args.unwrap_or_else(|| panic!("a call before the files: {c}"))
                .replace('"', "")
        })
        .collect();
    assert_eq!(args, expected_install_calls(&folder));
}

#[test]
fn install_fails_when_the_manager_fails() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("defs");
    let (manager, log) = stand_in(dir.path(), &folder, 3);
    let err = service::install(&spec(&folder), &manager).expect_err("the manager failed");
    let text = err.to_string();
    assert!(text.contains(&manager.display().to_string()), "{text}");
    assert_eq!(calls(&log).len(), 1, "install stops at the first failure");
}

/// On Windows, `/Run` alone restarts the task: its policy `StopExisting`
/// stops the old instance first, and a task that is not running starts.
#[test]
fn restart_calls_the_manager_once() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("defs");
    std::fs::create_dir(&folder).unwrap();
    let (manager, log) = stand_in(dir.path(), &folder, 0);
    service::restart(&spec(&folder), &manager).expect("restart");
    let calls = calls(&log);
    #[cfg(target_os = "macos")]
    assert_eq!(
        calls,
        [format!("kickstart -k gui/{}/horch-fleetd | 0", uid())]
    );
    #[cfg(target_os = "linux")]
    assert_eq!(calls, ["--user restart horch-fleetd.service | 0"]);
    #[cfg(windows)]
    assert_eq!(calls, ["/Run /TN horch-fleetd | 0"]);
}

#[cfg(not(target_os = "linux"))]
#[test]
fn watchdog_ping_is_not_supported_off_linux() {
    let err = service::watchdog_ping().expect_err("no systemd watchdog");
    assert_eq!(err.concern, "service");
    assert_eq!(err.os, std::env::consts::OS);
}

/// The child part of the watchdog tests: 2 pings.
#[cfg(target_os = "linux")]
fn ping_twice() {
    for _ in 0..2 {
        service::watchdog_ping()
            .expect("supported on Linux")
            .expect("the ping is sent");
    }
}

/// Runs `test` as a child with `NOTIFY_SOCKET` set to `name`, and checks
/// the 2 datagrams that `socket` gets.
#[cfg(target_os = "linux")]
fn check_pings(test: &str, name: &str, socket: &std::os::unix::net::UnixDatagram) {
    socket
        .set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .unwrap();
    run_child(test, &[("NOTIFY_SOCKET", name.as_ref())]);
    let mut buf = [0u8; 256];
    let mut recv = || {
        let n = socket.recv(&mut buf).expect("a datagram");
        String::from_utf8(buf[..n].to_vec()).unwrap()
    };
    let first = recv();
    let mut first_lines: Vec<&str> = first.lines().collect();
    first_lines.sort_unstable();
    assert_eq!(first_lines, ["READY=1", "WATCHDOG=1"], "{first:?}");
    assert_eq!(recv(), "WATCHDOG=1", "READY=1 is sent once");
}

#[cfg(target_os = "linux")]
#[test]
fn watchdog_ping_sends_watchdog_1_to_notify_socket() {
    if is_child() {
        return ping_twice();
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("notify");
    let socket = std::os::unix::net::UnixDatagram::bind(&path).unwrap();
    check_pings(
        "watchdog_ping_sends_watchdog_1_to_notify_socket",
        path.to_str().unwrap(),
        &socket,
    );
}

#[cfg(target_os = "linux")]
#[test]
fn watchdog_ping_sends_to_an_abstract_notify_socket() {
    use std::os::linux::net::SocketAddrExt;
    if is_child() {
        return ping_twice();
    }
    let name = format!("horch-service-test-{}", std::process::id());
    let addr = std::os::unix::net::SocketAddr::from_abstract_name(name.as_bytes()).unwrap();
    let socket = std::os::unix::net::UnixDatagram::bind_addr(&addr).unwrap();
    check_pings(
        "watchdog_ping_sends_to_an_abstract_notify_socket",
        &format!("@{name}"),
        &socket,
    );
}

#[test]
fn os_user_home_ignores_home_env() {
    if is_child() {
        // The harness prints the test name before it on the same line.
        println!("\nHOME_IS={}", service::os_user_home().unwrap().display());
        return;
    }
    let home = service::os_user_home().expect("the home of the OS user");
    assert!(home.is_absolute(), "{}", home.display());
    assert!(home.is_dir(), "{}", home.display());

    let fake = tempfile::tempdir().unwrap();
    let out = run_child(
        "os_user_home_ignores_home_env",
        &[
            ("HOME", fake.path().as_os_str()),
            ("USERPROFILE", fake.path().as_os_str()),
        ],
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    let child_home = stdout
        .lines()
        .find_map(|l| l.strip_prefix("HOME_IS="))
        .unwrap_or_else(|| panic!("no HOME_IS line in {stdout}"));
    assert_eq!(Path::new(child_home), home);
    assert_ne!(Path::new(child_home), fake.path());
}

/// The manager command of this OS is the 1 that [`service::install`] and
/// [`service::restart`] expect.
#[test]
fn manager_is_the_service_manager_of_this_os() {
    #[cfg(target_os = "macos")]
    let want = "launchctl";
    #[cfg(target_os = "linux")]
    let want = "systemctl";
    #[cfg(windows)]
    let want = "schtasks";
    assert_eq!(service::MANAGER, want);
}

/// The definition folder is in the home of the OS user, never in `HOME`.
#[test]
fn default_folder_is_in_the_os_user_home() {
    let home = service::os_user_home().expect("the home of the OS user");
    #[cfg(target_os = "macos")]
    let want = home.join("Library").join("LaunchAgents");
    #[cfg(target_os = "linux")]
    let want = home.join(".config").join("systemd").join("user");
    #[cfg(windows)]
    let want = home
        .join("AppData")
        .join("Local")
        .join("horch")
        .join("service");
    assert_eq!(service::default_folder().expect("the folder"), want);
}

/// The restart of the service of [`service::LABEL`] in
/// [`service::default_folder`] is 1 call of the manager, here a stand-in.
#[test]
fn restart_of_the_fleetd_label_calls_the_manager() {
    let dir = tempfile::tempdir().unwrap();
    let (manager, log) = stand_in(dir.path(), dir.path(), 0);
    let spec = Spec {
        label: service::LABEL.into(),
        exe: PathBuf::from(EXE),
        env: Vec::new(),
        folder: service::default_folder().expect("the folder"),
    };
    service::restart(&spec, &manager).expect("restart");
    let calls = calls(&log);
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert!(calls[0].contains(service::LABEL), "{calls:?}");
}
