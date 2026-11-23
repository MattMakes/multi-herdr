use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::digest::sha256_bytes;
use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::fleet_runtime::application::ledger::Cause;
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::ideas::adapters::idea_folder_files::IdeaFolder;
use horch_core::fleetd::ideas::adapters::ideas_store::{self, IdeaRow};
use horch_core::fleetd::ideas::application::approval::record_approval;
use horch_core::fleetd::ideas::application::direct::{
    add_direct_idea, plan_refused_key, start_direct, Direct, DirectAdded, Started,
    PLAN_REFUSED_KIND,
};
use horch_core::fleetd::ideas::application::intake::Target;
use horch_core::fleetd::ideas::application::needs_you::{open_items, NeedsYouList};
use horch_core::fleetd::ideas::application::plan_commit::{PlanCommit, Verdict, PLAN_YAML};
use horch_core::fleetd::ideas::domain::approval::Approval;
use horch_core::fleetd::ideas::domain::plan_validator::{DeclaredGates, RosterProfile};
use horch_core::fleetd::integration::adapters::git::CliGitGateway;
use horch_core::fleetd::integration::adapters::sqlite as features;
use horch_core::fleetd::integration::application::new_features::Project;
use horch_core::fleetd::task_execution::application::dispatch::{
    bind, write_task_files, DispatchCtx,
};
use horch_core::fleetd::worker_profiles::application::health;
use horch_core::fleetd::worktree_custody::adapters::git::CliWorktreeGit;
use horch_core::taskledger::git::Git;
use horch_core::taskledger::model::{Actor, FileMode};
use horch_core::taskledger::tasks;
use horch_core::taskledger::workers::NewWorker;
use horch_core::taskledger::Store;
use horch_marketplace::git::scrub_repo_env;
use horch_platform::channel::User;
use serde_json::Value;

const CLOCK: FleetSeconds = FleetSeconds(1_000);

const FEATURE_TICKET: &str = "# 03: Retry sync with backoff

<!-- horch-ticket: 1 -->
**Persona:** primary `coder-claude`
**Kind:** feature
**Check:** `cargo test`
**Own:** `src/a.rs`

## Goal

Retry with backoff.
";

const DOCS_TICKET: &str = "# 04: Document the retries

<!-- horch-ticket: 1 -->
**Persona:** primary `coder-claude`
**Kind:** docs
**Check:** `test -f docs/sync.md`
**Own:** `docs/sync.md`

## Goal

Write docs/sync.md.
";

const NO_CHECK_TICKET: &str = "# 05: Tidy the sync log

<!-- horch-ticket: 1 -->
**Persona:** primary `coder-claude`
**Kind:** feature
**Check:** none
**Own:** `src/log.rs`

## Goal

Tidy the log.
";

struct World {
    _dir: tempfile::TempDir,
    root: PathBuf,
    repo: PathBuf,
    store: Store,
    config: FleetConfig,
    roster: Vec<RosterProfile>,
    gates: DeclaredGates,
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
    let store = Store::open(&root.join("ledger.sqlite3")).unwrap();
    World {
        _dir: dir,
        root,
        repo,
        store,
        config: FleetConfig::default(),
        roster: vec![
            profile("coder-claude", "claude", "claude-opus-5-5", "coder-codex"),
            profile("coder-codex", "codex", "gpt-5", "coder-claude"),
        ],
        gates: DeclaredGates {
            fast: Some("cargo test".into()),
            full: Some("cargo test --workspace".into()),
        },
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

fn profile(name: &str, provider: &str, model: &str, fallback: &str) -> RosterProfile {
    RosterProfile {
        name: name.into(),
        provider: provider.into(),
        model: model.into(),
        effort: Some("medium".into()),
        fallback: Some(fallback.into()),
        sandbox_here: true,
    }
}

fn cause() -> Cause<'static> {
    Cause {
        actor: Actor::Operator,
        text: "horch idea add --direct",
    }
}

fn owner() -> User {
    User::current().unwrap()
}

fn sha256_hex(bytes: &[u8]) -> String {
    sha256_bytes(bytes).hex()
}

impl World {
    fn ideas(&self) -> PathBuf {
        self.root.join("ideas")
    }

    fn gateway(&self) -> CliGitGateway {
        CliGitGateway::new(
            CliWorktreeGit::new("git", self.root.join("fleet")).writing(&[self.repo.join(".git")]),
        )
    }

