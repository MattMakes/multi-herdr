use std::path::PathBuf;

use horch_e2e::{say, scenario_has, Call};
use serde_json::{json, Value};

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

const DETACHED: &str = "HORCH_FAKE_HERDR_DETACHED";

fn main() {
    if scenario_has("hang_split")
        && std::env::args().nth(1).as_deref() == Some("pane")
        && std::env::args().nth(2).as_deref() == Some("split")
    {
        std::thread::sleep(std::time::Duration::from_secs(60));
        std::process::exit(1);
    }
    #[cfg(unix)]
    if std::env::var_os(DETACHED).is_none() {
        use std::os::unix::process::CommandExt;
        let code = std::env::current_exe()
            .and_then(|exe| {
                std::process::Command::new(exe)
                    .args(std::env::args_os().skip(1))
                    .env(DETACHED, std::os::unix::process::parent_id().to_string())
                    .process_group(0)
                    .status()
            })
            .map(|s| s.code().unwrap_or(1))
            .unwrap_or(1);
        std::process::exit(code);
    }
    let caller = std::env::var(DETACHED).ok();
    std::env::remove_var(DETACHED);
    let mut call = Call::start("herdr");
    if let Some(caller) = caller.and_then(|c| c.parse::<u32>().ok()) {
        call.extra.insert("caller".into(), json!(caller));
    }
    if call.argv.get(..2) == Some(&["pane".to_string(), "wait-output".to_string()][..]) {
        let args: Vec<&str> = call.argv.iter().map(String::as_str).collect();
        let code = wait_output(&args[2..]);
        call.flush();
        std::process::exit(code);
    }
    let lock = StateLock::acquire();
    let code = run(&mut call);
    drop(lock);
    call.flush();
    for (pgid, started) in std::mem::take(&mut *KILL.lock().unwrap()) {
        kill_group(pgid, started);
    }
    std::process::exit(code);
}

static KILL: std::sync::Mutex<Vec<(u64, Option<u64>)>> = std::sync::Mutex::new(Vec::new());

struct StateLock(Option<PathBuf>);

