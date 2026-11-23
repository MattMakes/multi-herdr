use std::collections::BTreeMap;

use horch_e2e::hc_fake::{
    core_stopped_in, create_terminal, detach_into_own_group, exec_in_terminal, find_terminal,
    find_terminal_mut, flush_kills, load, log_call, public_terminal, remove_terminal_pid, save,
    state_dir, sync_spaces_with_terminals, terminal_screen, NewTerminal, StateLock,
};
use horch_e2e::{say, scenario_has};
use serde_json::{json, Value};

fn hc_error(kind: &str, message: &str, code: i32) -> i32 {
    eprintln!(
        "{}",
        json!({
            "error": {
                "hint": Value::Null,
                "kind": kind,
                "message": message,
            }
        })
    );
    code
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.first().map(String::as_str) == Some("warm") {
        return;
    }
    detach_into_own_group();
    log_call("hc", &argv);

    if argv.first().map(String::as_str) == Some("wait") {
        let code = cmd_wait(&argv[1..]);
        std::process::exit(code);
    }

    let lock = StateLock::acquire();
    let code = dispatch(&argv);
    drop(lock);
    flush_kills();
    std::process::exit(code);
}

fn dispatch(argv: &[String]) -> i32 {
    let args: Vec<&str> = argv.iter().map(String::as_str).collect();
    match args.first().copied() {
        Some("--version" | "-V") => {
            say("hc 0.1.21");
            0
        }
        Some("server") if args.get(1) == Some(&"status") => {
            if state_dir().is_some_and(|d| core_stopped_in(&d).exists()) {
                eprintln!("herdr-core is not running");
                return 1;
            }
            say(&format!("running (pid {}, default)", std::process::id()));
            0
        }
        Some("run") => cmd_run(&args[1..]),
        Some("ls") => cmd_ls(&args[1..]),
        Some("get") => cmd_get(&args[1..]),
        Some("send") => cmd_send(&args[1..]),
        Some("keys") => cmd_keys(&args[1..]),
        Some("read") => cmd_read(&args[1..]),
        Some("kill") => cmd_kill(&args[1..]),
        Some("rm") => cmd_rm(&args[1..]),
        Some("label") => cmd_label(&args[1..]),
        other => hc_error(
            "invalid_args",
            &format!("unknown hc command: {}", other.unwrap_or("")),
            2,
        ),
    }
}

fn cmd_run(args: &[&str]) -> i32 {
    if scenario_has("hang_split") {
        std::thread::sleep(std::time::Duration::from_secs(60));
        return 1;
    }
    if scenario_has("fail_split") {
        return hc_error("io", "hc run refused (scenario fail_split)", 1);
    }
    let mut keep = false;
    let mut no_parent = false;
    let mut name: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut labels: BTreeMap<String, String> = BTreeMap::new();
    let mut prog: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i] {
            "--" => {
                prog.extend(args[i + 1..].iter().map(|s| (*s).to_string()));
                break;
            }
            "--bg" | "--json" => i += 1,
            "--keep" => {
                keep = true;
                i += 1;
            }
            "--no-parent" => {
                no_parent = true;
                i += 1;
            }
            "--name" => {
                name = args.get(i + 1).map(|s| (*s).to_string());
                i += 2;
            }
            "--cwd" => {
                cwd = args.get(i + 1).map(|s| (*s).to_string());
                i += 2;
            }
            "--label" => {
                if let Some(kv) = args.get(i + 1) {
                    if let Some((k, v)) = kv.split_once('=') {
                        labels.insert(k.to_string(), v.to_string());
                    }
                }
                i += 2;
            }
            pos if !pos.starts_with('-') => {
                prog.extend(args[i..].iter().map(|s| (*s).to_string()));
                break;
            }
            _ => i += 1,
        }
    }
    let cwd = cwd.unwrap_or_else(|| {
        std::env::current_dir()
            .map(|d| d.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "/".into())
    });
    let parent = if no_parent {
        None
    } else {
        std::env::var("HC_TERMINAL_ID")
            .ok()
            .filter(|s| !s.is_empty())
    };
    let agent = prog
        .first()
        .and_then(|p| std::path::Path::new(p).file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "zsh".to_string());
    let argv = if prog.is_empty() {
        vec!["zsh".to_string()]
    } else {
        prog.clone()
    };

    let mut state = load();
    let term = create_terminal(
        &mut state,
        NewTerminal {
            agent: &agent,
            argv,
            cwd: cwd.clone(),
            keep,
            parent,
            name,
            labels,
        },
    );
    let addr = term["address"].as_str().unwrap().to_string();
    sync_spaces_with_terminals(&mut state);
    if scenario_has("exec") && !prog.is_empty() {
        let cmdline = prog
            .iter()
            .map(|w| format!("'{}'", w.replace('\'', "'\\''")))
            .collect::<Vec<_>>()
            .join(" ");
        exec_in_terminal(&mut state, &addr, &cwd, &cmdline);
    }
    save(&state);
    say(&json!({
        "address": addr,
        "agent": agent,
        "cwd": cwd,
        "state": "live",
        "turn": 0,
    })
    .to_string());
    0
}

