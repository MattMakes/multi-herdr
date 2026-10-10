//! A recording stand-in for the `herdr` CLI.
//!
//! State (workspaces and panes) lives in `$HORCH_FAKE_LOG.state.json`, so a
//! sequence of calls sees one consistent server. Read-only calls and the three
//! calls `horch telemetry ensure` and `horch fleet` need are answered; every
//! call that would move, focus, split or close something the operator can see
//! is answered too, but recorded as a violation, so a test can prove horch
//! never made it.
//!
//! `pane split` adds a pane to the state, in the tab and workspace of the
//! source pane, and answers with its id (`<workspace>:p<n>`). As in real
//! herdr, an id is never given out twice: each workspace counts up in
//! `next_pane`, and a closed pane's id is not reused. `pane close` removes
//! the pane. Both stay recorded as violations.
//!
//! `HORCH_FAKE_SCENARIO` holds one scenario or a comma-separated list, for
//! example `exec,fail_run`. Scenarios:
//! - `default`: answer everything.
//! - `fail_create`: `workspace create` exits 1. `HORCH_FAKE_FAIL_LABEL`
//!   fails it for one label only, in any scenario.
//! - `fail_split`: `pane split` exits 1 with a message on stderr. The state
//!   does not change.
//! - `fail_run`: `pane run` exits 1 with a message on stderr. The state does
//!   not change and no command runs.
//! - `exec`: `pane run` also executes the command, detached, with
//!   `HERDR_PANE_ID` set, the way a real pane would. Output goes to
//!   `$HORCH_FAKE_LOG.pane-<id>.out`. The process group id is stored in the
//!   state, and `pane close` kills that group.
//! - `shell`: each pane is a plain shell with no agent in it. `agent prompt`
//!   exits 1, as real herdr does for a pane without an agent. `pane
//!   send-text` types text into the pane; `pane send-keys <pane> enter` runs
//!   the typed line like `exec`, with its output appended to the pane.
//!
//! `pane read` prints the pane text: the output of its commands, then the
//! line typed and not yet entered. `pane wait-output` searches that text
//! with `--match`, as herdr 0.8.2 does: exit 0 with an `output_matched`
//! result, or exit 1 with herdr's `timeout` error on stderr. It takes no
//! state lock while it polls.
//!
//! A top-level command that herdr 0.8.2 does not have (for example the old
//! `herdr wait output`) exits 2 with herdr's `unknown command` message.
//!
//! Dataset workspaces (label `multi-herdr-dataset ...`) are exempt from the
//! violation rule: their coordinator splits and closes panes there.
//! `workspace close` removes a workspace and kills every pane command in it.

use std::path::PathBuf;

use horch_e2e::{say, scenario_has, Call};
use serde_json::{json, Value};

/// The top-level commands of herdr 0.8.2 (`herdr --help`).
const COMMANDS: [&str; 15] = [
    "api",
    "agent",
    "channel",
    "completion",
    "config",
    "integration",
    "notification",
    "pane",
    "server",
    "session",
    "status",
    "tab",
    "update",
    "workspace",
    "worktree",
];

const MUTATING: [&str; 7] = ["focus", "tile", "split", "move", "close", "swap", "resize"];

/// Set in the copy of this call that runs in its own process group.
const DETACHED: &str = "HORCH_FAKE_HERDR_DETACHED";

fn main() {
    // A real herdr call is a request to a server, which finishes it even
    // when the client is killed. Here the state lock is held by the calling
    // process, and `pane close` kills a pane's whole process group, which
    // can hold a call of its own. So the call runs in a copy of this process
    // in a new process group: a kill of the caller's group cannot leave the
    // lock stale or the state half written.
    #[cfg(unix)]
    if std::env::var_os(DETACHED).is_none() {
        use std::os::unix::process::CommandExt;
        let code = std::env::current_exe()
            .and_then(|exe| {
                std::process::Command::new(exe)
                    .args(std::env::args_os().skip(1))
                    .env(DETACHED, "1")
                    .process_group(0)
                    .status()
            })
            .map(|s| s.code().unwrap_or(1))
            .unwrap_or(1);
        std::process::exit(code);
    }
    std::env::remove_var(DETACHED);
    let mut call = Call::start("herdr");
    // A wait polls for seconds; holding the lock that long would stall the
    // very commands whose output it waits for.
    if call.argv.get(..2) == Some(&["pane".to_string(), "wait-output".to_string()][..]) {
        let args: Vec<&str> = call.argv.iter().map(String::as_str).collect();
        let code = wait_output(&args[2..]);
        call.flush();
        std::process::exit(code);
    }
    // One call at a time reads and writes the state: concurrent calls (a
    // coordinator and its workers) would otherwise lose each other's panes.
    let lock = StateLock::acquire();
    let code = run(&mut call);
    drop(lock);
    call.flush();
    // Kill only after the state is saved and unlocked: a pane that closes
    // itself kills this very process.
    for (pgid, started) in std::mem::take(&mut *KILL.lock().unwrap()) {
        kill_group(pgid, started);
    }
    std::process::exit(code);
}

