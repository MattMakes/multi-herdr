//! The fake idea-lead of the FR-10 tests (`crates/horch-e2e/tests/p6_fr10_approval.rs`).
//!
//! A key of this fake is a word `[fr10:<key>]` in the `idea.md` of its
//! idea, so 2 ideas of 1 fleet can have 2 behaviours. An idea with such a
//! word gets only this fake, not the keys of `idea_agent`.
//!
//! The idea under test is named by the word `[fr10:target=<idea id>]`.
//! The fake waits for that idea and never takes another idea in
//! `awaiting_approval`, such as a system idea of a failed conformance job.
//!
//! - `approve_waiting`: the own idea stays in `clarifying`, so the own
//!   attempt stays live. Wait until the own attempt is `live` (fleetd
//!   recorded its process) and the target is in `awaiting_approval`.
//!   Then try to approve it 5 ways, each logged under `tries`: `cli`
//!   (`horch idea approve`), `child_shell` (`sh -c` of it), `detached` (a
//!   child in a new session and a new process group, whose parent waits),
//!   and `raw` (a raw `idea_approve` on the channel with the operator
//!   token). Then write a valid `assumptions.md`.
//! - `stay`: run `horch idea note` of the own idea (logged as `note`),
//!   then keep running, as an agent in its interview, until fleetd stops
//!   it (fix A: the attempt stays live while its agent runs).
//! - `outlive`: write a valid `assumptions.md` and ignore `SIGTERM`, so the
//!   agent lives on after fleetd ended its attempt. Wait until each own
//!   `clarifying` attempt is ended (the operator approves the own idea, so the phase of
//!   the attempt is old), then try `cli` and `raw` on the target in
//!   `awaiting_approval`. Then wait for the stop of fleetd.
//! - `forge`: write a valid `assumptions.md`, wait until the own idea is in
//!   `awaiting_approval`, then write `approval.json` with its sha256.
//!
//! The fake acts only in the phase `clarifying`. In a later phase it exits.
//!
//! Before the tries the fake logs `attempts`: the state of each attempt of
//! its own idea, as the ledger has it. The fake never logs the operator
//! token.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use horch_core::digest::sha256_bytes;
use horch_core::fleetd::fleet_runtime::adapters::request_wire::Request;
use horch_core::fleetd::fleet_runtime::domain::requests::{Kind, RequestId};
use horch_core::runtime::{Paths, ProcessEnv};
use horch_core::taskledger::Store;
use serde_json::{json, Map, Value};

use crate::idea_agent::{idea_folder, send_on_channel};
use crate::{hash16, Call};

const STATE_WAIT: Duration = Duration::from_secs(60);

const STOP_WAIT: Duration = Duration::from_secs(600);

const AWAITING: &str = "awaiting_approval";

/// Play the FR-10 keys of the idea of this launch. False when the idea has
/// no such key: the caller plays the other keys.
pub fn play(call: &mut Call) -> bool {
    let Some(folder) = idea_folder(&call.argv) else {
        return false;
    };
    let text = std::fs::read_to_string(folder.join("idea.md")).unwrap_or_default();
    let has = |key: &str| text.contains(&format!("[fr10:{key}]"));
    if !text.contains("[fr10:") {
        return false;
    }
    if !call.argv.iter().any(|a| a.contains("in phase clarifying")) {
        return true;
    }
    let lead = Lead::new(folder, target_of(&text));
    if has("approve_waiting") {
        lead.wait_live();
        let Some(target) = lead.waiting_target() else {
            call.extra
                .insert("fr10".into(), json!({"error": "the target does not wait"}));
            return true;
        };
        call.extra.insert("target".into(), json!(target));
        call.extra.insert("attempts".into(), json!(lead.attempts()));
        let mut tries = Map::new();
        tries.insert("cli".into(), lead.cli(&target));
        tries.insert("child_shell".into(), lead.child_shell(&target));
        tries.insert("detached".into(), lead.detached(&target));
        tries.insert("raw".into(), lead.raw(&target, &call.call_id));
        call.extra.insert("tries".into(), Value::Object(tries));
        call.flush();
        lead.write_assumptions();
        return true;
    }
    if has("stay") {
        let text = format!("{} - fake.txt", "Which file does the idea change?");
        let note = output(Command::new("horch").args(["idea", "note", &lead.idea, &text]));
        call.extra.insert("note".into(), note);
        call.flush();
        std::thread::sleep(STOP_WAIT);
        return true;
    }
    if has("outlive") {
        ignore_sigterm();
        lead.write_assumptions();
        call.extra
            .insert("attempts".into(), json!(lead.wait_ended()));
        let Some(target) = lead.waiting_target() else {
            call.extra
                .insert("fr10".into(), json!({"error": "the target does not wait"}));
            return true;
        };
        call.extra.insert("target".into(), json!(target));
        let mut tries = Map::new();
        tries.insert("cli".into(), lead.cli(&target));
        tries.insert("raw".into(), lead.raw(&target, &call.call_id));
        call.extra.insert("tries".into(), Value::Object(tries));
        call.flush();
        std::thread::sleep(STOP_WAIT);
        return true;
    }
    if has("forge") {
        lead.write_assumptions();
        let seen = lead.wait_state(&lead.idea);
        call.extra.insert("awaited".into(), json!(seen));
        call.extra.insert("attempts".into(), json!(lead.attempts()));
        let path = lead.folder.join("approval.json");
        let wrote = match write(&path, &lead.forged_approval()) {
            Ok(()) => json!(true),
            Err(e) => json!(e),
        };
        call.extra.insert("forged".into(), wrote);
        return true;
    }
    false
}