fn cmd_ls(args: &[&str]) -> i32 {
    let mut show_all = false;
    let mut filter_labels: Vec<(&str, &str)> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i] {
            "--all" => {
                show_all = true;
                i += 1;
            }
            "--json" => i += 1,
            "--label" => {
                if let Some(kv) = args.get(i + 1) {
                    if let Some((k, v)) = kv.split_once('=') {
                        filter_labels.push((k, v));
                    }
                }
                i += 2;
            }
            _ => i += 1,
        }
    }
    let state = load();
    let mut hidden_finished: u64 = 0;
    let mut rows: Vec<Value> = Vec::new();
    for t in state["terminals"].as_array().into_iter().flatten() {
        let matches_labels = filter_labels
            .iter()
            .all(|(k, v)| t["labels"][*k].as_str() == Some(*v));
        if !matches_labels {
            continue;
        }
        if !show_all && t["state"].as_str() == Some("finished") {
            hidden_finished += 1;
            continue;
        }
        let status_str = t["status"]
            .as_object()
            .and_then(|o| o.get("state"))
            .cloned()
            .unwrap_or(Value::Null);
        rows.push(json!({
            "address": t["address"],
            "agent": t["agent"],
            "cwd": t["cwd"],
            "name": t["name"],
            "state": t["state"],
            "status": status_str,
            "turn": t["turn"],
        }));
    }
    say(&json!({
        "hidden_finished": hidden_finished,
        "terminals": rows,
    })
    .to_string());
    0
}