/// Process groups to kill once the call is done, with their leader's start
/// time when `pane run` recorded it.
static KILL: std::sync::Mutex<Vec<(u64, Option<u64>)>> = std::sync::Mutex::new(Vec::new());

/// `$HORCH_FAKE_LOG.state.lock/`, a mkdir lock. The holder writes its pid
/// into `pid` in the lock dir.
///
/// A waiter takes the lock over only from a holder that is dead: one whose
/// pid no longer runs, or one that left no pid after [`NO_PID_GRACE`]. Each
/// call runs in its own process group (see `main`), so only a SIGKILL of
/// the call itself leaves a stale lock. A live holder is never broken, so a
/// slow holder under load cannot lose the state. Measured under load 28 to
/// 62 (7624 calls): a hold takes at most 0.5 s and a wait at most 0.4 s.
///
/// A lock whose directory cannot be made at all means the harness root is
/// gone: a detached `horch tile` can call herdr after the test removed its
/// temp dir. The call fails then instead of waiting for ever.
struct StateLock(Option<PathBuf>);

/// How long a lock dir without a `pid` file counts as being set up. The
/// holder writes the pid right after `mkdir`.
const NO_PID_GRACE: std::time::Duration = std::time::Duration::from_secs(10);

/// How often a waiter checks whether the holder still runs.
const HOLDER_CHECK: std::time::Duration = std::time::Duration::from_secs(1);

