use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::{json, Value};

pub const DETACHED_ENV: &str = "HORCH_FAKE_HC_DETACHED";

const HOST_VARS: [&str; 12] = [
    "HORCH_BACKEND",
    "HERDR_PANE_ID",
    "HERDR_SOCKET_PATH",
    "HC_TERMINAL_ID",
    "HC_SCOPE",
    "HC_TERMINAL_SOCKET",
    "HERDR_APP_HOME",
    "HORCH_HC_BIN",
    "HORCH_HERDR_APP_BIN",
    "HORCH_HERDR_BIN",
    "XDG_STATE_HOME",
    "HORCH_STATE_DIR",
];

static KILL: Mutex<Vec<(u64, Option<u64>)>> = Mutex::new(Vec::new());

pub fn detach_into_own_group() {
    #[cfg(unix)]
    if std::env::var_os(DETACHED_ENV).is_none() {
        use std::os::unix::process::CommandExt;
        let code = std::env::current_exe()
            .and_then(|exe| {
                std::process::Command::new(exe)
                    .args(std::env::args_os().skip(1))
                    .env(DETACHED_ENV, "1")
                    .process_group(0)
                    .status()
            })
            .map(|s| s.code().unwrap_or(1))
            .unwrap_or(1);
        std::process::exit(code);
    }
    std::env::remove_var(DETACHED_ENV);
}

pub fn flush_kills() {
    for (pgid, started) in std::mem::take(&mut *KILL.lock().unwrap()) {
        kill_group(pgid, started);
    }
}

pub fn queue_kill(pgid: u64, started: Option<u64>) {
    KILL.lock().unwrap().push((pgid, started));
}

pub fn state_dir() -> Option<PathBuf> {
    std::env::var_os("HORCH_STATE_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HORCH_FAKE_LOG")
                .and_then(|p| PathBuf::from(p).parent().map(|d| d.join("state")))
        })
}

pub fn state_file_in(dir: &Path) -> PathBuf {
    dir.join("fake-hc.json")
}

pub fn log_file_in(dir: &Path) -> PathBuf {
    dir.join("fake-hc.log")
}

pub fn window_closed_in(dir: &Path) -> PathBuf {
    dir.join("window-closed")
}

pub fn core_stopped_in(dir: &Path) -> PathBuf {
    dir.join("hc-core-stopped")
}

pub fn term_out_in(dir: &Path, address: &str) -> PathBuf {
    dir.join(format!("fake-hc.term-{address}.out"))
}

pub fn log_call(program: &str, argv: &[String]) {
    let Some(dir) = state_dir() else {
        return;
    };
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let mut host_env = BTreeMap::new();
    for key in HOST_VARS {
        if let Ok(val) = std::env::var(key) {
            host_env.insert(key, val);
        }
    }
    let mut env_keys: Vec<String> = std::env::vars_os()
        .map(|(k, _)| k.to_string_lossy().into_owned())
        .collect();
    env_keys.sort();
    let line = json!({
        "program": program,
        "pid": std::process::id(),
        "call_id": format!("{}-{nanos}", std::process::id()),
        "argv": argv,
        "host_env": host_env,
        "env_keys": env_keys,
    });
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file_in(&dir))
    {
        let _ = writeln!(f, "{line}");
    }
}

pub struct StateLock(Option<PathBuf>);

const NO_PID_GRACE: std::time::Duration = std::time::Duration::from_secs(10);
const HOLDER_CHECK: std::time::Duration = std::time::Duration::from_secs(1);

impl StateLock {
    pub fn acquire() -> Self {
        let Some(dir) = state_dir() else {
            return Self(None);
        };
        Self::acquire_in(&dir)
    }

