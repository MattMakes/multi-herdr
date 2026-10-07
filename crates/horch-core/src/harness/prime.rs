//! Prime Agent-specific glue: containing its daemon, and finding the session it
//! wrote.
//!
//! Prime Agent supervises its own agents. Verified on 0.9.4: even a `--print`
//! run that failed authentication left a background service alive after the
//! process exited. herdr already supervises - a pane IS the agent's lifetime -
//! so left alone, `horch done` closing a worker's pane would leave that worker's
//! Prime daemon running, and a fleet would accumulate one per spawn.
//!
//! `--daemon-socket` is the containment: each launch gets its own socket, so its
//! daemon is its own process rather than a shared one. `prime-agent status
//! --json` maps a socket path back to a pid, which is how [`Daemon::finish`]
//! stops exactly this pane's service and leaves the operator's own alone -
//! `prime-agent shutdown` cannot be scoped and would stop every agent on the
//! machine, including other panes in the same fleet.
//!
//! Sessions are the other half. Prime has no `--session-id` (pi does), so horch
//! cannot name the session before launch. It can own the directory: with
//! `--session-dir` pointing somewhere only this launch writes, the session file
//! that appears there IS this worker's, with no timestamps or project paths to
//! disambiguate.
//!
//! The third part is the agent dir (design `ai_docs/plans/wave2/w1/design.md`
//! §6.9). Prime reads its settings and `models.json` from
//! `$PRIME_AGENT_CODING_AGENT_DIR`, so each launch gets its own: every entry
//! of the operator's dir is a link (`auth.json` included: Prime writes a
//! refreshed token through the link onto the operator's file), and only
//! `settings.json` and `models.json` are generated. The fleet window goes in a
//! `models.json` that holds the override and nothing else, and only when the
//! operator's own files set no window for the model's provider.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use anyhow::{Context, Result};

use serde_json::{json, Value};

use super::{CommandSpec, Harness, HarnessKind, LaunchEnv, PrepareRequest, Prepared, WindowInputs};
use crate::compaction::window::{OperatorWindow, WindowDecision};
use crate::roster::{HarnessDefault, Teammate};
use crate::runtime::RuntimeContext;
use crate::skills::Bundle;

/// One Prime Agent launch's private daemon socket and session directory.
#[derive(Debug, Clone)]
pub struct Daemon {
    socket: PathBuf,
    sessions: PathBuf,
    /// The `prime-agent` program that answers `status` for [`Daemon::finish`].
    bin: PathBuf,
    /// When this launch reserved its socket. Its daemon starts later; a
    /// process that started earlier is not it.
    installed: SystemTime,
}