impl StateLock {
    fn acquire() -> StateLock {
        let Some(state) = state_path() else {
            return StateLock(None);
        };
        let dir = state.with_extension("lock");
        let mut checked = std::time::Instant::now();
        loop {
            match std::fs::create_dir(&dir) {
                Ok(()) => {
                    let _ = std::fs::write(dir.join("pid"), std::process::id().to_string());
                    return StateLock(Some(dir));
                }
                Err(e) if e.kind() != std::io::ErrorKind::AlreadyExists => {
                    eprintln!(
                        "fake-herdr: cannot lock {} ({e}); the harness is gone",
                        dir.display()
                    );
                    std::process::exit(1);
                }
                Err(_) => {}
            }
            if checked.elapsed() >= HOLDER_CHECK {
                checked = std::time::Instant::now();
                if holder_is_dead(&dir) {
                    let _ = std::fs::remove_file(dir.join("pid"));
                    let _ = std::fs::remove_dir(&dir);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}

/// Whether the lock in `dir` was left by a call that no longer runs.
fn holder_is_dead(dir: &std::path::Path) -> bool {
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

/// Whether a process with this pid runs. Unknown (no `kill` to ask) counts
/// as running: a live lock is never broken.
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

impl Drop for StateLock {
    fn drop(&mut self) {
        if let Some(dir) = &self.0 {
            let _ = std::fs::remove_file(dir.join("pid"));
            let _ = std::fs::remove_dir(dir);
        }
    }
}

fn state_path() -> Option<PathBuf> {
    std::env::var_os("HORCH_FAKE_LOG").map(|p| {
        let mut p = PathBuf::from(p).into_os_string();
        p.push(".state.json");
        PathBuf::from(p)
    })
}

fn load() -> Value {
    state_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({"workspaces": [], "next": 1}))
}

/// Write the state to a temp file, then rename it into place, so a reader
/// never sees a half-written state.
fn save(state: &Value) {
    if let Some(p) = state_path() {
        let mut tmp = p.clone().into_os_string();
        tmp.push(format!(".tmp-{}", std::process::id()));
        if std::fs::write(&tmp, state.to_string()).is_ok() {
            let _ = std::fs::rename(&tmp, &p);
        }
    }
}

fn ok(result: Value) -> i32 {
    say(&json!({ "result": result }).to_string());
    0
}

fn run(call: &mut Call) -> i32 {
    let argv = call.argv.clone();
    let args: Vec<&str> = argv.iter().map(String::as_str).collect();
    if args.first() == Some(&"--version") {
        say("herdr 0.8.2 (fake)");
        return 0;
    }
    if let Some(noun) = args.first() {
        if !COMMANDS.contains(noun) {
            eprintln!("unknown command: {noun}\nrun 'herdr --help' for usage");
            return 2;
        }
    }
    let mut state = load();
    // Any verb that changes what the operator sees is a violation, whatever
    // noun it is attached to, except inside a dataset workspace: the
    // coordinator owns that one and splits and closes in it by design.
    if let Some(verb) = args.get(1) {
        if (MUTATING.contains(verb) || (args.first() == Some(&"tab") && *verb == "focus"))
            && !in_dataset_workspace(&state, &args)
        {
            call.violate(format!("mutating call: herdr {}", args.join(" ")));
        }
    }
    match args.as_slice() {
        ["workspace", "list", ..] => ok(json!({
            "type": "workspace_list",
            "workspaces": state["workspaces"].clone(),
        })),
        ["workspace", "create", rest @ ..] => {
            let label_fails = std::env::var("HORCH_FAKE_FAIL_LABEL")
                .ok()
                .is_some_and(|l| flag(rest, "--label") == Some(l.as_str()));
            if scenario_has("fail_create") || label_fails {
                eprintln!("fake-herdr: workspace create refused (scenario fail_create)");
                return 1;
            }
            let label = flag(rest, "--label").unwrap_or("workspace");
            let n = state["next"].as_u64().unwrap_or(1);
            let ws = format!("w{n}");
            let pane = format!("{ws}:p1");
            let tab = format!("{ws}:t1");
            state["next"] = json!(n + 1);
            let entry = json!({
                "workspace_id": ws, "label": label, "active_tab_id": tab,
                "panes": [{"pane_id": pane, "workspace_id": ws, "tab_id": tab}],
                "next_pane": 2,
                "focused": !rest.contains(&"--no-focus"),
            });
            state["workspaces"].as_array_mut().unwrap().push(entry);
            save(&state);
            ok(json!({
                "type": "workspace_created",
                "workspace": {"workspace_id": ws, "label": label, "active_tab_id": tab},
                "root_pane": {"pane_id": pane, "workspace_id": ws, "tab_id": tab},
            }))
        }
        ["pane", "get", id] => match find_pane(&state, id) {
            Some(p) => ok(json!({"type": "pane", "pane": p})),
            None => {
                eprintln!("fake-herdr: no pane {id}");
                1
            }
        },
        ["pane", "list", rest @ ..] => {
            let ws = flag(rest, "--workspace");
            let panes: Vec<Value> = state["workspaces"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|w| ws.is_none() || w["workspace_id"].as_str() == ws)
                .flat_map(|w| w["panes"].as_array().cloned().unwrap_or_default())
                .collect();
            ok(json!({"type": "pane_list", "panes": panes}))
        }
        ["pane", "run", pane, command] => {
            if scenario_has("fail_run") {
                eprintln!("fake-herdr: pane run refused (scenario fail_run)");
                return 1;
            }
            call.extra.insert("ran".into(), json!(command));
            if scenario_has("exec") {
                if let Some(pid) = exec_detached(pane, command) {
                    state["pids"][*pane] = json!(pid);
                    // With the pid, the start time: `pane close` kills the
                    // group only while its leader is this process.
                    if let Some(started) = horch_core::procid::start_time(pid) {
                        state["started"][*pane] = json!(started);
                    }
                    save(&state);
                }
            }
            ok(json!({"type": "ok"}))
        }
        ["pane", "read", pane, ..] => {
            say(&pane_text(&state, pane));
            0
        }
        ["pane", "send-text", pane, text] if scenario_has("shell") => {
            let typed = state["typed"][*pane].as_str().unwrap_or("").to_string();
            state["typed"][*pane] = json!(format!("{typed}{text}"));
            save(&state);
            ok(json!({"type": "ok"}))
        }
        ["pane", "send-keys", pane, keys @ ..] if scenario_has("shell") => {
            if keys.iter().any(|k| k.eq_ignore_ascii_case("enter")) {
                let line = state["typed"][*pane].as_str().unwrap_or("").to_string();
                state["typed"][*pane] = json!("");
                save(&state);
                // Not waited for: the line may call herdr, and this call
                // holds the state lock. `pane wait-output` polls for it.
                if !line.trim().is_empty() {
                    shell_line(pane, &line);
                }
            }
            ok(json!({"type": "ok"}))
        }
        ["agent", "prompt", pane, ..] if scenario_has("shell") => {
            eprintln!(
                r#"{{"error":{{"code":"agent_not_found","message":"no agent in pane {pane}"}}}}"#
            );
            1
        }
        ["pane", "split", from, ..] => {
            if scenario_has("fail_split") {
                eprintln!("fake-herdr: pane split refused (scenario fail_split)");
                return 1;
            }
            let pane = split_pane(&mut state, from);
            save(&state);
            ok(json!({"type": "pane", "pane": pane}))
        }
        ["pane", "close", pane] => {
            if !remove_pane(&mut state, pane) {
                eprintln!("fake-herdr: no pane {pane}");
                return 1;
            }
            save(&state);
            ok(json!({"type": "ok"}))
        }
        ["workspace", "close", ws] => {
            close_workspace(&mut state, ws);
            save(&state);
            ok(json!({"type": "ok"}))
        }
        ["session", "list", ..] => ok(json!({
            "type": "session_list",
            "sessions": [{"name": "default", "current": true}],
        })),
        ["integration", "status"] => {
            say("claude: installed\ncodex: installed");
            0
        }
        _ => ok(json!({"type": "ok"})),
    }
}

fn flag<'a>(args: &[&'a str], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| *a == name)
        .and_then(|i| args.get(i + 1).copied())
}

fn find_pane(state: &Value, id: &str) -> Option<Value> {
    state["workspaces"]
        .as_array()?
        .iter()
        .flat_map(|w| w["panes"].as_array().cloned().unwrap_or_default())
        .find(|p| p["pane_id"].as_str() == Some(id))
}

/// Add a pane next to `from`, in its tab and workspace. A source pane that is
/// not in the state still gets an answer, as the fake always gave one.
fn split_pane(state: &mut Value, from: &str) -> Value {
    let Some(ws) = state["workspaces"].as_array_mut().and_then(|all| {
        all.iter_mut().find(|w| {
            w["panes"]
                .as_array()
                .is_some_and(|p| p.iter().any(|p| p["pane_id"].as_str() == Some(from)))
        })
    }) else {
        return json!({"pane_id": format!("{from}-split")});
    };
    let wid = ws["workspace_id"].as_str().unwrap_or("w0").to_string();
    // The workspace's counter, never the live panes: a closed pane's id is
    // not given out again. A state without a counter (written by an older
    // fake) starts above its highest live id.
    let n = ws["next_pane"]
        .as_u64()
        .unwrap_or_else(|| highest_pane(ws, &wid) + 1);
    ws["next_pane"] = json!(n + 1);
    let panes = ws["panes"].as_array_mut().unwrap();
    let tab = panes
        .iter()
        .find(|p| p["pane_id"].as_str() == Some(from))
        .map(|p| p["tab_id"].clone())
        .unwrap_or(Value::Null);
    let pane = json!({"pane_id": format!("{wid}:p{n}"), "workspace_id": wid, "tab_id": tab});
    panes.push(pane.clone());
    pane
}

/// The highest `n` among the workspace's live `<workspace>:p<n>` ids.
fn highest_pane(ws: &Value, wid: &str) -> u64 {
    let prefix = format!("{wid}:p");
    ws["panes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| p["pane_id"].as_str()?.strip_prefix(&prefix)?.parse().ok())
        .max()
        .unwrap_or(0)
}

/// Remove a pane from the state and queue its process group for the kill,
/// if `exec` started one. False when the pane is not in the state.
fn remove_pane(state: &mut Value, pane: &str) -> bool {
    let mut found = false;
    for ws in state["workspaces"].as_array_mut().into_iter().flatten() {
        if let Some(panes) = ws["panes"].as_array_mut() {
            let before = panes.len();
            panes.retain(|p| p["pane_id"].as_str() != Some(pane));
            found |= panes.len() != before;
        }
    }
    let started = state["started"]
        .as_object_mut()
        .and_then(|m| m.remove(pane))
        .and_then(|s| s.as_u64());
    if let Some(pid) = state["pids"].as_object_mut().and_then(|m| m.remove(pane)) {
        if let Some(pid) = pid.as_u64() {
            KILL.lock().unwrap().push((pid, started));
        }
    }
    found
}

/// The label prefix of the workspaces `multi-herdr-dataset` creates.
const DATASET_LABEL: &str = "multi-herdr-dataset";

/// Whether the call targets a pane or workspace of a dataset workspace
/// (its third argument names it).
fn in_dataset_workspace(state: &Value, args: &[&str]) -> bool {
    let Some(target) = args.get(2) else {
        return false;
    };
    // Pane ids are `<workspace>:p<n>`, so a pane that is already gone still
    // names its workspace.
    let ws = target.split(':').next().unwrap_or(target);
    state["workspaces"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|w| {
            w["label"]
                .as_str()
                .is_some_and(|l| l.starts_with(DATASET_LABEL))
                && w["workspace_id"].as_str() == Some(ws)
        })
}

/// Remove a workspace and kill every pane command in it.
fn close_workspace(state: &mut Value, ws: &str) {
    let panes: Vec<String> = state["workspaces"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|w| w["workspace_id"].as_str() == Some(ws))
        .flat_map(|w| w["panes"].as_array().cloned().unwrap_or_default())
        .filter_map(|p| p["pane_id"].as_str().map(str::to_string))
        .collect();
    for pane in panes {
        remove_pane(state, &pane);
    }
    if let Some(all) = state["workspaces"].as_array_mut() {
        all.retain(|w| w["workspace_id"].as_str() != Some(ws));
    }
}

/// Kill the process group of a pane's command.
///
/// Only while it is still that group: the pid was recorded at `pane run`,
/// and a pane command that has ended frees it for any other program. A kill
/// of a reused group id can end another test run, even a whole gate. A
/// group without a recorded start time is not killed.
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
            use horch_e2e::process::signal_groups;
            signal_groups(&[group], "TERM");
        }
    }
}

