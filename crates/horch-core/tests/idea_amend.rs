//! horch idea amend (ticket 19 of fleetd-p6): before approval the
//! amendment joins the interview, after approval it is a revise with its
//! own approval, and after merge it is a new idea with `depends_on`
//! (docs/specs/fleetd/design.md, Ideas, "Amendments"; MAP.md, Q9).
//!
//! Each test makes its own temporary ledger, git repository and idea
//! folders. No test touches this repository or the live ledger, and no test
//! starts a process other than git.
//!
//! Unix only: the protected file reader refuses each file on Windows, where
//! the platform gives no mode.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::digest::sha256_bytes;
use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::ideas::adapters::idea_folder_files::IdeaFolder;
use horch_core::fleetd::ideas::adapters::ideas_store::{self as ideas, IdeaRow};
use horch_core::fleetd::ideas::application::amend::{amend_idea, Amended};
use horch_core::fleetd::ideas::application::approval::{approve, Approved};
use horch_core::fleetd::ideas::application::intake::{
    add_idea, pending_idea_files, write_idea_file, Target,
};
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::integration::application::new_features::Project;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::Store;
use horch_marketplace::git::scrub_repo_env;
use horch_platform::channel::User;
use rusqlite::{Connection, TransactionBehavior};

const ASSUMPTIONS: &str = "# I-0001 assumptions
feature: amend-flow

1. [confirmed] A retry waits 1 s, then 2 s, then 4 s. (Q1)
";

struct World {
    _dir: tempfile::TempDir,
    repo: PathBuf,
    ideas: PathBuf,
    conn: Connection,
    config: FleetConfig,
}

fn git(dir: &Path, args: &[&str]) {
    let mut cmd = Command::new("git");
    scrub_repo_env(&mut cmd);
    let out = cmd
        .current_dir(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
}

/// 1 idea, added with `horch idea add` and put in `state`.
fn world(state: &str) -> (World, String) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let repo = root.join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    git(&repo, &["add", "a.txt"]);
    git(&repo, &["commit", "-q", "-m", "first"]);
    let db = root.join("ledger.sqlite3");
    drop(Store::open(&db).unwrap());
    let w = World {
        _dir: dir,
        repo,
        ideas: root.join("ideas"),
        conn: Connection::open(&db).unwrap(),
        config: FleetConfig::default(),
    };
    let idea = w.add("Retry sync with backoff");
    w.put(&idea, state);
    (w, idea)
}

impl World {
    fn ctx(&self) -> JobCtx<'_> {
        JobCtx {
            guard: None,
            config: &self.config,
            needs_you: &NeedsYouList,
        }
    }

    fn project(&self) -> Project<'_> {
        Project {
            root: &self.repo,
            primary_branch: "main",
        }
    }

    fn folder(&self, idea: &str) -> IdeaFolder {
        IdeaFolder::new(&self.ideas, idea, User::current().unwrap()).unwrap()
    }

    fn add(&self, text: &str) -> String {
        let tx = rusqlite::Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)
            .unwrap();
        let cause = Cause {
            actor: Actor::Operator,
            text: "horch idea add",
        };
        let target = Target::New(self.project());
        let added = add_idea(&tx, &self.ctx(), text, "horch idea add", target, cause).unwrap();
        tx.commit().unwrap();
        write_idea_file(&self.ideas, User::current().unwrap(), &added.file).unwrap();
        added.file.id
    }

    /// Put `idea` in `state` with no event: the set-up of a test, not a
    /// move.
    fn put(&self, idea: &str, state: &str) {
        let sql = "UPDATE ideas SET state = ?2 WHERE id = ?1";
        assert_eq!(self.conn.execute(sql, [idea, state]).unwrap(), 1);
    }

    fn row(&self, idea: &str) -> IdeaRow {
        ideas::idea(&self.conn, idea).unwrap().unwrap()
    }

    /// `AmendIdea` in 1 committed write transaction, then its effect after
    /// the commit, as fleetd does.
    fn amend(&self, idea: &str, text: &str) -> Amended {
        let folder = self.folder(idea);
        let tx = rusqlite::Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)
            .unwrap();
        let amended = amend_idea(&tx, &self.ctx(), &folder, idea, text, self.project()).unwrap();
        tx.commit().unwrap();
        match &amended {
            Amended::Amendment(a) => a.write(&folder).unwrap(),
            Amended::NewIdea(added) => {
                write_idea_file(&self.ideas, User::current().unwrap(), &added.file).unwrap();
            }
            Amended::Refused(_) => {}
        }
        amended
    }

    fn approve(&self, idea: &str, seen: &str) -> Approved {
        let folder = self.folder(idea);
        let tx = rusqlite::Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)
            .unwrap();
        let approved = approve(&tx, None, &folder, idea, seen).unwrap();
        tx.commit().unwrap();
        if let Approved::Approved(file) = &approved {
            file.write(&folder).unwrap();
        }
        approved
    }

    fn idea_count(&self) -> i64 {
        self.conn
            .query_row("SELECT count(*) FROM ideas", [], |r| r.get(0))
            .unwrap()
    }
}

