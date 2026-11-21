//! The probes of the conformance suite in the fake agents (fleetd-p7
//! ticket 16, `docs/tickets/fleetd-p7/MAP.md` Q6).
//!
//! - `version:<text>`: `--version` prints `<text>` ([`version_or`]).
//! - `conformance`: the fake reports what it sees ([`report`]): the skills
//!   and the rules in its private config home, the MCP servers of its
//!   config, the effective compaction setting and the hook events that it
//!   sent to the wrapper. It also tries a write outside the allowed paths,
//!   a write inside them, and the 3 operator acts, each in a child under
//!   [`horch_platform::sandbox::sandboxed`] with the rules of its rendered
//!   config, and records each result (`operator_boundary`). A config that
//!   renders no sandbox rule runs the child with no sandbox. It then writes
//!   `outbox/result.json` of its task folder the same way ([`write_result`]).
//!
//! Each result is `allowed` or `refused: <why>`. The probe removes each
//! file that a write made. It never prints the token.

use std::path::{Path, PathBuf};

use horch_core::fleetd::worker_profiles::domain::operator_boundary::OperatorPaths;
use horch_core::runtime::{Paths, ProcessEnv};
use horch_platform::sandbox::{sandboxed, Denials};
use serde_json::{json, Map, Value};

use crate::exec::Cli;
use crate::{scenarios, Call};

/// The first argument of the probe child: the fake does 1 act and exits.
const CHILD: &str = "__horch_probe_act";

const ORPHAN: &str = "__horch_probe_orphan";

/// The text of the scenario key `version:<text>`, else `default`.
pub fn version_or(default: &str) -> String {
    scenarios()
        .iter()
        .find_map(|s| s.strip_prefix("version:").map(str::to_owned))
        .unwrap_or_else(|| default.to_owned())
}

/// When the fake runs as the probe child, do its act and exit. Call it
/// first in `main`.
pub fn child_entry() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let [first, act, path, out] = args.as_slice() {
        if first == ORPHAN {
            orphan_entry(act, Path::new(path), Path::new(out));
        }
    }
    let [first, act, path] = args.as_slice() else {
        return;
    };
    if first != CHILD {
        return;
    }
    match do_act(act, Path::new(path)) {
        Ok(()) => std::process::exit(0),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}

/// The probe orphan: the child makes a new session (so a new process
/// group), starts the grandchild and exits, so the grandchild has no
/// parent, group or session of the agent. The grandchild does `act` and
/// writes `allowed` or `refused: <why>` in `out`. This process exits when
/// its child exited.
#[cfg(unix)]
fn orphan_entry(act: &str, path: &Path, out: &Path) -> ! {
    // SAFETY: the fake is 1 thread here; each branch only calls exit,
    // setsid, waitpid or plain Rust code.
    unsafe {
        let child = libc::fork();
        if child > 0 {
            let mut status = 0;
            libc::waitpid(child, &mut status, 0);
            std::process::exit(0);
        }
        if child < 0 {
            std::process::exit(1);
        }
        libc::setsid();
        if libc::fork() != 0 {
            libc::_exit(0);
        }
    }
    let result = match do_act(act, path) {
        Ok(()) => "allowed".to_owned(),
        Err(e) => format!("refused: {e}"),
    };
    let tmp = out.with_extension("tmp");
    let _ = std::fs::write(&tmp, result).and_then(|()| std::fs::rename(&tmp, out));
    std::process::exit(0);
}

#[cfg(not(unix))]
fn orphan_entry(_act: &str, _path: &Path, _out: &Path) -> ! {
    std::process::exit(1);
}

fn do_act(act: &str, path: &Path) -> Result<(), String> {
    match act {
        "stat" => identity(path).map(|id| print!("{id}")),
        "write" => std::fs::write(path, "horch probe\n").map_err(|e| e.to_string()),
        "read" => match std::fs::read(path) {
            Ok(bytes) if bytes.is_empty() => Err("the read gave no bytes".to_owned()),
            Ok(_) => Ok(()),
            Err(e) => Err(e.to_string()),
        },
        "connect" => connect(path),
        other => Err(format!("unknown act {other}")),
    }
}