fn positional<'a>(args: &'a [&'a str]) -> Vec<&'a str> {
    args.iter().copied().filter(|a| *a != "--json").collect()
}

fn cmd_get(args: &[&str]) -> i32 {
    let pos = positional(args);
    let Some(&addr) = pos.first() else {
        return hc_error("invalid_args", "missing terminal address", 2);
    };
    let state = load();
    let Some(term) = find_terminal(&state, addr) else {
        return hc_error("not_found", &format!("terminal {addr} not found"), 3);
    };
    say(&public_terminal(term).to_string());
    0
}

fn cmd_send(args: &[&str]) -> i32 {
    let pos = positional(args);
    let (Some(&addr), Some(&text)) = (pos.first(), pos.get(1)) else {
        return hc_error("invalid_args", "usage: hc send [--json] <t> <text>", 2);
    };
    if scenario_has("fail_run") || scenario_has("fail_typing") {
        return hc_error("io", "hc send refused (scenario fail)", 1);
    }
    let mut state = load();
    let Some(term_ro) = find_terminal(&state, addr).cloned() else {
        return hc_error("not_found", &format!("terminal {addr} not found"), 3);
    };
    if term_ro["state"].as_str() == Some("finished") {
        return hc_error("finished", &format!("terminal {addr} has finished"), 11);
    }
    if term_ro["status"]["state"].as_str() == Some("blocked") {
        return hc_error(
            "blocked",
            &format!("terminal {addr} is blocked waiting for approval"),
            9,
        );
    }

    if scenario_has("mail_drop") && !text.contains("(redelivery") {
        if let Some(term) = find_terminal_mut(&mut state, addr) {
            term["status"] = json!({"app": "claude-code", "state": "idle"});
            term["reported"] = json!(true);
            let ms = term["status_ms"].as_u64().unwrap_or(1000) + 1;
            term["status_ms"] = json!(ms);
            save(&state);
        }
        say(&json!({"started": true, "status": "idle", "turn": term_ro["turn"]}).to_string());
        return 0;
    }

    if scenario_has("fleet_probe_ack") && text.contains("run horch inbox to confirm delivery") {
        if let Some(space) = term_ro["labels"]["herdr.space"].as_str() {
            if let Some(binary) = std::env::current_exe()
                .ok()
                .and_then(|p| p.canonicalize().ok())
                .and_then(|p| p.parent().map(|dir| dir.join("horch")))
            {
                let _ = std::process::Command::new(&binary)
                    .arg("inbox")
                    .env("HORCH_WORKSPACE_ID", space)
                    .env("HC_TERMINAL_ID", addr)
                    .env("HORCH_BACKEND", "hc")
                    .env_remove("HORCH_ROLE")
                    .env_remove("HORCH_RECORD_ID")
                    .env_remove("HORCH_WORKER_ID")
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
            }
        }
    }

    let cwd = term_ro["cwd"].as_str().unwrap_or("/").to_string();
    let (status_state, turn) = {
        let term = find_terminal_mut(&mut state, addr).unwrap();
        let next_turn = term["turn"].as_u64().unwrap_or(0) + 1;
        term["turn"] = json!(next_turn);
        term["sent"].as_array_mut().unwrap().push(json!(text));
        if scenario_has("fleet_probe_ack") && term["status"].is_null() {
            term["status"] = json!({"app": "claude-code", "state": "idle"});
            term["reported"] = json!(true);
            term["status_ms"] = json!(1000);
        }
        let st = term["status"]
            .as_object()
            .and_then(|o| o.get("state"))
            .cloned()
            .unwrap_or(Value::Null);
        (st, next_turn)
    };
    if scenario_has("exec") {
        exec_in_terminal(&mut state, addr, &cwd, text);
    }
    save(&state);
    say(&json!({
        "started": true,
        "status": status_state,
        "turn": turn,
    })
    .to_string());
    0
}

fn cmd_keys(args: &[&str]) -> i32 {
    let pos = positional(args);
    let Some(&addr) = pos.first() else {
        return hc_error("invalid_args", "missing terminal address", 2);
    };
    let keys = &pos[1..];
    let mut state = load();
    let Some(term) = find_terminal_mut(&mut state, addr) else {
        return hc_error("not_found", &format!("terminal {addr} not found"), 3);
    };
    if term["state"].as_str() == Some("finished") {
        return hc_error("finished", &format!("terminal {addr} has finished"), 11);
    }
    let next_turn = term["turn"].as_u64().unwrap_or(0) + 1;
    term["turn"] = json!(next_turn);
    for k in keys {
        term["keys"].as_array_mut().unwrap().push(json!(k));
    }
    let status_state = term["status"]
        .as_object()
        .and_then(|o| o.get("state"))
        .cloned()
        .unwrap_or(Value::Null);
    save(&state);
    say(&json!({
        "status": status_state,
        "turn": next_turn,
    })
    .to_string());
    0
}

fn cmd_read(args: &[&str]) -> i32 {
    let mut addr: Option<&str> = None;
    let mut max_lines: Option<usize> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i] {
            "--lines" => {
                max_lines = args.get(i + 1).and_then(|s| s.parse().ok());
                i += 2;
            }
            "--json" => i += 1,
            a if !a.starts_with('-') && addr.is_none() => {
                addr = Some(a);
                i += 1;
            }
            _ => i += 1,
        }
    }
    let Some(addr) = addr else {
        return hc_error("invalid_args", "missing terminal address", 2);
    };
    let state = load();
    if find_terminal(&state, addr).is_none() {
        return hc_error("not_found", &format!("terminal {addr} not found"), 3);
    };
    let screen = terminal_screen(&state, addr);
    if let Some(n) = max_lines {
        let all: Vec<&str> = screen.lines().collect();
        let start = all.len().saturating_sub(n);
        let tail = all[start..].join("\n");
        if !tail.is_empty() {
            say(&tail);
        }
    } else if !screen.is_empty() {
        print!("{screen}");
        if !screen.ends_with('\n') {
            println!();
        }
    }
    0
}

