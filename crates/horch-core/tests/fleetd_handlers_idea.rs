//! The fleetd handlers of the idea kinds: `idea_add` (also `--direct` and
//! `--to`), `idea_amend`, `idea_approve`, `idea_revise`, `idea_park` and
//! `idea_cancel` (docs/tickets/fleetd-p6/22-handlers.md; RR-8, SR-8).
//!
//! Each test sends requests through `requests::handle` to a temp ledger at
//! generation 1, and reads the rows and the result. Each test has its own
//! temp git repository and idea folders. No test touches this repository or
//! the live ledger, and no test starts a process other than git.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use horch_core::digest::sha256_bytes;
use horch_core::fleetd::fleet_runtime::adapters::request_wire::{Outcome, Reply, Request};
use horch_core::fleetd::fleet_runtime::application::handlers_idea::{self, IdeaHandlers};
use horch_core::fleetd::fleet_runtime::application::requests::{handle, Handlers};
use horch_core::fleetd::fleet_runtime::domain::authorize::Decision;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::ledger::Generation;
use horch_core::fleetd::fleet_runtime::domain::requests::{Kind, RequestId};
use horch_core::fleetd::ideas::adapters::idea_folder_files::IdeaFolder;
use horch_core::fleetd::ideas::adapters::ideas_store::{self, IdeaRow};
use horch_core::fleetd::ideas::domain::plan_validator::RosterProfile;
use horch_core::fleetd::integration::adapters::git::CliGitGateway;
use horch_core::fleetd::worktree_custody::adapters::git::CliWorktreeGit;
use horch_core::taskledger::{util, Store};
use horch_marketplace::git::scrub_repo_env;
use horch_platform::channel::User;
use rusqlite::Connection;
use serde_json::{json, Value};

const ID: &str = "6f1c2a3e-0b9d-4c55-8e7f-1a2b3c4d5e6f";
const ID_2: &str = "7a2d3b4f-1c0e-4d66-9f80-2b3c4d5e6f70";

/// A ticket in format C1 of a code kind.
const TICKET: &str = "# 03: Retry sync with backoff

<!-- horch-ticket: 1 -->
**Persona:** primary `coder-claude`
**Kind:** feature
**Check:** `cargo test`
**Own:** `src/a.rs`

## Goal

Retry with backoff.
";

const ASSUMPTIONS: &str = "# assumptions
feature: retry-sync

1. [assumed] A retry waits.
";

