//! `horch ideas` and `horch idea show` (ticket 24 of fleetd-p6): the board
//! of each idea that is not merged and not cancelled, what it waits on, the
//! open needs-you items, and what you approve
//! (docs/specs/fleetd/design.md, "Ideas"; "Between features", "Build on
//! what is in flight"; OR-3).
//!
//! Each test makes its own ledger, fleet root and git repository in a
//! temporary folder, and runs the real binary with its own home, data and
//! state folders and the watchdog off. No test reads the clock of the
//! machine: each time that a test checks, the test writes.

use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::ideas::adapters::ideas_store::{self, NewIdea};
use horch_core::fleetd::ideas::application::needs_you;
use horch_core::fleetd::ideas::domain::needs_you::Write;
use horch_core::fleetd::integration::adapters::sqlite::{self as rows, NewFeature};
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::Store;

const WAITING_SINCE: &str = "2026-11-01T10:00:00Z";
const ASKED_AT: &str = "2026-11-02T11:00:00Z";

struct World {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    store: Store,
}

struct Out {
    code: i32,
    out: String,
    err: String,
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

/// A ledger with the features `F-0001` (`f1`) and `F-0002` (`f2`) of the
/// primary branch `main`.
fn world() -> World {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    std::fs::create_dir_all(root.join("home")).unwrap();
    let mut store = Store::open(&root.join("ledger.sqlite3")).unwrap();
    for (id, name) in [("F-0001", "f1"), ("F-0002", "f2")] {
        let branch = format!("fleet/{name}");
        let new = NewFeature {
            id,
            name: Some(name),
            branch: Some(&branch),
            primary_branch: "main",
        };
        store
            .immediate(|tx| rows::insert_feature(tx, None, &new, cause("feature accepted")))
            .unwrap();
    }
    World {
        _tmp: tmp,
        root,
        store,
    }
}

impl World {
    /// The idea `id` of `feature`, put in `state` with no event: the set-up
    /// of a test, not a move.
    fn idea(&mut self, id: &str, feature: &str, state: &str) {
        let new = NewIdea {
            id,
            feature_id: feature,
            priority: 0,
        };
        self.store
            .immediate(|tx| ideas_store::insert_idea(tx, None, &new, cause("idea added")))
            .unwrap();
        let sql = "UPDATE ideas SET state = ?2 WHERE id = ?1";
        assert_eq!(self.store.conn().execute(sql, [id, state]).unwrap(), 1);
    }

    fn waiting_on_you(&self, id: &str, since: &str) {
        let sql = "UPDATE ideas SET waiting_on_you_since = ?2 WHERE id = ?1";
        assert_eq!(self.store.conn().execute(sql, [id, since]).unwrap(), 1);
    }

    /// An open question of `feature`. The question names a task that the
    /// test does not make, so the insert runs with no foreign key check.
    fn question(&self, id: &str, feature: &str) {
        let conn = rusqlite::Connection::open(self.root.join("ledger.sqlite3")).unwrap();
        conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
        conn.execute(
            "INSERT INTO questions (id, task_id, feature_id, owner, state, due_at, asked_at)
             VALUES (?1, 'T-0001-01', ?2, ?3, 'open', 0, ?4)",
            [id, feature, &format!("idea-lead:{feature}"), ASKED_AT],
        )
        .unwrap();
    }

    fn needs_you(&mut self, key: &str, entity_id: &str, text: &str) {
        let w = Write {
            key,
            kind: "approval",
            entity: "idea",
            entity_id,
            text,
        };
        self.store
            .immediate(|tx| needs_you::open_item(tx, &w))
            .unwrap();
    }

    fn resolve(&mut self, key: &str) {
        self.store
            .immediate(|tx| needs_you::resolve(tx, key))
            .unwrap();
    }

    fn idea_dir(&self, id: &str) -> PathBuf {
        let dir = self.root.join("fleet").join("ideas").join(id);
        std::fs::create_dir_all(dir.join("amendments")).unwrap();
        dir
    }