impl Daemon {
    /// Reserve a socket and session directory for the pane running `role`.
    /// `bin` is the `prime-agent` program the launch runs.
    pub fn install(state_root: &Path, role: &str, bin: &Path) -> Result<Daemon> {
        let slug: String = role
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        let base = state_root
            .join("prime")
            .join(format!("{slug}-{}", crate::mint_uuid()));
        let sessions = base.join("sessions");
        // Owner only: the session files hold the whole conversation.
        crate::fsx::ensure_private_dir(&base)?;
        crate::fsx::ensure_private_dir(&sessions)?;
        // Who runs this launch, so a later launch's sweep leaves its agent
        // dir alone while it runs.
        let pid = std::process::id();
        let started = crate::procid::start_time(pid)
            .map(|t| t.to_string())
            .unwrap_or_default();
        crate::fsx::write_atomic(
            &base.join(LAUNCHER),
            format!("{pid} {started}\n").as_bytes(),
            crate::fsx::PRIVATE_FILE,
        )?;
        Ok(Daemon {
            // Unix sockets have a path length limit (~104 bytes on macOS), so
            // this stays a short name in an already-short state directory.
            socket: base.join("d.sock"),
            sessions,
            bin: bin.to_path_buf(),
            installed: SystemTime::now(),
        })
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    pub fn sessions_dir(&self) -> &Path {
        &self.sessions
    }

    /// This launch's agent dir, beside the socket and the sessions.
    fn agent_dir(&self) -> PathBuf {
        self.socket
            .parent()
            .unwrap_or(&self.sessions)
            .join(AGENT_DIR)
    }

    /// Stop this launch's daemon, and only this one.
    ///
    /// `status` names the pid; a pid can be given to another program once
    /// its process ends. So `status` is asked twice, and SIGTERM goes only
    /// to a pid that both answers name with the same start time, and that
    /// started after [`Daemon::install`]: horch never stops a daemon it did
    /// not start, such as the operator's own. Otherwise the daemon is left
    /// running, and the reason is logged. A daemon left running keeps its
    /// socket.
    ///
    /// Residual risk (U-63, checked on prime-agent 0.9.4,
    /// docs/live-checks/harnesses.md): Prime reports no start time of its
    /// own. `status --json` gives `pid`, `socketPath` and `uptimeSeconds`,
    /// and the uptime is `ps -o etimes` of that same pid, so it adds nothing
    /// to [`crate::procid`]. Prime takes the pid from `ss -lxp` or `lsof`:
    /// a `prime-agent` process that holds the socket. So a wrong SIGTERM
    /// needs a `prime-agent` process that started after this launch, got the
    /// dead daemon's pid and holds this launch's private socket.
    pub fn finish(&self) {
        let left_running = daemon_pid(&self.bin, &self.socket).is_some_and(|pid| {
            let first = pid_start(pid);
            let again = daemon_pid(&self.bin, &self.socket).map(|p| (p, pid_start(p)));
            let started_at = u32::try_from(pid).ok().and_then(crate::procid::started_at);
            match may_stop((pid, first), again, started_at, self.installed) {
                Ok(()) => {
                    terminate(pid, first);
                    false
                }
                Err(why) => {
                    // Not `eprintln!`: after a hangup stderr is gone, and
                    // a failed write must not end the cleanup.
                    use std::io::Write as _;
                    let _ = writeln!(
                        std::io::stderr(),
                        "horch: left Prime daemon pid {pid} running: {why}"
                    );
                    true
                }
            }
        });
        if !left_running {
            let _ = std::fs::remove_file(&self.socket);
            // The agent dir links the operator's live `auth.json`; it is
            // not needed after the pane. `remove_dir_all` does not follow
            // links. The sessions stay for a resume.
            let _ = std::fs::remove_dir_all(self.agent_dir());
            if let Some(launch) = self.socket.parent() {
                let _ = std::fs::remove_file(launch.join(STATUS_HOOK));
            }
        }
    }
}

/// How far a start time on the wall clock can be off: Linux counts it from a
/// boot time in whole seconds.
const START_SLACK: std::time::Duration = std::time::Duration::from_secs(1);

fn pid_start(pid: i32) -> Option<u64> {
    u32::try_from(pid).ok().and_then(crate::procid::start_time)
}

/// Whether [`Daemon::finish`] may signal the daemon that `status` named:
/// `first` and `again` are the 2 answers, each a pid and its start time.
/// Split out so the rules are testable without Prime installed.
pub(crate) fn may_stop(
    first: (i32, Option<u64>),
    again: Option<(i32, Option<u64>)>,
    started_at: Option<SystemTime>,
    installed: SystemTime,
) -> std::result::Result<(), &'static str> {
    let Some(again) = again else {
        return Err("a second status call does not name it");
    };
    if again.0 != first.0 {
        return Err("a second status call names another pid");
    }
    if cfg!(windows) {
        // No start times here: the pid is all there is (see `procid`).
        return Ok(());
    }
    if first.1.is_none() || again.1 != first.1 {
        return Err("its start time is unknown or changed between 2 status calls");
    }
    match started_at {
        Some(t) if t + START_SLACK >= installed => Ok(()),
        _ => Err("it started before this launch, so this launch did not start it"),
    }
}