/// A temp ledger at generation 1, a git repository with 1 commit on
/// `main`, and the idea handlers.
struct World {
    _dir: tempfile::TempDir,
    root: PathBuf,
    repo: PathBuf,
    store: Store,
    conn: Connection,
    handlers: Handlers,
}

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let root = horch_platform::path::canonicalize(dir.path()).unwrap();
    let repo = root.join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    git(&repo, &["add", "a.txt"]);
    git(&repo, &["commit", "-q", "-m", "first"]);
    let db = root.join("ledger.sqlite3");
    let store = Store::open(&db).unwrap();
    store
        .conn()
        .execute(
            "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
             VALUES (1, 1, ?1, 0, ?2, 0, ?2)",
            rusqlite::params![std::process::id(), util::now()],
        )
        .unwrap();
    let conn = Connection::open(&db).unwrap();
    conn.busy_timeout(Duration::from_secs(10)).unwrap();
    let gateway = CliGitGateway::new(
        CliWorktreeGit::new("git", root.join("fleet")).writing(&[repo.join(".git")]),
    );
    let mut handlers = Handlers::new();
    handlers_idea::register(
        &mut handlers,
        IdeaHandlers {
            guard: Generation(1),
            config: Arc::new(FleetConfig::default()),
            ideas: root.join("ideas"),
            owner: owner(),
            roster: Arc::new(roster),
            git: Arc::new(gateway),
        },
    );
    World {
        _dir: dir,
        root,
        repo,
        store,
        conn,
        handlers,
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let mut cmd = Command::new("git");
    scrub_repo_env(&mut cmd);
    let out = cmd
        .current_dir(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

fn owner() -> User {
    User::current().unwrap()
}

fn roster() -> Vec<RosterProfile> {
    let profile = |name: &str, provider: &str, fallback: &str| RosterProfile {
        name: name.into(),
        provider: provider.into(),
        model: "m".into(),
        effort: Some("medium".into()),
        fallback: Some(fallback.into()),
        sandbox_here: true,
    };
    vec![
        profile("coder-claude", "claude", "coder-codex"),
        profile("coder-codex", "codex", "coder-claude"),
    ]
}

fn sha(bytes: &[u8]) -> String {
    sha256_bytes(bytes).hex()
}

fn applied(reply: Reply) -> Value {
    match reply.outcome {
        Outcome::Applied(result) => result,
        Outcome::Refused(reason) => panic!("refused: {reason}"),
    }
}

fn refused(reply: Reply) -> String {
    match reply.outcome {
        Outcome::Refused(reason) => reason,
        Outcome::Applied(result) => panic!("applied: {result}"),
    }
}

impl World {
    fn send_as(&mut self, request_id: &str, kind: Kind, payload: Value) -> Reply {
        let Value::Object(payload) = payload else {
            unreachable!("a payload is an object");
        };
        let req = Request {
            request_id: RequestId::parse(request_id).unwrap(),
            kind,
            payload,
            token: Some("t".into()),
        };
        let decision = Decision::Authorized {
            auth: "token; channel".into(),
        };
        handle(
            &mut self.conn,
            Generation(1),
            &req,
            Some("me"),
            &decision,
            &self.handlers,
        )
        .unwrap()
    }

    fn send(&mut self, kind: Kind, payload: Value) -> Reply {
        self.send_as(ID, kind, payload)
    }

    /// The payload of `horch idea add <text>` with no `--to`.
    fn add_payload(&self, text: &str) -> Value {
        json!({
            "text": text,
            "project": self.repo.to_string_lossy(),
            "primary_branch": "main",
        })
    }

    /// 1 captured idea of a new feature, through its request.
    fn add(&mut self, request_id: &str, text: &str) -> Value {
        let payload = self.add_payload(text);
        applied(self.send_as(request_id, Kind::IdeaAdd, payload))
    }

    fn row(&self, idea: &str) -> IdeaRow {
        ideas_store::idea(self.store.conn(), idea).unwrap().unwrap()
    }

    fn ideas(&self) -> i64 {
        self.store
            .conn()
            .query_row("SELECT COUNT(*) FROM ideas", [], |r| r.get(0))
            .unwrap()
    }

    /// Put `idea` in `state` with no event: the set-up of a test.
    fn put(&self, idea: &str, state: &str) {
        let sql = "UPDATE ideas SET state = ?2 WHERE id = ?1";
        assert_eq!(self.store.conn().execute(sql, [idea, state]).unwrap(), 1);
    }

    /// 1 idea in `awaiting_approval` with assumptions.md in its folder.
    /// Gives its id and the sha256 of assumptions.md.
    fn awaiting_approval(&mut self) -> (String, String) {
        let id = self.add(ID_2, "retry sync")["idea"]
            .as_str()
            .unwrap()
            .to_string();
        self.put(&id, "awaiting_approval");
        let folder = IdeaFolder::create(&self.root.join("ideas"), &id, owner()).unwrap();
        folder.write_assumptions(ASSUMPTIONS).unwrap();
        (id, sha(ASSUMPTIONS.as_bytes()))
    }

    fn ticket(&self, text: &str) -> PathBuf {
        let path = self.root.join("ticket.md");
        std::fs::write(&path, text).unwrap();
        path
    }
}

#[test]
fn idea_add_captures_1_idea_in_a_new_feature() {
    let mut w = world();
    let result = w.add(ID, "make the board faster");
    assert_eq!(result["idea"], "I-0001");
    let row = w.row("I-0001");
    assert_eq!(row.state, "captured");
    assert_eq!(result["feature"], row.feature_id.as_str());
    assert_eq!(result["effects"]["idea_md"], "I-0001");
}

#[test]
fn idea_add_to_joins_the_feature() {
    let mut w = world();
    let first = w.add(ID, "make the board faster");
    let feature = first["feature"].as_str().unwrap();
    let joined = applied(w.send_as(
        ID_2,
        Kind::IdeaAdd,
        json!({ "text": "and sort it", "to": feature }),
    ));
    assert_eq!(joined["idea"], "I-0002");
    assert_eq!(joined["feature"], feature);
    assert_eq!(w.row("I-0002").feature_id, feature);
}

#[test]
fn idea_add_sent_twice_with_1_request_id_has_1_idea() {
    let mut w = world();
    let first = w.add(ID, "make the board faster");
    let second = w.add(ID, "make the board faster");
    assert_eq!(first, second);
    assert_eq!(w.ideas(), 1);
}

#[test]
fn idea_add_without_a_project_or_with_a_bad_field_is_refused() {
    let mut w = world();
    let reason = refused(w.send(Kind::IdeaAdd, json!({ "text": "no project" })));
    assert!(reason.contains("project"), "{reason}");
    let reason = refused(w.send_as(
        ID_2,
        Kind::IdeaAdd,
        json!({ "text": "x", "project": w.repo.to_string_lossy(), "primary_branch": 1 }),
    ));
    assert!(reason.contains("primary_branch"), "{reason}");
    assert_eq!(w.ideas(), 0);
}

#[test]
fn idea_add_direct_has_its_plan_and_your_approval() {
    let mut w = world();
    let path = w.ticket(TICKET);
    let sha256 = sha(TICKET.as_bytes());
    let result = applied(w.send(
        Kind::IdeaAdd,
        json!({
            "direct": path.to_string_lossy(),
            "sha256": sha256,
            "project": w.repo.to_string_lossy(),
            "primary_branch": "main",
        }),
    ));
    let row = w.row("I-0001");
    assert_eq!(row.state, "captured");
    assert_eq!(row.approved_sha256.as_deref(), Some(sha256.as_str()));
    let effects = &result["effects"];
    assert_eq!(effects["idea_md"], "I-0001");
    assert_eq!(effects["ticket"]["text"], TICKET);
    let ticket = effects["ticket"]["path"].as_str().unwrap();
    assert!(ticket.starts_with("tickets/01-"), "{ticket}");
    assert!(
        effects["plan_yaml"].as_str().unwrap().contains(ticket),
        "{effects}"
    );
    assert_eq!(
        effects["approval_json"]["sha256"],
        row.approval_sha256.unwrap().as_str()
    );
    let tip = git(&w.repo, &["rev-parse", "main"]);
    assert_eq!(effects["branch"]["tip"], tip.as_str());
    let name = effects["branch"]["name"].as_str().unwrap();
    assert!(name.starts_with("refs/heads/fleet/"), "{name}");
}

#[test]
fn direct_ticket_with_another_sha256_is_refused() {
    let mut w = world();
    let path = w.ticket(TICKET);
    let other = sha(b"another ticket");
    let reason = refused(w.send(
        Kind::IdeaAdd,
        json!({
            "direct": path.to_string_lossy(),
            "sha256": other,
            "project": w.repo.to_string_lossy(),
            "primary_branch": "main",
        }),
    ));
    assert!(reason.contains("sha256"), "{reason}");
    assert!(reason.contains(&other), "{reason}");
    assert_eq!(w.ideas(), 0);
}

#[test]
fn idea_approve_moves_the_idea_to_specifying() {
    let mut w = world();
    let (id, seen) = w.awaiting_approval();
    let result = applied(w.send(Kind::IdeaApprove, json!({ "idea": id, "sha256": seen })));
    let row = w.row(&id);
    assert_eq!(row.state, "specifying");
    assert_eq!(row.approved_sha256.as_deref(), Some(seen.as_str()));
    assert_eq!(result["idea"], id.as_str());
    assert_eq!(result["state"], "specifying");
    let approval = &result["effects"]["approval_json"];
    assert_eq!(approval["sha256"], row.approval_sha256.unwrap().as_str());
    assert_eq!(
        sha(approval["text"].as_str().unwrap().as_bytes()),
        approval["sha256"].as_str().unwrap()
    );
}

#[test]
fn idea_approve_of_changed_assumptions_is_refused_with_the_reason() {
    let mut w = world();
    let (id, _) = w.awaiting_approval();
    let stale = sha(b"what you read before");
    let reason = refused(w.send(Kind::IdeaApprove, json!({ "idea": id, "sha256": stale })));
    assert!(!reason.is_empty());
    assert_eq!(w.row(&id).state, "awaiting_approval");
    assert_eq!(w.row(&id).approved_sha256, None);
}

#[test]
fn idea_revise_moves_the_idea_to_clarifying() {
    let mut w = world();
    let (id, _) = w.awaiting_approval();
    let result = applied(w.send(
        Kind::IdeaRevise,
        json!({ "idea": id, "text": "retry 3 times, not 5" }),
    ));
    assert_eq!(w.row(&id).state, "clarifying");
    assert_eq!(result["state"], "clarifying");
    let amendment = &result["effects"]["amendment"];
    assert_eq!(amendment["n"], 1);
    assert_eq!(amendment["text"], "retry 3 times, not 5");
}

#[test]
fn idea_amend_of_a_captured_idea_adds_an_amendment() {
    let mut w = world();
    w.add(ID_2, "make the board faster");
    let result = applied(w.send(
        Kind::IdeaAmend,
        json!({ "idea": "I-0001", "text": "only the list view" }),
    ));
    assert_eq!(w.row("I-0001").state, "captured");
    assert_eq!(result["idea"], "I-0001");
    let amendment = &result["effects"]["amendment"];
    assert_eq!(amendment["n"], 1);
    assert_eq!(amendment["text"], "only the list view");
}

#[test]
fn idea_amend_of_a_merged_idea_adds_a_new_idea() {
    let mut w = world();
    w.add(ID_2, "make the board faster");
    w.put("I-0001", "merged");
    let result = applied(w.send(
        Kind::IdeaAmend,
        json!({ "idea": "I-0001", "text": "and the detail view" }),
    ));
    assert_eq!(result["idea"], "I-0002");
    assert_eq!(result["effects"]["idea_md"], "I-0002");
    assert_eq!(w.row("I-0002").state, "captured");
    assert_eq!(w.row("I-0001").state, "merged");
}

#[test]
fn idea_park_moves_the_idea_to_parked() {
    let mut w = world();
    w.add(ID_2, "make the board faster");
    let result = applied(w.send(
        Kind::IdeaPark,
        json!({ "idea": "I-0001", "reason": "later" }),
    ));
    assert_eq!(w.row("I-0001").state, "parked");
    assert_eq!(result["state"], "parked");
    assert_eq!(result["rescues"], json!([]));
    let again = refused(w.send_as(
        "8b3e4c50-2d1f-4e77-a091-3c4d5e6f7081",
        Kind::IdeaPark,
        json!({ "idea": "I-0001", "reason": "later" }),
    ));
    assert!(again.contains("I-0001"), "{again}");
}

#[test]
fn idea_cancel_moves_the_idea_to_cancelled() {
    let mut w = world();
    w.add(ID_2, "make the board faster");
    let reason = refused(w.send(Kind::IdeaCancel, json!({ "idea": "I-0001", "reason": " " })));
    assert!(reason.contains("reason"), "{reason}");
    assert_eq!(w.row("I-0001").state, "captured");
    let result = applied(w.send_as(
        "8b3e4c50-2d1f-4e77-a091-3c4d5e6f7081",
        Kind::IdeaCancel,
        json!({ "idea": "I-0001", "reason": "not needed" }),
    ));
    assert_eq!(w.row("I-0001").state, "cancelled");
    assert_eq!(result["cancelled"], true);
    assert_eq!(result["stop"], json!([]));
}

#[test]
fn a_payload_with_an_unknown_field_is_refused() {
    let mut w = world();
    w.add(ID_2, "make the board faster");
    let reason = refused(w.send(
        Kind::IdeaPark,
        json!({ "idea": "I-0001", "reason": "later", "force": true }),
    ));
    assert!(reason.contains("idea_park"), "{reason}");
    assert_eq!(w.row("I-0001").state, "captured");
}
