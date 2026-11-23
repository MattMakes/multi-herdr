use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::ledger::Entity;
use horch_core::fleetd::ideas::adapters::ideas_store::{self as ideas, NewIdea};
use horch_core::fleetd::ideas::application::needs_you::{self, NeedsYouList};
use horch_core::fleetd::integration::adapters::git::CliGitGateway;
use horch_core::fleetd::integration::adapters::sqlite as rows;
use horch_core::fleetd::integration::application::new_features::{
    self as nf, feature_name_key, NameSource, Named, Project, FEATURE_NAME_KIND,
};
use horch_core::fleetd::integration::application::ports::IntegrationGitGateway;
use horch_core::fleetd::worktree_custody::adapters::git::CliWorktreeGit;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs;
use horch_core::taskledger::Store;
use horch_marketplace::git::scrub_repo_env;
use rusqlite::{Connection, TransactionBehavior};

const CLOCK: FleetSeconds = FleetSeconds(1_000);

struct World {
    _dir: tempfile::TempDir,
    root: PathBuf,
    repo: PathBuf,
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
        root,
        repo,
        conn,
        config: FleetConfig::default(),
    }
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

impl World {
    fn ctx(&self) -> JobCtx<'_> {
        JobCtx {
            guard: None,
            config: &self.config,
            needs_you: &NeedsYouList,
        }
    }

    fn gateway(&self) -> CliGitGateway {
        CliGitGateway::new(
            CliWorktreeGit::new("git", self.root.join("fleet")).writing(&[self.repo.join(".git")]),
        )
    }

    fn project(&self) -> Project<'_> {
        Project {
            root: &self.repo,
            primary_branch: "main",
        }
    }

    fn main_tip(&self) -> String {
        git(&self.repo, &["rev-parse", "main"])
    }

    fn write<T>(&self, f: impl FnOnce(&Connection) -> T) -> T {
        let tx = rusqlite::Transaction::new_unchecked(&self.conn, TransactionBehavior::Immediate)
            .unwrap();
        let out = f(&tx);
        tx.commit().unwrap();
        out
    }

    fn open(&self) -> String {
        let project = self.project();
        self.write(|tx| nf::open_feature(tx, &self.ctx(), &project, cause("idea added")).unwrap())
    }

    fn name(&self, feature: &str, source: NameSource<'_>) -> Named {
        let git = self.gateway();
        let tip = self.main_tip();
        self.write(|tx| {
            nf::name_feature_branch(
                tx,
                &self.ctx(),
                &git,
                feature,
                source,
                &tip,
                CLOCK,
                cause("assumptions written"),
            )
            .unwrap()
        })
    }

    fn clarifying_idea(&self, idea: &str) -> String {
        let feature = self.open();
        self.write(|tx| {
            let new = NewIdea {
                id: idea,
                feature_id: &feature,
                priority: 0,
            };
            ideas::insert_idea(tx, None, &new, cause("idea added")).unwrap();
            transition(
                tx,
                None,
                Entity::Idea,
                idea,
                "clarifying",
                cause("phase started"),
                &NeedsYouList,
            )
            .unwrap();
        });
        feature
    }

    fn feature(&self, id: &str) -> rows::FeatureRow {
        rows::feature(&self.conn, id).unwrap().unwrap()
    }

    fn ref_tip(&self, name: &str) -> Option<String> {
        self.gateway().tip(&self.repo, name).unwrap()
    }

    fn move_feature(&self, id: &str, to: &str) {
        self.write(|tx| {
            transition(
                tx,
                None,
                Entity::Feature,
                id,
                to,
                cause("test"),
                &NeedsYouList,
            )
            .unwrap();
        });
    }

    fn events(&self, kind: &str, entity: &str, id: &str) -> Vec<String> {
        self.conn
            .prepare(
                "SELECT data_json FROM events WHERE kind = ?1 AND entity = ?2 AND entity_id = ?3
                 ORDER BY event_id",
            )
            .unwrap()
            .query_map([kind, entity, id], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }
}