/// The pid of the Prime daemon listening on `socket`, from `prime-agent status`.
fn daemon_pid(bin: &Path, socket: &Path) -> Option<i32> {
    let output = Command::new(bin).args(["status", "--json"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    pid_for_socket(&String::from_utf8_lossy(&output.stdout), socket)
}

/// Split out from [`daemon_pid`] so the matching is testable without Prime
/// installed.
pub(crate) fn pid_for_socket(json: &str, socket: &Path) -> Option<i32> {
    let records: Vec<serde_json::Value> = serde_json::from_str(json).ok()?;
    records
        .iter()
        .find_map(|r| {
            let path = r.get("socketPath")?.as_str()?;
            (Path::new(path) == socket)
                .then(|| r.get("pid")?.as_i64())
                .flatten()
        })
        // A pid that names no single process (0, or one that turns negative
        // as a `pid_t`) names no daemon.
        .and_then(|pid| u32::try_from(pid).ok().and_then(crate::procid::os_pid))
}

/// SIGTERM to `pid` while it still has start time `started` (see
/// [`crate::procid::signal_same`]).
#[cfg(unix)]
fn terminate(pid: i32, started: Option<u64>) {
    // SIGTERM, not SIGKILL: the daemon flushes its sessions on the way out, and
    // a session file half-written is a resume that fails later in a pane nobody
    // is watching.
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

/// The newest session file Prime wrote into a directory this launch owns.
///
/// No filtering by project or time: nothing else writes here, so whatever
/// appears is this worker's session.
pub fn find_session(sessions_dir: &Path) -> Option<PathBuf> {
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(sessions_dir).ok()?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(modified) = entry.metadata().and_then(|m| m.modified()) else {
            continue;
        };
        if newest.as_ref().is_none_or(|(t, _)| modified > *t) {
            newest = Some((modified, path));
        }
    }
    newest.map(|(_, p)| p)
}

/// The variable Prime reads its agent dir from (0.9.4 `dist/config.js`).
pub const AGENT_DIR_ENV: &str = "PRIME_AGENT_CODING_AGENT_DIR";
/// The 2 agent-dir files a launch generates; every other entry is a link.
const SETTINGS: &str = "settings.json";
const MODELS: &str = "models.json";
/// Always linked, also when the source has none: a login in the pane then
/// lands in the operator's file, not in a dir that is removed.
const AUTH: &str = "auth.json";
/// Prime's global harness state dir (0.9.4 `core/refinement/refinement.js`).
const HARNESS_STATE: &str = "harness";
/// The launch's agent dir and the file that names its launcher.
const AGENT_DIR: &str = "agent";
const LAUNCHER: &str = "launcher";
/// The herdr pane a launch ran in, for [`finish_pane`].
const PANE: &str = "pane";

/// The agent dir a launch links from: the teammate's own
/// `PRIME_AGENT_CODING_AGENT_DIR`, else the inherited one, else
/// `~/.prime/agent`. A leading `~/` is expanded against home, as Prime does.
/// An inherited value inside `<state_root>/prime/` is another launch's agent
/// dir (a pane started from a Prime pane) and is ignored. Absolute, so a
/// link to an entry resolves from the launch's agent dir too.
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

/// Clean up earlier launches under `prime_root` whose launcher is gone: for
/// panes killed before [`Daemon::finish`] ran. A launch with a daemon
/// socket gets [`Daemon::finish`] with its launcher file's time as the
/// install time, so only a daemon that launch started is stopped; a launch
/// with no launcher file keeps its daemon. Without a socket, its agent dir
/// and status hook are removed. `bin` answers `status`. Best effort: an
/// error leaves the entry.
fn sweep_agent_dirs(prime_root: &Path, bin: &Path) {
    let Ok(entries) = std::fs::read_dir(prime_root) else {
        return;
    };
    for entry in entries.flatten() {
        let launch = entry.path();
        if launcher_alive(&launch) {
            continue;
        }
        let socket = launch.join("d.sock");
        if std::fs::symlink_metadata(&socket).is_ok() {
            let installed = std::fs::metadata(launch.join(LAUNCHER)).and_then(|m| m.modified());
            if let Ok(installed) = installed {
                Daemon {
                    socket,
                    sessions: launch.join("sessions"),
                    bin: bin.to_path_buf(),
                    installed,
                }
                .finish();
            }
            continue;
        }
        let _ = std::fs::remove_dir_all(launch.join(AGENT_DIR));
        let _ = std::fs::remove_file(launch.join(STATUS_HOOK));
    }
}

/// Stop the daemon of every launch that ran in herdr pane `pane` and still
/// has a socket, through [`Daemon::finish`] with the launcher file's time as
/// the install time. `horch done` runs it after it closes the pane: herdr
/// sends SIGHUP, then SIGTERM, then SIGKILL to a closed pane's processes
/// within about 1 s, so the launcher's own finish often does not run.
/// `horch done` itself runs under Prime's daemon, in another session, so the
/// close does not end it; a stop before the close could. `bin` answers
/// `status`. Does nothing for a pane that ran no Prime launch.
pub fn finish_pane(state_root: &Path, bin: &Path, pane: &str) {
    let Ok(entries) = std::fs::read_dir(state_root.join("prime")) else {
        return;
    };
    for entry in entries.flatten() {
        let launch = entry.path();
        let ran_here = std::fs::read_to_string(launch.join(PANE)).is_ok_and(|p| p.trim() == pane);
        let socket = launch.join("d.sock");
        if !ran_here || std::fs::symlink_metadata(&socket).is_err() {
            continue;
        }
        let Ok(installed) = std::fs::metadata(launch.join(LAUNCHER)).and_then(|m| m.modified())
        else {
            continue;
        };
        Daemon {
            socket,
            sessions: launch.join("sessions"),
            bin: bin.to_path_buf(),
            installed,
        }
        .finish();
    }
}

/// Keep this launcher alive when its pane closes. Closing a pane hangs up
/// its terminal: SIGHUP goes to the pane's foreground process group, which
/// is horch and the Prime CLI. Prime's daemon runs in its own group, so it
/// does not get it. Before this, the default action ended horch before
/// [`Prepared::finish`], and the daemon ran on (a C5 daemon ran 6 h 44 min
/// after `horch done`). A handler, not SIG_IGN: an exec resets it, so the
/// Prime CLI still ends on the hangup, and then horch stops the daemon.
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

/// The Prime extension that reports this pane's agent state to herdr (X2,
/// `ai_docs/plans/wave3/x2-design.md`), in the launch dir beside the socket.
/// herdr detects no Prime agent itself, so without it a Prime pane reads
/// `agent_status: unknown` and never looks idle.
const STATUS_HOOK: &str = "herdr-status.mjs";
/// The `--source` and `--agent` of every report.
pub const STATUS_SOURCE: &str = "horch:prime";
pub const STATUS_AGENT: &str = "prime";

/// The status extension. Prime runs it in its worker process. It runs only
/// `herdr pane report-agent|release-agent` with fixed args (no shell, output
/// ignored, 2 s limit), in seq order through 1 queue, and ignores every
/// failure: herdr down gives no error in the pane. Only the root session
/// reports: an RLM child writes its session elsewhere. `{{HERDR}}`, `{{PANE}}`
/// and `{{SESSIONS}}` are JSON string literals.
const STATUS_HOOK_JS: &str = r#"// Written by horch for 1 Prime launch: reports the pane's agent state to herdr.
import { spawn } from "node:child_process";
import path from "node:path";

const HERDR = {{HERDR}};
const PANE = {{PANE}};
const SESSIONS = path.resolve({{SESSIONS}});
const SOURCE = "horch:prime";
const AGENT = "prime";

let seq = Date.now() * 1000;
let queue = Promise.resolve();

function run(args) {
  return new Promise((resolve) => {
    try {
      const child = spawn(HERDR, args, { stdio: "ignore", timeout: 2000, windowsHide: true });
      child.on("error", () => resolve());
      child.on("close", () => resolve());
    } catch {
      resolve();
    }
  });
}

// herdr 0.8.2 reads the pane id only right after the subcommand.
function send(command, more = []) {
  seq += 1;
  const full = [...command, PANE, "--source", SOURCE, "--agent", AGENT, "--seq", String(seq), ...more];
  queue = queue.then(() => run(full));
  return queue;
}

function root(ctx) {
  try {
    const file = ctx?.sessionManager?.getSessionFile?.();
    return typeof file === "string" && path.dirname(path.resolve(file)) === SESSIONS;
  } catch {
    return false;
  }
}

const report = (state) => send(["pane", "report-agent"], ["--state", state]);

export default function (pi) {
  pi.on("session_start", (_event, ctx) => {
    if (root(ctx)) void report(ctx?.isIdle?.() === false ? "working" : "idle");
  });
  pi.on("agent_start", (_event, ctx) => {
    if (root(ctx)) void report("working");
  });
  pi.on("agent_end", (_event, ctx) => {
    if (root(ctx)) void report("idle");
  });
  pi.on("session_shutdown", (event, ctx) => {
    if (event?.reason === "quit" && root(ctx)) return send(["pane", "release-agent"]);
  });
}
"#;

/// [`STATUS_HOOK_JS`] for 1 launch.
fn status_hook(herdr: &Path, pane: &str, sessions: &Path) -> String {
    let lit = |s: &str| Value::String(s.to_string()).to_string();
    STATUS_HOOK_JS
        .replace("{{HERDR}}", &lit(&herdr.to_string_lossy()))
        .replace("{{PANE}}", &lit(pane))
        .replace("{{SESSIONS}}", &lit(&sessions.to_string_lossy()))
}

/// A `--seq` above every report the hook sent before now: it counts from
/// its load time in ms × 1000, 1 per report.
fn release_seq() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64 * 1000 + 999)
        .unwrap_or_default()
}