#[cfg(unix)]
fn identity(path: &Path) -> Result<String, String> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path)
        .map(|m| format!("{}:{}", m.dev(), m.ino()))
        .map_err(|e| e.to_string())
}

#[cfg(not(unix))]
fn identity(_path: &Path) -> Result<String, String> {
    Err("no device and inode on this OS".to_owned())
}

#[cfg(unix)]
fn connect(path: &Path) -> Result<(), String> {
    std::os::unix::net::UnixStream::connect(path)
        .map(drop)
        .map_err(|e| e.to_string())
}

#[cfg(not(unix))]
fn connect(_path: &Path) -> Result<(), String> {
    Err("no Unix socket on this OS".to_owned())
}

/// For a fake with no exec mode (OpenCode, pi, Prime): with the key
/// `conformance`, record the operator boundary probe in the log. These
/// CLIs render no sandbox rule (MAP.md Q5).
pub fn record_operator_boundary(call: &mut Call) {
    if crate::scenario_has("conformance") {
        let probe = json!({"operator_boundary": operator_boundary(None)});
        call.extra.insert("conformance".into(), probe);
    }
}

/// The report of 1 conformance run of `cli` with `argv`. `hooks` are the
/// hook events that the fake printed to the wrapper.
pub fn report(cli: Cli, argv: &[String], hooks: Vec<String>) -> Value {
    let seen = config(cli, argv);
    json!({
        "skills": seen.skills,
        "rules": seen.rules,
        "mcp_servers": seen.mcp_servers,
        "compaction": seen.compaction,
        "hooks": hooks,
        "operator_boundary": operator_boundary(seen.rules_of_sandbox.as_ref()),
    })
}

/// The shared write of a conformance run: write `outbox/result.json` of
/// the task folder of `prompt` (`<task>/inbox/START.md`) in a child under
/// the sandbox rules of the config, as the agent does. The file stays, so
/// the wrapper finds it. The result is `allowed` or `refused: <why>`.
pub fn write_result(cli: Cli, argv: &[String], prompt: &str) -> String {
    let Some(task) = Path::new(prompt).parent().and_then(Path::parent) else {
        return "not run: the prompt is not in a task folder".to_owned();
    };
    let rules = config(cli, argv).rules_of_sandbox;
    in_child(
        rules.as_ref(),
        "write",
        &task.join("outbox").join("result.json"),
    )
}

fn config(cli: Cli, argv: &[String]) -> Seen {
    match cli {
        Cli::Claude => claude_config(argv),
        Cli::Codex => codex_config(argv),
        Cli::Agy => agy_config(),
    }
}

/// What the fake sees in its config.
#[derive(Default)]
struct Seen {
    skills: Vec<String>,
    rules: Vec<String>,
    mcp_servers: Vec<String>,
    compaction: Option<String>,
    rules_of_sandbox: Option<Rules>,
}

/// The sandbox rules of a rendered config.
struct Rules {
    writable: Vec<PathBuf>,
    denials: Denials,
}

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn home() -> PathBuf {
    env_path("HOME").unwrap_or_default()
}

fn cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_default()
}

