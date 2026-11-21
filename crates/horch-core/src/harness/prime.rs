use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use anyhow::{Context, Result};

use serde_json::{json, Value};

use super::claude::{default_leaves, insert_at_path};
use super::{CommandSpec, Harness, HarnessKind, LaunchEnv, PrepareRequest, Prepared, WindowInputs};
use crate::compaction::window::{OperatorWindow, WindowDecision};
use crate::roster::{HarnessDefault, Teammate};
use crate::runtime::RuntimeContext;
use crate::skills::Bundle;

const SOCKET: &str = "d.sock";
pub const MAX_SOCKET_BYTES: usize = 103;

pub fn scan_dir_name() -> String {
    format!("prime-agent-{}", real_uid())
}

#[cfg(unix)]
fn real_uid() -> u32 {
    // SAFETY: getuid has no preconditions and cannot fail.
    unsafe { libc::getuid() }
}

#[cfg(not(unix))]
fn real_uid() -> u32 {
    0
}

fn launch_socket(launch: &Path) -> Option<PathBuf> {
    [
        launch.join(scan_dir_name()).join(SOCKET),
        launch.join(SOCKET),
    ]
    .into_iter()
    .find(|s| std::fs::symlink_metadata(s).is_ok())
}

#[derive(Debug, Clone)]
pub struct Daemon {
    launch: PathBuf,
    socket: PathBuf,
    sessions: PathBuf,
    bin: PathBuf,
    installed: SystemTime,
}

impl Daemon {
    pub fn install(state_root: &Path, bin: &Path) -> Result<Daemon> {
        let id: String = crate::mint_uuid()
            .chars()
            .filter(char::is_ascii_hexdigit)
            .take(12)
            .collect();
        let launch = state_root.join("prime").join(id);
        let scan = launch.join(scan_dir_name());
        let socket = scan.join(SOCKET);
        let len = socket.as_os_str().len();
        if len > MAX_SOCKET_BYTES {
            anyhow::bail!(
                "the Prime daemon socket path {} is {len} bytes, over the limit of \
                 {MAX_SOCKET_BYTES} bytes; set HORCH_STATE_DIR to a shorter dir",
                socket.display()
            );
        }
        let sessions = launch.join("sessions");
        crate::fsx::ensure_private_dir(&launch)?;
        crate::fsx::ensure_private_dir(&scan)?;
        crate::fsx::ensure_private_dir(&sessions)?;
        let pid = std::process::id();
        let started = crate::procid::start_time(pid)
            .map(|t| t.to_string())
            .unwrap_or_default();
        crate::fsx::write_atomic(
            &launch.join(LAUNCHER),
            format!("{pid} {started}\n").as_bytes(),
            crate::fsx::PRIVATE_FILE,
        )?;
        Ok(Daemon {
            launch,
            socket,
            sessions,
            bin: bin.to_path_buf(),
            installed: SystemTime::now(),
        })
    }

    fn of_launch(launch: &Path, socket: PathBuf, bin: &Path) -> Option<Daemon> {
        let installed = std::fs::metadata(launch.join(LAUNCHER))
            .and_then(|m| m.modified())
            .ok()?;
        Some(Daemon {
            launch: launch.to_path_buf(),
            socket,
            sessions: launch.join("sessions"),
            bin: bin.to_path_buf(),
            installed,
        })
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    pub fn sessions_dir(&self) -> &Path {
        &self.sessions
    }

    pub fn launch_dir(&self) -> &Path {
        &self.launch
    }

    fn agent_dir(&self) -> PathBuf {
        self.launch.join(AGENT_DIR)
    }

    fn tmpdir(&self) -> std::ffi::OsString {
        let mut dir = self.launch.clone().into_os_string();
        dir.push("/");
        dir
    }

    pub fn finish(&self) {
        if let Some(line) = self.stop() {
            use std::io::Write as _;
            let _ = writeln!(std::io::stderr(), "{line}");
        }
    }

    pub(crate) fn stop(&self) -> Option<String> {
        let problem = listening(&self.socket)
            .then(|| self.shutdown_problem())
            .flatten();
        if problem.is_none() {
            self.remove_launch_files();
        }
        problem
    }

    fn shutdown_problem(&self) -> Option<String> {
        match self.shutdown() {
            Stop::Stopped => None,
            Stop::TimedOut => Some(format!(
                "horch: the Prime stop for {} still runs after {} s; going on",
                self.socket.display(),
                STOP_WAIT.as_secs()
            )),
            Stop::Failed => self.terminate_supervisor(),
        }
    }

    fn remove_launch_files(&self) {
        let _ = std::fs::remove_file(&self.socket);
        if let Some(scan) = self.socket.parent().filter(|s| *s != self.launch) {
            let _ = std::fs::remove_dir(scan);
        }
        let _ = std::fs::remove_dir_all(self.agent_dir());
    }

    fn shutdown(&self) -> Stop {
        let mut cmd = Command::new(&self.bin);
        cmd.args(["shutdown", "--force", "--json"])
            .env("TMPDIR", self.tmpdir())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        let Ok(mut child) = crate::runtime::process::spawn_detached(&mut cmd) else {
            return Stop::Failed;
        };
        match wait_for_stop(&mut child, STOP_WAIT) {
            Waited::Exited(true) => {
                let mut out = String::new();
                if let Some(mut stdout) = child.stdout.take() {
                    use std::io::Read as _;
                    let _ = stdout.read_to_string(&mut out);
                }
                if stopped_names(&out, &self.socket) {
                    Stop::Stopped
                } else {
                    Stop::Failed
                }
            }
            Waited::Exited(false) => Stop::Failed,
            Waited::TimedOut => Stop::TimedOut,
        }
    }

    fn terminate_supervisor(&self) -> Option<String> {
        let Some(pid) = self.daemon_pid() else {
            return Some(format!(
                "horch: left Prime daemon {} running: status names no pid",
                self.socket.display()
            ));
        };
        let first = pid_start(pid);
        let again = self.daemon_pid().map(|p| (p, pid_start(p)));
        let started_at = u32::try_from(pid).ok().and_then(crate::procid::started_at);
        match may_stop((pid, first), again, started_at, self.installed) {
            Ok(()) => {
                terminate(pid, first);
                None
            }
            Err(why) => Some(format!("horch: left Prime daemon pid {pid} running: {why}")),
        }
    }

    fn daemon_pid(&self) -> Option<i32> {
        let mut cmd = Command::new(&self.bin);
        cmd.args(["status", "--json"])
            .env("TMPDIR", self.tmpdir())
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        crate::runtime::process::scrub_child_env(&mut cmd);
        let output = cmd.output().ok()?;
        if !output.status.success() {
            return None;
        }
        pid_for_socket(&String::from_utf8_lossy(&output.stdout), &self.socket)
    }
}

#[cfg(unix)]
fn listening(socket: &Path) -> bool {
    match std::os::unix::net::UnixStream::connect(socket) {
        Ok(_) => true,
        Err(e) => {
            !(matches!(
                e.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
            ) || e.raw_os_error() == Some(libc::ENOTSOCK))
        }
    }
}

#[cfg(not(unix))]
fn listening(socket: &Path) -> bool {
    std::fs::symlink_metadata(socket).is_ok()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Waited {
    Exited(bool),
    TimedOut,
}

fn wait_for_stop(child: &mut std::process::Child, limit: std::time::Duration) -> Waited {
    let deadline = std::time::Instant::now() + limit;
    match crate::runtime::process::wait_until(child, deadline, std::time::Duration::from_millis(50))
    {
        Ok(Some(status)) => Waited::Exited(status.success()),
        Err(_) => Waited::Exited(false),
        Ok(None) => Waited::TimedOut,
    }
}

pub const STOP_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
    Stopped,
    Failed,
    TimedOut,
}

pub(crate) fn stopped_names(json: &str, socket: &Path) -> bool {
    serde_json::from_str::<Value>(json).is_ok_and(|doc| {
        doc.get("stopped")
            .and_then(Value::as_array)
            .is_some_and(|stopped| {
                stopped.iter().any(|r| {
                    r.get("socketPath")
                        .and_then(Value::as_str)
                        .is_some_and(|p| same_socket(Path::new(p), socket))
                })
            })
    })
}

fn same_socket(reported: &Path, socket: &Path) -> bool {
    if reported == socket {
        return true;
    }
    let dir = |p: &Path| p.parent().and_then(|d| std::fs::canonicalize(d).ok());
    reported.file_name() == socket.file_name()
        && dir(reported).is_some()
        && dir(reported) == dir(socket)
}

const START_SLACK: std::time::Duration = std::time::Duration::from_secs(1);

fn pid_start(pid: i32) -> Option<u64> {
    u32::try_from(pid).ok().and_then(crate::procid::start_time)
}

pub(crate) fn may_stop(
    first: (i32, Option<u64>),
    again: Option<(i32, Option<u64>)>,
    started_at: Option<SystemTime>,
    installed: SystemTime,
) -> std::result::Result<(), &'static str> {
    let again = again.ok_or("a second status call does not name it")?;
    require(again.0 == first.0, "a second status call names another pid")?;
    if cfg!(windows) {
        Ok(())
    } else {
        require(
            first.1.is_some() && again.1 == first.1,
            "its start time is unknown or changed between 2 status calls",
        )?;
        require(
            started_at.is_some_and(|t| t + START_SLACK >= installed),
            "it started before this launch, so this launch did not start it",
        )
    }
}