fn cmd_kill(args: &[&str]) -> i32 {
    let pos = positional(args);
    let Some(&addr) = pos.first() else {
        return hc_error("invalid_args", "missing terminal address", 2);
    };
    let mut state = load();
    let Some(term) = find_terminal_mut(&mut state, addr) else {
        return hc_error("not_found", &format!("terminal {addr} not found"), 3);
    };
    term["state"] = json!("finished");
    let pub_term = public_terminal(term);
    remove_terminal_pid(&mut state, addr);
    sync_spaces_with_terminals(&mut state);
    save(&state);
    say(&pub_term.to_string());
    0
}

fn cmd_rm(args: &[&str]) -> i32 {
    let pos = positional(args);
    let Some(&addr) = pos.first() else {
        return hc_error("invalid_args", "missing terminal address", 2);
    };
    let mut state = load();
    let before = state["terminals"].as_array().map_or(0, Vec::len);
    if let Some(terms) = state["terminals"].as_array_mut() {
        terms.retain(|t| t["address"].as_str() != Some(addr));
    }
    let after = state["terminals"].as_array().map_or(0, Vec::len);
    if before == after {
        return hc_error("not_found", &format!("terminal {addr} not found"), 3);
    }
    remove_terminal_pid(&mut state, addr);
    sync_spaces_with_terminals(&mut state);
    save(&state);
    say(&format!("removed {addr}"));
    0
}

fn cmd_label(args: &[&str]) -> i32 {
    let pos = positional(args);
    let Some(&addr) = pos.first() else {
        return hc_error("invalid_args", "missing terminal address", 2);
    };
    let mut state = load();
    let Some(term) = find_terminal_mut(&mut state, addr) else {
        return hc_error("not_found", &format!("terminal {addr} not found"), 3);
    };
    for kv in &pos[1..] {
        if let Some((k, v)) = kv.split_once('=') {
            if v.is_empty() {
                if let Some(map) = term["labels"].as_object_mut() {
                    map.remove(k);
                }
            } else {
                term["labels"][k] = json!(v);
            }
        }
    }
    let pub_term = public_terminal(term);
    sync_spaces_with_terminals(&mut state);
    save(&state);
    say(&pub_term.to_string());
    0
}

fn cmd_wait(args: &[String]) -> i32 {
    let mut addr: Option<&str> = None;
    let mut needle: Option<&str> = None;
    let mut timeout_secs: f64 = 5.0;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => i += 1,
            "--output" => {
                needle = args.get(i + 1).map(String::as_str);
                i += 2;
            }
            "--timeout" => {
                if let Some(s) = args.get(i + 1).and_then(|v| v.parse::<f64>().ok()) {
                    timeout_secs = s;
                }
                i += 2;
            }
            a if !a.starts_with('-') && addr.is_none() => {
                addr = Some(a);
                i += 1;
            }
            _ => i += 1,
        }
    }
    let Some(addr) = addr else {
        return hc_error("invalid_args", "missing terminal address", 2);
    };
    let needle = needle.unwrap_or("");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs_f64(timeout_secs);
    loop {
        let state = load();
        let Some(term) = find_terminal(&state, addr) else {
            return hc_error("not_found", &format!("terminal {addr} not found"), 3);
        };
        let screen = terminal_screen(&state, addr);
        if screen.contains(needle) {
            say(&json!({
                "children": [],
                "ended": Value::Null,
                "exit_code": Value::Null,
                "reason": "output",
            })
            .to_string());
            return 0;
        }
        if term["state"].as_str() == Some("finished") {
            say(&json!({
                "children": [],
                "ended": 0,
                "exit_code": 0,
                "reason": "exit",
            })
            .to_string());
            return 0;
        }
        if std::time::Instant::now() >= deadline {
            say(&json!({
                "children": [],
                "ended": Value::Null,
                "exit_code": Value::Null,
                "reason": "timeout",
            })
            .to_string());
            return 0;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}
