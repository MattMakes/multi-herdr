//! horch idea add: 1 immutable idea file and 1 captured row for each idea,
//! with no AI, and 2 ideas sent at the same time stay 2 ideas
//! (docs/specs/fleetd/design.md, "Capturing an idea needs no AI"; FR-1;
//! FM-8; docs/tickets/fleetd-p6/16-intake.md).
//!
//! Each test makes its own temporary ledger, git repository and idea
//! folder. No test touches this repository or the live ledger, and no test
//! starts a process other than git.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Barrier};

use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::ideas::adapters::idea_folder_files::IdeaFolder;
use horch_core::fleetd::ideas::adapters::ideas_store as ideas;
use horch_core::fleetd::ideas::application::intake::{
    add_idea, idea_text_from_file, pending_idea_files, write_idea_file, Added, Target,
};
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::ideas::domain::idea_folder::{FILE_LIMIT, IDEA_MD};
use horch_core::fleetd::integration::adapters::sqlite as features;
use horch_core::fleetd::integration::application::new_features::Project;
use horch_core::fsx::Created;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::Store;
use horch_marketplace::git::scrub_repo_env;
use horch_platform::channel::User;
use rusqlite::{Connection, TransactionBehavior};

struct World {
    _dir: tempfile::TempDir,
    db: PathBuf,
    repo: PathBuf,
    ideas: PathBuf,
    conn: Connection,
    config: FleetConfig,
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
    drop(Store::open(&db).unwrap());
    let conn = Connection::open(&db).unwrap();
    World {
        _dir: dir,
        db,
        repo,
        ideas: root.join("ideas"),
        conn,
        config: FleetConfig::default(),
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

fn cause() -> Cause<'static> {
    Cause {
        actor: Actor::Operator,
        text: "horch idea add",
    }
}

fn owner() -> User {
    User::current().unwrap()
}

/// `AddIdea` in 1 committed write transaction on `conn`. idea.md is not
/// written.
fn add(
    conn: &Connection,
    config: &FleetConfig,
    repo: &Path,
    text: &str,
    to: Option<&str>,
) -> Added {
    let ctx = JobCtx {
        guard: None,
        config,
        needs_you: &NeedsYouList,
    };
    let target = match to {
        Some(feature) => Target::Join(feature),
        None => Target::New(Project {
            root: repo,
            primary_branch: "main",
        }),
    };
    let tx = rusqlite::Transaction::new_unchecked(conn, TransactionBehavior::Immediate).unwrap();
    let added = add_idea(&tx, &ctx, text, "horch idea add", target, cause()).unwrap();
    tx.commit().unwrap();
    added
}

impl World {
    fn add(&self, text: &str, to: Option<&str>) -> Added {
        add(&self.conn, &self.config, &self.repo, text, to)
    }

    /// The text of idea.md of `idea`, read back through the idea folder.
    fn idea_text(&self, idea: &str) -> String {
        IdeaFolder::new(&self.ideas, idea, owner())
            .unwrap()
            .read_idea()
            .unwrap()
            .text
    }