/// Whether the horch process that wrote `<launch>/launcher` still runs.
fn launcher_alive(launch: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(launch.join(LAUNCHER)) else {
        return false;
    };
    let mut fields = text.split_whitespace();
    let Some(pid) = fields.next().and_then(|p| p.parse::<u32>().ok()) else {
        return false;
    };
    let started = fields.next().and_then(|t| t.parse().ok());
    crate::procid::alive(pid, started)
}

/// `<workdir>/.prime/agent/settings.json`: Prime's project settings.
fn project_settings(workdir: &Path) -> PathBuf {
    workdir.join(".prime").join("agent").join(SETTINGS)
}

/// The window the operator's Prime files set for `model` (design §6.4,
/// review finding 10): `models` is the agent dir's `models.json`;
/// `settings` are the agent dir's and the project's `settings.json`. A
/// `models.json` that defines the model's provider at all is the
/// operator's, with or without a window for this model; so is one horch
/// cannot parse or read. A `compaction` setting moves Prime's trigger by an
/// amount horch does not model.
fn prime_operator_window(
    models: &Path,
    settings: &[PathBuf],
    model: &str,
) -> Option<OperatorWindow> {
    // A file horch refuses to read (a FIFO, over 1 MiB) is the operator's,
    // trigger unknown.
    let read = |path: &Path| {
        crate::fsx::read_regular_bounded(path).map_err(|why| OperatorWindow {
            tokens: None,
            detail: format!("{} ({})", path.display(), why.reason()),
        })
    };
    match read(models) {
        Err(window) => return Some(window),
        Ok(Some(text)) => {
            if let Some(window) = models_window(&models.to_string_lossy(), &text, model) {
                return Some(window);
            }
        }
        Ok(None) => {}
    }
    for path in settings {
        match read(path) {
            Err(window) => return Some(window),
            Ok(Some(text)) if compaction_set(&text) => {
                return Some(OperatorWindow {
                    tokens: None,
                    detail: format!("{} (compaction set; trigger unknown)", path.display()),
                })
            }
            Ok(_) => {}
        }
    }
    None
}

