//! A recording stand-in for the `herdr` CLI.
//!
//! State (workspaces and panes) lives in `$HORCH_FAKE_LOG.state.json`, so a
//! sequence of calls sees one consistent server. Read-only calls and the three
//! calls `horch telemetry ensure` and `horch fleet` need are answered; every
//! call that would move, focus, split or close something the operator can see
//! is answered too, but recorded as a violation, so a test can prove horch
//! never made it.
//!
//! Scenarios:
//! - `default`: answer everything.
//! - `fail_create`: `workspace create` exits 1. `HORCH_FAKE_FAIL_LABEL`
//!   fails it for one label only, in any scenario.
//! - `exec`: `pane run` also executes the command, detached, with
//!   `HERDR_PANE_ID` set, the way a real pane would. Output goes to
//!   `$HORCH_FAKE_LOG.pane-<id>.out`.

use std::path::PathBuf;

use horch_e2e::{say, scenario, Call};
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
    // Any verb that changes what the operator sees is a violation, whatever
    // noun it is attached to.
    if let Some(verb) = args.get(1) {
        if MUTATING.contains(verb) || (args.first() == Some(&"tab") && *verb == "focus") {
            call.violate(format!("mutating call: herdr {}", args.join(" ")));
        }
    }
    let mut state = load();
    match args.as_slice() {
        ["workspace", "list", ..] => ok(json!({
            "type": "workspace_list",
            "workspaces": state["workspaces"].clone(),
        })),
        ["workspace", "create", rest @ ..] => {
            let label_fails = std::env::var("HORCH_FAKE_FAIL_LABEL")
                .ok()
                .is_some_and(|l| flag(rest, "--label") == Some(l.as_str()));
            if scenario() == "fail_create" || label_fails {
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
            call.extra.insert("ran".into(), json!(command));
            if scenario() == "exec" {
                exec_detached(pane, command);
            }
            ok(json!({"type": "ok"}))
        }
        ["pane", "split", from, ..] => {
            let id = format!("{from}-split");
            ok(json!({"type": "pane", "pane": {"pane_id": id}}))
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

/// Run `command` the way a herdr pane would: in a shell, with the pane's id
/// in its environment, not waiting for it.
fn exec_detached(pane: &str, command: &str) {
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
    let _ = cmd.spawn();
}
