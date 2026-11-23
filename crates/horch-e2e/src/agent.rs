use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::fleetd::task_execution::adapters::task_files::attempt_dir;
use horch_core::fleetd::task_execution::domain::task::judged_task;
use horch_core::runtime::{Paths, ProcessEnv};
use horch_core::taskledger::model::FileMode;
use horch_core::taskledger::{tasks, Store};
use serde_json::{json, Value};

use crate::idea_agent::{idea_folder, judge, VERDICT_FILE, VERDICT_KEYS};
use crate::{scenario_has, Call};

const TASK_YAML_ENV: &str = "HORCH_TASK_YAML";

pub fn play(call: &mut Call) {
    let keys = ["bound", "commit", "ask", "ask_file", "result", "noresult"];
    if !keys.iter().chain(&VERDICT_KEYS).any(|k| scenario_has(k)) {
        return;
    }
    if idea_folder(&call.argv).is_some() {
        return;
    }
    let binding = binding();
    let task_yaml = task_yaml(binding.as_ref());
    let (role, own) = role(task_yaml.as_deref(), binding.as_ref());
    if scenario_has("bound") {
        let entry = entry(
            call,
            binding.as_ref().map(|b| b.0.as_str()),
            task_yaml.as_deref(),
        );
        call.extra.insert("entry".into(), entry);
        call.flush();
    }
    let worktree = worktree_of(task_yaml.as_deref());
    if role != Role::Judge && scenario_has("commit") {
        let out = commit(call, &worktree);
        call.extra.insert("commit".into(), out);
    }
    if role != Role::Judge && scenario_has("ask") {
        let text = format!(
            "QUESTION: fake {} asks: which option is correct?",
            call.fake
        );
        let mut out = horch(&worktree, &["tell", "orchestrator", &text]);
        out["text"] = json!(text);
        call.extra.insert("ask".into(), out);
    }
    if role != Role::Judge && scenario_has("ask_file") {
        let text = format!("fake {} asks: which option is correct?", call.fake);
        let mut out = horch(&worktree, &["ask", &text]);
        out["text"] = json!(text);
        call.extra.insert("ask_file".into(), out);
    }
    if role != Role::Worker {
        judge(call, own.first().map_or(VERDICT_FILE, String::as_str));
    }
    if scenario_has("result") && !scenario_has("noresult") {
        let summary = format!("fake {} committed its work", call.fake);
        let args = ["result", "--status", "completed", "--summary", &summary];
        call.extra.insert("result".into(), horch(&worktree, &args));
    }
    call.flush();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Worker,
    Judge,
    Any,
}

fn role(task_yaml: Option<&Path>, binding: Option<&(String, i64)>) -> (Role, Vec<String>) {
    let task_id = task_yaml
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|text| yaml_value(&text, "task_id"))
        .or_else(|| binding.map(|b| b.0.clone()));
    let Some(task_id) = task_id else {
        return (Role::Any, Vec::new());
    };
    let Ok(store) = Store::open_read_only(&Paths::from_env(&ProcessEnv).ledger_db) else {
        return (Role::Any, Vec::new());
    };
    let own = tasks::files(store.conn(), &task_id, FileMode::Own).unwrap_or_default();
    let role = match tasks::get(store.conn(), &task_id) {
        Ok(t) if judged_task(&t.kind, t.parent_task_id.as_deref()).is_some() => Role::Judge,
        Ok(_) => Role::Worker,
        Err(_) => Role::Any,
    };
    (role, own)
}

pub(crate) fn worktree() -> PathBuf {
    worktree_of(task_yaml(binding().as_ref()).as_deref())
}

fn task_yaml(binding: Option<&(String, i64)>) -> Option<PathBuf> {
    env_path(TASK_YAML_ENV).or_else(|| {
        let (task_id, n) = binding?;
        let fleet_root = Paths::from_env(&ProcessEnv).fleet_root;
        Some(attempt_dir(&fleet_root, task_id, &format!("A{n}")).join("task.yaml"))
    })
}

