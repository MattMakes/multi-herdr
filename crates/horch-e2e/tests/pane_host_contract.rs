//! Shared `PaneHost` contract suite run against both `fake-herdr` and
//! `fake-hc` + `fake-herdr-app`, plus an opt-in run against a real isolated
//! `herdr-core` (`HORCH_LIVE_HC=1`) (HCB-14).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use horch_core::runtime::{Backend, MapEnv, RuntimeContext};
use horch_core::workspace::model::Direction;
use horch_core::workspace::PaneHost;
use horch_e2e::harness::Harness;

/// Wrap a fake binary in `<h.bin>/<name>` so direct `PaneHost` calls from this
/// test process inherit the harness's isolated `HORCH_STATE_DIR`, `HOME`, and
/// `HORCH_FAKE_SCENARIO` without mutating process-global environment variables.
#[cfg(unix)]
fn install_sealed_wrapper(h: &Harness, name: &str, scenario: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let target = std::fs::read_link(h.bin.join(name))
        .unwrap_or_else(|_| h.bin.join(name))
        .canonicalize()
        .expect("canonicalizing fake binary");
    let wrapper_dir = h.root.join("wrapped-bin");
    std::fs::create_dir_all(&wrapper_dir).unwrap();
    let wrapper = wrapper_dir.join(name);
    let script = format!(
        "#!/bin/sh\n\
         if [ \"$1\" = \"__warm__\" ]; then exit 0; fi\n\
         export HOME='{home}'\n\
         export PATH='{bin}:/usr/bin:/bin'\n\
         export HORCH_STATE_DIR='{state}'\n\
         export HORCH_FAKE_LOG='{log}'\n\
         export HORCH_FAKE_SCENARIO='{scenario}'\n\
         export HERDR_APP_HOME='{app_home}'\n\
         export HC_TERMINAL_SOCKET='{hc_sock}'\n\
         exec '{target}' \"$@\"\n",
        home = h.home.display(),
        bin = h.bin.display(),
        state = h.state.display(),
        log = h.log.display(),
        scenario = scenario,
        app_home = h.home.join(".config/herdr-app").display(),
        hc_sock = h.hc_socket().display(),
        target = target.display(),
    );
    std::fs::write(&wrapper, script).unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let warmed = horch_e2e::harness::warm(Command::new(&wrapper).arg("__warm__"))
        .expect("warming sealed wrapper");
    assert!(warmed.status.success());
    wrapper
}

fn wait_for_visible(host: &PaneHost, pane: &str, needle: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let text = host.pane_read(pane, "visible").unwrap_or_default();
        if text.contains(needle) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "did not observe {needle:?} in pane {pane}; last visible screen:\n{text}"
        );
        std::thread::sleep(Duration::from_millis(40));
    }
}