/// What a `models.json` says about `<p>/<m>`: None when it does not define
/// provider `<p>`. The tokens are the override for `<m>`, else the
/// `contextWindow` of the custom model `<m>`.
fn models_window(path: &str, text: &str, model: &str) -> Option<OperatorWindow> {
    let (provider, id) = model.split_once('/')?;
    let Ok(doc) = serde_json::from_str::<Value>(text) else {
        return Some(OperatorWindow {
            tokens: None,
            detail: format!("{path} (not valid JSON)"),
        });
    };
    let defined = doc.get("providers")?.get(provider)?;
    let custom = || {
        defined
            .get("models")?
            .as_array()?
            .iter()
            .find(|m| m.get("id").and_then(Value::as_str) == Some(id))?
            .get("contextWindow")?
            .as_u64()
    };
    let tokens = defined
        .get("modelOverrides")
        .and_then(|o| o.get(id))
        .and_then(|o| o.get("contextWindow"))
        .and_then(Value::as_u64)
        .or_else(custom);
    Some(OperatorWindow {
        tokens,
        detail: format!("{path} (provider {provider} defined)"),
    })
}

/// Whether a `settings.json` sets `compaction.reserveTokens` or
/// `compaction.enabled`.
fn compaction_set(text: &str) -> bool {
    serde_json::from_str::<Value>(text).is_ok_and(|doc| {
        doc.get("compaction")
            .is_some_and(|c| c.get("reserveTokens").is_some() || c.get("enabled").is_some())
    })
}

/// The fleet `models.json` for an applied decision: the override for the
/// launch's model and nothing else. None: the operator's file is linked.
fn fleet_models(decision: Option<&WindowDecision>, model: &str) -> Option<Value> {
    let decision = decision.filter(|d| d.applied)?;
    let tokens = decision.tokens?;
    let (provider, id) = model.split_once('/')?;
    Some(json!({"providers": {provider: {"modelOverrides": {id: {"contextWindow": tokens}}}}}))
}