    fn add(&mut self, ticket: &str, to: Option<&str>) -> DirectAdded {
        let gateway = self.gateway();
        let tip = git(&self.repo, &["rev-parse", "main"]);
        let repo = self.repo.clone();
        let config = self.config.clone();
        let roster = self.roster.clone();
        let ctx = JobCtx {
            guard: None,
            config: &config,
            needs_you: &NeedsYouList,
        };
        let direct = Direct {
            roster: &roster,
            git: &gateway,
            primary_tip: &tip,
            clock: CLOCK,
        };
        let target = match to {
            Some(feature) => Target::Join(feature),
            None => Target::New(Project {
                root: &repo,
                primary_branch: "main",
            }),
        };
        let added = self
            .store
            .immediate(|tx| add_direct_idea(tx, &ctx, &direct, ticket, target, cause()))
            .unwrap();
        added.write(&self.ideas(), owner(), &gateway).unwrap();
        added
    }

    fn start(&mut self, idea: &str) -> Started {
        let ideas = self.ideas();
        let owner = owner();
        let ctx = PlanCommit {
            ideas: &ideas,
            owner: &owner,
            roster: &self.roster,
            gates: &self.gates,
            guard: None,
            refusals: &NeedsYouList,
        };
        start_direct(&mut self.store, &ctx, idea).unwrap()
    }

    fn idea(&self, id: &str) -> IdeaRow {
        ideas_store::idea(self.store.conn(), id).unwrap().unwrap()
    }

    fn folder(&self, idea: &str) -> PathBuf {
        self.ideas().join(idea)
    }

