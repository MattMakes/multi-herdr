//! A recording stand-in for the `codex` CLI.
//!
//! As a pane agent it records argv and exits 0. As `codex app-server` it
//! speaks the JSON-RPC lines the quota probe uses, in the shape Codex 0.158.0
//! ships.
//!
//! Violation: any method other than `initialize`, `initialized` and
//! `account/rateLimits/read`. A thread or a turn is never allowed.
//!
//! Scenarios: `limits_weekly` (the default), `limits_5h_weekly`,
//! `limits_not_allowed`, `hang`, `error`. `inspect_skills`: a pane launch
//! also writes `$CODEX_HOME/skills` and the files under it to
//! `$HORCH_FAKE_LOG.skills.json` (`horch_e2e::write_skills_report`).
//! A pane launch also plays the Phase 4 keys `bound`, `commit`, `ask`,
//! `result` and `noresult` (`horch_e2e::agent`).
//! It then plays the Phase 6 keys of the fake idea-lead and the fake judge
//! (`horch_e2e::idea_agent`).
//!
//! As a pane agent, the scenario `stay` makes a launch behave like the codex
//! TUI: a fresh launch writes a rollout file for its cwd under
//! `$HOME/.codex/sessions/`, then the launch sleeps until killed. The session
//! id is a UUID derived from the canonical cwd, recorded as `session_id`; a
//! `resume <id>` launch records `resumed` and writes nothing.
//!
//! A pane launch plays the Phase 5 report keys after the Phase 4 keys, each
//! as a tool command of the real Codex: `report_progress` runs
//! `horch ledger progress`, `report_done` runs `horch ledger done`, and
//! `report_result` runs `horch result`. A command that an allow rule of
//! `$CODEX_HOME/rules/` matches runs outside the sandbox, and a forbidden
//! rule refuses it. Each other command runs with writes confined to the cwd
//! and the roots of the last `-c permissions.<p>.workspace_roots=` pair,
//! with the `TMPDIR` of `shell_environment_policy.set.TMPDIR`. Each key logs
//! its exit code, output, `sandboxed` and `writable`. `rules` logs the rules.
//! The sandbox is the one of the platform (`horch_platform::sandbox`).
//!
//! As `codex sandbox ... -C <dir> -- <command>` it runs the command as a
//! child with writes confined to `dir` and the profile's workspace roots,
//! records them as `sandbox_writable`, and exits with the command's code.
//! On macOS the confinement is Seatbelt, as in the real CLI. Elsewhere it is
//! the job sandbox of the platform (`horch_platform::sandbox::sandboxed_job`).

use std::io::BufRead;
use std::path::{Path, PathBuf};

use horch_e2e::{hang, hash16, say, scenario, scenario_has, write_skills_report, Call};
use horch_platform::sandbox::{sandboxed, Denials};
use serde_json::{json, Value};

const ALLOWED: [&str; 3] = ["initialize", "initialized", "account/rateLimits/read"];

fn main() {
    horch_e2e::probe::child_entry();
    // horch reads the model catalog before each Codex launch with
    // multi_agent off (4278edc). Answer like the real CLI and record no
    // call: it is a lookup, not an agent run.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("debug")
        && args.get(1).map(String::as_str) == Some("models")
    {
        say(r#"{"models":[{"slug":"gpt-5.6-sol","multi_agent_version":"v2"}]}"#);
        return;
    }
    let mut call = Call::start("codex");
    if call.argv.first().map(String::as_str) == Some("sandbox") {
        sandbox(&mut call);
    }
    if call.argv.iter().any(|a| a == "--version") {
        say(&horch_e2e::probe::version_or("codex-cli 0.158.0"));
        call.flush();
        return;
    }
    if call.argv == ["update"] {
        update(&call);
    }
    if let Some((cli, prompt)) = horch_e2e::exec::Cli::of_argv("codex", &call.argv) {
        let prompt = prompt.to_owned();
        let conformance_check = std::fs::read_to_string(&prompt)
            .is_ok_and(|t| t.starts_with("This is a conformance check of horch."));
        if scenario_has("inspect_skills") && !conformance_check {
            inspect_skills();
        }
        horch_e2e::exec::run(call, cli, &prompt);
    }
    if call.argv.first().map(String::as_str) != Some("app-server") {
        horch_e2e::agent::play(&mut call);
        report(&mut call);
        horch_e2e::idea_agent::play(&mut call);
        if scenario_has("inspect_skills") {
            inspect_skills();
        }
        if scenario_has("stay") {
            stay(&mut call);
        }
        call.flush();
        return;
    }
    call.flush();
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        call.stdin.push(line.clone());
        let msg: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
        let method = msg["method"].as_str().unwrap_or_default().to_string();
        if !ALLOWED.contains(&method.as_str()) {
            call.violate(format!("method not allowed for a probe: '{method}'"));
        }
        call.flush();
        let id = msg["id"].clone();
        match method.as_str() {
            "initialize" => say(&json!({"id": id, "result": {
                "userAgent": "codex_cli_rs/0.158.0 (fake)"}})
            .to_string()),
            "account/rateLimits/read" => match scenario().as_str() {
                "hang" => {
                    call.flush();
                    hang();
                }
                "error" => say(&json!({"id": id, "error": {
                    "code": -32603, "message": "not signed in"}})
                .to_string()),
                s => {
                    // A notification first: a real server may send them any time.
                    say(&json!({"method": "account/rateLimits/updated",
                                "params": {"rateLimits": {"primary": null}}})
                    .to_string());
                    say(&json!({"id": id, "result": result(s)}).to_string());
                }
            },
            _ => {}
        }
    }
    call.flush();
}