    pub fn acquire_in(state_dir: &Path) -> Self {
        let _ = std::fs::create_dir_all(state_dir);
        let lock = state_dir.join("fake-hc.lock");
        let mut checked = std::time::Instant::now();
        loop {
            match std::fs::create_dir(&lock) {
                Ok(()) => {
                    let _ = std::fs::write(lock.join("pid"), std::process::id().to_string());
                    return Self(Some(lock));
                }
                Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists => {
                    eprintln!("fake-hc: cannot lock {}: {e}", lock.display());
                    std::process::exit(1);
                }
                Err(_) => {}
            }
            if checked.elapsed() >= HOLDER_CHECK {
                checked = std::time::Instant::now();
                if holder_is_dead(&lock) {
                    let _ = std::fs::remove_file(lock.join("pid"));
                    let _ = std::fs::remove_dir(&lock);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}

impl Drop for StateLock {
    fn drop(&mut self) {
        if let Some(lock) = &self.0 {
            let _ = std::fs::remove_file(lock.join("pid"));
            let _ = std::fs::remove_dir(lock);
        }
    }
}

fn holder_is_dead(dir: &Path) -> bool {
    match std::fs::read_to_string(dir.join("pid"))
        .ok()
        .and_then(|p| p.trim().parse::<u32>().ok())
    {
        Some(pid) => !process_runs(pid),
        None => std::fs::metadata(dir)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > NO_PID_GRACE),
    }
}

fn process_runs(pid: u32) -> bool {
    if cfg!(windows) {
        return true;
    }
    std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(format!("kill -0 {pid} 2>/dev/null"))
        .status()
        .map_or(true, |s| s.success())
}

pub fn empty_state() -> Value {
    json!({
        "next_term": 1,
        "next_pane": 1,
        "next_space": 1,
        "next_tab": 1,
        "terminals": [],
        "spaces": [],
        "pids": {},
        "started": {},
    })
}

pub fn load_in(dir: &Path) -> Value {
    std::fs::read_to_string(state_file_in(dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(empty_state)
}

pub fn load() -> Value {
    state_dir().map_or_else(empty_state, |d| load_in(&d))
}

pub fn save_in(dir: &Path, state: &Value) {
    let path = state_file_in(dir);
    let tmp = dir.join(format!(".fake-hc.json.tmp.{}", std::process::id()));
    if std::fs::write(&tmp, state.to_string()).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

pub fn save(state: &Value) {
    if let Some(dir) = state_dir() {
        save_in(&dir, state);
    }
}

pub fn next_counter(state: &mut Value, key: &str) -> u64 {
    let n = state[key].as_u64().unwrap_or(1);
    state[key] = json!(n + 1);
    n
}

pub fn mint_space_id(state: &mut Value) -> String {
    let n = next_counter(state, "next_space");
    format!("sp{n:032x}")
}

pub fn mint_tab_id(state: &mut Value) -> String {
    let n = next_counter(state, "next_tab");
    format!("tb{n:032x}")
}

pub fn mint_pane_id(state: &mut Value) -> String {
    let n = next_counter(state, "next_pane");
    format!("p{n}")
}

pub struct NewTerminal<'a> {
    pub agent: &'a str,
    pub argv: Vec<String>,
    pub cwd: String,
    pub keep: bool,
    pub parent: Option<String>,
    pub name: Option<String>,
    pub labels: BTreeMap<String, String>,
}

pub fn create_terminal(state: &mut Value, spec: NewTerminal<'_>) -> Value {
    let n = next_counter(state, "next_term");
    let address = format!("t{n}");
    let term = json!({
        "address": address,
        "agent": spec.agent,
        "argv": spec.argv,
        "cwd": spec.cwd,
        "foreground": Value::Null,
        "keep": spec.keep,
        "labels": spec.labels,
        "name": spec.name,
        "parent": spec.parent,
        "pid": 10000 + n,
        "reported": false,
        "state": "live",
        "status": Value::Null,
        "status_ms": Value::Null,
        "turn": 0,
        "sent": [],
        "keys": [],
        "test_screen": Value::Null,
    });
    state["terminals"]
        .as_array_mut()
        .unwrap()
        .push(term.clone());
    term
}

pub fn find_terminal<'a>(state: &'a Value, address: &str) -> Option<&'a Value> {
    state["terminals"]
        .as_array()?
        .iter()
        .find(|t| t["address"].as_str() == Some(address))
}

pub fn find_terminal_mut<'a>(state: &'a mut Value, address: &str) -> Option<&'a mut Value> {
    state["terminals"]
        .as_array_mut()?
        .iter_mut()
        .find(|t| t["address"].as_str() == Some(address))
}

pub fn public_terminal(t: &Value) -> Value {
    json!({
        "address": t["address"],
        "argv": t["argv"],
        "cwd": t["cwd"],
        "foreground": t["foreground"],
        "keep": t["keep"],
        "labels": t["labels"],
        "name": t["name"],
        "parent": t["parent"],
        "pid": t["pid"],
        "reported": t["reported"],
        "state": t["state"],
        "status": t["status"],
        "status_ms": t["status_ms"],
        "turn": t["turn"],
    })
}

pub fn sync_spaces_with_terminals(state: &mut Value) {
    let live_terms: Vec<(String, Option<String>, Option<String>)> = state["terminals"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|t| t["state"].as_str() == Some("live"))
        .map(|t| {
            let addr = t["address"].as_str().unwrap_or_default().to_string();
            let sp = t["labels"]["herdr.space"].as_str().map(str::to_owned);
            let tb = t["labels"]["herdr.tab"].as_str().map(str::to_owned);
            (addr, sp, tb)
        })
        .collect();
    let live_set: std::collections::BTreeSet<&str> =
        live_terms.iter().map(|(a, _, _)| a.as_str()).collect();

    if let Some(spaces) = state["spaces"].as_array_mut() {
        for sp in spaces.iter_mut() {
            if let Some(tabs) = sp["tabs"].as_array_mut() {
                for tb in tabs.iter_mut() {
                    if let Some(terms) = tb["terminals"].as_array_mut() {
                        terms.retain(|entry| {
                            entry["terminal"]
                                .as_str()
                                .is_some_and(|a| live_set.contains(a))
                        });
                    }
                }
            }
        }
    }

    for (addr, sp_opt, tb_opt) in live_terms {
        let (Some(sp_id), Some(tb_id)) = (sp_opt, tb_opt) else {
            continue;
        };
        let already = state["spaces"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|sp| sp["tabs"].as_array().into_iter().flatten())
            .flat_map(|tb| tb["terminals"].as_array().into_iter().flatten())
            .any(|e| e["terminal"].as_str() == Some(addr.as_str()));
        if already {
            continue;
        }
        let target_exists = state["spaces"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|sp| sp["id"].as_str() == Some(sp_id.as_str()))
            .flat_map(|sp| sp["tabs"].as_array().into_iter().flatten())
            .any(|tb| tb["id"].as_str() == Some(tb_id.as_str()));
        if !target_exists {
            continue;
        }
        let pane_id = mint_pane_id(state);
        for sp in state["spaces"].as_array_mut().into_iter().flatten() {
            if sp["id"].as_str() != Some(sp_id.as_str()) {
                continue;
            }
            for tb in sp["tabs"].as_array_mut().into_iter().flatten() {
                if tb["id"].as_str() == Some(tb_id.as_str()) {
                    tb["terminals"]
                        .as_array_mut()
                        .unwrap()
                        .push(json!({"terminal": addr, "pane": pane_id}));
                }
            }
        }
    }
}

pub fn remove_terminal_pid(state: &mut Value, address: &str) {
    let started = state["started"]
        .as_object_mut()
        .and_then(|m| m.remove(address))
        .and_then(|s| s.as_u64());
    if let Some(pid) = state["pids"]
        .as_object_mut()
        .and_then(|m| m.remove(address))
        .and_then(|p| p.as_u64())
    {
        queue_kill(pid, started);
    }
}

pub fn terminal_screen(state: &Value, address: &str) -> String {
    let Some(term) = find_terminal(state, address) else {
        return String::new();
    };
    if let Some(text) = term["test_screen"].as_str() {
        return text.to_string();
    }
    let out = state_dir()
        .and_then(|d| std::fs::read_to_string(term_out_in(&d, address)).ok())
        .unwrap_or_default();
    if !out.is_empty() {
        return out;
    }
    let sent: Vec<&str> = term["sent"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| v.as_str())
        .collect();
    if sent.is_empty() {
        String::new()
    } else {
        format!("{}\n", sent.join("\n"))
    }
}

fn default_socket() -> PathBuf {
    if let Some(sock) = std::env::var_os("HC_TERMINAL_SOCKET") {
        return PathBuf::from(sock);
    }
    if let Some(xdg) = std::env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(xdg).join("herdr-core/default/default.sock");
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_default();
    home.join(".local/state/herdr-core/default/default.sock")
}

pub fn exec_in_terminal(state: &mut Value, address: &str, cwd: &str, command: &str) {
    let shell_pair = if cfg!(windows) {
        ("powershell", "-Command")
    } else {
        ("/bin/sh", "-c")
    };
    let mut cmd = std::process::Command::new(shell_pair.0);
    cmd.arg(shell_pair.1)
        .arg(command)
        .env("HC_TERMINAL_ID", address)
        .env("HC_TERMINAL_SOCKET", default_socket())
        .stdin(std::process::Stdio::null());
    if Path::new(cwd).is_dir() {
        cmd.current_dir(cwd);
    }
    if let Some(dir) = state_dir() {
        let out_path = term_out_in(&dir, address);
        if let Ok(out) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(out_path)
        {
            if let Ok(err) = out.try_clone() {
                cmd.stderr(err);
            }
            cmd.stdout(out);
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    if let Ok(child) = cmd.spawn() {
        let pid = child.id();
        state["pids"][address] = json!(pid);
        if let Some(started) = horch_core::procid::start_time(pid) {
            state["started"][address] = json!(started);
        }
    }
}

fn kill_group(pgid: u64, started: Option<u64>) {
    if cfg!(windows) {
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pgid.to_string(), "/T", "/F"])
            .status();
        return;
    }
    #[cfg(unix)]
    {
        let (Ok(group), Some(started)) = (u32::try_from(pgid), started) else {
            return;
        };
        if horch_core::procid::is_same_group(group, started) {
            use crate::process::signal_groups;
            signal_groups(&[group], "TERM");
        }
    }
}