/// The agent dir's `settings.json`: the operator's object, without the
/// legacy `apiKeys` (Prime 0.9.4 reads it only in a migration), plus each
/// `agent_settings` key path of the teammate's Prime harness defaults that
/// neither the operator's file nor the project's sets (a `force` entry sets
/// it over the operator's file; a later entry wins). None: no key to add,
/// or the operator's file is not a JSON object; the operator's file is then
/// linked as it is, so no copy of it exists.
fn fleet_settings(
    operator: Option<&str>,
    project: Option<&str>,
    teammate: &Teammate,
) -> Option<serde_json::Map<String, Value>> {
    let mut settings = match operator {
        Some(text) => match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(map)) => map,
            _ => return None,
        },
        None => serde_json::Map::new(),
    };
    let project: Option<Value> = project.and_then(|t| serde_json::from_str(t).ok());
    let mut leaves: Vec<(String, Value, bool)> = Vec::new();
    for entry in HarnessDefault::for_harness(&teammate.harness_defaults, HarnessKind::Prime) {
        let Some(defaults) = &entry.agent_settings else {
            continue;
        };
        let mut paths = Vec::new();
        leaf_paths(defaults, "", &mut paths);
        for (path, value) in paths {
            leaves.retain(|(p, _, _)| *p != path);
            leaves.push((path, value, entry.force.is_some()));
        }
    }
    let mut inserted = false;
    for (path, value, forced) in leaves {
        let set_by = |doc: &Value| {
            let mut node = doc;
            for key in path.split('.') {
                match node.get(key) {
                    Some(next) => node = next,
                    // A parent that is not an object blocks the key too.
                    None => return !node.is_object(),
                }
            }
            true
        };
        let operator_sets = set_by(&Value::Object(settings.clone()));
        if !forced && (operator_sets || project.as_ref().is_some_and(set_by)) {
            continue;
        }
        inserted |= insert_at(&mut settings, &path, value);
    }
    if !inserted {
        return None;
    }
    settings.remove("apiKeys");
    Some(settings)
}

/// Every leaf key path of an object, dotted: a nested object is walked, any
/// other value (or an empty object) is a leaf.
fn leaf_paths(obj: &serde_json::Map<String, Value>, prefix: &str, out: &mut Vec<(String, Value)>) {
    for (key, value) in obj {
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match value {
            Value::Object(inner) if !inner.is_empty() => leaf_paths(inner, &path, out),
            _ => out.push((path, value.clone())),
        }
    }
}