fn window(used: i64, mins: i64, resets: i64) -> Value {
    json!({"usedPercent": used, "windowDurationMins": mins, "resetsAt": resets})
}

/// The `account/rateLimits/read` result. `accountId` is there on purpose:
/// TEL-11 proves horch drops it.
fn result(scenario: &str) -> Value {
    let (primary, secondary, allowed) = match scenario {
        // An older plan shape: 5h in the first slot, the week in the second.
        "limits_5h_weekly" => (
            window(20, 300, 1_790_620_000),
            window(40, 10_080, 1_791_054_949),
            json!(true),
        ),
        "limits_not_allowed" => (window(50, 10_080, 1_791_054_949), Value::Null, json!(false)),
        _ => (window(99, 10_080, 1_791_054_949), Value::Null, json!(true)),
    };
    let snapshot = json!({
        "limitId": "codex", "limitName": null, "primary": primary, "secondary": secondary,
        "credits": {"hasCredits": false, "unlimited": false, "balance": null},
        "planType": "prolite", "rateLimitReachedType": null,
    });
    json!({
        "ordinaryUsageAllowed": allowed,
        "rateLimits": snapshot,
        "rateLimitsByLimitId": {"codex": snapshot},
        "rateLimitResetCredits": null,
        "accountId": "acct_SENTINEL",
    })
}

/// The private home's `skills` entry, and every file under it, relative.
fn inspect_skills() {
    let home = std::env::var_os("CODEX_HOME").map(PathBuf::from);
    let skills = home.as_ref().map(|h| h.join("skills"));
    let mut files = Vec::new();
    if let Some(dir) = &skills {
        list_files(dir, dir, &mut files);
    }
    files.sort();
    write_skills_report(
        "codex",
        &skills.into_iter().collect::<Vec<_>>(),
        json!({"env": {"CODEX_HOME": home.map(|h| h.to_string_lossy().into_owned())},
               "files": files}),
    );
}

fn list_files(root: &Path, dir: &Path, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            list_files(root, &path, out);
        } else if let Ok(rel) = path.strip_prefix(root) {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

/// The `stay` launch: record or write the session, then block until killed.
fn stay(call: &mut Call) {
    if call.argv.first().map(String::as_str) == Some("resume") {
        if let Some(id) = call.argv.iter().skip(1).find(|arg| {
            arg.len() == 36
                && arg.split('-').map(str::len).eq([8, 4, 4, 4, 12])
                && arg.bytes().all(|b| b == b'-' || b.is_ascii_hexdigit())
        }) {
            call.extra.insert("resumed".into(), json!(id));
        }
    } else {
        let id = write_rollout();
        call.extra.insert("session_id".into(), json!(id));
    }
    // Eager: a `stay` launch is killed, so nothing is written after it.
    call.flush();
    hang();
}

/// Write `rollout-<stamp>-<uuid>.jsonl` naming this process's cwd, the file
/// horch's session discovery reads. Returns the uuid.
fn write_rollout() -> String {
    let cwd = std::env::current_dir().unwrap_or_default();
    let cwd = std::fs::canonicalize(&cwd).unwrap_or(cwd);
    let prompt = std::env::args().skip(1).last().unwrap_or_default();
    let launch = format!("{}:{}:{}", cwd.display(), std::process::id(), prompt);
    let h = format!("{}{}", hash16(&launch), hash16("codex"));
    let id = format!(
        "{}-{}-4{}-8{}-{}",
        &h[0..8],
        &h[8..12],
        &h[13..16],
        &h[17..20],
        &h[20..32]
    );
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let dir = home.join(".codex/sessions/2026/10/02");
    let _ = std::fs::create_dir_all(&dir);
    let line = json!({"type": "session_meta", "payload": {"id": id, "cwd": cwd.to_string_lossy()}});
    let _ = std::fs::write(
        dir.join(format!("rollout-2026-10-02T00-00-00-{id}.jsonl")),
        format!(
            "{line}\n{}\n",
            json!({"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":prompt}]}})
        ),
    );
    id
}

