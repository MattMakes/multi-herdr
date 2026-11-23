use std::collections::BTreeMap;
use std::path::PathBuf;

use horch_e2e::hc_fake::{
    create_terminal, detach_into_own_group, exec_in_terminal, find_terminal, flush_kills, load,
    log_call, mint_pane_id, mint_space_id, mint_tab_id, remove_terminal_pid, save, state_dir,
    sync_spaces_with_terminals, window_closed_in, NewTerminal, StateLock,
};
use horch_e2e::{say, scenario_has};
use serde_json::{json, Map, Value};

fn app_sock_path() -> PathBuf {
    if let Some(home) = std::env::var_os("HERDR_APP_HOME") {
        return PathBuf::from(home).join("herdr-app.sock");
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_default();
    home.join(".config/herdr-app/herdr-app.sock")
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.first().map(String::as_str) == Some("warm") {
        return;
    }
    detach_into_own_group();
    log_call("herdr-app", &argv);

    if matches!(argv.first().map(String::as_str), Some("--version" | "-V")) {
        say("herdr-app 0.1.16");
        return;
    }

    if state_dir().is_some_and(|d| window_closed_in(&d).exists()) {
        eprintln!(
            "herdr-app: no Herdr window at {}: No such file or directory (os error 2)",
            app_sock_path().display()
        );
        std::process::exit(1);
    }

    if scenario_has("hang_split")
        && argv.first().map(String::as_str) == Some("do")
        && argv.get(1).map(String::as_str) == Some("pane.split")
    {
        std::thread::sleep(std::time::Duration::from_secs(60));
        std::process::exit(1);
    }

    let lock = StateLock::acquire();
    let code = dispatch(&argv);
    drop(lock);
    flush_kills();
    std::process::exit(code);
}

fn dispatch(argv: &[String]) -> i32 {
    let args: Vec<&str> = argv.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["status", ..] => {
            say(&json!({"ok": true, "pid": std::process::id()}).to_string());
            0
        }
        ["ls", ..] => cmd_ls(),
        ["do", action, rest @ ..] => cmd_do(action, rest),
        _ => {
            eprintln!("herdr-app: unknown command {:?}", args);
            2
        }
    }
}

fn cmd_ls() -> i32 {
    let mut state = load();
    sync_spaces_with_terminals(&mut state);
    save(&state);

    let mut spaces_out = Vec::new();
    for sp in state["spaces"].as_array().into_iter().flatten() {
        let mut tabs_out = Vec::new();
        for tb in sp["tabs"].as_array().into_iter().flatten() {
            let mut terms_out = Vec::new();
            for entry in tb["terminals"].as_array().into_iter().flatten() {
                let Some(addr) = entry["terminal"].as_str() else {
                    continue;
                };
                let Some(term) = find_terminal(&state, addr) else {
                    continue;
                };
                if term["state"].as_str() != Some("live") {
                    continue;
                }
                let app = term["status"]
                    .as_object()
                    .and_then(|o| o.get("app"))
                    .cloned()
                    .unwrap_or(Value::Null);
                let status = term["status"]
                    .as_object()
                    .and_then(|o| o.get("state"))
                    .cloned()
                    .unwrap_or(Value::Null);
                terms_out.push(json!({
                    "terminal": addr,
                    "pane": entry["pane"],
                    "status": status,
                    "app": app,
                    "name": term["name"],
                    "sub_agents": [],
                }));
            }
            tabs_out.push(json!({
                "id": tb["id"],
                "name": tb["name"],
                "active": tb["active"],
                "settled": tb["settled"],
                "terminals": terms_out,
            }));
        }
        spaces_out.push(json!({
            "id": sp["id"],
            "name": sp["name"],
            "folder": sp["folder"],
            "machine": Value::Null,
            "active": sp["active"],
            "tabs": tabs_out,
        }));
    }
    say(&json!({"spaces": spaces_out}).to_string());
    0
}

fn parse_payload(rest: &[&str]) -> Result<Map<String, Value>, String> {
    if rest.len() == 1 && rest[0].trim_start().starts_with('{') {
        return serde_json::from_str::<Map<String, Value>>(rest[0])
            .map_err(|e| format!("invalid JSON payload: {e}"));
    }
    let mut map = Map::new();
    let mut i = 0;
    while i < rest.len() {
        let arg = rest[i];
        if let Some(key) = arg.strip_prefix("--") {
            match rest.get(i + 1) {
                Some(next) if !next.starts_with("--") => {
                    let val = match *next {
                        "true" => Value::Bool(true),
                        "false" => Value::Bool(false),
                        s => Value::String(s.to_string()),
                    };
                    map.insert(key.replace('-', "_"), val);
                    i += 2;
                }
                _ => {
                    map.insert(key.replace('-', "_"), Value::Bool(true));
                    i += 1;
                }
            }
        } else {
            i += 1;
        }
    }
    Ok(map)
}