fn require(ok: bool, why: &'static str) -> std::result::Result<(), &'static str> {
    if ok {
        Ok(())
    } else {
        Err(why)
    }
}

pub(crate) fn pid_for_socket(json: &str, socket: &Path) -> Option<i32> {
    let records: Vec<serde_json::Value> = serde_json::from_str(json).ok()?;
    records
        .iter()
        .find_map(|r| {
            let path = r.get("socketPath")?.as_str()?;
            same_socket(Path::new(path), socket)
                .then(|| r.get("pid")?.as_i64())
                .flatten()
        })
        .and_then(|pid| u32::try_from(pid).ok().and_then(crate::procid::os_pid))
}

#[cfg(unix)]
fn terminate(pid: i32, started: Option<u64>) {
    if let (Ok(pid), Some(started)) = (u32::try_from(pid), started) {
        crate::procid::signal_same(pid, started, libc::SIGTERM);
    }
}

#[cfg(not(unix))]
fn terminate(pid: i32, _started: Option<u64>) {
    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T"])
        .status();
}

pub fn find_session(sessions_dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(sessions_dir)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let is_session = path.extension().and_then(|e| e.to_str()) == Some("jsonl");
            let modified = entry.metadata().and_then(|m| m.modified()).ok()?;
            is_session.then_some((modified, path))
        })
        .min_by_key(|(modified, _)| std::cmp::Reverse(*modified))
        .map(|(_, path)| path)
}

pub const AGENT_DIR_ENV: &str = "PRIME_AGENT_CODING_AGENT_DIR";
const SETTINGS: &str = "settings.json";
const MODELS: &str = "models.json";
const AUTH: &str = "auth.json";
const HARNESS_STATE: &str = "harness";
const AGENT_DIR: &str = "agent";
const LAUNCHER: &str = "launcher";
const PANE: &str = "pane";

pub(crate) fn source_agent_dir(ctx: &RuntimeContext, teammate: &Teammate) -> PathBuf {
    let home = &ctx.paths.home;
    let expand = |p: &str| crate::roster::expand_home(p, Some(home));
    let own = teammate
        .env
        .get(AGENT_DIR_ENV)
        .filter(|v| !v.is_empty())
        .map(|v| expand(v));
    let launches = absolute(&ctx.paths.state_root.join("prime"));
    let inherited = ctx
        .inherited
        .prime_agent_dir
        .as_deref()
        .map(|p| p.to_str().map(expand).unwrap_or_else(|| p.to_path_buf()))
        .filter(|p| !absolute(p).starts_with(&launches));
    absolute(
        &own.or(inherited)
            .unwrap_or_else(|| home.join(".prime").join("agent")),
    )
}

fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

fn sweep_agent_dirs(prime_root: &Path, bin: &Path) {
    let Ok(entries) = std::fs::read_dir(prime_root) else {
        return;
    };
    for entry in entries.flatten() {
        let launch = entry.path();
        if launcher_alive(&launch) {
            continue;
        }
        match launch_socket(&launch) {
            Some(socket) => {
                if let Some(daemon) = Daemon::of_launch(&launch, socket, bin) {
                    daemon.finish();
                }
            }
            None => {
                let _ = std::fs::remove_dir_all(launch.join(AGENT_DIR));
            }
        }
    }
}

pub fn finish_pane(state_root: &Path, bin: &Path, pane: &str) {
    let Ok(entries) = std::fs::read_dir(state_root.join("prime")) else {
        return;
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|launch| ran_in_pane(launch, pane))
        .filter_map(|launch| {
            let socket = launch_socket(&launch)?;
            Daemon::of_launch(&launch, socket, bin)
        })
        .for_each(|daemon| daemon.finish());
}

fn ran_in_pane(launch: &Path, pane: &str) -> bool {
    std::fs::read_to_string(launch.join(PANE)).is_ok_and(|p| p.trim() == pane)
}

#[cfg(unix)]
fn outlive_hangup() {
    extern "C" fn on_hangup(_: libc::c_int) {}
    // SAFETY: the handler does nothing, so it is async-signal-safe.
    unsafe {
        let handler = on_hangup as extern "C" fn(libc::c_int) as libc::sighandler_t;
        libc::signal(libc::SIGHUP, handler);
    }
}