/// `codex update`: record the call, then answer as the scenario says.
/// `update_fail:codex` exits 1 with a coloured error line, and
/// `update_hang:codex` sleeps 60 s first. Otherwise it prints 1 line and
/// exits 0.
fn update(call: &Call) -> ! {
    call.flush();
    if horch_e2e::scenario_has("update_fail:codex") {
        eprintln!("\u{1b}[31merror: codex update failed: network unreachable\u{1b}[0m");
        std::process::exit(1);
    }
    if horch_e2e::scenario_has("update_hang:codex") {
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
    horch_e2e::say("codex-cli 0.158.0 is the latest version");
    std::process::exit(0);
}

/// The Phase 5 report keys of a pane launch: see the module doc.
fn report(call: &mut Call) {
    let keys: [(&str, &[&str]); 3] = [
        (
            "report_progress",
            &["ledger", "progress", "fake codex: step 1 is done"],
        ),
        (
            "report_done",
            &["ledger", "done", "fake codex: all steps are done"],
        ),
        (
            "report_result",
            &[
                "result",
                "--status",
                "completed",
                "--summary",
                "fake codex wrote its result",
            ],
        ),
    ];
    if !keys.iter().any(|(key, _)| scenario_has(key)) {
        return;
    }
    let rules = rules();
    call.extra.insert(
        "rules".into(),
        json!(rules
            .iter()
            .map(|(pattern, decision)| json!({"pattern": pattern, "decision": decision}))
            .collect::<Vec<_>>()),
    );
    let cwd = std::env::current_dir().unwrap_or_default();
    let mut writable = vec![cwd.to_string_lossy().into_owned()];
    writable.extend(last_config(&call.argv, ".workspace_roots=").map_or_else(Vec::new, quoted));
    let tmpdir = last_config(&call.argv, "shell_environment_policy.set.TMPDIR=")
        .and_then(|value| quoted(value).into_iter().next());
    for (key, args) in keys.iter().filter(|(key, _)| scenario_has(key)) {
        let mut argv = vec!["horch".to_string()];
        argv.extend(args.iter().map(|a| a.to_string()));
        let out = tool_command(&argv, &rules, &writable, tmpdir.as_deref());
        call.extra.insert((*key).into(), out);
    }
    call.flush();
}

/// Run `argv` as Codex runs a tool command: a forbidden rule refuses it, an
/// allow rule runs it unconfined, else it runs confined to `writable`.
fn tool_command(
    argv: &[String],
    rules: &[(Vec<String>, String)],
    writable: &[String],
    tmpdir: Option<&str>,
) -> Value {
    let matched: Vec<&str> = rules
        .iter()
        .filter(|(pattern, _)| argv.starts_with(pattern))
        .map(|(_, decision)| decision.as_str())
        .collect();
    if matched.contains(&"forbidden") {
        return json!({"exit": null, "forbidden": true, "sandboxed": false});
    }
    let confine = !matched.contains(&"allow");
    let Some(program) = on_path(&argv[0]) else {
        return json!({"error": format!("{} is not on PATH", argv[0])});
    };
    let mut cmd = std::process::Command::new(program);
    cmd.args(&argv[1..]);
    if confine {
        if let Some(tmpdir) = tmpdir {
            cmd.env("TMPDIR", tmpdir);
        }
        let roots: Vec<PathBuf> = writable.iter().map(PathBuf::from).collect();
        cmd = match sandboxed(cmd, &roots, &Denials::default()) {
            Ok(cmd) => cmd,
            Err(e) => return json!({"error": format!("no sandbox on this OS: {e}")}),
        };
    }
    match cmd.output() {
        Ok(out) => json!({
            "exit": out.status.code(),
            "stdout": String::from_utf8_lossy(&out.stdout),
            "stderr": String::from_utf8_lossy(&out.stderr),
            "sandboxed": confine,
            "writable": writable,
        }),
        Err(e) => json!({"error": format!("{argv:?}: {e}")}),
    }
}

/// The prefix rules of `$CODEX_HOME/rules/*.rules`: each pattern and its
/// decision (`allow` when the rule sets none).
fn rules() -> Vec<(Vec<String>, String)> {
    let Some(home) = std::env::var_os("CODEX_HOME") else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = std::fs::read_dir(Path::new(&home).join("rules"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "rules"))
        .collect();
    files.sort();
    let mut rules = Vec::new();
    for text in files.iter().filter_map(|f| std::fs::read_to_string(f).ok()) {
        for block in text.split("prefix_rule(").skip(1) {
            let field = |name: &str| {
                block
                    .lines()
                    .find_map(|l| l.trim().strip_prefix(name)?.trim().strip_prefix('='))
                    .map(str::trim)
            };
            let pattern = field("pattern").map_or_else(Vec::new, quoted);
            let decision = field("decision")
                .and_then(|d| quoted(d).into_iter().next())
                .unwrap_or_else(|| "allow".into());
            if !pattern.is_empty() {
                rules.push((pattern, decision));
            }
        }
    }
    rules
}

/// The value of the last `-c` pair of `argv` whose text contains `key`,
/// from the end of `key` on.
fn last_config<'a>(argv: &'a [String], key: &str) -> Option<&'a str> {
    argv.windows(2)
        .rev()
        .filter(|w| matches!(w[0].as_str(), "-c" | "--config"))
        .find_map(|w| w[1].split_once(key).map(|(_, value)| value))
}