const NO_PID_GRACE: std::time::Duration = std::time::Duration::from_secs(10);

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
    if let Some(verb) = args.get(1) {
        if MUTATING.contains(verb) || (args.first() == Some(&"tab") && *verb == "focus") {
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
            let mut root_pane = json!({"pane_id": pane, "workspace_id": ws, "tab_id": tab});
            if scenario_has("mail_drop") {
                root_pane["agent"] = json!("claude");
                root_pane["agent_status"] = json!("working");
            }
            let entry = json!({
                "workspace_id": ws, "label": label, "active_tab_id": tab,
                "panes": [root_pane],
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
        ["pane", "test-state", id, rest @ ..] => {
            let Some(pane) = find_pane_mut(&mut state, id) else {
                eprintln!("fake-herdr: no pane {id}");
                return 1;
            };
            if let Some(status) = flag(rest, "--status") {
                pane["agent_status"] = json!(status);
            }
            if let Some(agent) = flag(rest, "--agent") {
                pane["agent"] = json!(agent);
            }
            if let Some(text) = flag(rest, "--text") {
                pane["test_screen"] = json!(text);
            }
            save(&state);
            ok(json!({"type": "ok"}))
        }
        ["notification", "show", _, ..] => ok(json!({"type": "ok"})),
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
            if scenario_has("fleet_probe_ack") {
                if let Some(target) = find_pane_mut(&mut state, pane) {
                    target["agent"] = json!("claude");
                    target["agent_status"] = json!("idle");
                    save(&state);
                }
            }
            if scenario_has("exec") {
                if let Some(pid) = exec_detached(pane, command) {
                    state["pids"][*pane] = json!(pid);
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
        ["agent", "prompt", pane, ..] | ["pane", "send-text", pane, ..]
            if scenario_has("fail_typing") =>
        {
            eprintln!("fake-herdr: typing into {pane} refused (scenario fail_typing)");
            1
        }
        ["agent", "prompt", pane, ..] | ["pane", "send-text", pane, ..]
            if scenario_has("fail_first_typing")
                && match state["typing_failures"][*pane].as_u64().unwrap_or(0) {
                    0 => args[0] == "agent",
                    1 => args[0] == "pane",
                    _ => false,
                } =>
        {
            let failures = state["typing_failures"][*pane].as_u64().unwrap_or(0);
            state["typing_failures"][*pane] = json!(failures + 1);
            save(&state);
            eprintln!("fake-herdr: typing into {pane} refused once (scenario fail_first_typing)");
            1
        }
        ["agent", "prompt", pane, text]
            if scenario_has("mail_drop") && !text.contains("(redelivery") =>
        {
            if let Some(target) = find_pane_mut(&mut state, pane) {
                target["agent_status"] = json!("idle");
                save(&state);
            }
            ok(json!({"type": "ok"}))
        }
        ["pane", "send-text", pane, text] if scenario_has("shell") => {
            let typed = state["typed"][*pane].as_str().unwrap_or("").to_string();
            state["typed"][*pane] = json!(format!("{typed}{text}"));
            save(&state);
            ok(json!({"type": "ok"}))
        }
        ["agent", "prompt", pane, text]
            if scenario_has("fleet_probe_ack")
                && text.contains("run horch inbox to confirm delivery") =>
        {
            let workspace = pane.split(':').next().unwrap_or(pane);
            let Some(binary) = std::env::current_exe()
                .ok()
                .and_then(|p| p.canonicalize().ok())
                .and_then(|p| p.parent().map(|dir| dir.join("horch")))
            else {
                return 1;
            };
            let mut command = std::process::Command::new(&binary);
            command
                .arg("inbox")
                .env("HORCH_WORKSPACE_ID", workspace)
                .env_remove("HORCH_ROLE")
                .env_remove("HORCH_RECORD_ID")
                .env_remove("HORCH_WORKER_ID")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            if let Err(error) = command.spawn() {
                call.extra.insert(
                    "probe_inbox_error".into(),
                    json!(format!("{}: {error}", binary.display())),
                );
                return 1;
            }
            ok(json!({"type": "ok"}))
        }
        ["agent", "prompt", pane, text]
            if text.starts_with("/compact")
                && std::env::var_os("HORCH_FAKE_COMPACT_TRANSCRIPT").is_some() =>
        {
            use std::io::Write;
            let path = std::env::var_os("HORCH_FAKE_COMPACT_TRANSCRIPT").unwrap();
            let lines = [
                json!({"type":"system","subtype":"compact_boundary","timestamp":"2026-10-29T04:00:00Z","compactMetadata":{"trigger":"manual","preTokens":311225,"postTokens":1200}}),
                json!({"type":"assistant","timestamp":"2026-10-29T04:00:01Z","sessionId":"fixture","message":{"id":"post-compact","model":"claude-opus-5-5","role":"assistant","usage":{"input_tokens":1200,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":1}}}),
            ];
            let result = std::fs::OpenOptions::new()
                .append(true)
                .open(path)
                .and_then(|mut file| {
                    for line in lines {
                        writeln!(file, "{line}")?;
                    }
                    Ok(())
                });
            if result.is_err() {
                return 1;
            }
            if let Some(p) = find_pane_mut(&mut state, pane) {
                p["agent_status"] = json!("idle");
            }
            save(&state);
            ok(json!({"type": "ok"}))
        }
        ["pane", "send-keys", pane, keys @ ..] if scenario_has("shell") => {
            if keys.iter().any(|k| k.eq_ignore_ascii_case("enter")) {
                let line = state["typed"][*pane].as_str().unwrap_or("").to_string();
                state["typed"][*pane] = json!("");
                save(&state);
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

fn find_pane_mut<'a>(state: &'a mut Value, id: &str) -> Option<&'a mut Value> {
    state["workspaces"]
        .as_array_mut()?
        .iter_mut()
        .flat_map(|workspace| workspace["panes"].as_array_mut().into_iter().flatten())
        .find(|pane| pane["pane_id"].as_str() == Some(id))
}

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

fn pane_out(pane: &str) -> Option<PathBuf> {
    std::env::var_os("HORCH_FAKE_LOG").map(|p| {
        let mut p = p;
        p.push(format!(".pane-{}.out", pane.replace(':', "_")));
        PathBuf::from(p)
    })
}

fn pane_text(state: &Value, pane: &str) -> String {
    if let Some(text) =
        find_pane(state, pane).and_then(|p| p["test_screen"].as_str().map(str::to_owned))
    {
        return text;
    }
    let out = pane_out(pane)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    let typed = state["typed"][pane].as_str().unwrap_or("");
    format!("{out}{typed}")
}

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

fn pane_shell() -> (&'static str, &'static str) {
    if cfg!(windows) {
        ("powershell", "-Command")
    } else {
        ("/bin/sh", "-c")
    }
}

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