/// The file that holds what pane `pane` printed.
fn pane_out(pane: &str) -> Option<PathBuf> {
    std::env::var_os("HORCH_FAKE_LOG").map(|p| {
        let mut p = p;
        p.push(format!(".pane-{}.out", pane.replace(':', "_")));
        PathBuf::from(p)
    })
}

/// What a read of `pane` shows: its output, then the line typed into it.
fn pane_text(state: &Value, pane: &str) -> String {
    let out = pane_out(pane)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    let typed = state["typed"][pane].as_str().unwrap_or("");
    format!("{out}{typed}")
}

/// `pane wait-output <pane> --match <text> [--timeout <ms>]`, answered the
/// way herdr 0.8.2 answers it.
fn wait_output(args: &[&str]) -> i32 {
    let needle = flag(args, "--match").unwrap_or("");
    let timeout = flag(args, "--timeout").and_then(|t| t.parse::<u64>().ok());
    let mut skip = false;
    let pane = args.iter().copied().find(|a| {
        let value = skip;
        skip = a.starts_with("--") && *a != "--raw";
        !value && !a.starts_with("--")
    });
    let Some(pane) = pane else {
        eprintln!("error: the following required arguments were not provided: <PANE_ID>");
        return 2;
    };
    let deadline = timeout.map(|t| std::time::Instant::now() + std::time::Duration::from_millis(t));
    loop {
        let state = load();
        if find_pane(&state, pane).is_none() {
            eprintln!(
                r#"{{"error":{{"code":"pane_not_found","message":"pane {pane} not found"}},"id":"cli:pane:wait-output"}}"#
            );
            return 1;
        }
        let text = pane_text(&state, pane);
        if let Some(line) = text.lines().find(|l| l.contains(needle)) {
            say(&json!({"id": "cli:pane:wait-output", "result": {
                "type": "output_matched", "pane_id": pane, "matched_line": line,
            }})
            .to_string());
            return 0;
        }
        if deadline.is_some_and(|d| std::time::Instant::now() >= d) {
            eprintln!(
                r#"{{"error":{{"code":"timeout","message":"timed out waiting for output match"}},"id":"cli:pane:wait-output"}}"#
            );
            return 1;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// The shell a pane runs its lines in. horch builds PowerShell lines on
/// Windows (`PaneShell::PowerShell`), so the fake runs them with PowerShell,
/// as a real herdr pane does; `cmd /C` would misread the `&` call operator.
fn pane_shell() -> (&'static str, &'static str) {
    if cfg!(windows) {
        ("powershell", "-Command")
    } else {
        ("/bin/sh", "-c")
    }
}

/// Start one entered line in `pane`, its output appended to the pane.
fn shell_line(pane: &str, line: &str) {
    let (shell, flag) = pane_shell();
    let mut cmd = std::process::Command::new(shell);
    cmd.arg(flag).arg(line).env("HERDR_PANE_ID", pane);
    cmd.stdin(std::process::Stdio::null());
    let out = pane_out(pane).and_then(|p| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(p)
            .ok()
    });
    if let Some(out) = out {
        if let Ok(err) = out.try_clone() {
            cmd.stderr(err);
        }
        cmd.stdout(out);
    }
    let _ = cmd.spawn();
}

/// Run `command` the way a herdr pane would: in a shell, with the pane's id
/// in its environment, not waiting for it. Returns the pid, which is also the
/// process group id on unix.
fn exec_detached(pane: &str, command: &str) -> Option<u32> {
    let out = pane_out(pane);
    let (shell, flag) = pane_shell();
    let mut cmd = std::process::Command::new(shell);
    cmd.arg(flag).arg(command).env("HERDR_PANE_ID", pane);
    cmd.stdin(std::process::Stdio::null());
    if let Some(out) = out.and_then(|p| std::fs::File::create(p).ok()) {
        if let Ok(err) = out.try_clone() {
            cmd.stderr(err);
        }
        cmd.stdout(out);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.spawn().ok().map(|child| child.id())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one_workspace() -> Value {
        json!({"workspaces": [{
            "workspace_id": "w1", "label": "x", "active_tab_id": "w1:t1",
            "panes": [{"pane_id": "w1:p1", "workspace_id": "w1", "tab_id": "w1:t1"}],
            "next_pane": 2,
        }], "next": 2})
    }

    fn id(pane: &Value) -> &str {
        pane["pane_id"].as_str().unwrap()
    }

    /// A closed pane's id is never given out again, the newest one included.
    #[test]
    fn pane_ids_are_never_reused_after_a_close() {
        let mut state = one_workspace();
        assert_eq!(id(&split_pane(&mut state, "w1:p1")), "w1:p2");
        assert_eq!(id(&split_pane(&mut state, "w1:p1")), "w1:p3");
        assert!(remove_pane(&mut state, "w1:p3"));
        assert!(remove_pane(&mut state, "w1:p2"));
        assert_eq!(id(&split_pane(&mut state, "w1:p1")), "w1:p4");
        assert!(find_pane(&state, "w1:p2").is_none());
        assert!(find_pane(&state, "w1:p3").is_none());
    }

    /// A state an older fake wrote has no counter: ids start above the
    /// highest live one.
    #[test]
    fn a_state_without_a_counter_starts_above_the_live_ids() {
        let mut state = one_workspace();
        let ws = &mut state["workspaces"][0];
        ws.as_object_mut().unwrap().remove("next_pane");
        ws["panes"]
            .as_array_mut()
            .unwrap()
            .push(json!({"pane_id": "w1:p5", "workspace_id": "w1", "tab_id": "w1:t1"}));
        assert_eq!(id(&split_pane(&mut state, "w1:p1")), "w1:p6");
        assert_eq!(id(&split_pane(&mut state, "w1:p1")), "w1:p7");
    }
}