/// Shared scenarios over an already-created workspace and its root pane.
/// Needs no GUI window once the root pane exists, so the opt-in real-core run
/// executes the exact same assertions.
fn run_scenarios_in_workspace(host: &PaneHost, workspace_id: &str, root_pane: &str) {
    assert!(host.server_reachable(), "host should be reachable");

    // Root pane's workspace_id matches the workspace that made it.
    let root_info = host.pane_get(root_pane).expect("pane_get root");
    assert_eq!(root_info.pane_id, root_pane);
    assert_eq!(root_info.workspace_id.as_deref(), Some(workspace_id));

    // Split right, then down, and list 3 panes in that workspace.
    let right_pane = host
        .pane_split(root_pane, Direction::Right)
        .expect("pane_split right");
    let down_pane = host
        .pane_split(&right_pane, Direction::Down)
        .expect("pane_split down");
    let listed = host.pane_list(workspace_id).expect("pane_list");
    assert_eq!(listed.len(), 3, "expected 3 panes, got: {listed:#?}");
    for expected in [root_pane, right_pane.as_str(), down_pane.as_str()] {
        let found = listed
            .iter()
            .find(|p| p.pane_id == expected)
            .unwrap_or_else(|| panic!("missing pane {expected} in {listed:#?}"));
        assert_eq!(found.workspace_id.as_deref(), Some(workspace_id));
    }

    // Run a command and read its evaluated output back with pane_read(.., "visible").
    host.pane_run(&right_pane, "echo CONTRACT_RUN_$((40+2))")
        .expect("pane_run");
    wait_for_visible(host, &right_pane, "CONTRACT_RUN_42");

    // Prompt via the shared delivery path, see the evaluated output on screen,
    // then press enter.
    let timing = horch_core::messaging::delivery::Timing {
        tail_timeout: Duration::from_millis(250),
        poll_start: Duration::from_millis(20),
        poll_max: Duration::from_millis(50),
        settle_short: Duration::from_millis(50),
        settle_long: Duration::from_millis(50),
        second_enter: Duration::from_millis(50),
    };
    horch_core::messaging::delivery::send_line_with(
        host,
        &down_pane,
        horch_core::messaging::delivery::TypedLine::DeliveryEchoPosix,
        &timing,
    )
    .expect("send_line_with prompt");
    wait_for_visible(host, &down_pane, "HORCH_DELIVERY_73");
    host.pane_send_keys(&down_pane, "enter")
        .expect("pane_send_keys enter");

    // Closing a pane makes pane_get fail while server_reachable stays true.
    host.pane_close(&down_pane).expect("pane_close");
    assert!(
        host.pane_get(&down_pane).is_err(),
        "pane_get on closed pane {down_pane} must fail"
    );
    assert!(
        host.server_reachable(),
        "server_reachable must stay true after closing one pane"
    );

    // Closing the workspace removes it from workspace_list.
    host.workspace_close(workspace_id).expect("workspace_close");
    let workspaces = host.workspace_list().expect("workspace_list");
    assert!(
        !workspaces.iter().any(|w| w.workspace_id == workspace_id),
        "closed workspace {workspace_id} still in {workspaces:#?}"
    );
}

fn run_full_contract(host: &PaneHost, cwd: &Path) {
    let cwd_str = cwd.to_string_lossy();
    let ws = host
        .workspace_create("contract-ws", Some(&cwd_str), false)
        .expect("workspace_create");
    run_scenarios_in_workspace(host, &ws.workspace_id, &ws.root_pane_id);
}

#[cfg(unix)]
#[test]
fn hcb_14_contract_on_herdr() {
    let h = Harness::new("hcb14-herdr");
    let herdr_bin = install_sealed_wrapper(&h, "herdr", "exec,shell");
    let env = MapEnv::new(&h.project)
        .with("HOME", &h.home.to_string_lossy())
        .with("HORCH_STATE_DIR", &h.state.to_string_lossy())
        .with("HORCH_HERDR_BIN", &herdr_bin.to_string_lossy());
    let ctx = RuntimeContext::from_env(&env).unwrap();
    let host = PaneHost::for_backend(&ctx, Backend::Herdr).with_timeout(Duration::from_secs(10));
    run_full_contract(&host, &h.project);
}

#[cfg(unix)]
#[test]
fn hcb_14_contract_on_hc() {
    let h = Harness::new("hcb14-hc");
    let hc_bin = install_sealed_wrapper(&h, "hc", "exec,shell");
    let app_bin = install_sealed_wrapper(&h, "herdr-app", "exec,shell");
    let env = MapEnv::new(&h.project)
        .with("HOME", &h.home.to_string_lossy())
        .with("HORCH_STATE_DIR", &h.state.to_string_lossy())
        .with("HORCH_BACKEND", "hc")
        .with("HORCH_HC_BIN", &hc_bin.to_string_lossy())
        .with("HORCH_HERDR_APP_BIN", &app_bin.to_string_lossy())
        .with("HC_TERMINAL_SOCKET", &h.hc_socket().to_string_lossy())
        .with(
            "HERDR_APP_HOME",
            &h.home.join(".config/herdr-app").to_string_lossy(),
        );
    let ctx = RuntimeContext::from_env(&env).unwrap();
    let host = PaneHost::for_backend(&ctx, Backend::Hc).with_timeout(Duration::from_secs(10));
    run_full_contract(&host, &h.project);
}