/// Set `value` at the dotted `path`; whether it was set. A parent that is
/// not an object is left alone, and so is the value.
fn insert_at(obj: &mut serde_json::Map<String, Value>, path: &str, value: Value) -> bool {
    let mut parts: Vec<&str> = path.split('.').collect();
    let Some(key) = parts.pop() else {
        return false;
    };
    let mut node = obj;
    for parent in parts {
        match node
            .entry(parent)
            .or_insert_with(|| json!({}))
            .as_object_mut()
        {
            Some(next) => node = next,
            None => return false,
        }
    }
    node.insert(key.to_string(), value);
    true
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

/// Build this launch's agent dir at `dir` from the operator's `src`
/// (design §6.9). Nothing is copied from `src` but a settings object with a
/// fleet key added; nothing is written into the workdir. In `src`, only a
/// missing `harness/` dir is created (empty), so the pane's global harness
/// state persists there as it does without horch.
fn build_agent_dir(src: &Path, dir: &Path, req: &PrepareRequest<'_>) -> Result<()> {
    crate::fsx::ensure_private_dir(dir)?;
    if std::fs::symlink_metadata(src.join(HARNESS_STATE)).is_err() {
        let _ = crate::fsx::ensure_private_dir(&src.join(HARNESS_STATE));
    }
    let entries = std::fs::read_dir(src).with_context(|| format!("reading {}", src.display()))?;
    for entry in entries {
        let name = entry
            .with_context(|| format!("reading {}", src.display()))?
            .file_name();
        let text = name.to_string_lossy();
        // A linked lock dir is one this pane can never take, and a temp
        // file is another process's write in progress.
        if name == SETTINGS || name == MODELS || text.ends_with(".lock") || text.ends_with(".tmp") {
            continue;
        }
        let at = dir.join(&name);
        link(&src.join(&name), &at).with_context(|| format!("linking {}", at.display()))?;
    }
    let auth = dir.join(AUTH);
    if std::fs::symlink_metadata(&auth).is_err() {
        // Dangling when the operator has no `auth.json` yet: Prime then
        // creates nothing here and writes a login through the link.
        link(&src.join(AUTH), &auth).with_context(|| format!("linking {}", auth.display()))?;
    }

    let operator = src.join(SETTINGS);
    let settings_at = dir.join(SETTINGS);
    let operator_text = crate::fsx::read_regular_bounded(&operator);
    let project_text = crate::fsx::read_regular_bounded(&project_settings(req.workdir));
    // A file horch refuses to read sets everything as far as horch knows.
    let settings = match (&operator_text, &project_text) {
        (Ok(operator), Ok(project)) => {
            fleet_settings(operator.as_deref(), project.as_deref(), req.teammate)
        }
        _ => None,
    };
    match settings {
        Some(settings) => {
            let text = serde_json::to_string_pretty(&Value::Object(settings))? + "\n";
            crate::fsx::write_atomic(&settings_at, text.as_bytes(), 0o600)?;
        }
        None if std::fs::symlink_metadata(&operator).is_ok() => link(&operator, &settings_at)
            .with_context(|| format!("linking {}", settings_at.display()))?,
        None => {}
    }

    let models_at = dir.join(MODELS);
    let operator_models = src.join(MODELS);
    match fleet_models(req.compact_window, req.model) {
        Some(models) => {
            let text = serde_json::to_string_pretty(&models)? + "\n";
            crate::fsx::write_atomic(&models_at, text.as_bytes(), 0o600)?;
        }
        None if std::fs::symlink_metadata(&operator_models).is_ok() => {
            link(&operator_models, &models_at)
                .with_context(|| format!("linking {}", models_at.display()))?
        }
        None => {}
    }
    Ok(())
}

/// The value after `--model` in a built command.
fn command_model(cmd: &Command) -> Option<String> {
    let args: Vec<_> = cmd.get_args().filter_map(|a| a.to_str()).collect();
    args.windows(2)
        .find(|w| w[0] == "--model")
        .map(|w| w[1].to_string())
}

/// The Prime Agent adapter.
pub struct Prime;

impl Harness for Prime {
    fn kind(&self) -> HarnessKind {
        HarnessKind::Prime
    }

    /// A socket and a session directory that belong to this pane alone -
    /// otherwise `horch done` would leave its daemon running and the fleet
    /// would accumulate one per spawn.
    fn prepare(&self, ctx: &RuntimeContext, req: &PrepareRequest<'_>) -> Result<Prepared> {
        sweep_agent_dirs(&ctx.paths.state_root.join("prime"), &ctx.bins.harness.prime);
        outlive_hangup();
        let daemon = Daemon::install(&ctx.paths.state_root, req.role, &ctx.bins.harness.prime)?;
        let mut prepared = Prepared {
            // pi-family builders append `--` before the prompt. These go into
            // the teammate's args, which come before that delimiter, or Prime
            // would treat them as prompt text.
            extra_args: vec![
                "--daemon-socket".into(),
                daemon.socket().to_string_lossy().into_owned(),
                "--session-dir".into(),
                daemon.sessions_dir().to_string_lossy().into_owned(),
            ],
            sessions_dir: Some(daemon.sessions_dir().to_path_buf()),
            ..Prepared::default()
        };
        // Without the operator's agent dir there is nothing to link, and a
        // login would land in a directory nobody keeps: Prime runs on its
        // own default dir, without a fleet window.
        let src = source_agent_dir(ctx, req.teammate);
        if src.is_dir() {
            let dir = daemon.agent_dir();
            match build_agent_dir(&src, &dir, req) {
                Ok(()) => prepared
                    .env
                    .push((AGENT_DIR_ENV.into(), dir.into_os_string())),
                Err(e) => {
                    let _ = std::fs::remove_dir_all(&dir);
                    eprintln!(
                        "horch[{}]: Prime agent dir not built, the pane uses {}: {e:#}",
                        req.role,
                        src.display()
                    )
                }
            }
        }
        // In a herdr pane only: herdr has no Prime detector, so the pane
        // reports its own state. Without the hook the pane still runs.
        let pane = ctx.herdr.pane.as_ref().map(|p| p.as_str().to_string());
        if let Some(pane) = &pane {
            let launch = daemon.socket().parent().unwrap_or(daemon.sessions_dir());
            if let Err(e) = crate::fsx::write_atomic(
                &launch.join(PANE),
                format!("{pane}\n").as_bytes(),
                crate::fsx::PRIVATE_FILE,
            ) {
                eprintln!(
                    "horch[{}]: Prime pane file not written, horch done cannot stop the daemon: {e:#}",
                    req.role
                );
            }
            let hook = daemon
                .socket()
                .parent()
                .unwrap_or(daemon.sessions_dir())
                .join(STATUS_HOOK);
            let text = status_hook(&ctx.bins.harness.herdr, pane, daemon.sessions_dir());
            match crate::fsx::write_atomic(&hook, text.as_bytes(), crate::fsx::PRIVATE_FILE) {
                Ok(()) => {
                    prepared.extra_args.push("-e".into());
                    prepared
                        .extra_args
                        .push(hook.to_string_lossy().into_owned());
                }
                Err(e) => eprintln!(
                    "horch[{}]: Prime status hook not written, herdr shows no Prime state: {e:#}",
                    req.role
                ),
            }
        }
        // After the CLI has exited, not before: stopping the daemon early would
        // take the session it is still writing with it.
        prepared.on_finish(move || daemon.finish());
        // After the daemon: its worker sends nothing more. Also when Prime
        // did not quit cleanly. A closed pane or herdr down: nothing to do.
        if let Some(pane) = pane {
            let herdr = crate::workspace::herdr::Herdr::with_bin(&ctx.bins.harness.herdr);
            prepared.on_finish(move || {
                let _ = herdr.release_agent(&pane, STATUS_SOURCE, STATUS_AGENT, release_seq());
            });
        }
        Ok(prepared)
    }

    /// The provider of the model in the operator's `models.json`, or a
    /// `compaction` setting in the agent dir's or the project's
    /// `settings.json` (design §6.4).
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

    /// The `contextWindow` for the command's `--model` in `models.json` of
    /// the command's `$PRIME_AGENT_CODING_AGENT_DIR`.
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

    /// Prime writes into a directory this pane owns, so the session there is
    /// unambiguously this worker's. The resume handle is the file path, which
    /// is what `--resume` takes.
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
        {"socketPath":"/state/prime/codex-1-abc/d.sock","pid":99865,"sessionCount":1}
    ]"#;

    /// The whole point of the per-launch socket: stopping this pane's daemon
    /// must never reach the operator's default one, or closing one worker would
    /// kill every Prime session on the machine.
    #[test]
    fn only_this_launchs_socket_matches() {
        assert_eq!(
            pid_for_socket(STATUS, Path::new("/state/prime/codex-1-abc/d.sock")),
            Some(99865)
        );
        assert_eq!(
            pid_for_socket(STATUS, Path::new("/state/prime/other/d.sock")),
            None
        );
    }

    /// A daemon that is not running yet, or output that is not what we expect,
    /// must yield nothing rather than a pid that belongs to something else.
    #[test]
    fn unmatched_or_unparseable_status_kills_nothing() {
        assert_eq!(pid_for_socket("not json", Path::new("/a/d.sock")), None);
        assert_eq!(pid_for_socket("[]", Path::new("/a/d.sock")), None);
        assert_eq!(
            pid_for_socket(r#"[{"pid":1}]"#, Path::new("/a/d.sock")),
            None
        );
        // -1 as a pid is every process; 0 is this group; 4294967295 turns
        // into -1.
        for pid in ["-1", "0", "4294967295"] {
            let json = format!(r#"[{{"socketPath":"/a/d.sock","pid":{pid}}}]"#);
            assert_eq!(pid_for_socket(&json, Path::new("/a/d.sock")), None, "{pid}");
        }
    }

    /// SIGTERM only for one process across both `status` calls, started
    /// after this launch reserved its socket.
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

    /// `finish` against a `status` program that names a child of this test:
    /// a child started before the launch is left running; one started after
    /// it gets SIGTERM.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn finish_stops_only_a_daemon_this_launch_started() {
        use std::os::unix::fs::PermissionsExt;
        let state = tempfile::tempdir().unwrap();
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let bin = state.path().join("prime-agent");
        let mut daemon = Daemon::install(state.path(), "prime-1", &bin).unwrap();
        std::fs::write(
            &bin,
            format!(
                "#!/bin/sh\necho '[{{\"socketPath\":\"{}\",\"pid\":{}}}]'\n",
                daemon.socket().display(),
                child.id()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();

        // Reserved long after the child started: not this launch's daemon.
        // The daemon's socket; a plain file does for this check.
        std::fs::write(daemon.socket(), "").unwrap();
        daemon.installed = SystemTime::now() + std::time::Duration::from_secs(60);
        daemon.finish();
        assert_eq!(
            child.try_wait().unwrap(),
            None,
            "a foreign daemon was stopped"
        );
        assert!(daemon.socket().exists(), "a running daemon lost its socket");

        daemon.installed = SystemTime::now() - std::time::Duration::from_secs(60);
        daemon.finish();
        let status = child.wait().unwrap();
        assert!(!status.success(), "this launch's daemon was not stopped");
        assert!(
            !daemon.socket().exists(),
            "a stopped daemon kept its socket"
        );
    }

    /// A pane closing hangs up the launcher: it lives on to stop the
    /// daemon, and a program it runs after still ends on a hangup.
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
    fn each_launch_reserves_its_own_socket_and_sessions() {
        let state = tempfile::tempdir().unwrap();
        let a = Daemon::install(state.path(), "prime-1", Path::new("prime-agent")).unwrap();
        let b = Daemon::install(state.path(), "prime-1", Path::new("prime-agent")).unwrap();
        assert_ne!(a.socket(), b.socket());
        assert!(a.sessions_dir().is_dir());
        assert!(a.socket().to_string_lossy().contains("prime-1"));
    }

    /// The session file is found by being the only thing in a directory horch
    /// owns - no timestamps, no project matching, nothing to race.
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
}