#[test]
fn amend_before_approval_adds_a_file() {
    for state in ["captured", "clarifying"] {
        let (w, idea) = world(state);
        let amended = w.amend(&idea, "Also cap the delay at 30 s");
        assert!(matches!(amended, Amended::Amendment(_)), "{state}");
        // The sub-orchestrator reads it in its phase: the idea stays.
        assert_eq!(w.row(&idea).state, state);
        assert_eq!(
            w.folder(&idea).read_amendments().unwrap(),
            vec!["Also cap the delay at 30 s".to_string()]
        );
        let second = w.amend(&idea, "Log each retry");
        assert!(matches!(second, Amended::Amendment(_)));
        assert_eq!(w.folder(&idea).read_amendments().unwrap().len(), 2);
        assert_eq!(w.idea_count(), 1, "no new idea");
    }
}

#[test]
fn amend_after_approval_needs_a_new_approval() {
    for state in ["specifying", "ticketing", "in_progress", "integrating"] {
        let (w, idea) = world(state);
        let amended = w.amend(&idea, "Also cap the delay at 30 s");
        assert!(matches!(amended, Amended::Amendment(_)), "{state}");
        assert_eq!(w.row(&idea).state, "awaiting_approval", "{state}");
        assert_eq!(
            w.folder(&idea).read_amendments().unwrap(),
            vec!["Also cap the delay at 30 s".to_string()]
        );
        assert_eq!(w.idea_count(), 1, "no new idea");
        // The change has its own approval: only your approve moves it on.
        w.folder(&idea).write_assumptions(ASSUMPTIONS).unwrap();
        let seen = sha256_bytes(ASSUMPTIONS.as_bytes()).hex();
        assert!(matches!(w.approve(&idea, &seen), Approved::Approved(_)));
        assert_eq!(w.row(&idea).state, "specifying");
    }
}

#[test]
fn amend_after_merge_makes_a_new_idea_with_depends_on() {
    for state in ["merged", "cancelled"] {
        let (w, idea) = world(state);
        let Amended::NewIdea(added) = w.amend(&idea, "Also cap the delay at 30 s") else {
            panic!("{state}: no new idea");
        };
        assert_ne!(added.file.id, idea);
        assert_eq!(added.file.depends_on, vec![idea.clone()]);
        assert_eq!(w.row(&idea).state, state, "the old idea stays");
        assert_eq!(w.row(&added.file.id).state, "captured");
        let file = w.folder(&added.file.id).read_idea().unwrap();
        assert_eq!(file.depends_on, vec![idea.clone()]);
        assert_eq!(file.text, "Also cap the delay at 30 s");
        assert!(w.folder(&idea).read_amendments().unwrap().is_empty());
    }
}

#[test]
fn a_crash_after_the_commit_rewrites_the_new_idea_with_its_depends_on() {
    let (w, idea) = world("merged");
    let folder = w.folder(&idea);
    let tx = rusqlite::Transaction::new_unchecked(&w.conn, TransactionBehavior::Immediate).unwrap();
    let amended = amend_idea(&tx, &w.ctx(), &folder, &idea, "Cap at 30 s", w.project()).unwrap();
    tx.commit().unwrap();
    let Amended::NewIdea(added) = amended else {
        panic!("no new idea");
    };
    // No idea.md was written: the next pass finds the effect undone.
    let pending = pending_idea_files(&w.conn, &w.ideas).unwrap();
    assert_eq!(pending, vec![added.file]);
}

#[test]
fn amend_of_a_parked_idea_is_refused_and_writes_nothing() {
    let (w, idea) = world("parked");
    assert!(matches!(w.amend(&idea, "Cap at 30 s"), Amended::Refused(_)));
    assert_eq!(w.row(&idea).state, "parked");
    assert!(w.folder(&idea).read_amendments().unwrap().is_empty());
    assert_eq!(w.idea_count(), 1);
}

#[test]
fn amend_while_awaiting_approval_goes_back_to_clarifying() {
    let (w, idea) = world("awaiting_approval");
    assert!(matches!(
        w.amend(&idea, "Cap at 30 s"),
        Amended::Amendment(_)
    ));
    assert_eq!(w.row(&idea).state, "clarifying");
    assert_eq!(w.folder(&idea).read_amendments().unwrap().len(), 1);
}