    fn horch(&self, args: &[&str]) -> Out {
        let out = Command::new(env!("CARGO_BIN_EXE_horch"))
            .args(args)
            .current_dir(&self.root)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.root.join("home"))
            .env("XDG_DATA_HOME", self.root.join("data"))
            .env("HORCH_DATA_DIR", self.root.join("data"))
            .env("HORCH_STATE_DIR", self.root.join("state"))
            .env("HORCH_LEDGER_DB", self.root.join("ledger.sqlite3"))
            .env("HORCH_PROJECT_DIR", self.root.join("repo"))
            .env("FLEET_ROOT", self.root.join("fleet"))
            .env("HORCH_NO_WATCHDOG", "1")
            .output()
            .unwrap();
        let o = Out {
            code: out.status.code().unwrap_or(-1),
            out: String::from_utf8_lossy(&out.stdout).into_owned(),
            err: String::from_utf8_lossy(&out.stderr).into_owned(),
        };
        assert_eq!(
            o.code, 0,
            "{args:?}\nstdout:\n{}\nstderr:\n{}",
            o.out, o.err
        );
        o
    }
}

/// The 1 line of the board that names `id`.
fn line_of<'a>(board: &'a str, id: &str) -> &'a str {
    let lines: Vec<&str> = board.lines().filter(|l| l.contains(id)).collect();
    assert_eq!(lines.len(), 1, "1 line names {id}:\n{board}");
    lines[0]
}

#[test]
fn ideas_board_lists_each_idea_and_what_it_waits_on() {
    let mut w = world();
    w.idea("I-0001", "F-0001", "awaiting_approval");
    w.waiting_on_you("I-0001", WAITING_SINCE);
    w.idea("I-0002", "F-0002", "clarifying");
    w.question("Q-0007", "F-0002");
    w.idea("I-0003", "F-0001", "in_progress");
    w.idea("I-0004", "F-0001", "integrating");
    w.idea("I-0005", "F-0001", "merged");
    w.idea("I-0006", "F-0001", "cancelled");

    let board = w.horch(&["ideas"]).out;
    let you = line_of(&board, "I-0001");
    for word in [
        "f1",
        "idea awaiting_approval",
        "waits on you",
        WAITING_SINCE,
    ] {
        assert!(you.contains(word), "no {word:?} in {you:?}");
    }
    let question = line_of(&board, "I-0002");
    for word in [
        "f2",
        "idea clarifying",
        "waits on question Q-0007",
        ASKED_AT,
    ] {
        assert!(question.contains(word), "no {word:?} in {question:?}");
    }
    assert!(line_of(&board, "I-0003").contains("waits on task"));
    assert!(line_of(&board, "I-0004").contains("waits on gate"));
    assert!(
        !board.contains("I-0005"),
        "a merged idea is shown:\n{board}"
    );
    assert!(
        !board.contains("I-0006"),
        "a cancelled idea is shown:\n{board}"
    );

    let json: serde_json::Value = serde_json::from_str(&w.horch(&["ideas", "--json"]).out).unwrap();
    let ideas = json["ideas"].as_array().unwrap();
    assert_eq!(ideas.len(), 4, "{json}");
    assert_eq!(ideas[0]["id"], "I-0001");
    assert_eq!(ideas[0]["waits_on"], "you");
    assert_eq!(ideas[0]["since"], WAITING_SINCE);
    assert_eq!(ideas[1]["waits_on"], "question Q-0007");
}

#[test]
fn ideas_board_shows_only_open_needs_you_items() {
    let mut w = world();
    w.idea("I-0001", "F-0001", "awaiting_approval");
    w.idea("I-0002", "F-0001", "awaiting_approval");
    w.needs_you(
        "approval:I-0001",
        "I-0001",
        "approve the assumptions of I-0001",
    );
    w.needs_you(
        "approval:I-0002",
        "I-0002",
        "approve the assumptions of I-0002",
    );
    w.resolve("approval:I-0002");

    let board = w.horch(&["ideas"]).out;
    let (_, needs_you) = board
        .split_once("Needs you")
        .unwrap_or_else(|| panic!("no Needs you section:\n{board}"));
    assert!(
        needs_you.contains("approve the assumptions of I-0001"),
        "{board}"
    );
    assert!(needs_you.contains("approval:I-0001"), "{board}");
    assert!(
        !board.contains("approve the assumptions of I-0002"),
        "a resolved item is shown:\n{board}"
    );

    let json: serde_json::Value = serde_json::from_str(&w.horch(&["ideas", "--json"]).out).unwrap();
    let items = json["needs_you"].as_array().unwrap();
    assert_eq!(items.len(), 1, "{json}");
    assert_eq!(items[0]["key"], "approval:I-0001");
}