    fn idea_count(&self) -> i64 {
        self.conn
            .query_row("SELECT count(*) FROM ideas", [], |r| r.get(0))
            .unwrap()
    }
}

#[test]
fn intake_test_two_ideas_added_back_to_back_produce_two_files_and_two_rows() {
    let w = world();
    // 2 requests at the same time, each on its own connection: the write
    // transaction gives each its own id.
    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = ["Retry sync with backoff", "Log each retry"]
        .into_iter()
        .map(|text| {
            let (db, repo, barrier) = (w.db.clone(), w.repo.clone(), barrier.clone());
            std::thread::spawn(move || {
                let conn = Connection::open(&db).unwrap();
                conn.busy_timeout(std::time::Duration::from_secs(60))
                    .unwrap();
                barrier.wait();
                add(&conn, &FleetConfig::default(), &repo, text, None)
            })
        })
        .collect();
    let added: Vec<Added> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    for a in &added {
        assert_eq!(
            write_idea_file(&w.ideas, owner(), &a.file).unwrap(),
            Created::Written
        );
    }

    let mut ids: Vec<&str> = added.iter().map(|a| a.file.id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(ids, ["I-0001", "I-0002"]);
    assert_eq!(w.idea_count(), 2);
    for a in &added {
        let row = ideas::idea(&w.conn, &a.file.id).unwrap().unwrap();
        assert_eq!(row.state, "captured");
        assert_eq!(row.feature_id, a.feature);
        assert_eq!(w.idea_text(&a.file.id), a.file.text);
    }
    assert_ne!(
        added[0].feature, added[1].feature,
        "each idea opens its own feature"
    );
    assert_ne!(added[0].file.text, added[1].file.text);
    let files = std::fs::read_dir(&w.ideas).unwrap().count();
    assert_eq!(files, 2, "1 folder for each idea");

    // A third idea after them gets the next number.
    let third = w.add("Cap the delay at 30 s", None);
    assert_eq!(third.file.id, "I-0003");
}

#[test]
fn idea_md_holds_exactly_the_text() {
    let w = world();
    let text =
        "  Retry sync → with backoff, café\n---\nnot: front matter\n\n\tlast line, no newline";
    let added = w.add(text, None);
    write_idea_file(&w.ideas, owner(), &added.file).unwrap();
    assert_eq!(w.idea_text(&added.file.id), text);
    assert_eq!(added.file.source, "horch idea add");

    // `-f <file>`: the text of the file is the idea, byte for byte.
    let notes = w.ideas.with_file_name("rough-notes.md");
    std::fs::write(&notes, "# Rough notes\n\n- retry\n- log\n").unwrap();
    let from_file = idea_text_from_file(&notes).unwrap();
    assert_eq!(from_file, "# Rough notes\n\n- retry\n- log\n");
    let added = w.add(&from_file, None);
    write_idea_file(&w.ideas, owner(), &added.file).unwrap();
    assert_eq!(w.idea_text(&added.file.id), from_file);

    // A file over 64 KiB is refused, and so is an empty idea.
    let big = w.ideas.with_file_name("big.md");
    std::fs::write(&big, "x".repeat(FILE_LIMIT as usize + 1)).unwrap();
    let err = idea_text_from_file(&big).unwrap_err().to_string();
    assert!(err.contains("64 KiB"), "{err}");
    let ctx = JobCtx {
        guard: None,
        config: &w.config,
        needs_you: &NeedsYouList,
    };
    let tx = rusqlite::Transaction::new_unchecked(&w.conn, TransactionBehavior::Immediate).unwrap();
    let project = Project {
        root: &w.repo,
        primary_branch: "main",
    };
    let err = add_idea(
        &tx,
        &ctx,
        " \n\t",
        "horch idea add",
        Target::New(project),
        cause(),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("empty"), "{err}");
    drop(tx);
    assert_eq!(w.idea_count(), 2, "a refused idea writes no row");

    // The file is immutable: writing it again with other bytes fails, and
    // the same bytes are accepted.
    let mut changed = added.file.clone();
    assert_eq!(
        write_idea_file(&w.ideas, owner(), &changed).unwrap(),
        Created::AlreadyIdentical
    );
    changed.text.push_str("more");
    assert!(write_idea_file(&w.ideas, owner(), &changed).is_err());
    assert_eq!(w.idea_text(&added.file.id), from_file);
}

#[test]
fn crash_after_the_row_writes_idea_md_on_the_next_pass() {
    let w = world();
    let text = "Retry sync with backoff\nand log each retry\n";
    // The commit happens, then the process dies before the effect.
    let added = w.add(text, None);
    let path = w.ideas.join(&added.file.id).join(IDEA_MD);
    assert!(!path.exists());

    // The next pass, in a new process: only the ledger is left.
    drop(w.conn);
    let conn = Connection::open(&w.db).unwrap();
    let pending = pending_idea_files(&conn, &w.ideas).unwrap();
    assert_eq!(pending, vec![added.file.clone()]);
    for file in &pending {
        assert_eq!(
            write_idea_file(&w.ideas, owner(), file).unwrap(),
            Created::Written
        );
    }
    let read = IdeaFolder::new(&w.ideas, &added.file.id, owner())
        .unwrap()
        .read_idea()
        .unwrap();
    assert_eq!(read, added.file);
    assert_eq!(read.text, text);

    // The pass after it finds nothing to write.
    assert!(pending_idea_files(&conn, &w.ideas).unwrap().is_empty());
}

#[test]
fn idea_with_to_joins_the_feature() {
    let w = world();
    let first = w.add("Retry sync with backoff", None);
    let second = w.add("Also cap the delay at 30 s", Some(&first.feature));
    assert_eq!(second.feature, first.feature);
    assert_ne!(second.file.id, first.file.id);
    let row = ideas::idea(&w.conn, &second.file.id).unwrap().unwrap();
    assert_eq!(
        (row.state.as_str(), row.feature_id.as_str()),
        ("captured", first.feature.as_str())
    );
    let feature_count: i64 = w
        .conn
        .query_row("SELECT count(*) FROM features", [], |r| r.get(0))
        .unwrap();
    assert_eq!(feature_count, 1, "--to opens no feature");
    assert_eq!(
        features::feature(&w.conn, &first.feature)
            .unwrap()
            .unwrap()
            .state,
        "open"
    );

    // An unknown feature is refused, and no row is written.
    let ctx = JobCtx {
        guard: None,
        config: &w.config,
        needs_you: &NeedsYouList,
    };
    let tx = rusqlite::Transaction::new_unchecked(&w.conn, TransactionBehavior::Immediate).unwrap();
    let err = add_idea(
        &tx,
        &ctx,
        "x",
        "horch idea add",
        Target::Join("F-9999"),
        cause(),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("F-9999"), "{err}");
    drop(tx);
    assert_eq!(w.idea_count(), 2);
}