fn worktree_of(task_yaml: Option<&Path>) -> PathBuf {
    task_yaml
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|text| yaml_value(&text, "worktree"))
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
}

fn binding() -> Option<(String, i64)> {
    let worker = std::env::var("HORCH_WORKER_ID").ok()?;
    let store = Store::open_read_only(&Paths::from_env(&ProcessEnv).ledger_db).ok()?;
    store
        .conn()
        .query_row(
            "SELECT a.task_id, w.attempt FROM attempts a
             JOIN workers w ON w.worker_id = a.worker_id
             WHERE a.worker_id = ?1 AND a.task_id IS NOT NULL
               AND a.state != 'ended' AND w.attempt IS NOT NULL
             ORDER BY a.rowid DESC LIMIT 1",
            [&worker],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok()
}

fn entry(call: &Call, task_id: Option<&str>, task_yaml: Option<&Path>) -> Value {
    let entry = json!({
        "fake": call.fake,
        "worker_id": std::env::var("HORCH_WORKER_ID").ok(),
        "task_id": task_id,
        "bound": task_id.is_some(),
        "task_yaml": task_yaml.map(|p| p.to_string_lossy().into_owned()),
        "task_yaml_exists": task_yaml.is_some_and(Path::is_file),
    });
    if let Some(dir) = task_yaml.and_then(Path::parent) {
        let written = std::fs::create_dir_all(dir).and_then(|()| {
            let tmp = dir.join("entry.json.tmp");
            std::fs::write(&tmp, entry.to_string())?;
            std::fs::rename(&tmp, dir.join("entry.json"))
        });
        if let Err(e) = written {
            eprintln!(
                "fake {}: writing entry.json in {}: {e}",
                call.fake,
                dir.display()
            );
        }
    }
    entry
}

fn commit(call: &Call, worktree: &Path) -> Value {
    let name = format!("fake-{}.txt", call.fake);
    let appended = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(worktree.join(&name))
        .and_then(|mut f| {
            std::io::Write::write_all(&mut f, format!("{}\n", call.call_id).as_bytes())
        });
    if let Err(e) = appended {
        return json!({"error": format!("writing {name}: {e}")});
    }
    let message = format!("fake {}: commit {name}", call.fake);
    for args in [
        vec!["add", "--", name.as_str()],
        vec!["commit", "-q", "-m", message.as_str(), "--", name.as_str()],
    ] {
        if let Err(e) = git(worktree, &args) {
            return json!({"error": e});
        }
    }
    match git(worktree, &["rev-parse", "HEAD"]) {
        Ok(head) => json!({"head": head}),
        Err(e) => json!({"error": e}),
    }
}

pub(crate) fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let bin = env_path("HORCH_GIT_BIN").unwrap_or_else(|| PathBuf::from("git"));
    let mut cmd = Command::new(bin);
    horch_marketplace::git::scrub_repo_env(&mut cmd);
    cmd.current_dir(dir).args([
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "commit.gpgsign=false",
        "-c",
        "user.name=fake agent",
        "-c",
        "user.email=fake@horch.invalid",
    ]);
    let out = cmd
        .args(args)
        .output()
        .map_err(|e| format!("git {args:?}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub(crate) fn horch(dir: &Path, args: &[&str]) -> Value {
    match Command::new("horch").current_dir(dir).args(args).output() {
        Ok(out) => json!({
            "exit": out.status.code(),
            "stdout": String::from_utf8_lossy(&out.stdout),
            "stderr": String::from_utf8_lossy(&out.stderr),
        }),
        Err(e) => json!({"error": format!("horch {args:?}: {e}")}),
    }
}

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn yaml_value(text: &str, name: &str) -> Option<String> {
    let value = text
        .lines()
        .find_map(|line| line.strip_prefix(name)?.strip_prefix(": "))?;
    if value.starts_with('"') {
        serde_json::from_str(value).ok()
    } else {
        Some(value.to_string())
    }
}