#[test]
fn idea_show_prints_the_assumptions_and_their_sha256() {
    let mut w = world();
    w.idea("I-0001", "F-0001", "awaiting_approval");
    let dir = w.idea_dir("I-0001");
    std::fs::write(dir.join("idea.md"), "Add a dark mode to the board.\n").unwrap();
    std::fs::write(
        dir.join("amendments").join("01.md"),
        "Keep the light mode as the default.\n",
    )
    .unwrap();
    let assumptions = "# Assumptions\n\nfeature: dark-mode\n\n1. [assumed] The board only.\n";
    std::fs::write(dir.join("assumptions.md"), assumptions).unwrap();
    std::fs::write(dir.join("plan.verdict.json"), "{\"verdict\": \"pass\"}\n").unwrap();
    let sha = horch_core::digest::sha256_bytes(assumptions.as_bytes()).hex();

    let out = w.horch(&["idea", "show", "I-0001"]).out;
    for word in [
        "I-0001",
        "idea awaiting_approval",
        "Add a dark mode to the board.",
        "Keep the light mode as the default.",
        "1. [assumed] The board only.",
        sha.as_str(),
        "\"verdict\": \"pass\"",
    ] {
        assert!(out.contains(word), "no {word:?} in:\n{out}");
    }

    let json: serde_json::Value =
        serde_json::from_str(&w.horch(&["idea", "show", "I-0001", "--json"]).out).unwrap();
    assert_eq!(json["state"], "awaiting_approval");
    assert_eq!(json["assumptions"]["sha256"], sha.as_str());
    assert_eq!(json["assumptions"]["text"], assumptions);
    assert_eq!(json["amendments"][0]["name"], "01.md");
}

/// plan.yaml names the feature id, so `horch idea show` prints it, in text
/// and in JSON, also when the feature has a name.
#[test]
fn idea_show_prints_the_feature_id() {
    let mut w = world();
    w.idea("I-0001", "F-0001", "ticketing");

    let out = w.horch(&["idea", "show", "I-0001"]).out;
    assert!(
        out.contains("Feature id: F-0001\n"),
        "no feature id in:\n{out}"
    );

    let json: serde_json::Value =
        serde_json::from_str(&w.horch(&["idea", "show", "I-0001", "--json"]).out).unwrap();
    assert_eq!(json["feature_id"], "F-0001", "{json}");
}

fn git(repo: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", repo)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A repository with `main` and the branches `fleet/f1` and `fleet/f2`,
/// each with 1 commit that adds 1 file.
fn repo(root: &Path) {
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.email", "t@example.com"]);
    git(&repo, &["config", "user.name", "t"]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    git(&repo, &["add", "a.txt"]);
    git(&repo, &["commit", "-q", "-m", "a"]);
    for (branch, file, text) in [
        ("fleet/f1", "own.txt", "hello from f1\n"),
        ("fleet/f2", "other.txt", "hello from f2\n"),
    ] {
        git(&repo, &["checkout", "-q", "-b", branch, "main"]);
        std::fs::write(repo.join(file), text).unwrap();
        git(&repo, &["add", file]);
        git(&repo, &["commit", "-q", "-m", file]);
    }
    git(&repo, &["checkout", "-q", "main"]);
}

#[test]
fn idea_show_in_flight_lists_the_changed_files_of_other_features() {
    let mut w = world();
    repo(&w.root);
    w.idea("I-0001", "F-0001", "ticketing");

    let out = w.horch(&["idea", "show", "I-0001", "--in-flight"]).out;
    let (_, in_flight) = out
        .split_once("In flight")
        .unwrap_or_else(|| panic!("no In flight section:\n{out}"));
    for word in ["F-0002", "fleet/f2", "other.txt", "+hello from f2"] {
        assert!(in_flight.contains(word), "no {word:?} in:\n{out}");
    }
    assert!(
        !in_flight.contains("own.txt"),
        "the own feature is in flight:\n{out}"
    );

    let json: serde_json::Value = serde_json::from_str(
        &w.horch(&["idea", "show", "I-0001", "--in-flight", "--json"])
            .out,
    )
    .unwrap();
    let features = json["in_flight"].as_array().unwrap();
    assert_eq!(features.len(), 1, "{json}");
    assert_eq!(features[0]["feature"], "F-0002");
    assert_eq!(features[0]["files"][0], "other.txt");
}