#[cfg(unix)]
fn find_real_hc() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join("hc");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(unix)]
fn operator_core_status(real_hc: &Path) -> Option<String> {
    let out = Command::new(real_hc)
        .args(["server", "status"])
        .env_remove("HC_TERMINAL_ID")
        .env_remove("HC_TERMINAL_SOCKET")
        .env_remove("HC_SCOPE")
        .env_remove("HC_SESSION")
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(unix)]
fn core_pid_from_status(line: &str) -> Option<u32> {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(line.trim()) {
        if let Some(pid) = v["pid"].as_u64().and_then(|n| u32::try_from(n).ok()) {
            return Some(pid);
        }
    }
    // TTY format: `core <host> (<hash>) running, pid 23875, up 21188s`
    let (_, after) = line.split_once("pid ")?;
    let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

#[cfg(unix)]
fn find_socket_under(root: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_socket_under(&path) {
                return Some(found);
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some("sock") {
            return Some(path);
        }
    }
    None
}

#[cfg(unix)]
struct IsolatedCoreGuard {
    wrapper_hc: PathBuf,
    scratch: PathBuf,
    operator_pid: Option<u32>,
}

#[cfg(unix)]
impl Drop for IsolatedCoreGuard {
    fn drop(&mut self) {
        if let Ok(out) = Command::new(&self.wrapper_hc)
            .args(["server", "status"])
            .output()
        {
            let status = String::from_utf8_lossy(&out.stdout);
            let isolated_pid = core_pid_from_status(&status);
            if out.status.success() && isolated_pid.is_some() && isolated_pid != self.operator_pid {
                let _ = Command::new(&self.wrapper_hc)
                    .args(["server", "stop"])
                    .output();
            }
        }
        let _ = std::fs::remove_dir_all(&self.scratch);
    }
}

#[cfg(unix)]
#[test]
#[ignore = "opt-in real herdr-core check; run with HORCH_LIVE_HC=1"]
fn hcb_14_contract_on_a_real_hc_core() {
    use std::os::unix::fs::PermissionsExt;

    if std::env::var_os("HORCH_LIVE_HC").is_none_or(|v| v != "1") {
        eprintln!("skipped: set HORCH_LIVE_HC=1 to run against a real isolated hc core");
        return;
    }
    let real_hc = find_real_hc().expect("HORCH_LIVE_HC=1 set, but `hc` was not found on PATH");
    let before_operator = operator_core_status(&real_hc);
    let operator_pid = before_operator.as_deref().and_then(core_pid_from_status);

    let pid = std::process::id();
    // Keep scratch path short so Unix domain sockets stay well below macOS's
    // 104-byte sun_path limit.
    let scratch = PathBuf::from(format!("/tmp/hc-c-{pid}"));
    let _ = std::fs::remove_dir_all(&scratch);
    let state_home = scratch.join("s");
    let config_home = scratch.join("c");
    let data_home = scratch.join("d");
    let bin_dir = scratch.join("b");
    let work_dir = scratch.join("w");
    for d in [&state_home, &config_home, &data_home, &bin_dir, &work_dir] {
        std::fs::create_dir_all(d).unwrap();
    }
    let session = format!("horch-contract-{pid}");
    let wrapper_hc = bin_dir.join("hc");
    let hc_script = format!(
        "#!/bin/sh\n\
         unset HC_TERMINAL_ID HC_TERMINAL_SOCKET HC_SCOPE\n\
         export HC_SESSION='{session}'\n\
         export XDG_STATE_HOME='{state}'\n\
         export XDG_CONFIG_HOME='{config}'\n\
         export XDG_DATA_HOME='{data}'\n\
         exec '{real_hc}' \"$@\"\n",
        session = session,
        state = state_home.display(),
        config = config_home.display(),
        data = data_home.display(),
        real_hc = real_hc.display(),
    );
    std::fs::write(&wrapper_hc, hc_script).unwrap();
    std::fs::set_permissions(&wrapper_hc, std::fs::Permissions::from_mode(0o755)).unwrap();

    // Windowless `herdr-app` stand-in: exits 1 so `HcApp` exercises its
    // windowless labelled `hc` paths without touching the operator's window.
    let wrapper_app = bin_dir.join("herdr-app");
    std::fs::write(
        &wrapper_app,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'herdr-app 0.1.16'; exit 0; fi\n\
         echo 'herdr-app: no window in contract test' >&2\nexit 1\n",
    )
    .unwrap();
    std::fs::set_permissions(&wrapper_app, std::fs::Permissions::from_mode(0o755)).unwrap();

    let _guard = IsolatedCoreGuard {
        wrapper_hc: wrapper_hc.clone(),
        scratch: scratch.clone(),
        operator_pid,
    };

    // Start the isolated core and confirm its pid differs from the operator's.
    let started = Command::new(&wrapper_hc)
        .args(["server", "status"])
        .output()
        .expect("starting isolated hc core");
    assert!(
        started.status.success(),
        "isolated hc server status failed: {}{}",
        String::from_utf8_lossy(&started.stdout),
        String::from_utf8_lossy(&started.stderr)
    );
    let isolated_status = String::from_utf8_lossy(&started.stdout).trim().to_string();
    let isolated_pid = core_pid_from_status(&isolated_status)
        .unwrap_or_else(|| panic!("could not parse pid from {isolated_status:?}"));
    if let Some(op_pid) = operator_pid {
        assert_ne!(
            isolated_pid, op_pid,
            "isolated core must never reuse operator core pid"
        );
    }

    let sock = find_socket_under(&scratch)
        .unwrap_or_else(|| panic!("no .sock file created under {}", scratch.display()));

    // Seed the windowless workspace's root terminal via a labelled `hc run`.
    let ws_id = "spcontract0000000000000000000001";
    let tb_id = "tbcontract0000000000000000000001";
    let seeded = Command::new(&wrapper_hc)
        .args([
            "run",
            "--bg",
            "--json",
            "--keep",
            "--no-parent",
            "--cwd",
            &work_dir.to_string_lossy(),
            "--label",
            &format!("herdr.space={ws_id}"),
            "--label",
            &format!("herdr.tab={tb_id}"),
        ])
        .output()
        .expect("seeding root terminal on isolated core");
    assert!(
        seeded.status.success(),
        "{}",
        String::from_utf8_lossy(&seeded.stderr)
    );
    let seeded_json: serde_json::Value = serde_json::from_slice(&seeded.stdout).unwrap();
    let root_term = seeded_json["address"].as_str().unwrap().to_string();
    // Give the login shell inside the new PTY a brief moment to print its prompt.
    std::thread::sleep(Duration::from_millis(400));

    let env = MapEnv::new(&work_dir)
        .with("HOME", &scratch.to_string_lossy())
        .with(
            "HORCH_STATE_DIR",
            &scratch.join("horch-state").to_string_lossy(),
        )
        .with("HORCH_BACKEND", "hc")
        .with("HORCH_HC_BIN", &wrapper_hc.to_string_lossy())
        .with("HORCH_HERDR_APP_BIN", &wrapper_app.to_string_lossy())
        .with("HC_TERMINAL_SOCKET", &sock.to_string_lossy())
        .with("HERDR_APP_HOME", &scratch.join("no-app").to_string_lossy());
    let ctx = RuntimeContext::from_env(&env).unwrap();
    let host = PaneHost::for_backend(&ctx, Backend::Hc).with_timeout(Duration::from_secs(15));

    run_scenarios_in_workspace(&host, ws_id, &root_term);

    drop(_guard);

    if let Some(op_pid) = operator_pid {
        let after = operator_core_status(&real_hc).expect("operator core still running");
        assert_eq!(
            core_pid_from_status(&after),
            Some(op_pid),
            "operator core pid changed! before={before_operator:?}, after={after:?}"
        );
    }
}