#[cfg(not(unix))]
fn outlive_hangup() {}

pub const STATUS_SOURCE: &str = "herdr:pi";
pub const STATUS_AGENT: &str = "prime-agent";

fn release_seq() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64 * 1000 + 999)
        .unwrap_or_default()
}

fn launcher_alive(launch: &Path) -> bool {
    read_launcher(launch).is_some_and(|(pid, started)| crate::procid::alive(pid, started))
}

fn read_launcher(launch: &Path) -> Option<(u32, Option<u64>)> {
    let text = std::fs::read_to_string(launch.join(LAUNCHER)).ok()?;
    let mut fields = text.split_whitespace();
    let pid = fields.next()?.parse().ok()?;
    let started = fields.next().and_then(|t| t.parse().ok());
    Some((pid, started))
}

fn project_settings(workdir: &Path) -> PathBuf {
    workdir.join(".prime").join("agent").join(SETTINGS)
}

fn prime_operator_window(
    models: &Path,
    settings: &[PathBuf],
    model: &str,
) -> Option<OperatorWindow> {
    models_override(models, model)
        .or_else(|| settings.iter().find_map(|path| settings_override(path)))
}

fn read_operator_file(path: &Path) -> std::result::Result<Option<String>, OperatorWindow> {
    crate::fsx::read_regular_bounded(path).map_err(|why| OperatorWindow {
        tokens: None,
        detail: format!("{} ({})", path.display(), why.reason()),
    })
}

fn models_override(models: &Path, model: &str) -> Option<OperatorWindow> {
    match read_operator_file(models) {
        Err(window) => Some(window),
        Ok(Some(text)) => models_window(&models.to_string_lossy(), &text, model),
        Ok(None) => None,
    }
}

fn settings_override(path: &Path) -> Option<OperatorWindow> {
    match read_operator_file(path) {
        Err(window) => Some(window),
        Ok(Some(text)) if compaction_set(&text) => Some(OperatorWindow {
            tokens: None,
            detail: format!("{} (compaction set; trigger unknown)", path.display()),
        }),
        Ok(_) => None,
    }
}

fn models_window(path: &str, text: &str, model: &str) -> Option<OperatorWindow> {
    let (provider, id) = model.split_once('/')?;
    let Ok(doc) = serde_json::from_str::<Value>(text) else {
        return Some(OperatorWindow {
            tokens: None,
            detail: format!("{path} (not valid JSON)"),
        });
    };
    let defined = doc.get("providers")?.get(provider)?;
    let tokens = defined
        .get("modelOverrides")
        .and_then(|o| o.get(id))
        .and_then(|o| o.get("contextWindow"))
        .and_then(Value::as_u64)
        .or_else(|| custom_window(defined, id));
    Some(OperatorWindow {
        tokens,
        detail: format!("{path} (provider {provider} defined)"),
    })
}

fn custom_window(provider: &Value, id: &str) -> Option<u64> {
    provider
        .get("models")?
        .as_array()?
        .iter()
        .find(|m| m.get("id").and_then(Value::as_str) == Some(id))?
        .get("contextWindow")?
        .as_u64()
}

fn compaction_set(text: &str) -> bool {
    serde_json::from_str::<Value>(text).is_ok_and(|doc| {
        doc.get("compaction")
            .is_some_and(|c| c.get("reserveTokens").is_some() || c.get("enabled").is_some())
    })
}

fn fleet_models(decision: Option<&WindowDecision>, model: &str) -> Option<Value> {
    let decision = decision.filter(|d| d.applied)?;
    let tokens = decision.tokens?;
    let (provider, id) = model.split_once('/')?;
    Some(json!({"providers": {provider: {"modelOverrides": {id: {"contextWindow": tokens}}}}}))
}

fn fleet_settings(
    operator: Option<&str>,
    project: Option<&str>,
    teammate: &Teammate,
) -> Option<serde_json::Map<String, Value>> {
    let mut settings = operator.map_or(Some(serde_json::Map::new()), parse_object)?;
    let project: Option<Value> = project.and_then(|t| serde_json::from_str(t).ok());
    let mut inserted = false;
    for (path, value, forced) in prime_leaves(teammate) {
        let operator_sets = sets_path(&Value::Object(settings.clone()), &path);
        let blocked =
            !forced && (operator_sets || project.as_ref().is_some_and(|doc| sets_path(doc, &path)));
        if !blocked {
            inserted |= insert_at(&mut settings, &path, value);
        }
    }
    inserted.then(|| {
        settings.remove("apiKeys");
        settings
    })
}

fn prime_leaves(teammate: &Teammate) -> Vec<(String, Value, bool)> {
    let entries = HarnessDefault::for_harness(&teammate.harness_defaults, HarnessKind::Prime)
        .filter_map(|entry| Some((entry.agent_settings.as_ref()?, entry.force.is_some())));
    default_leaves(entries)
}

fn parse_object(text: &str) -> Option<serde_json::Map<String, Value>> {
    match serde_json::from_str(text) {
        Ok(Value::Object(map)) => Some(map),
        _ => None,
    }
}

fn sets_path(doc: &Value, path: &str) -> bool {
    let mut node = doc;
    for key in path.split('.') {
        let Some(next) = node.get(key) else {
            return !node.is_object();
        };
        node = next;
    }
    true
}

fn insert_at(obj: &mut serde_json::Map<String, Value>, path: &str, value: Value) -> bool {
    let mut parts: Vec<&str> = path.split('.').collect();
    let Some(key) = parts.pop() else {
        return false;
    };
    insert_at_path(obj, &parts, key, value)
}

#[cfg(unix)]
fn link(target: &Path, at: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, at)
}

#[cfg(windows)]
fn link(target: &Path, at: &Path) -> std::io::Result<()> {
    if target.is_dir() {
        std::os::windows::fs::symlink_dir(target, at)
    } else {
        std::os::windows::fs::symlink_file(target, at)
    }
}

fn build_agent_dir(src: &Path, dir: &Path, req: &PrepareRequest<'_>) -> Result<()> {
    crate::fsx::ensure_private_dir(dir)?;
    if std::fs::symlink_metadata(src.join(HARNESS_STATE)).is_err() {
        let _ = crate::fsx::ensure_private_dir(&src.join(HARNESS_STATE));
    }
    link_operator_entries(src, dir)?;
    write_or_link(
        generated_settings(src, req).map(Value::Object),
        &src.join(SETTINGS),
        &dir.join(SETTINGS),
    )?;
    write_or_link(
        fleet_models(req.compact_window, req.model),
        &src.join(MODELS),
        &dir.join(MODELS),
    )
}