/// The names of the entries of `dir`, or none.
fn names_in(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

fn sorted(mut names: Vec<String>) -> Vec<String> {
    names.sort();
    names.dedup();
    names
}

/// The values of each `flag` of `argv`. A variadic flag takes each value up
/// to the next flag.
fn values_of(argv: &[String], flag: &str, variadic: bool) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < argv.len() {
        if argv[i] == flag {
            let mut j = i + 1;
            while j < argv.len() && !argv[j].starts_with('-') {
                out.push(argv[j].clone());
                j += 1;
                if !variadic {
                    break;
                }
            }
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

/// A JSON value given inline or as a file path.
fn json_arg(text: &str) -> Value {
    let text = if text.trim_start().starts_with('{') {
        text.to_owned()
    } else {
        std::fs::read_to_string(text).unwrap_or_default()
    };
    serde_json::from_str(&text).unwrap_or(Value::Null)
}

fn keys_of(v: &Value) -> Vec<String> {
    v.as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default()
}

fn paths_of(v: &Value) -> Vec<PathBuf> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_default()
}

fn claude_config(argv: &[String]) -> Seen {
    let home = env_path("CLAUDE_CONFIG_DIR").unwrap_or_else(|| home().join(".claude"));
    let mut skills = names_in(&home.join("skills"));
    for plugin in values_of(argv, "--plugin-dir", false) {
        skills.extend(names_in(&Path::new(&plugin).join("skills")));
    }
    let mut rules = names_in(&home.join("rules"));
    if home.join("CLAUDE.md").is_file() {
        rules.push("CLAUDE.md".into());
    }
    let mut mcp_servers = Vec::new();
    for config in values_of(argv, "--mcp-config", true) {
        mcp_servers.extend(keys_of(&json_arg(&config)["mcpServers"]));
    }
    let settings = values_of(argv, "--settings", false)
        .last()
        .map_or(Value::Null, |s| json_arg(s));
    let window = "CLAUDE_CODE_AUTO_COMPACT_WINDOW";
    let compaction = std::env::var(window)
        .ok()
        .or_else(|| settings["env"][window].as_str().map(str::to_owned));
    let sandbox = &settings["sandbox"];
    let rules_of_sandbox = (sandbox["enabled"] == true).then(|| {
        let fs = &sandbox["filesystem"];
        let mut writable = vec![cwd()];
        writable.extend(paths_of(&fs["allowWrite"]));
        Rules {
            writable,
            denials: Denials {
                read: paths_of(&fs["denyRead"]),
                connect: Vec::new(),
                write: paths_of(&fs["denyWrite"]),
            },
        }
    });
    Seen {
        skills: sorted(skills),
        rules: sorted(rules),
        mcp_servers: sorted(mcp_servers),
        compaction,
        rules_of_sandbox,
    }
}

fn codex_config(argv: &[String]) -> Seen {
    let home = env_path("CODEX_HOME").unwrap_or_else(|| home().join(".codex"));
    let mut rules = names_in(&home.join("rules"));
    if home.join("AGENTS.md").is_file() {
        rules.push("AGENTS.md".into());
    }
    let pairs: Vec<(String, String)> = values_of(argv, "-c", false)
        .into_iter()
        .chain(values_of(argv, "--config", false))
        .filter_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            Some((k.trim().to_owned(), v.trim().to_owned()))
        })
        .collect();
    let toml = std::fs::read_to_string(home.join("config.toml")).unwrap_or_default();
    let mut mcp_servers: Vec<String> = pairs
        .iter()
        .filter_map(|(k, _)| k.strip_prefix("mcp_servers.")?.split('.').next())
        .map(str::to_owned)
        .collect();
    // `-c mcp_servers={}` empties the table of `config.toml`.
    if !pairs.iter().any(|(k, _)| k == "mcp_servers") {
        mcp_servers.extend(toml.lines().filter_map(|l| {
            let name = l.trim().strip_prefix("[mcp_servers.")?;
            Some(name.split(['.', ']']).next()?.to_owned())
        }));
    }
    let limit = "model_auto_compact_token_limit";
    let compaction = pairs
        .iter()
        .rev()
        .find(|(k, _)| k == limit)
        .map(|(_, v)| v.clone())
        .or_else(|| {
            toml.lines().find_map(|l| {
                let (k, v) = l.split_once('=')?;
                (k.trim() == limit).then(|| v.trim().to_owned())
            })
        });
    Seen {
        skills: sorted(names_in(&home.join("skills"))),
        rules: sorted(rules),
        mcp_servers: sorted(mcp_servers),
        compaction,
        rules_of_sandbox: codex_rules(&pairs),
    }
}

