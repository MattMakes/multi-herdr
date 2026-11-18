//! The Phase 4 keys of a fake agent in a pane (fake-claude, fake-codex and
//! fake-antigravity), so an e2e test can play each end of an attempt.
//!
//! The attempt is the one of `$HORCH_TASK_YAML`, the variable a pane of a
//! push attempt has. Without it, the attempt of the worker
//! `$HORCH_WORKER_ID` in the ledger names its folder under the fleet root.
//!
//! Scenario keys, in the order they run:
//! - `bound`: at process entry, before any other key, write `entry.json` in
//!   the attempt folder: the task id that the ledger binds to the worker
//!   (`task_id`, null when none), `bound`, `task_yaml` and
//!   `task_yaml_exists`. The same object goes in the call log as `entry`.
//! - `commit`: append 1 line to `fake-<fake>.txt` in the worktree of
//!   `task.yaml` (else the cwd) and commit that file. Logged as `commit`.
//! - `ask`: `horch tell orchestrator "QUESTION: ..."`. Logged as `ask`.
//! - `result`: `horch result --status completed --summary ...` in the
//!   worktree. Logged as `result`.
//! - `noresult`: exit with no result. It wins over `result`.
//!
//! `horch` is the one on `PATH`, as in a pane.

use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::fleetd::task_execution::adapters::task_files::attempt_dir;
use horch_core::runtime::{Paths, ProcessEnv};
use horch_core::taskledger::Store;
use serde_json::{json, Value};

use crate::{scenario_has, Call};

/// The variable of a pane that names the `task.yaml` of its attempt.
const TASK_YAML_ENV: &str = "HORCH_TASK_YAML";

/// Run the Phase 4 keys of `$HORCH_FAKE_SCENARIO` for a pane launch. Call it
/// first in the launch path, so `bound` records the state at entry.
pub fn play(call: &mut Call) {
    let keys = ["bound", "commit", "ask", "result", "noresult"];
    if !keys.iter().any(|k| scenario_has(k)) {
        return;
    }
    let binding = binding();
    let task_yaml = task_yaml(binding.as_ref());
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
    if scenario_has("commit") {
        let out = commit(call, &worktree);
        call.extra.insert("commit".into(), out);
    }
    if scenario_has("ask") {
        let text = format!(
            "QUESTION: fake {} asks: which option is correct?",
            call.fake
        );
        let mut out = horch(&worktree, &["tell", "orchestrator", &text]);
        out["text"] = json!(text);
        call.extra.insert("ask".into(), out);
    }
    if scenario_has("result") && !scenario_has("noresult") {
        let summary = format!("fake {} committed its work", call.fake);
        let args = ["result", "--status", "completed", "--summary", &summary];
        call.extra.insert("result".into(), horch(&worktree, &args));
    }
    call.flush();
}

/// The worktree of the attempt of this pane: the `worktree` of its
/// `task.yaml`, else the cwd.
pub(crate) fn worktree() -> PathBuf {
    worktree_of(task_yaml(binding().as_ref()).as_deref())
}

/// The `task.yaml` of the attempt: `$HORCH_TASK_YAML`, else the one of the
/// attempt that the ledger binds to the worker.
fn task_yaml(binding: Option<&(String, i64)>) -> Option<PathBuf> {
    env_path(TASK_YAML_ENV).or_else(|| {
        let (task_id, n) = binding?;
        let fleet_root = Paths::from_env(&ProcessEnv).fleet_root;
        Some(attempt_dir(&fleet_root, task_id, &format!("A{n}")).join("task.yaml"))
    })
}

/// The `worktree` of `task_yaml`, else the cwd.
fn worktree_of(task_yaml: Option<&Path>) -> PathBuf {
    task_yaml
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|text| yaml_value(&text, "worktree"))
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
}

/// The task id and the attempt number that the ledger binds to the worker
/// `$HORCH_WORKER_ID`: its newest attempt that has not ended.
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

/// Record the facts at entry, and write them to `entry.json` beside
/// `task_yaml` when the attempt has a folder.
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

/// Append 1 line to `fake-<fake>.txt` in `worktree` and commit it. The new
/// HEAD, or the error.
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

/// Run git in `dir` with no hook and a fixed identity; its trimmed stdout.
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

/// Run `horch <args>` in `dir`; its exit code and output.
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

/// The value of the top-level key `name` of `task.yaml`: 1 key on each
/// line, a plain scalar or a double-quoted scalar with JSON escapes.
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