fn link_operator_entries(src: &Path, dir: &Path) -> Result<()> {
    let entries = std::fs::read_dir(src).with_context(|| format!("reading {}", src.display()))?;
    for entry in entries {
        let name = entry
            .with_context(|| format!("reading {}", src.display()))?
            .file_name();
        if !is_launch_private(&name) {
            let at = dir.join(&name);
            link(&src.join(&name), &at).with_context(|| format!("linking {}", at.display()))?;
        }
    }
    link_auth(src, dir)
}

fn link_auth(src: &Path, dir: &Path) -> Result<()> {
    let auth = dir.join(AUTH);
    if std::fs::symlink_metadata(&auth).is_err() {
        link(&src.join(AUTH), &auth).with_context(|| format!("linking {}", auth.display()))?;
    }
    Ok(())
}

fn is_launch_private(name: &std::ffi::OsStr) -> bool {
    let text = name.to_string_lossy();
    name == SETTINGS || name == MODELS || text.ends_with(".lock") || text.ends_with(".tmp")
}

fn generated_settings(
    src: &Path,
    req: &PrepareRequest<'_>,
) -> Option<serde_json::Map<String, Value>> {
    let operator = crate::fsx::read_regular_bounded(&src.join(SETTINGS)).ok()?;
    let project = crate::fsx::read_regular_bounded(&project_settings(req.workdir)).ok()?;
    fleet_settings(operator.as_deref(), project.as_deref(), req.teammate)
}

fn write_or_link(generated: Option<Value>, operator: &Path, at: &Path) -> Result<()> {
    if let Some(value) = generated {
        let text = serde_json::to_string_pretty(&value)? + "\n";
        crate::fsx::write_atomic(at, text.as_bytes(), 0o600)?;
        Ok(())
    } else if std::fs::symlink_metadata(operator).is_ok() {
        link(operator, at).with_context(|| format!("linking {}", at.display()))
    } else {
        Ok(())
    }
}

fn command_model(cmd: &Command) -> Option<String> {
    let args: Vec<_> = cmd.get_args().filter_map(|a| a.to_str()).collect();
    args.windows(2)
        .find(|w| w[0] == "--model")
        .map(|w| w[1].to_string())
}

fn fleet_agent_dir_env(
    ctx: &RuntimeContext,
    req: &PrepareRequest<'_>,
    daemon: &Daemon,
) -> Option<(String, std::ffi::OsString)> {
    let src = source_agent_dir(ctx, req.teammate);
    if !src.is_dir() {
        return None;
    }
    let dir = daemon.agent_dir();
    match build_agent_dir(&src, &dir, req) {
        Ok(()) => Some((AGENT_DIR_ENV.into(), dir.into_os_string())),
        Err(e) => {
            let _ = std::fs::remove_dir_all(&dir);
            eprintln!(
                "horch[{}]: Prime agent dir not built, the pane uses {}: {e:#}",
                req.role,
                src.display()
            );
            None
        }
    }
}

fn write_pane_file(daemon: &Daemon, role: &str, pane: &str) {
    if let Err(e) = crate::fsx::write_atomic(
        &daemon.launch_dir().join(PANE),
        format!("{pane}\n").as_bytes(),
        crate::fsx::PRIVATE_FILE,
    ) {
        eprintln!(
            "horch[{}]: Prime pane file not written, horch done cannot stop the daemon: {e:#}",
            role
        );
    }
}

pub struct Prime;

impl Harness for Prime {
    fn kind(&self) -> HarnessKind {
        HarnessKind::Prime
    }

    fn prepare(&self, ctx: &RuntimeContext, req: &PrepareRequest<'_>) -> Result<Prepared> {
        sweep_agent_dirs(&ctx.paths.state_root.join("prime"), &ctx.bins.harness.prime);
        outlive_hangup();
        let daemon = Daemon::install(&ctx.paths.state_root, &ctx.bins.harness.prime)
            .with_context(|| format!("preparing the Prime launch of {}", req.role))?;
        let mut prepared = Prepared {
            extra_args: vec![
                "--daemon-socket".into(),
                daemon.socket().to_string_lossy().into_owned(),
                "--session-dir".into(),
                daemon.sessions_dir().to_string_lossy().into_owned(),
            ],
            sessions_dir: Some(daemon.sessions_dir().to_path_buf()),
            ..Prepared::default()
        };
        prepared.env.extend(fleet_agent_dir_env(ctx, req, &daemon));
        let pane = ctx.herdr.pane.as_ref().map(|p| p.as_str().to_string());
        if let Some(pane) = &pane {
            write_pane_file(&daemon, req.role, pane);
        }
        prepared.on_finish(move || daemon.finish());
        if let Some(pane) = pane {
            let herdr = crate::workspace::PaneHost::of(ctx);
            prepared.on_finish(move || {
                let _ = herdr.release_agent(&pane, STATUS_SOURCE, STATUS_AGENT, release_seq());
            });
        }
        Ok(prepared)
    }

    fn operator_window(&self, inputs: &WindowInputs<'_>) -> Option<OperatorWindow> {
        prime_operator_window(
            &inputs.prime_agent_dir.join(MODELS),
            &[
                inputs.prime_agent_dir.join(SETTINGS),
                project_settings(inputs.workdir),
            ],
            inputs.model,
        )
    }

    fn window_in_command(&self, cmd: &Command) -> Option<u64> {
        let (_, dir) = cmd.get_envs().find(|(k, _)| *k == AGENT_DIR_ENV)?;
        let models = Path::new(dir?).join(MODELS);
        let text = crate::fsx::read_regular_bounded(&models).ok()??;
        models_window(&models.to_string_lossy(), &text, &command_model(cmd)?)?.tokens
    }

    fn expose_skills(
        &self,
        teammate: &Teammate,
        skills: &Bundle,
        _home: Option<&Path>,
    ) -> Result<Teammate> {
        Ok(super::pi::with_skill_flag(teammate, skills))
    }

    fn command(&self, env: &LaunchEnv, spec: &CommandSpec<'_>) -> Result<Command> {
        super::pi::pi_family_command(
            env.bins.prime.clone(),
            spec.teammate,
            spec.session,
            spec.prompt,
            spec.model_override,
        )
    }

    fn discover_sessions(
        &self,
        _ctx: &RuntimeContext,
        _workdir: &Path,
        _since: SystemTime,
        sessions_dir: Option<&Path>,
    ) -> Vec<String> {
        sessions_dir
            .and_then(find_session)
            .map(|p| p.to_string_lossy().into_owned())
            .into_iter()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS: &str = r#"[
        {"socketPath":"/tmp/prime-agent-501/daemon.sock","pid":96848,"sessionCount":0},
        {"socketPath":"/state/prime/0a1b2c3d4e5f/prime-agent-501/d.sock","pid":99865,"sessionCount":1}
    ]"#;