/// The rules of the `-c permissions.<profile>.*` pairs: a path of
/// `filesystem` with `"none"` is denied, a path with `"write"` and a path of
/// `workspace_roots` with `true` are writable. No `filesystem` pair is no
/// sandbox rule.
fn codex_rules(pairs: &[(String, String)]) -> Option<Rules> {
    let of = |suffix: &str| {
        pairs
            .iter()
            .filter(|(k, _)| k.starts_with("permissions.") && k.ends_with(suffix))
            .flat_map(|(_, v)| path_pairs(v))
            .collect::<Vec<_>>()
    };
    let filesystem = of(".filesystem");
    if !pairs
        .iter()
        .any(|(k, _)| k.starts_with("permissions.") && k.ends_with(".filesystem"))
    {
        return None;
    }
    let mut writable = vec![cwd()];
    let mut read = Vec::new();
    for (path, value) in filesystem {
        match value.as_str() {
            "none" => read.push(path),
            "write" => writable.push(path),
            _ => {}
        }
    }
    writable.extend(
        of(".workspace_roots")
            .into_iter()
            .filter(|(_, v)| v == "true")
            .map(|(p, _)| p),
    );
    Some(Rules {
        writable,
        denials: Denials {
            read,
            connect: Vec::new(),
            write: Vec::new(),
        },
    })
}

/// The `"<absolute path>"=<value>` pairs of an inline TOML table. A nested
/// table is not a value: its own pairs are read as pairs.
fn path_pairs(text: &str) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('"') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('"') else { break };
        let key = &after[..close];
        let tail = &after[close + 1..];
        let Some(value) = tail.strip_prefix('=') else {
            rest = tail;
            continue;
        };
        let (value, next) = if let Some(quoted) = value.strip_prefix('"') {
            let Some(end) = quoted.find('"') else { break };
            (&quoted[..end], &quoted[end + 1..])
        } else if let Some(nested) = value.strip_prefix('{') {
            ("", nested)
        } else {
            let end = value.find([',', '}']).unwrap_or(value.len());
            (value[..end].trim(), &value[end..])
        };
        if Path::new(key).is_absolute() && !value.is_empty() {
            out.push((PathBuf::from(key), value.to_owned()));
        }
        rest = next;
    }
    out
}

fn agy_config() -> Seen {
    let home = home().join(".gemini").join("antigravity-cli");
    let settings = json_arg(&home.join("settings.json").to_string_lossy());
    Seen {
        skills: sorted(names_in(&home.join("skills"))),
        rules: sorted(names_in(&home.join("rules"))),
        mcp_servers: sorted(keys_of(&settings["mcpServers"])),
        compaction: None,
        rules_of_sandbox: None,
    }
}

/// Try each act in a child under `rules` (no sandbox for `None`), and give
/// the result of each.
fn operator_boundary(rules: Option<&Rules>) -> Value {
    let paths = OperatorPaths::of(&Paths::from_env(&ProcessEnv).fleet_root);
    let cwd = cwd();
    let tag = format!("horch-probe-{}", std::process::id());
    let outside = cwd.parent().unwrap_or(&cwd).join(&tag);
    let request = paths.requests.join(format!("{tag}.json"));
    let acts: [(&str, &str, PathBuf); 5] = [
        ("write_outside", "write", outside),
        ("write_inside", "write", cwd.join(&tag)),
        ("read_token", "read", paths.token.clone()),
        ("connect_channel", "connect", paths.channel.clone()),
        ("write_requests", "write", request),
    ];
    let mut report = Map::new();
    report.insert(
        "sandbox".into(),
        json!(if rules.is_some() { "on" } else { "none" }),
    );
    let fleet_root = paths.run.parent().unwrap_or(&paths.run);
    report.insert("fleet_root".into(), json!(seen_in_child(rules, fleet_root)));
    let mut orphan = Map::new();
    for (name, act, path) in acts {
        let result = match name {
            "write_outside" => write_outside(rules, &path),
            _ => in_child(rules, act, &path),
        };
        if act == "write" {
            let _ = std::fs::remove_file(&path);
        }
        if matches!(name, "read_token" | "connect_channel" | "write_requests") {
            let escapes = crate::scenario_has("orphan_escapes");
            let out = cwd.join(format!("{tag}-orphan-{name}"));
            let result = in_orphan(rules.filter(|_| !escapes), act, &path, &out);
            if act == "write" {
                let _ = std::fs::remove_file(&path);
            }
            orphan.insert(name.into(), Value::String(result));
        }
        report.insert(name.into(), Value::String(result));
    }
    report.insert("orphan".into(), Value::Object(orphan));
    Value::Object(report)
}