    fn tasks(&self, idea: &str) -> Vec<(String, String, Option<String>, Value)> {
        let mut stmt = self
            .store
            .conn()
            .prepare(
                "SELECT task_id, kind, primary_persona, parent_task_id, spec_json
                 FROM tasks WHERE idea_id = ?1 ORDER BY rowid",
            )
            .unwrap();
        stmt.query_map([idea], |r| {
            Ok((
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                serde_json::from_str(&r.get::<_, String>(4)?).unwrap(),
            ))
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
    }

    fn task_id(&self, idea: &str, key: &str) -> String {
        self.store
            .conn()
            .query_row(
                "SELECT task_id FROM tasks
                 WHERE idea_id = ?1 AND json_extract(spec_json, '$.key') = ?2",
                [idea, key],
                |r| r.get(0),
            )
            .unwrap()
    }
}

#[test]
fn intake_test_direct_idea_produces_1_validated_plan_with_1_work_task() {
    let mut w = world();
    let added = w.add(FEATURE_TICKET, None);
    let idea = added.idea.file.id.clone();
    assert_eq!(w.idea(&idea).state, "captured");

    let feature = features::feature(w.store.conn(), &added.idea.feature)
        .unwrap()
        .unwrap();
    assert_eq!(feature.name.as_deref(), Some("retry-sync-with-backoff"));
    assert_eq!(
        feature.branch.as_deref(),
        Some("fleet/retry-sync-with-backoff")
    );
    assert!(!git(
        &w.repo,
        &[
            "rev-parse",
            "--verify",
            "refs/heads/fleet/retry-sync-with-backoff"
        ]
    )
    .is_empty());

    let copied = "tickets/01-retry-sync-with-backoff.md";
    assert_eq!(added.ticket, copied);
    let text = std::fs::read_to_string(w.folder(&idea).join(copied)).unwrap();
    assert_eq!(text, FEATURE_TICKET);
    let sha = sha256_hex(FEATURE_TICKET.as_bytes());
    let approval: Value = serde_json::from_str(
        &std::fs::read_to_string(w.folder(&idea).join("approval.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(approval["file"], copied);
    assert_eq!(approval["sha256"], sha.as_str());
    assert_eq!(w.idea(&idea).approved_sha256.as_deref(), Some(sha.as_str()));
    assert!(w.folder(&idea).join(PLAN_YAML).is_file());

    let started = w.start(&idea);
    let Started::Committed(Verdict::Committed { tasks, .. }) = started else {
        panic!("the plan is not committed: {started:?}");
    };
    assert_eq!(tasks.len(), 1, "{tasks:?}");
    assert_eq!(w.idea(&idea).state, "in_progress");
    let rows = w.tasks(&idea);
    assert_eq!(rows.len(), 1);
    let (_, persona, parent, _) = &rows[0];
    assert_eq!(persona, "coder-claude");
    assert_eq!(parent, &None);
    let land_check: Option<String> = w
        .store
        .conn()
        .query_row(
            "SELECT land_check FROM tasks WHERE idea_id = ?1",
            [&idea],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(land_check.as_deref(), Some("cargo test"));
    let covers: i64 = w
        .store
        .conn()
        .query_row(
            "SELECT count(*) FROM task_covers WHERE idea_id = ?1 AND req_id = 'R1'",
            [&idea],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(covers, 1);
}

#[test]
fn direct_idea_of_a_judgment_kind_gets_its_judge_task() {
    let mut w = world();
    let added = w.add(DOCS_TICKET, None);
    let idea = added.idea.file.id.clone();
    let started = w.start(&idea);
    let Started::Committed(Verdict::Committed { tasks, .. }) = started else {
        panic!("the plan is not committed: {started:?}");
    };
    assert_eq!(tasks.len(), 2, "{tasks:?}");
    let rows = w.tasks(&idea);
    let (_, work_profile, _, work_spec) = &rows[0];
    let (_, judge_profile, judge_parent, judge_spec) = &rows[1];
    assert_eq!(work_profile, "coder-claude");
    assert_eq!(work_spec["kind"], "docs");
    assert_eq!(judge_profile, "coder-codex");
    assert_eq!(judge_spec["kind"], "judge");
    let work_key = work_spec["key"].as_str().unwrap();
    assert_eq!(judge_spec["judges"], work_key);
    assert_eq!(
        judge_parent.as_deref(),
        Some(w.task_id(&idea, work_key).as_str())
    );
}

#[test]
fn direct_plan_without_check_is_refused_and_goes_to_needs_you() {
    let mut w = world();
    let added = w.add(NO_CHECK_TICKET, None);
    let idea = added.idea.file.id.clone();
    let started = w.start(&idea);
    let Started::Refused(problems) = started else {
        panic!("the plan is not refused: {started:?}");
    };
    assert!(
        problems
            .iter()
            .any(|p| p.message.contains("acceptance check")),
        "{problems:?}"
    );
    assert_eq!(w.idea(&idea).state, "ticketing");
    assert!(w.tasks(&idea).is_empty());
    let items = open_items(w.store.conn()).unwrap();
    let item = items
        .iter()
        .find(|i| i.key == plan_refused_key(&idea))
        .unwrap_or_else(|| panic!("no needs-you item: {items:?}"));
    assert_eq!(item.kind, PLAN_REFUSED_KIND);
    assert!(item.text.contains("acceptance check"), "{}", item.text);
}

#[test]
fn direct_idea_waits_in_captured_while_another_idea_of_its_feature_is_ticketing() {
    let mut w = world();
    let first = w.add(NO_CHECK_TICKET, None);
    let first_id = first.idea.file.id.clone();
    assert!(matches!(w.start(&first_id), Started::Refused(_)));
    assert_eq!(w.idea(&first_id).state, "ticketing");

    let second = w.add(FEATURE_TICKET, Some(&first.idea.feature));
    let second_id = second.idea.file.id.clone();
    assert_eq!(second.idea.feature, first.idea.feature);
    assert_eq!(w.start(&second_id), Started::Waiting);
    assert_eq!(w.idea(&second_id).state, "captured");
    assert!(w.tasks(&second_id).is_empty());

    let other = w.add(FEATURE_TICKET, None);
    assert_ne!(other.idea.feature, first.idea.feature);
    assert!(matches!(
        w.start(&other.idea.file.id),
        Started::Committed(_)
    ));
}

#[test]
fn idea_with_approved_assumptions_and_no_requirements_yaml_is_refused() {
    let mut w = world();
    let added = w.add(FEATURE_TICKET, None);
    let idea = added.idea.file.id.clone();
    let folder = IdeaFolder::new(&w.ideas(), &idea, owner()).unwrap();
    let assumptions = "# Assumptions\n\nfeature: retry-sync-with-backoff\n1. [confirmed] Retry.\n";
    folder.write_assumptions(assumptions).unwrap();
    let sha = sha256_hex(assumptions.as_bytes());
    let file = w
        .store
        .immediate(|tx| {
            let approval = Approval::of_assumptions(&idea, &sha, "2026-11-18T00:00:00Z");
            record_approval(tx, None, &approval, &sha, cause())
        })
        .unwrap();
    file.write(&folder).unwrap();
    assert!(!w.folder(&idea).join("requirements.yaml").exists());

    let started = w.start(&idea);
    let Started::Refused(problems) = started else {
        panic!("a normal idea with no requirements.yaml is not refused: {started:?}");
    };
    assert!(
        problems
            .iter()
            .any(|p| p.message.contains("requirements.yaml")),
        "{problems:?}"
    );
    assert_eq!(w.idea(&idea).state, "ticketing");
    assert!(w.tasks(&idea).is_empty());
}

#[test]
fn committed_judge_task_has_its_verdict_file_as_own() {
    let mut w = world();
    let added = w.add(DOCS_TICKET, None);
    let idea = added.idea.file.id.clone();
    let started = w.start(&idea);
    assert!(
        matches!(started, Started::Committed(Verdict::Committed { .. })),
        "the plan is not committed: {started:?}"
    );
    let rows = w.tasks(&idea);
    let work_key = rows[0].3["key"].as_str().unwrap().to_string();
    let judge_key = rows[1].3["key"].as_str().unwrap().to_string();
    let own =
        |key: &str| tasks::files(w.store.conn(), &w.task_id(&idea, key), FileMode::Own).unwrap();
    assert_eq!(own(&judge_key), vec!["docs/sync.review.md".to_string()]);
    assert_eq!(own(&work_key), vec!["docs/sync.md".to_string()]);
    assert_eq!(
        rows[1].3["outputs"],
        serde_json::json!(["docs/sync.review.md"])
    );
}

impl World {
    fn unhealthy(&mut self, profile: &str) {
        self.store
            .immediate(|tx| {
                health::mark_unhealthy(tx, None, profile, "adapter-conformance-failed: test")
            })
            .unwrap();
    }
}

#[test]
fn judge_task_skips_an_unhealthy_profile() {
    let mut w = world();
    w.roster
        .push(profile("coder-agy", "agy", "gemini-3", "coder-claude"));
    w.unhealthy("coder-codex");
    let added = w.add(DOCS_TICKET, None);
    let idea = added.idea.file.id.clone();
    let started = w.start(&idea);
    assert!(
        matches!(started, Started::Committed(Verdict::Committed { .. })),
        "the plan is not committed: {started:?}"
    );
    let rows = w.tasks(&idea);
    assert_eq!(rows[1].1, "coder-agy");
}

#[test]
fn plan_with_no_healthy_judge_profile_is_refused_with_the_cause() {
    let mut w = world();
    w.unhealthy("coder-codex");
    let added = w.add(DOCS_TICKET, None);
    let idea = added.idea.file.id.clone();
    let started = w.start(&idea);
    let Started::Refused(problems) = started else {
        panic!("the plan is not refused: {started:?}");
    };
    let named = |text: &str| {
        text.contains("no profile of another provider")
            && text.contains("coder-codex: unhealthy: adapter-conformance-failed: test")
    };
    assert!(problems.iter().any(|p| named(&p.message)), "{problems:?}");
    assert!(w.tasks(&idea).is_empty());
    let items = open_items(w.store.conn()).unwrap();
    let item = items
        .iter()
        .find(|i| i.key == plan_refused_key(&idea))
        .unwrap_or_else(|| panic!("no needs-you item: {items:?}"));
    assert!(named(&item.text), "{}", item.text);
}

#[test]
fn judge_task_never_gets_a_council_seat() {
    let seat = |name: &str, provider: &str, model: &str, fallback: Option<&str>| RosterProfile {
        name: name.into(),
        provider: provider.into(),
        model: model.into(),
        effort: Some("high".into()),
        fallback: fallback.map(Into::into),
        sandbox_here: true,
    };
    let seats = [
        seat("fable-creative", "claude", "fable", Some("coder-claude")),
        seat(
            "astra-creative",
            "codex",
            "gpt-6-astra",
            Some("coder-codex"),
        ),
        seat("opus-creative", "claude", "opus", None),
    ];
    for s in seats {
        let mut w = world();
        let (author, judge) = if s.provider == "claude" {
            ("coder-codex", "coder-claude")
        } else {
            ("coder-claude", "coder-codex")
        };
        let name = s.name.clone();
        let at = w.roster.iter().position(|p| p.name == judge).unwrap();
        w.roster.insert(at, s);
        let ticket = DOCS_TICKET.replace("`coder-claude`", &format!("`{author}`"));
        let added = w.add(&ticket, None);
        let idea = added.idea.file.id.clone();
        let started = w.start(&idea);
        assert!(
            matches!(started, Started::Committed(Verdict::Committed { .. })),
            "{name}: the plan is not committed: {started:?}"
        );
        assert_eq!(w.tasks(&idea)[1].1, judge, "{name}");
    }
}

#[test]
fn plan_with_only_a_council_seat_to_judge_is_refused_with_the_cause() {
    let mut w = world();
    w.roster.retain(|p| p.name != "coder-codex");
    w.roster.push(RosterProfile {
        name: "astra-creative".into(),
        provider: "codex".into(),
        model: "gpt-6-astra".into(),
        effort: Some("high".into()),
        fallback: None,
        sandbox_here: true,
    });
    w.roster[0].fallback = Some("coder-claude".into());
    let added = w.add(DOCS_TICKET, None);
    let idea = added.idea.file.id.clone();
    let started = w.start(&idea);
    let Started::Refused(problems) = started else {
        panic!("the plan is not refused: {started:?}");
    };
    assert!(
        problems.iter().any(|p| p
            .message
            .contains("astra-creative: its model gpt-6-astra is of the reserved tier astra")),
        "{problems:?}"
    );
}

fn committed_docs_judge(w: &mut World) -> (String, String) {
    let added = w.add(DOCS_TICKET, None);
    let idea = added.idea.file.id.clone();
    let started = w.start(&idea);
    assert!(
        matches!(started, Started::Committed(Verdict::Committed { .. })),
        "the plan is not committed: {started:?}"
    );
    let judge_key = w.tasks(&idea)[1].3["key"].as_str().unwrap().to_string();
    let judge = w.task_id(&idea, &judge_key);
    (idea, judge)
}

const VERDICT_FORMAT: [&str; 9] = [
    "docs/sync.review.md",
    "## Verdict",
    "pass",
    "critical_open",
    "## Findings",
    "[CRITICAL]",
    "[HIGH]",
    "[MEDIUM]",
    "[LOW]",
];

#[test]
fn direct_judge_task_gets_verdict_output_and_generated_ticket() {
    let mut w = world();
    let (idea, judge) = committed_docs_judge(&mut w);
    let rows = w.tasks(&idea);
    assert_eq!(
        tasks::files(w.store.conn(), &judge, FileMode::Own).unwrap(),
        vec!["docs/sync.review.md".to_string()]
    );
    assert_eq!(
        rows[1].3["outputs"],
        serde_json::json!(["docs/sync.review.md"])
    );
    let rel = "tickets/02-document-the-retries-judge.md";
    assert_eq!(rows[1].3["ticket"], rel);
    let file = std::fs::read_to_string(w.folder(&idea).join(rel)).unwrap();
    let snapshot = tasks::get(w.store.conn(), &judge)
        .unwrap()
        .ticket_snapshot
        .unwrap_or_default();
    assert_eq!(snapshot, file);
    for part in VERDICT_FORMAT {
        assert!(snapshot.contains(part), "{part}: {snapshot}");
    }
}

#[test]
fn dispatched_judge_task_renders_kind_judge_and_verdict_instructions_in_task_files() {
    let mut w = world();
    let (idea, judge) = committed_docs_judge(&mut w);
    let work_key = w.tasks(&idea)[0].3["key"].as_str().unwrap().to_string();
    let work = w.task_id(&idea, &work_key);
    let ledger_git = Git::new("git");
    let config = w.config.clone();
    let ctx = DispatchCtx {
        jobs: JobCtx {
            guard: None,
            config: &config,
            needs_you: &NeedsYouList,
        },
        clock: CLOCK,
        git: &ledger_git,
    };
    let bind_ready = |w: &mut World, task: &str, profile: &str| {
        w.store
            .conn()
            .execute(
                "UPDATE tasks SET status = 'ready' WHERE task_id = ?1",
                [task],
            )
            .unwrap();
        w.store
            .immediate(|tx| bind(tx, &ctx, task, profile, &NewWorker::default()))
            .unwrap()
    };
    let work_bound = bind_ready(&mut w, &work, "coder-claude");
    let work_branch = work_bound.worktree.unwrap().branch;
    git(&w.repo, &["branch", &work_branch, "main"]);
    let bound = bind_ready(&mut w, &judge, "coder-codex");
    let fleet_root = w.root.join("fleet");
    let files = write_task_files(w.store.conn(), &fleet_root, &bound.attempt_id, &[]).unwrap();
    let yaml: serde_yaml::Value =
        serde_yaml::from_str(&std::fs::read_to_string(&files.task_yaml).unwrap()).unwrap();
    assert_eq!(yaml["kind"], "judge", "{yaml:?}");
    let start = std::fs::read_to_string(&files.start_md).unwrap();
    assert!(
        start.starts_with("# 02: Judge: Document the retries"),
        "{start}"
    );
    for part in VERDICT_FORMAT {
        assert!(start.contains(part), "{part}: {start}");
    }
}