    #[test]
    fn only_this_launchs_socket_matches() {
        assert_eq!(
            pid_for_socket(
                STATUS,
                Path::new("/state/prime/0a1b2c3d4e5f/prime-agent-501/d.sock")
            ),
            Some(99865)
        );
        assert_eq!(
            pid_for_socket(STATUS, Path::new("/state/prime/other/d.sock")),
            None
        );
    }

    #[test]
    fn unmatched_or_unparseable_status_kills_nothing() {
        assert_eq!(pid_for_socket("not json", Path::new("/a/d.sock")), None);
        assert_eq!(pid_for_socket("[]", Path::new("/a/d.sock")), None);
        assert_eq!(
            pid_for_socket(r#"[{"pid":1}]"#, Path::new("/a/d.sock")),
            None
        );
        let orphan = r#"[{"socketPath":"/a/d.sock","status":"orphan-file","isDefault":false}]"#;
        assert_eq!(pid_for_socket(orphan, Path::new("/a/d.sock")), None);
        for pid in ["-1", "0", "4294967295"] {
            let json = format!(r#"[{{"socketPath":"/a/d.sock","pid":{pid}}}]"#);
            assert_eq!(pid_for_socket(&json, Path::new("/a/d.sock")), None, "{pid}");
        }
    }

    #[test]
    fn ctx_05_prime_shutdown_counts_only_when_it_names_this_socket() {
        let socket = Path::new("/s/prime/a/prime-agent-501/d.sock");
        let stopped = r#"{"stopped":[{"socketPath":"/s/prime/a/prime-agent-501/d.sock",
            "action":"stopped background service (pid 71613)"}],"failed":[]}"#;
        assert!(stopped_names(stopped, socket));
        assert!(!stopped_names(
            stopped,
            Path::new("/s/prime/b/prime-agent-501/d.sock")
        ));
        let refused = r#"{"stopped":[],"failed":[{"socketPath":"/s/prime/a/prime-agent-501/d.sock",
            "reason":"confirmation required; use \"prime-agent shutdown --force --json\""}]}"#;
        assert!(!stopped_names(refused, socket));
        assert!(!stopped_names(r#"{"stopped":[],"failed":[]}"#, socket));
        assert!(!stopped_names("", socket));
        assert!(!stopped_names(
            r#"["/s/prime/a/prime-agent-501/d.sock"]"#,
            socket
        ));
    }

    #[test]
    fn only_the_same_daemon_started_by_this_launch_is_stopped() {
        use std::time::Duration;
        let installed = SystemTime::now();
        let after = Some(installed + Duration::from_secs(3));
        let ok = (7, Some(100));
        assert_eq!(may_stop(ok, Some(ok), after, installed), Ok(()));
        assert!(may_stop(ok, None, after, installed).is_err(), "gone");
        assert!(
            may_stop(ok, Some((8, Some(100))), after, installed).is_err(),
            "another pid"
        );
        let before = Some(installed - Duration::from_secs(60));
        if cfg!(windows) {
            return;
        }
        assert!(
            may_stop(ok, Some((7, Some(101))), after, installed).is_err(),
            "the pid was given to another process"
        );
        assert!(may_stop((7, None), Some((7, None)), after, installed).is_err());
        assert!(
            may_stop(ok, Some(ok), before, installed).is_err(),
            "the operator's own daemon, started earlier"
        );
        assert!(may_stop(ok, Some(ok), None, installed).is_err());
    }

    #[cfg(unix)]
    fn short_tempdir() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("hp")
            .tempdir_in("/tmp")
            .or_else(|_| tempfile::Builder::new().prefix("hp").tempdir())
            .unwrap()
    }

    #[cfg(unix)]
    fn fake_bin(dir: &Path, shutdown_json: &str, status_json: &str) -> (PathBuf, PathBuf) {
        let bin = dir.join("prime-agent");
        let log = dir.join("calls.log");
        let _ = crate::test_support::write_warmed(
            &bin,
            &format!(
                "#!/bin/sh\necho \"$1 $TMPDIR\" >> '{log}'\ncase \"$1\" in\n\
                 shutdown) cat <<'JSON'\n{shutdown_json}\nJSON\n;;\n\
                 status) cat <<'JSON'\n{status_json}\nJSON\n;;\nesac\n",
                log = log.display()
            ),
            &["warm"],
        );
        std::fs::remove_file(&log).unwrap();
        (bin, log)
    }

    #[cfg(unix)]
    fn calls(log: &Path) -> Vec<String> {
        std::fs::read_to_string(log)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    #[cfg(unix)]
    fn stale_socket(path: &Path) {
        use std::os::unix::ffi::OsStrExt;
        // SAFETY: an all-zero `sockaddr_un` is a valid value.
        let mut addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
        addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
        let bytes = path.as_os_str().as_bytes();
        assert!(bytes.len() < addr.sun_path.len(), "{}", path.display());
        for (dst, src) in addr.sun_path.iter_mut().zip(bytes) {
            *dst = *src as libc::c_char;
        }
        // SAFETY: the child calls only `socket`, `bind` and `_exit`, which
        // are async-signal-safe; the parent waits for it.
        unsafe {
            match libc::fork() {
                -1 => panic!("fork: {}", std::io::Error::last_os_error()),
                0 => {
                    let fd = libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0);
                    let size = std::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t;
                    let bound = fd >= 0
                        && libc::bind(fd, (&addr as *const libc::sockaddr_un).cast(), size) == 0;
                    libc::_exit(if bound { 0 } else { 1 });
                }
                child => {
                    let mut status = 0;
                    assert_eq!(libc::waitpid(child, &mut status, 0), child);
                    assert!(libc::WIFEXITED(status) && libc::WEXITSTATUS(status) == 0);
                }
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn ctx_05_prime_finish_stops_with_a_scoped_shutdown() {
        let tmp = short_tempdir();
        let state = tmp.path().join("state");
        let probe = Daemon::install(&state, Path::new("prime-agent")).unwrap();
        let (bin, log) = fake_bin(
            tmp.path(),
            &format!(
                r#"{{"stopped":[{{"socketPath":"{}","action":"stopped"}}],"failed":[]}}"#,
                probe.socket().display()
            ),
            "[]",
        );
        let daemon = Daemon {
            bin: bin.clone(),
            ..probe
        };
        std::fs::create_dir_all(daemon.agent_dir()).unwrap();
        let listener = std::os::unix::net::UnixListener::bind(daemon.socket()).unwrap();
        assert_eq!(daemon.stop(), None);
        drop(listener);
        assert_eq!(
            calls(&log),
            [format!("shutdown {}/", daemon.launch_dir().display())]
        );
        assert!(std::fs::symlink_metadata(daemon.socket()).is_err());
        assert!(!daemon.socket().parent().unwrap().exists(), "scan dir");
        assert!(!daemon.agent_dir().exists());
        assert!(daemon.sessions_dir().is_dir());
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn ctx_05_prime_finish_stops_only_a_daemon_this_launch_started() {
        use std::os::unix::process::ExitStatusExt;
        let tmp = short_tempdir();
        let mut child = std::process::Command::new("sleep")
            .arg("3600")
            .spawn()
            .unwrap();
        let state = tmp.path().join("state");
        let probe = Daemon::install(&state, Path::new("prime-agent")).unwrap();
        let (bin, log) = fake_bin(
            tmp.path(),
            r#"{"stopped":[{"socketPath":"/elsewhere/prime-agent-501/d.sock","action":"x"}],"failed":[]}"#,
            &format!(
                r#"[{{"socketPath":"{}","pid":{}}}]"#,
                probe.socket().display(),
                child.id()
            ),
        );
        let mut daemon = Daemon { bin, ..probe };
        std::fs::create_dir_all(daemon.agent_dir()).unwrap();
        let _listener = std::os::unix::net::UnixListener::bind(daemon.socket()).unwrap();

        daemon.installed = SystemTime::now() + std::time::Duration::from_secs(60);
        let line = daemon.stop().expect("left running");
        assert!(line.contains(&format!("pid {}", child.id())), "{line}");
        assert!(line.contains("started before this launch"), "{line}");
        if let Some(status) = child.try_wait().unwrap() {
            panic!("a foreign daemon ended: {status:?}");
        }
        assert!(daemon.socket().exists(), "a running daemon lost its socket");
        assert!(
            daemon.agent_dir().exists(),
            "a running daemon lost its agent dir"
        );
        let tmpdir = format!("{}/", daemon.launch_dir().display());
        assert_eq!(
            calls(&log),
            [
                format!("shutdown {tmpdir}"),
                format!("status {tmpdir}"),
                format!("status {tmpdir}")
            ]
        );

        daemon.installed = SystemTime::now() - std::time::Duration::from_secs(60);
        assert_eq!(daemon.stop(), None);
        let status = child.wait().unwrap();
        assert_eq!(status.signal(), Some(libc::SIGTERM), "{status:?}");
        assert!(
            !daemon.socket().exists(),
            "a stopped daemon kept its socket"
        );
        assert!(!daemon.agent_dir().exists());
    }

    #[cfg(unix)]
    #[test]
    fn ctx_05_prime_status_with_no_pid_leaves_the_files() {
        let tmp = short_tempdir();
        let state = tmp.path().join("state");
        let probe = Daemon::install(&state, Path::new("prime-agent")).unwrap();
        let (bin, _log) = fake_bin(
            tmp.path(),
            r#"{"stopped":[],"failed":[]}"#,
            &format!(
                r#"[{{"socketPath":"{}","status":"orphan-file","isDefault":false}}]"#,
                probe.socket().display()
            ),
        );
        let daemon = Daemon { bin, ..probe };
        std::fs::create_dir_all(daemon.agent_dir()).unwrap();
        let _listener = std::os::unix::net::UnixListener::bind(daemon.socket()).unwrap();
        assert_eq!(
            daemon.stop(),
            Some(format!(
                "horch: left Prime daemon {} running: status names no pid",
                daemon.socket().display()
            ))
        );
        assert!(daemon.socket().exists());
        assert!(daemon.agent_dir().is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn ctx_05_prime_finish_without_a_daemon_runs_no_stop() {
        let tmp = short_tempdir();
        let (bin, log) = fake_bin(tmp.path(), "{}", "[]");
        for stale_file in [true, false] {
            let daemon = Daemon::install(&tmp.path().join("state"), &bin).unwrap();
            std::fs::create_dir_all(daemon.agent_dir().join("daemon-workers")).unwrap();
            if stale_file {
                stale_socket(daemon.socket());
            }
            assert_eq!(daemon.stop(), None);
            assert!(std::fs::symlink_metadata(daemon.socket()).is_err());
            assert!(!daemon.agent_dir().exists());
        }
        assert!(calls(&log).is_empty(), "{:?}", calls(&log));
    }

    #[test]
    fn ctx_05_prime_install_refuses_a_long_socket_path() {
        let tmp = tempfile::tempdir().unwrap();
        let state = tmp.path().join("s".repeat(90));
        let err = Daemon::install(&state, Path::new("prime-agent"))
            .unwrap_err()
            .to_string();
        let socket_len = state.as_os_str().len()
            + "/prime/".len()
            + 12
            + 1
            + scan_dir_name().len()
            + 1
            + SOCKET.len();
        assert!(err.contains(&format!("is {socket_len} bytes")), "{err}");
        assert!(err.contains(&state.display().to_string()), "{err}");
        assert!(err.contains("103"), "{err}");
        assert!(!state.exists(), "a launch dir was made");
    }

    #[cfg(unix)]
    #[test]
    fn the_wait_for_the_stop_is_bounded() {
        let mut slow = std::process::Command::new("sleep")
            .arg("5")
            .spawn()
            .unwrap();
        let started = std::time::Instant::now();
        let limit = std::time::Duration::from_millis(200);
        assert_eq!(wait_for_stop(&mut slow, limit), Waited::TimedOut);
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        slow.kill().unwrap();
        slow.wait().unwrap();
        let mut ok = std::process::Command::new("true").spawn().unwrap();
        assert_eq!(wait_for_stop(&mut ok, limit * 10), Waited::Exited(true));
        let mut bad = std::process::Command::new("false").spawn().unwrap();
        assert_eq!(wait_for_stop(&mut bad, limit * 10), Waited::Exited(false));
    }

    #[cfg(unix)]
    #[test]
    fn the_launcher_outlives_a_hangup_and_its_child_does_not() {
        use std::os::unix::process::ExitStatusExt;
        outlive_hangup();
        // SAFETY: raising a signal that has a handler.
        assert_eq!(unsafe { libc::raise(libc::SIGHUP) }, 0);
        let status = std::process::Command::new("sh")
            .args(["-c", "kill -HUP $$; exit 0"])
            .status()
            .unwrap();
        assert_eq!(status.signal(), Some(libc::SIGHUP), "{status:?}");
    }

    #[test]
    fn ctx_05_prime_each_launch_reserves_its_own_socket_and_sessions() {
        let state = tempfile::tempdir().unwrap();
        let a = Daemon::install(state.path(), Path::new("prime-agent")).unwrap();
        let b = Daemon::install(state.path(), Path::new("prime-agent")).unwrap();
        assert_ne!(a.socket(), b.socket());
        assert!(a.sessions_dir().is_dir());
        let id = a.launch_dir().file_name().unwrap().to_str().unwrap();
        assert_eq!(id.len(), 12);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()), "{id}");
        assert_eq!(a.launch_dir(), state.path().join("prime").join(id));
        assert_eq!(
            a.socket(),
            a.launch_dir()
                .join(format!("prime-agent-{}", real_uid()))
                .join("d.sock")
        );
        assert!(a.socket().parent().unwrap().is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(a.socket().parent().unwrap()), 0o700);
            assert_eq!(mode(a.launch_dir()), 0o700);
        }
    }

    #[test]
    fn the_session_in_our_own_directory_is_ours() {
        let dir = tempfile::tempdir().unwrap();
        assert!(find_session(dir.path()).is_none());
        std::fs::write(dir.path().join("notes.txt"), "x").unwrap();
        assert!(find_session(dir.path()).is_none(), "only .jsonl counts");
        std::fs::write(dir.path().join("s-1.jsonl"), "{}").unwrap();
        assert_eq!(
            find_session(dir.path()).unwrap().file_name().unwrap(),
            "s-1.jsonl"
        );
    }

    fn prime_default(agent_settings: Value, force: bool) -> HarnessDefault {
        HarnessDefault {
            harness: HarnessKind::Prime,
            base: None,
            env: Default::default(),
            args: Vec::new(),
            settings: None,
            config: None,
            agent_settings: agent_settings.as_object().cloned(),
            force: force.then(|| "test".to_string()),
        }
    }

    fn with_defaults(defaults: Vec<HarnessDefault>) -> Teammate {
        Teammate {
            agent: HarnessKind::Prime,
            harness_defaults: defaults,
            ..Teammate::default()
        }
    }

    #[test]
    fn may_stop_names_the_first_rule_that_fails() {
        use std::time::Duration;
        let installed = SystemTime::now();
        let after = Some(installed + Duration::from_secs(3));
        let ok = (7, Some(100));
        assert_eq!(
            may_stop(ok, None, after, installed),
            Err("a second status call does not name it")
        );
        assert_eq!(
            may_stop(ok, Some((8, Some(100))), after, installed),
            Err("a second status call names another pid")
        );
        if cfg!(windows) {
            assert_eq!(may_stop(ok, Some((7, None)), None, installed), Ok(()));
            return;
        }
        let start_changed = "its start time is unknown or changed between 2 status calls";
        assert_eq!(
            may_stop(ok, Some((7, Some(101))), after, installed),
            Err(start_changed)
        );
        assert_eq!(
            may_stop((7, None), Some((7, None)), after, installed),
            Err(start_changed)
        );
        let early = "it started before this launch, so this launch did not start it";
        let before = Some(installed - Duration::from_secs(60));
        assert_eq!(may_stop(ok, Some(ok), before, installed), Err(early));
        assert_eq!(may_stop(ok, Some(ok), None, installed), Err(early));
        let slack = Some(installed - Duration::from_millis(900));
        assert_eq!(may_stop(ok, Some(ok), slack, installed), Ok(()));
    }

    #[test]
    fn fleet_settings_adds_defaults_the_operator_and_project_do_not_set() {
        let t = with_defaults(vec![
            prime_default(json!({"a": 1, "b": {"c": 2, "d": 3}, "e": 5}), false),
            prime_default(json!({"b": {"c": 9}, "f": {"g": true}}), true),
        ]);
        let operator = r#"{"apiKeys":{"x":"secret"},"a":0,"b":{"d":4},"e":{"k":1}}"#;
        let project = r#"{"f":{"other":1}}"#;
        let got = fleet_settings(Some(operator), Some(project), &t).unwrap();
        assert_eq!(
            Value::Object(got),
            json!({
                "a": 0,
                "b": {"c": 9, "d": 4},
                "e": {"k": 1},
                "f": {"g": true}
            })
        );

        let t = with_defaults(vec![prime_default(json!({"p": 1}), false)]);
        assert_eq!(
            fleet_settings(None, Some(r#"{"p":0}"#), &t),
            None,
            "the project sets it"
        );
        let got = fleet_settings(None, Some("not json"), &t).unwrap();
        assert_eq!(Value::Object(got), json!({"p": 1}));
        let got = fleet_settings(None, None, &t).unwrap();
        assert_eq!(Value::Object(got), json!({"p": 1}));
    }

    #[test]
    fn fleet_settings_leaves_the_operators_file_alone_when_it_adds_nothing() {
        let t = with_defaults(vec![prime_default(json!({"a": 1}), false)]);
        assert_eq!(
            fleet_settings(Some(r#"{"a":0,"apiKeys":{"x":"y"}}"#), None, &t),
            None
        );
        for text in ["[]", "not json", "3"] {
            assert_eq!(fleet_settings(Some(text), None, &t), None, "{text}");
        }
        assert_eq!(
            fleet_settings(Some("{}"), None, &with_defaults(vec![])),
            None
        );
        for force in [false, true] {
            let t = with_defaults(vec![prime_default(json!({"a": {"b": 1}}), force)]);
            assert_eq!(
                fleet_settings(Some(r#"{"a":3}"#), None, &t),
                None,
                "{force}"
            );
        }
        let mut other = prime_default(json!({"z": 1}), true);
        other.harness = HarnessKind::Pi;
        assert_eq!(
            fleet_settings(None, None, &with_defaults(vec![other])),
            None
        );
    }

    #[test]
    fn insert_at_sets_nested_keys_and_refuses_a_non_object_parent() {
        let mut obj = serde_json::Map::new();
        assert!(insert_at(&mut obj, "a.b.c", json!(1)));
        assert_eq!(Value::Object(obj.clone()), json!({"a": {"b": {"c": 1}}}));
        assert!(insert_at(&mut obj, "a.b.d", json!(2)));
        assert_eq!(obj["a"]["b"], json!({"c": 1, "d": 2}));
        assert!(insert_at(&mut obj, "top", json!(true)));
        let mut blocked = serde_json::Map::new();
        blocked.insert("a".into(), json!(3));
        assert!(!insert_at(&mut blocked, "a.b", json!(1)));
        assert_eq!(Value::Object(blocked), json!({"a": 3}));
    }

    #[test]
    fn the_operator_window_comes_from_models_then_settings() {
        let dir = tempfile::tempdir().unwrap();
        let models = dir.path().join("models.json");
        let user = dir.path().join("settings.json");
        let project = dir.path().join("project.json");
        let window =
            |model: &str| prime_operator_window(&models, &[user.clone(), project.clone()], model);

        assert_eq!(window("p/m"), None, "no files");

        std::fs::write(
            &models,
            r#"{"providers":{"p":{"modelOverrides":{"m":{"contextWindow":1000}},"models":[{"id":"x","contextWindow":2000}]}}}"#,
        )
        .unwrap();
        let w = window("p/m").unwrap();
        assert_eq!(w.tokens, Some(1000));
        assert_eq!(
            w.detail,
            format!("{} (provider p defined)", models.display())
        );
        assert_eq!(window("p/x").unwrap().tokens, Some(2000));
        assert_eq!(window("p/none").unwrap().tokens, None);
        assert_eq!(window("other/m"), None, "provider not defined");
        assert_eq!(window("no-slash"), None);

        std::fs::write(&models, "{oops").unwrap();
        let w = window("p/m").unwrap();
        assert_eq!(
            (w.tokens, w.detail),
            (None, format!("{} (not valid JSON)", models.display()))
        );

        std::fs::remove_file(&models).unwrap();
        std::fs::write(&user, r#"{"theme":"x","compaction":{"keepRecent":1}}"#).unwrap();
        assert_eq!(window("p/m"), None, "compaction without the trigger keys");
        std::fs::write(&project, r#"{"compaction":{"enabled":false}}"#).unwrap();
        let w = window("p/m").unwrap();
        assert_eq!(w.tokens, None);
        assert_eq!(
            w.detail,
            format!("{} (compaction set; trigger unknown)", project.display())
        );
        std::fs::write(&user, r#"{"compaction":{"reserveTokens":5}}"#).unwrap();
        assert!(window("p/m")
            .unwrap()
            .detail
            .starts_with(&user.display().to_string()));
    }

    #[test]
    fn models_window_takes_the_override_before_the_custom_model() {
        let window = |text: &str, model: &str| models_window("/m.json", text, model);
        let defined = |tokens: Option<u64>| {
            Some(OperatorWindow {
                tokens,
                detail: "/m.json (provider p defined)".into(),
            })
        };
        let both = r#"{"providers":{"p":{"modelOverrides":{"m":{"contextWindow":1000}},"models":[{"id":"m","contextWindow":2000}]}}}"#;
        assert_eq!(window(both, "p/m"), defined(Some(1000)));
        let bare = r#"{"providers":{"p":{"modelOverrides":{"m":{}},"models":[{"id":"m","contextWindow":2000}]}}}"#;
        assert_eq!(window(bare, "p/m"), defined(Some(2000)));
        let text = r#"{"providers":{"p":{"modelOverrides":{"m":{"contextWindow":"1000"}},"models":[{"id":"m","contextWindow":2000}]}}}"#;
        assert_eq!(window(text, "p/m"), defined(Some(2000)));
        let text = r#"{"providers":{"p":{"modelOverrides":{"m":{"contextWindow":-1}}}}}"#;
        assert_eq!(window(text, "p/m"), defined(None));
        let twice =
            r#"{"providers":{"p":{"models":[{"id":"m"},{"id":"m","contextWindow":3000}]}}}"#;
        assert_eq!(window(twice, "p/m"), defined(None));
        let object = r#"{"providers":{"p":{"models":{"id":"m","contextWindow":5}}}}"#;
        assert_eq!(window(object, "p/m"), defined(None));
        let slashed = r#"{"providers":{"p":{"modelOverrides":{"a/b":{"contextWindow":7}}}}}"#;
        assert_eq!(window(slashed, "p/a/b"), defined(Some(7)));
    }

    #[test]
    fn models_window_is_none_unless_the_document_defines_the_provider() {
        let window = |text: &str, model: &str| models_window("/m.json", text, model);
        let defined = Some(OperatorWindow {
            tokens: None,
            detail: "/m.json (provider p defined)".into(),
        });
        assert_eq!(window(r#"{"providers":{"p":null}}"#, "p/m"), defined);
        assert_eq!(window(r#"{"providers":{"p":{}}}"#, "p/m"), defined);
        assert_eq!(window(r#"{"providers":{"q":{}}}"#, "p/m"), None);
        assert_eq!(window("{}", "p/m"), None);
        assert_eq!(window(r#"{"providers":[]}"#, "p/m"), None);
        assert_eq!(window("[1]", "p/m"), None);
        assert_eq!(
            window("{oops", "p/m"),
            Some(OperatorWindow {
                tokens: None,
                detail: "/m.json (not valid JSON)".into(),
            })
        );
        assert_eq!(window("{oops", "no-slash"), None);
        assert_eq!(window(r#"{"providers":{"p":{}}}"#, "no-slash"), None);
    }

    #[test]
    fn find_session_picks_the_newest_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        let at = |name: &str, secs: u64| {
            let path = dir.path().join(name);
            std::fs::write(&path, "{}").unwrap();
            let file = std::fs::File::options().write(true).open(&path).unwrap();
            file.set_modified(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs))
                .unwrap();
        };
        at("old.jsonl", 1000);
        at("new.jsonl", 3000);
        at("mid.jsonl", 2000);
        at("newest.txt", 9000);
        assert_eq!(
            find_session(dir.path()).unwrap().file_name().unwrap(),
            "new.jsonl"
        );
        assert!(find_session(&dir.path().join("missing")).is_none());
    }

    #[test]
    fn the_launcher_file_names_a_live_horch_or_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let launch = dir.path();
        assert!(!launcher_alive(launch), "no file");
        std::fs::write(launch.join(LAUNCHER), "not-a-pid 1\n").unwrap();
        assert!(!launcher_alive(launch), "garbage");
        std::fs::write(launch.join(LAUNCHER), "").unwrap();
        assert!(!launcher_alive(launch), "empty");

        let pid = std::process::id();
        let started = crate::procid::start_time(pid)
            .map(|t| t.to_string())
            .unwrap_or_default();
        std::fs::write(launch.join(LAUNCHER), format!("{pid} {started}\n")).unwrap();
        assert!(launcher_alive(launch), "this process");
        if !started.is_empty() {
            std::fs::write(launch.join(LAUNCHER), format!("{pid} 1\n")).unwrap();
            assert!(!launcher_alive(launch), "same pid, another start time");
        }
    }

    #[test]
    fn finish_pane_does_nothing_for_a_pane_that_ran_no_launch() {
        let state = tempfile::tempdir().unwrap();
        finish_pane(state.path(), Path::new("prime-agent"), "p1");
        let launch = state.path().join("prime/aaaaaaaaaaaa");
        std::fs::create_dir_all(launch.join(scan_dir_name())).unwrap();
        std::fs::write(launch.join(PANE), "other\n").unwrap();
        std::fs::write(launch.join(scan_dir_name()).join(SOCKET), "").unwrap();
        finish_pane(state.path(), Path::new("prime-agent"), "p1");
        assert!(launch.join(scan_dir_name()).join(SOCKET).exists());
        std::fs::write(launch.join(PANE), "p1\n").unwrap();
        finish_pane(state.path(), Path::new("prime-agent"), "p1");
        assert!(launch.join(scan_dir_name()).join(SOCKET).exists());
    }
}