fn accepted(named: Named) -> nf::BranchRef {
    match named {
        Named::Accepted { effect, .. } => effect,
        Named::Refused { reason } => panic!("refused: {reason}"),
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let mut cmd = Command::new("git");
    scrub_repo_env(&mut cmd);
    cmd.current_dir(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args);
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

#[test]
fn idea_without_to_opens_a_new_feature() {
    let w = world();
    let feature = w.clarifying_idea("I-0001");
    let row = w.feature(&feature);
    assert_eq!(row.state, "open");
    assert_eq!(row.primary_branch, "main");
    assert_eq!(row.name, None, "the sub-orchestrator gives the name later");
    assert_eq!(row.branch, None, "no branch before the name");
    let idea = ideas::idea(&w.conn, "I-0001").unwrap().unwrap();
    assert_eq!(idea.feature_id, feature);
    let run = runs::feature_run(&w.conn, &feature).unwrap().unwrap();
    let rowid: i64 = w
        .conn
        .query_row(
            "SELECT rowid FROM runs WHERE run_id = ?1",
            [&run.run_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(feature, format!("F-{rowid:04}"));
    assert_eq!(run.slug, feature);
    assert_eq!(run.status, "active");
    assert_eq!(run.base_branch.as_deref(), Some("main"));
    assert_eq!(run.run_branch, None);
    assert_eq!(Path::new(&run.project_root), w.repo.as_path());
    let created: i64 = w
        .conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE kind = 'transition' AND entity = 'feature'
               AND entity_id = ?1 AND from_state IS NULL AND to_state = 'open'",
            [&feature],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(created, 1, "the feature row is created through the 1 door");
}

#[test]
fn two_ideas_open_2_features_in_1_workspace() {
    let w = world();
    let first = w.clarifying_idea("I-0001");
    let second = w.clarifying_idea("I-0002");
    assert_ne!(first, second);
    let a = accepted(w.name(
        &first,
        NameSource::SubOrchestrator {
            idea: "I-0001",
            name: "retry-sync",
        },
    ));
    let b = accepted(w.name(
        &second,
        NameSource::SubOrchestrator {
            idea: "I-0002",
            name: "dark-mode",
        },
    ));
    nf::create_ref(&w.gateway(), &a).unwrap();
    nf::create_ref(&w.gateway(), &b).unwrap();
    for (feature, branch) in [(&first, "fleet/retry-sync"), (&second, "fleet/dark-mode")] {
        let row = w.feature(feature);
        assert_eq!(row.state, "open");
        assert_eq!(row.branch.as_deref(), Some(branch));
        let run = runs::feature_run(&w.conn, feature).unwrap().unwrap();
        assert_eq!(run.status, "active", "2 runs are active in 1 workspace");
        assert_eq!(run.run_branch.as_deref(), Some(branch));
        assert_eq!(
            w.ref_tip(&format!("refs/heads/{branch}")),
            Some(w.main_tip())
        );
    }
}

#[test]
fn feature_row_and_branch_row_exist_before_the_ref() {
    let w = world();
    let feature = w.clarifying_idea("I-0001");
    let effect = accepted(w.name(
        &feature,
        NameSource::SubOrchestrator {
            idea: "I-0001",
            name: "retry-sync",
        },
    ));
    let row = w.feature(&feature);
    assert_eq!(row.name.as_deref(), Some("retry-sync"));
    assert_eq!(row.branch.as_deref(), Some("fleet/retry-sync"));
    let branch = rows::branch(&w.conn, "fleet/retry-sync").unwrap().unwrap();
    assert_eq!(branch.feature_id, feature);
    assert_eq!(branch.task_id, None);
    assert_eq!(branch.landing_state, None);
    assert_eq!(branch.owner, "fleetd");
    assert_eq!(branch.seen_tip, Some(w.main_tip()));
    assert_eq!(w.ref_tip("refs/heads/fleet/retry-sync"), None);
    assert_eq!(effect.repo, w.repo);
    assert_eq!(effect.name, "refs/heads/fleet/retry-sync");
    assert_eq!(effect.tip, w.main_tip());
    assert!(nf::create_ref(&w.gateway(), &effect).unwrap());
    assert_eq!(w.ref_tip("refs/heads/fleet/retry-sync"), Some(w.main_tip()));
}

#[test]
fn crash_after_the_commit_creates_the_ref_once() {
    let w = world();
    let feature = w.open();
    drop(w.name(
        &feature,
        NameSource::Direct {
            title: "Retry sync",
        },
    ));
    assert_eq!(w.ref_tip("refs/heads/fleet/retry-sync"), None);
    let pending = nf::pending_refs(&w.conn, &w.gateway()).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].name, "refs/heads/fleet/retry-sync");
    assert_eq!(pending[0].tip, w.main_tip());
    assert!(nf::create_ref(&w.gateway(), &pending[0]).unwrap());
    assert_eq!(w.ref_tip("refs/heads/fleet/retry-sync"), Some(w.main_tip()));
    git(&w.repo, &["checkout", "-q", "fleet/retry-sync"]);
    std::fs::write(w.repo.join("b.txt"), "b\n").unwrap();
    git(&w.repo, &["add", "b.txt"]);
    git(&w.repo, &["commit", "-q", "-m", "landed"]);
    git(&w.repo, &["checkout", "-q", "main"]);
    let moved = git(&w.repo, &["rev-parse", "fleet/retry-sync"]);
    assert!(nf::pending_refs(&w.conn, &w.gateway()).unwrap().is_empty());
    assert!(!nf::create_ref(&w.gateway(), &pending[0]).unwrap());
    assert_eq!(w.ref_tip("refs/heads/fleet/retry-sync"), Some(moved));
}

#[test]
fn direct_name_gets_a_number_suffix_when_it_exists() {
    let w = world();
    let mut names = Vec::new();
    for _ in 0..3 {
        let feature = w.open();
        match w.name(
            &feature,
            NameSource::Direct {
                title: "Retry sync!",
            },
        ) {
            Named::Accepted { name, branch, .. } => {
                assert_eq!(branch, format!("fleet/{name}"));
                names.push(name);
            }
            Named::Refused { reason } => panic!("refused: {reason}"),
        }
    }
    assert_eq!(names, ["retry-sync", "retry-sync-2", "retry-sync-3"]);
    git(&w.repo, &["branch", "fleet/dark-mode", "main"]);
    let feature = w.open();
    let effect = accepted(w.name(&feature, NameSource::Direct { title: "Dark mode" }));
    assert_eq!(effect.name, "refs/heads/fleet/dark-mode-2");
}

#[test]
fn non_kebab_name_is_refused() {
    let w = world();
    let taken = w.clarifying_idea("I-0001");
    accepted(w.name(
        &taken,
        NameSource::SubOrchestrator {
            idea: "I-0001",
            name: "retry-sync",
        },
    ));
    let feature = w.clarifying_idea("I-0002");
    for (n, name) in ["Retry_Sync", "retry--sync", "-retry", "", "retry-sync"]
        .into_iter()
        .enumerate()
    {
        let named = w.name(
            &feature,
            NameSource::SubOrchestrator {
                idea: "I-0002",
                name,
            },
        );
        let Named::Refused { reason } = named else {
            panic!("{name:?} is accepted");
        };
        assert!(reason.contains(name), "{reason}");
        let row = w.feature(&feature);
        assert_eq!((row.name, row.branch), (None, None), "{name:?}");
        let idea = ideas::idea(&w.conn, "I-0002").unwrap().unwrap();
        assert_eq!(idea.state, "clarifying");
        let items: Vec<_> = needs_you::open_items(&w.conn)
            .unwrap()
            .into_iter()
            .filter(|i| i.kind == FEATURE_NAME_KIND)
            .collect();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].key, feature_name_key("I-0002"));
        assert_eq!(items[0].entity_id, "I-0002");
        assert_eq!(items[0].text, reason);
        let bells = w.events("doorbell", "idea", "I-0002");
        assert_eq!(bells.len(), n + 1);
        let data: serde_json::Value = serde_json::from_str(&bells[n]).unwrap();
        assert_eq!(data["owner"], format!("idea-lead:{feature}"));
        assert_eq!(data["text"], reason.as_str());
    }
    accepted(w.name(
        &feature,
        NameSource::SubOrchestrator {
            idea: "I-0002",
            name: "retry-sync-v2",
        },
    ));
    assert!(needs_you::open_items(&w.conn)
        .unwrap()
        .iter()
        .all(|i| i.kind != FEATURE_NAME_KIND));
}