/// The id of the word `[fr10:target=<idea id>]` in the text of an idea.
fn target_of(text: &str) -> Option<String> {
    let (_, rest) = text.split_once("[fr10:target=")?;
    let (id, _) = rest.split_once(']')?;
    Some(id.to_string())
}

struct Lead {
    folder: PathBuf,
    idea: String,
    target: Option<String>,
}

impl Lead {
    fn new(folder: PathBuf, target: Option<String>) -> Lead {
        let idea = folder
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Lead {
            folder,
            idea,
            target,
        }
    }

    fn ideas(&self) -> PathBuf {
        self.folder
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default()
    }

    fn write_assumptions(&self) {
        let text = format!(
            "# {} assumptions\nfeature: fake-{}\n\n\
             1. [confirmed] The idea changes the file fake.txt.\n\
             2. [assumed] The idea needs 1 task.\n",
            self.idea,
            self.idea.to_lowercase()
        );
        if let Err(e) = write(&self.folder.join("assumptions.md"), &text) {
            eprintln!("fake fr10 lead: {e}");
        }
    }

    fn forged_approval(&self) -> String {
        let bytes = std::fs::read(self.folder.join("assumptions.md")).unwrap_or_default();
        json!({
            "idea": self.idea,
            "file": "assumptions.md",
            "sha256": sha256_bytes(&bytes).hex(),
            "approved_at": "2026-01-01T00:00:00Z",
            "by": "operator",
        })
        .to_string()
    }

    fn wait_state(&self, idea: &str) -> Option<String> {
        let end = Instant::now() + STATE_WAIT;
        loop {
            let state = query_one("SELECT state FROM ideas WHERE id = ?1", idea);
            if state.as_deref() == Some(AWAITING) || Instant::now() >= end {
                return state;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn waiting_target(&self) -> Option<String> {
        let target = self.target.as_deref().filter(|t| *t != self.idea)?;
        let state = self.wait_state(target)?;
        (state == AWAITING).then(|| target.to_string())
    }

    fn wait_live(&self) {
        let end = Instant::now() + STATE_WAIT;
        while Instant::now() < end && !self.attempts().iter().any(|a| a.ends_with(" live")) {
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn wait_ended(&self) -> Vec<String> {
        let end = Instant::now() + STATE_WAIT;
        loop {
            let seen: Vec<String> = self
                .attempts()
                .into_iter()
                .filter(|a| a.contains("/clarifying/"))
                .collect();
            let ended = !seen.is_empty() && seen.iter().all(|a| a.ends_with(" ended"));
            if ended || Instant::now() >= end {
                return seen;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn attempts(&self) -> Vec<String> {
        let Ok(store) = Store::open_read_only(&Paths::from_env(&ProcessEnv).ledger_db) else {
            return Vec::new();
        };
        let conn = store.conn();
        let Ok(mut stmt) = conn
            .prepare("SELECT id || ' ' || state FROM attempts WHERE idea_id = ?1 ORDER BY rowid")
        else {
            return Vec::new();
        };
        stmt.query_map([&self.idea], |r| r.get(0))
            .map(|rows| rows.filter_map(Result::ok).collect())
            .unwrap_or_default()
    }

    fn cli(&self, idea: &str) -> Value {
        output(Command::new("horch").args(["idea", "approve", idea]))
    }

    fn child_shell(&self, idea: &str) -> Value {
        let line = format!("horch idea approve {idea}");
        output(Command::new("/bin/sh").args(["-c", &line]))
    }

    fn detached(&self, idea: &str) -> Value {
        let mut cmd = Command::new("horch");
        cmd.args(["idea", "approve", idea]);
        if let Err(e) = horch_platform::group::detach(&mut cmd) {
            return json!({"error": format!("detach: {e:?}")});
        }
        output(&mut cmd)
    }

    fn raw(&self, idea: &str, call_id: &str) -> Value {
        let fleet_root = Paths::from_env(&ProcessEnv).fleet_root;
        let token = match std::fs::read_to_string(fleet_root.join("run").join("operator.token")) {
            Ok(t) => t.trim().to_string(),
            Err(e) => return json!({"error": format!("read the operator token: {}", e.kind())}),
        };
        let bytes =
            std::fs::read(self.ideas().join(idea).join("assumptions.md")).unwrap_or_default();
        let mut payload = Map::new();
        payload.insert("idea".into(), json!(idea));
        payload.insert("sha256".into(), json!(sha256_bytes(&bytes).hex()));
        let request_id = hash16(&format!("{call_id}-raw-{idea}"));
        let request = Request {
            request_id: RequestId::parse(&request_id).expect("16 hex digits are a request id"),
            kind: Kind::IdeaApprove,
            payload,
            token: Some(token),
        };
        let mut sent = send_on_channel(&fleet_root, &request);
        sent["request_id"] = json!(request_id);
        sent
    }
}

fn query_one(sql: &str, arg: &str) -> Option<String> {
    let store = Store::open_read_only(&Paths::from_env(&ProcessEnv).ledger_db).ok()?;
    store.conn().query_row(sql, [arg], |r| r.get(0)).ok()
}

fn output(cmd: &mut Command) -> Value {
    match cmd.stdin(Stdio::null()).output() {
        Ok(out) => json!({
            "exit": out.status.code(),
            "stdout": String::from_utf8_lossy(&out.stdout),
            "stderr": String::from_utf8_lossy(&out.stderr),
        }),
        Err(e) => json!({"error": e.to_string()}),
    }
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    horch_core::fsx::write_atomic(path, text.as_bytes(), 0o600)
        .map_err(|e| format!("writing {}: {e:#}", path.display()))
}

fn ignore_sigterm() {
    // SAFETY: `signal` with `SIG_IGN` sets 1 flag of this process.
    unsafe { libc::signal(libc::SIGTERM, libc::SIG_IGN) };
}