fn spawn_with_labels(
    state: &mut Value,
    sp_id: &str,
    tb_id: &str,
    cwd: &str,
    argv_val: Option<&Value>,
) -> String {
    let mut labels = BTreeMap::new();
    labels.insert("herdr.space".into(), sp_id.to_string());
    labels.insert("herdr.tab".into(), tb_id.to_string());
    let prog: Vec<String> = argv_val
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
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
    let term = create_terminal(
        state,
        NewTerminal {
            agent: &agent,
            argv,
            cwd: cwd.to_string(),
            keep: true,
            parent: None,
            name: None,
            labels,
        },
    );
    let addr = term["address"].as_str().unwrap().to_string();
    if scenario_has("exec") && !prog.is_empty() {
        let cmdline = if prog.len() == 3 && prog[1] == "-lc" {
            prog[2].clone()
        } else {
            prog.iter()
                .map(|w| format!("'{}'", w.replace('\'', "'\\''")))
                .collect::<Vec<_>>()
                .join(" ")
        };
        exec_in_terminal(state, &addr, cwd, &cmdline);
    }
    addr
}

fn cmd_do(action: &str, rest: &[&str]) -> i32 {
    let payload = match parse_payload(rest) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("herdr-app: {e}");
            return 2;
        }
    };
    let mut state = load();
    sync_spaces_with_terminals(&mut state);

    match action {
        "space.new" => {
            let name = payload
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("space")
                .to_string();
            let label_fails = std::env::var("HORCH_FAKE_FAIL_LABEL")
                .ok()
                .is_some_and(|l| l == name);
            if scenario_has("fail_create") || label_fails {
                eprintln!("herdr-app: space.new refused (scenario fail_create)");
                return 1;
            }
            let folder = payload
                .get("folder")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| {
                    std::env::current_dir()
                        .map(|d| d.to_string_lossy().into_owned())
                        .unwrap_or_else(|_| "/".into())
                });
            let focus = payload
                .get("focus")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let sp_id = mint_space_id(&mut state);
            let tb_id = mint_tab_id(&mut state);
            let pane_id = mint_pane_id(&mut state);
            let addr = spawn_with_labels(&mut state, &sp_id, &tb_id, &folder, payload.get("argv"));
            if focus {
                for s in state["spaces"].as_array_mut().into_iter().flatten() {
                    s["active"] = json!(false);
                }
            }
            let active = focus || state["spaces"].as_array().is_none_or(Vec::is_empty);
            state["spaces"].as_array_mut().unwrap().push(json!({
                "id": sp_id,
                "name": name,
                "folder": folder,
                "machine": Value::Null,
                "active": active,
                "tabs": [{
                    "id": tb_id,
                    "name": "1",
                    "active": true,
                    "settled": false,
                    "terminals": [{"terminal": addr, "pane": pane_id}],
                }],
            }));
            save(&state);
            say(&json!({
                "space": sp_id,
                "tab": tb_id,
                "terminal": addr,
            })
            .to_string());
            0
        }
        "space.close" => {
            let Some(sp_id) = payload.get("id").and_then(Value::as_str) else {
                eprintln!("herdr-app: space.close missing id");
                return 2;
            };
            let exists = state["spaces"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|s| s["id"].as_str() == Some(sp_id));
            if !exists {
                eprintln!("herdr-app: space {sp_id} not found");
                return 1;
            }
            let addrs: Vec<String> = state["terminals"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|t| t["labels"]["herdr.space"].as_str() == Some(sp_id))
                .filter_map(|t| t["address"].as_str().map(str::to_owned))
                .collect();
            for addr in &addrs {
                remove_terminal_pid(&mut state, addr);
            }
            if let Some(terms) = state["terminals"].as_array_mut() {
                terms.retain(|t| t["labels"]["herdr.space"].as_str() != Some(sp_id));
            }
            if let Some(spaces) = state["spaces"].as_array_mut() {
                spaces.retain(|s| s["id"].as_str() != Some(sp_id));
            }
            save(&state);
            say(&json!({"ok": true}).to_string());
            0
        }
        "tab.new" => {
            if scenario_has("fail_split") {
                eprintln!("herdr-app: tab.new refused (scenario fail_split)");
                return 1;
            }
            let Some(sp_id) = payload.get("space").and_then(Value::as_str) else {
                eprintln!("herdr-app: tab.new missing space");
                return 2;
            };
            let sp_id = sp_id.to_string();
            let Some(sp_folder) = state["spaces"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|s| s["id"].as_str() == Some(sp_id.as_str()))
                .and_then(|s| s["folder"].as_str().map(str::to_owned))
            else {
                eprintln!("herdr-app: space {sp_id} not found");
                return 1;
            };
            let folder = payload
                .get("folder")
                .and_then(Value::as_str)
                .unwrap_or(&sp_folder)
                .to_string();
            let focus = payload
                .get("focus")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let tb_id = mint_tab_id(&mut state);
            let pane_id = mint_pane_id(&mut state);
            let addr = spawn_with_labels(&mut state, &sp_id, &tb_id, &folder, payload.get("argv"));
            let sp = state["spaces"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|s| s["id"].as_str() == Some(sp_id.as_str()))
                .unwrap();
            let next_num = sp["tabs"].as_array().map_or(1, |t| t.len() + 1);
            let default_name = next_num.to_string();
            let name = payload
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(&default_name)
                .to_string();
            if focus {
                for tb in sp["tabs"].as_array_mut().into_iter().flatten() {
                    tb["active"] = json!(false);
                }
            }
            sp["tabs"].as_array_mut().unwrap().push(json!({
                "id": tb_id,
                "name": name,
                "active": focus,
                "settled": false,
                "terminals": [{"terminal": addr, "pane": pane_id}],
            }));
            save(&state);
            say(&json!({
                "space": sp_id,
                "tab": tb_id,
                "terminal": addr,
            })
            .to_string());
            0
        }
        "tab.select" => {
            let Some(tb_id) = payload.get("id").and_then(Value::as_str) else {
                eprintln!("herdr-app: tab.select missing id");
                return 2;
            };
            let mut found = false;
            for sp in state["spaces"].as_array_mut().into_iter().flatten() {
                let has = sp["tabs"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|t| t["id"].as_str() == Some(tb_id));
                if has {
                    found = true;
                    for tb in sp["tabs"].as_array_mut().into_iter().flatten() {
                        tb["active"] = json!(tb["id"].as_str() == Some(tb_id));
                    }
                }
            }
            if !found {
                eprintln!("herdr-app: tab {tb_id} not found");
                return 1;
            }
            save(&state);
            say(&json!({"ok": true}).to_string());
            0
        }
        "pane.split" => {
            if scenario_has("fail_split") {
                eprintln!("herdr-app: pane.split refused (scenario fail_split)");
                return 1;
            }
            let Some(from_pane) = payload.get("pane").and_then(Value::as_str) else {
                eprintln!("herdr-app: pane.split missing pane");
                return 2;
            };
            let mut target: Option<(String, String, String)> = None;
            for sp in state["spaces"].as_array().into_iter().flatten() {
                let sp_id = sp["id"].as_str().unwrap_or_default();
                let sp_folder = sp["folder"].as_str().unwrap_or("/");
                for tb in sp["tabs"].as_array().into_iter().flatten() {
                    let tb_id = tb["id"].as_str().unwrap_or_default();
                    for entry in tb["terminals"].as_array().into_iter().flatten() {
                        if entry["pane"].as_str() == Some(from_pane) {
                            let from_addr = entry["terminal"].as_str().unwrap_or_default();
                            let cwd = find_terminal(&state, from_addr)
                                .and_then(|t| t["cwd"].as_str())
                                .unwrap_or(sp_folder)
                                .to_string();
                            target = Some((sp_id.to_string(), tb_id.to_string(), cwd));
                        }
                    }
                }
            }
            let Some((sp_id, tb_id, cwd)) = target else {
                eprintln!("herdr-app: pane {from_pane} not found");
                return 1;
            };
            let pane_id = mint_pane_id(&mut state);
            let addr = spawn_with_labels(&mut state, &sp_id, &tb_id, &cwd, payload.get("argv"));
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
            save(&state);
            say(&json!({
                "pane": pane_id,
                "terminal": addr,
            })
            .to_string());
            0
        }
        "pane.focus" => {
            let term = payload.get("terminal").and_then(Value::as_str);
            let pane = payload.get("pane").and_then(Value::as_str);
            let found = state["spaces"]
                .as_array()
                .into_iter()
                .flatten()
                .flat_map(|sp| sp["tabs"].as_array().into_iter().flatten())
                .flat_map(|tb| tb["terminals"].as_array().into_iter().flatten())
                .any(|e| {
                    term.is_some_and(|t| e["terminal"].as_str() == Some(t))
                        || pane.is_some_and(|p| e["pane"].as_str() == Some(p))
                });
            if !found {
                eprintln!("herdr-app: pane.focus target not found");
                return 1;
            }
            say(&json!({"ok": true}).to_string());
            0
        }
        other => {
            eprintln!("herdr-app: unsupported action {other}");
            2
        }
    }
}