#[test]
fn to_a_merged_feature_is_refused() {
    let w = world();
    let merged = w.open();
    w.move_feature(&merged, "merged");
    let abandoned = w.open();
    w.move_feature(&abandoned, "abandoned");
    for (feature, state) in [(&merged, "merged"), (&abandoned, "abandoned")] {
        let tx =
            rusqlite::Transaction::new_unchecked(&w.conn, TransactionBehavior::Immediate).unwrap();
        let err = nf::join_feature(&tx, &w.ctx(), feature, cause("idea added")).unwrap_err();
        drop(tx);
        let text = err.to_string();
        assert!(text.contains(feature.as_str()), "{text}");
        assert!(text.contains(state), "{text}");
        assert_eq!(w.feature(feature).state, state);
    }
    let tx = rusqlite::Transaction::new_unchecked(&w.conn, TransactionBehavior::Immediate).unwrap();
    let err = nf::join_feature(&tx, &w.ctx(), "F-9999", cause("idea added")).unwrap_err();
    assert!(err.to_string().contains("F-9999"), "{err}");
}

#[test]
fn to_a_ready_feature_moves_it_to_open() {
    let w = world();
    let feature = w.open();
    accepted(w.name(
        &feature,
        NameSource::Direct {
            title: "Retry sync",
        },
    ));
    w.move_feature(&feature, "ready");
    for to in [feature.as_str(), "retry-sync"] {
        let joined = w.write(|tx| nf::join_feature(tx, &w.ctx(), to, cause("idea added")).unwrap());
        assert_eq!(joined, feature);
        assert_eq!(w.feature(&feature).state, "open");
    }
    let moves: i64 = w
        .conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE entity = 'feature' AND entity_id = ?1
               AND from_state = 'ready' AND to_state = 'open'",
            [&feature],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(moves, 1, "an open feature stays open");
}