/// Each double-quoted string of `text`, with no escape in it.
fn quoted(text: &str) -> Vec<String> {
    text.split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

/// The path of `name` in a folder of `PATH`.
fn on_path(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
}

/// `codex sandbox [opts] -- <command>`: see the module doc.
fn sandbox(call: &mut Call) -> ! {
    let args = call.argv[1..].to_vec();
    let Some(split) = args.iter().position(|a| a == "--") else {
        call.violate("codex sandbox without -- before the command");
        call.flush();
        std::process::exit(2);
    };
    let (opts, command) = (&args[..split], &args[split + 1..]);
    let mut writable: Vec<String> = Vec::new();
    let mut opts = opts.iter();
    while let Some(opt) = opts.next() {
        let value = opts.next();
        match opt.as_str() {
            "-C" => writable.extend(value.cloned()),
            // `permissions.<p>.workspace_roots={"<root>"=true}`
            "-c" => writable.extend(
                value
                    .and_then(|pair| pair.split_once(".workspace_roots="))
                    .and_then(|(_, roots)| roots.split('"').nth(1))
                    .map(str::to_string),
            ),
            _ => {}
        }
    }
    call.extra
        .insert("sandbox_writable".into(), json!(writable.clone()));
    call.flush();
    let code = match confined(command, &writable).status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(_) => 127,
    };
    std::process::exit(code);
}

/// `command` under a Seatbelt profile that denies every write outside
/// `writable` and `/dev`.
#[cfg(target_os = "macos")]
fn confined(command: &[String], writable: &[String]) -> std::process::Command {
    let allow: String = writable
        .iter()
        .map(|p| {
            // Seatbelt matches resolved paths: /var is /private/var.
            let real = std::fs::canonicalize(p).unwrap_or_else(|_| PathBuf::from(p));
            format!(" (subpath {:?})", real.to_string_lossy())
        })
        .collect();
    let profile = format!(
        "(version 1)(allow default)(deny file-write*)(allow file-write*{allow} (subpath \"/dev\"))"
    );
    let mut cmd = std::process::Command::new("/usr/bin/sandbox-exec");
    cmd.arg("-p").arg(profile).args(command);
    cmd
}

#[cfg(not(target_os = "macos"))]
fn confined(command: &[String], writable: &[String]) -> std::process::Command {
    let plain = || {
        let mut cmd = std::process::Command::new(&command[0]);
        cmd.args(&command[1..]);
        cmd
    };
    let roots: Vec<PathBuf> = writable.iter().map(PathBuf::from).collect();
    horch_platform::sandbox::sandboxed_job(plain(), &roots, &Denials::default())
        .unwrap_or_else(|_| plain())
}