fn write_outside(rules: Option<&Rules>, path: &Path) -> String {
    let folder = path.parent().unwrap_or(path);
    let host = identity(folder);
    let seen = seen_in_child(rules, folder);
    if host.as_deref() != Ok(seen.as_str()) {
        return format!(
            "not run: the sandbox does not see the real {}: it sees {seen}, the host has {}",
            folder.display(),
            host.unwrap_or_else(|e| format!("refused: {e}"))
        );
    }
    let result = in_child(rules, "write", path);
    if result.starts_with("refused") && path.exists() {
        return format!(
            "allowed: {} is on the host, the child said {result}",
            path.display()
        );
    }
    result
}

fn seen_in_child(rules: Option<&Rules>, path: &Path) -> String {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => return format!("not run: {e}"),
    };
    let mut cmd = std::process::Command::new(exe);
    cmd.arg(CHILD).arg("stat").arg(path);
    if let Some(rules) = rules {
        cmd = match sandboxed(cmd, &rules.writable, &rules.denials) {
            Ok(cmd) => cmd,
            Err(e) => return format!("not run: {e}"),
        };
    }
    cmd.stdin(std::process::Stdio::null());
    match cmd.output() {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim().to_owned(),
        Ok(out) => format!("refused: {}", String::from_utf8_lossy(&out.stderr).trim()),
        Err(e) => format!("not run: {e}"),
    }
}

fn in_orphan(rules: Option<&Rules>, act: &str, path: &Path, out: &Path) -> String {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => return format!("not run: {e}"),
    };
    let mut cmd = std::process::Command::new(exe);
    cmd.arg(ORPHAN).arg(act).arg(path).arg(out);
    if let Some(rules) = rules {
        cmd = match sandboxed(cmd, &rules.writable, &rules.denials) {
            Ok(cmd) => cmd,
            Err(e) => return format!("not run: {e}"),
        };
    }
    cmd.stdin(std::process::Stdio::null());
    if let Err(e) = cmd.output() {
        return format!("not run: {e}");
    }
    let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if let Ok(result) = std::fs::read_to_string(out) {
            let _ = std::fs::remove_file(out);
            return result;
        }
        if std::time::Instant::now() >= end {
            return "not run: the orphan wrote no result in 10 s".to_owned();
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn in_child(rules: Option<&Rules>, act: &str, path: &Path) -> String {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => return format!("not run: {e}"),
    };
    let mut cmd = std::process::Command::new(exe);
    cmd.arg(CHILD).arg(act).arg(path);
    if let Some(rules) = rules {
        cmd = match sandboxed(cmd, &rules.writable, &rules.denials) {
            Ok(cmd) => cmd,
            Err(e) => return format!("not run: {e}"),
        };
    }
    cmd.stdin(std::process::Stdio::null());
    match cmd.output() {
        Ok(out) if out.status.success() => "allowed".to_owned(),
        Ok(out) => format!("refused: {}", String::from_utf8_lossy(&out.stderr).trim()),
        Err(e) => format!("not run: {e}"),
    }
}
