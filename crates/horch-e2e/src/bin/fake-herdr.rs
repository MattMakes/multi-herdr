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
//! source pane, and answers with its id (`<workspace>:p<n>`). `pane close`
//! removes the pane. Both stay recorded as violations.
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
//!
//! Dataset workspaces (label `multi-herdr-dataset ...`) are exempt from the
//! violation rule: their coordinator splits and closes panes there.
//! `workspace close` removes a workspace and kills every pane command in it.

use std::path::PathBuf;

use horch_e2e::{say, scenario_has, Call};
use serde_json::{json, Value};

const MUTATING: [&str; 7] = ["focus", "tile", "split", "move", "close", "swap", "resize"];

fn main() {
    let mut call = Call::start("herdr");
    let code = run(&mut call);
    call.flush();
    std::process::exit(code);
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

fn save(state: &Value) {
    if let Some(p) = state_path() {
        let _ = std::fs::write(p, state.to_string());
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
                    save(&state);
                }
            }
            ok(json!({"type": "ok"}))
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
    let panes = ws["panes"].as_array_mut().unwrap();
    let tab = panes
        .iter()
        .find(|p| p["pane_id"].as_str() == Some(from))
        .map(|p| p["tab_id"].clone())
        .unwrap_or(Value::Null);
    // The first free `<workspace>:p<n>`: ids are never reused while live.
    let mut n = panes.len() + 1;
    while panes
        .iter()
        .any(|p| p["pane_id"].as_str() == Some(format!("{wid}:p{n}").as_str()))
    {
        n += 1;
    }
    let pane = json!({"pane_id": format!("{wid}:p{n}"), "workspace_id": wid, "tab_id": tab});
    panes.push(pane.clone());
    pane
}

/// Remove a pane from the state and kill its process group, if `exec` started
/// one. False when the pane is not in the state.
fn remove_pane(state: &mut Value, pane: &str) -> bool {
    let mut found = false;
    for ws in state["workspaces"].as_array_mut().into_iter().flatten() {
        if let Some(panes) = ws["panes"].as_array_mut() {
            let before = panes.len();
            panes.retain(|p| p["pane_id"].as_str() != Some(pane));
            found |= panes.len() != before;
        }
    }
    if let Some(pid) = state["pids"].as_object_mut().and_then(|m| m.remove(pane)) {
        if let Some(pid) = pid.as_u64() {
            kill_group(pid);
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
    state["workspaces"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|w| {
            w["label"]
                .as_str()
                .is_some_and(|l| l.starts_with(DATASET_LABEL))
                && (w["workspace_id"].as_str() == Some(target)
                    || w["panes"]
                        .as_array()
                        .is_some_and(|p| p.iter().any(|p| p["pane_id"].as_str() == Some(target))))
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
fn kill_group(pgid: u64) {
    if cfg!(windows) {
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pgid.to_string(), "/T", "/F"])
            .status();
    } else {
        let _ = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(format!("kill -TERM -- -{pgid} 2>/dev/null"))
            .status();
    }
}

/// Run `command` the way a herdr pane would: in a shell, with the pane's id
/// in its environment, not waiting for it. Returns the pid, which is also the
/// process group id on unix.
fn exec_detached(pane: &str, command: &str) -> Option<u32> {
    let out = std::env::var_os("HORCH_FAKE_LOG").map(|p| {
        let mut p = p;
        p.push(format!(".pane-{}.out", pane.replace(':', "_")));
        PathBuf::from(p)
    });
    let (shell, flag) = if cfg!(windows) {
        ("cmd", "/C")
    } else {
        ("/bin/sh", "-c")
    };
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
