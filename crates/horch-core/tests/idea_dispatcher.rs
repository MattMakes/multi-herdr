//! The dispatcher loop: each pass moves each idea to its next state when
//! its files validate, and the ideas of 1 feature are planned 1 at a time
//! (docs/specs/fleetd/design.md, "Ideas: the dispatcher and
//! sub-orchestrators": The idea state machine, How many sub-orchestrators;
//! FR-2, FM-8; docs/tickets/fleetd-p6/26-dispatcher.md).
//!
//! Each test makes its own temporary ledger, idea folders and git
//! repository. No test touches this repository or the live ledger, no test
//! reads the clock of the machine, and no test starts a process other than
//! git.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use horch_core::fleetd::fleet_runtime::domain::budget::PassBudget;
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::ledger::Entity;
use horch_core::fleetd::ideas::adapters::ideas_store::{self, NewIdea};
use horch_core::fleetd::ideas::application::dispatcher::{
    Dispatch, Dispatcher, PassReport, PrimaryTip, PLANNING,
};
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::ideas::application::plan_commit::PlanCommit;
use horch_core::fleetd::ideas::domain::plan_validator::{DeclaredGates, RosterProfile};
use horch_core::fleetd::integration::adapters::git::CliGitGateway;
use horch_core::fleetd::integration::adapters::sqlite as feature_rows;
use horch_core::fleetd::integration::application::new_features::{self as nf, Project};
use horch_core::fleetd::worktree_custody::adapters::git::CliWorktreeGit;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::Store;
use horch_marketplace::git::scrub_repo_env;
use horch_platform::channel::User;

const STAMP: &str = "2026-01-01T00:00:00Z";

/// A ticket in format C1.
const TICKET: &str = "# 01: Add backoff

<!-- horch-ticket: 1 -->
**Persona:** primary `coder-claude`
**Kind:** feature
**Check:** `cargo test`
**Own:** `src/a.rs`

## Goal

Retry with backoff.
";

const SPEC: &str = "# Spec\n\n## Retries\n\nThe client retries with backoff.\n";

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
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

// ─── the world of a test ────────────────────────────────────────────────

struct World {
    _dir: tempfile::TempDir,
    root: PathBuf,
    repo: PathBuf,
    main_tip: String,
    store: Store,
    config: FleetConfig,
    owner: User,
    roster: Vec<RosterProfile>,
    gates: DeclaredGates,
    dispatcher: Dispatcher,
    ideas_added: u32,
    budget: u64,
}

fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let repo = root.join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    git(&repo, &["add", "a.txt"]);
    git(&repo, &["commit", "-q", "-m", "first"]);
    let main_tip = git(&repo, &["rev-parse", "main"]);
    let ideas = root.join("ideas");
    std::fs::create_dir(&ideas).unwrap();
    std::fs::set_permissions(&ideas, std::fs::Permissions::from_mode(0o700)).unwrap();
    let store = Store::open(&root.join("ledger.sqlite3")).unwrap();
    World {
        _dir: dir,
        root,
        repo,
        main_tip,
        store,
        config: FleetConfig::default(),
        owner: User::current().unwrap(),
        roster: vec![
            profile("coder-claude", "claude", "claude-opus-5-5", "coder-codex"),
            profile("coder-codex", "codex", "gpt-5", "coder-claude"),
        ],
        gates: DeclaredGates {
            fast: Some("cargo test".into()),
            full: Some("cargo test --workspace".into()),
        },
        dispatcher: Dispatcher::new(),
        ideas_added: 0,
        budget: 100,
    }
}

impl World {
    fn ideas(&self) -> PathBuf {
        self.root.join("ideas")
    }

    fn folder(&self, idea: &str) -> PathBuf {
        self.ideas().join(idea)
    }

    /// A new `open` feature with its internal run, and no name.
    fn feature(&mut self) -> String {
        let project = Project {
            root: &self.repo,
            primary_branch: "main",
        };
        let ctx = JobCtx {
            guard: None,
            config: &self.config,
            needs_you: &NeedsYouList,
        };
        self.store
            .immediate(|tx| nf::open_feature(tx, &ctx, &project, cause("idea added")))
            .unwrap()
    }

    /// A new idea of `feature` in `captured`, with its folder.
    fn idea(&mut self, feature: &str) -> String {
        self.ideas_added += 1;
        let id = format!("I-{:04}", self.ideas_added);
        let new = NewIdea {
            id: &id,
            feature_id: feature,
            priority: 0,
        };
        self.store
            .immediate(|tx| ideas_store::insert_idea(tx, None, &new, cause("idea added")))
            .unwrap();
        let folder = self.folder(&id);
        std::fs::create_dir_all(folder.join("tickets")).unwrap();
        for d in [folder.clone(), folder.join("tickets")] {
            std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        self.write(
            &id,
            "idea.md",
            &format!("---\nid: {id}\n---\nRetry sync.\n"),
        );
        id
    }

    /// Write the file `name` of the folder of `idea`, mode 0600.
    fn write(&self, idea: &str, name: &str, text: &str) {
        let path = self.folder(idea).join(name);
        std::fs::write(&path, text).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    fn remove(&self, idea: &str, name: &str) {
        let _ = std::fs::remove_file(self.folder(idea).join(name));
    }

    /// A valid assumptions.md of `idea` that names the feature `name`.
    fn assumptions(&self, idea: &str, name: &str) {
        let text = format!(
            "# {idea} assumptions\n\nfeature: {name}\n1. [confirmed] The client retries a failed request.\n"
        );
        self.write(idea, "assumptions.md", &text);
    }

    /// spec.md and requirements.yaml of `idea`; R1 cites `cite`.
    fn spec(&self, idea: &str, cite: &str) {
        self.write(idea, "spec.md", SPEC);
        let reqs = format!(
            "idea: {idea}\nrequirements:\n  - id: R1\n    text: The client retries with backoff.\n    cites: [{cite}]\n"
        );
        self.write(idea, "requirements.yaml", &reqs);
    }

    /// The ticket and plan.yaml of `idea` of `feature` on `branch`; the
    /// task runs the check `check`, or none when it is empty.
    fn plan(&self, idea: &str, feature: &str, branch: &str, check: &str) {
        self.write(idea, "tickets/01-a.md", TICKET);
        let acceptance = if check.is_empty() {
            String::new()
        } else {
            format!("    acceptance:\n      - run: {check}\n")
        };
        let plan = format!(
            "idea: {idea}\nrepo: {repo}\nfeature: {feature}\nbase_branch: {branch}\ntasks:\n  - key: a\n    title: Add backoff\n    ticket: tickets/01-a.md\n    kind: feature\n    profile: coder-claude\n    covers: [R1]\n{acceptance}",
            repo = self.repo.display(),
        );
        self.write(idea, "plan.yaml", &plan);
    }

    /// 1 pass of the dispatcher.
    fn pass(&mut self) -> PassReport {
        let ideas = self.ideas();
        let gateway = CliGitGateway::new(
            CliWorktreeGit::new("git", self.root.join("fleet")).writing(&[self.repo.join(".git")]),
        );
        let primaries = [PrimaryTip {
            repo: &self.repo,
            branch: "main",
            tip: &self.main_tip,
        }];
        let ctx = Dispatch {
            plan: PlanCommit {
                ideas: &ideas,
                owner: &self.owner,
                roster: &self.roster,
                gates: &self.gates,
                guard: None,
                refusals: &NeedsYouList,
            },
            job: JobCtx {
                guard: None,
                config: &self.config,
                needs_you: &NeedsYouList,
            },
            git: &gateway,
            primaries: &primaries,
            clock: FleetSeconds(1_000),
            stamp: STAMP,
            budget: PassBudget::new(self.budget),
        };
        let report = self.dispatcher.pass(&mut self.store, &ctx).unwrap();
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        report
    }

    fn state(&self, idea: &str) -> String {
        ideas_store::idea(self.store.conn(), idea)
            .unwrap()
            .unwrap()
            .state
    }

    /// The operator approves `idea` (ticket 18 owns the command; the test
    /// moves the idea through the door).
    fn approve(&mut self, idea: &str) {
        self.store
            .immediate(|tx| {
                transition(
                    tx,
                    None,
                    Entity::Idea,
                    idea,
                    "specifying",
                    cause("the operator approves"),
                    &NeedsYouList,
                )?;
                Ok(())
            })
            .unwrap();
    }

    fn branch(&self, feature: &str) -> Option<String> {
        feature_rows::feature(self.store.conn(), feature)
            .unwrap()
            .unwrap()
            .branch
    }

    fn phase_attempts(&self, idea: &str) -> i64 {
        ideas_store::idea(self.store.conn(), idea)
            .unwrap()
            .unwrap()
            .phase_attempts
    }

    /// The number of ideas of `feature` in a planning state.
    fn planning(&self, feature: &str) -> i64 {
        let sql = format!(
            "SELECT COUNT(*) FROM ideas WHERE feature_id = ?1 AND state IN ({})",
            PLANNING
                .iter()
                .map(|s| format!("'{s}'"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        self.store
            .conn()
            .query_row(&sql, [feature], |r| r.get(0))
            .unwrap()
    }
}

// ─── the tests ──────────────────────────────────────────────────────────

#[test]
fn second_idea_of_a_feature_waits_in_captured_until_the_first_leaves_ticketing() {
    let mut w = world();
    let f = w.feature();
    let first = w.idea(&f);
    let second = w.idea(&f);

    w.pass();
    assert_eq!(w.state(&first), "clarifying");
    assert_eq!(w.state(&second), "captured");

    w.assumptions(&first, "retry-sync");
    w.pass();
    assert_eq!(w.state(&first), "awaiting_approval");
    assert_eq!(w.state(&second), "captured");
    assert_eq!(w.branch(&f).as_deref(), Some("fleet/retry-sync"));

    w.approve(&first);
    w.spec(&first, "A1");
    w.pass();
    assert_eq!(w.state(&first), "ticketing");
    assert_eq!(w.state(&second), "captured");

    w.plan(&first, &f, "fleet/retry-sync", "cargo test");
    w.pass();
    assert_eq!(w.state(&first), "in_progress");
    // The first idea left ticketing in this pass; the second moves on the
    // next pass.
    w.pass();
    assert_eq!(w.state(&second), "clarifying");
}

#[test]
fn a_younger_idea_never_passes_an_older_captured_idea_of_its_feature() {
    let mut w = world();
    let f = w.feature();
    let older = w.idea(&f);
    let younger = w.idea(&f);
    // A budget of 1 row: the cursor gives the younger idea its turn too.
    w.budget = 1;
    for _ in 0..4 {
        w.pass();
        assert!(w.planning(&f) <= 1);
        assert_ne!(w.state(&younger), "clarifying");
    }
    assert_eq!(w.state(&older), "clarifying");
}

#[test]
fn ideas_of_2_features_move_at_the_same_time() {
    let mut w = world();
    let f1 = w.feature();
    let f2 = w.feature();
    let a = w.idea(&f1);
    let b = w.idea(&f2);

    let report = w.pass();
    assert_eq!(report.moves.len(), 2);
    assert_eq!(w.state(&a), "clarifying");
    assert_eq!(w.state(&b), "clarifying");

    w.assumptions(&a, "retry-sync");
    w.assumptions(&b, "log-retries");
    let report = w.pass();
    assert_eq!(w.state(&a), "awaiting_approval");
    assert_eq!(w.state(&b), "awaiting_approval");
    assert_eq!(report.refs.len(), 2);
}

#[test]
fn a_direct_idea_moves_from_captured_to_ticketing() {
    let mut w = world();
    let f = w.feature();
    let direct = w.idea(&f);
    w.plan(&direct, &f, "fleet/x", "cargo test");
    w.pass();
    assert_eq!(w.state(&direct), "ticketing");
}

#[test]
fn clarifying_moves_to_awaiting_approval_only_with_valid_assumptions() {
    let mut w = world();
    let f = w.feature();
    let idea = w.idea(&f);
    w.pass();
    assert_eq!(w.state(&idea), "clarifying");

    // No assumptions.md.
    w.pass();
    assert_eq!(w.state(&idea), "clarifying");

    // An item left [open].
    w.write(
        &idea,
        "assumptions.md",
        "# a\n\nfeature: retry-sync\n1. [open] Which requests retry?\n",
    );
    w.pass();
    assert_eq!(w.state(&idea), "clarifying");
    assert_eq!(w.branch(&f), None);

    // A link in place of the file is not read (SR-6).
    w.remove(&idea, "assumptions.md");
    let elsewhere = w.root.join("elsewhere.md");
    std::fs::write(
        &elsewhere,
        "# a\n\nfeature: retry-sync\n1. [confirmed] All.\n",
    )
    .unwrap();
    std::os::unix::fs::symlink(&elsewhere, w.folder(&idea).join("assumptions.md")).unwrap();
    w.pass();
    assert_eq!(w.state(&idea), "clarifying");

    // A feature name that is not kebab-case is refused, and the idea stays.
    w.remove(&idea, "assumptions.md");
    w.write(
        &idea,
        "assumptions.md",
        "# a\n\nfeature: Retry_Sync\n1. [confirmed] All.\n",
    );
    w.pass();
    assert_eq!(w.state(&idea), "clarifying");

    w.assumptions(&idea, "retry-sync");
    w.pass();
    assert_eq!(w.state(&idea), "awaiting_approval");
    assert_eq!(w.branch(&f).as_deref(), Some("fleet/retry-sync"));
}

#[test]
fn specifying_moves_to_ticketing_only_with_cited_requirements() {
    let mut w = world();
    let f = w.feature();
    let idea = w.idea(&f);
    w.pass();
    w.assumptions(&idea, "retry-sync");
    w.pass();
    w.approve(&idea);
    assert_eq!(w.state(&idea), "specifying");

    // No spec.md and no requirements.yaml.
    w.pass();
    assert_eq!(w.state(&idea), "specifying");

    // R1 cites A9, which is not an assumption or an answer.
    w.spec(&idea, "A9");
    w.pass();
    assert_eq!(w.state(&idea), "specifying");

    // spec.md with no `## ` heading.
    w.spec(&idea, "A1");
    w.write(&idea, "spec.md", "# Spec\n\nNo section.\n");
    w.pass();
    assert_eq!(w.state(&idea), "specifying");

    w.spec(&idea, "A1");
    w.pass();
    assert_eq!(w.state(&idea), "ticketing");
}

#[test]
fn a_refused_plan_is_validated_again_only_when_a_file_changes() {
    let mut w = world();
    let f = w.feature();
    let idea = w.idea(&f);
    w.pass();
    w.assumptions(&idea, "retry-sync");
    w.pass();
    w.approve(&idea);
    w.spec(&idea, "A1");
    w.pass();
    assert_eq!(w.state(&idea), "ticketing");

    // No plan.yaml: nothing to validate.
    w.pass();
    assert_eq!(w.phase_attempts(&idea), 0);

    // A task with no check is refused 1 time, not on each pass.
    w.plan(&idea, &f, "fleet/retry-sync", "");
    w.pass();
    w.pass();
    w.pass();
    assert_eq!(w.state(&idea), "ticketing");
    assert_eq!(w.phase_attempts(&idea), 1);

    w.plan(&idea, &f, "fleet/retry-sync", "cargo test");
    w.pass();
    assert_eq!(w.state(&idea), "in_progress");
}

/// A small fixed-seed generator: the same seed gives the same sequence on
/// each host.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }
}

#[test]
fn property_no_sequence_of_passes_puts_2_ideas_of_1_feature_in_planning() {
    for seed in 1..=12u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let mut w = world();
        let names = ["alpha", "beta"];
        let features: Vec<String> = (0..2).map(|_| w.feature()).collect();
        let mut ideas: Vec<(String, usize)> = Vec::new();
        for _ in 0..3 {
            for (k, f) in features.iter().enumerate() {
                let id = w.idea(f);
                ideas.push((id, k));
            }
        }
        for step in 0..40 {
            let (idea, k) = ideas[rng.below(ideas.len() as u64) as usize].clone();
            let feature = &features[k];
            let branch = format!("fleet/{}", names[k]);
            match rng.below(9) {
                0 => w.assumptions(&idea, names[k]),
                1 => w.write(&idea, "assumptions.md", "# a\n1. [open] Scope?\n"),
                2 => w.spec(&idea, "A1"),
                3 => w.spec(&idea, "A7"),
                4 => w.plan(&idea, feature, &branch, "cargo test"),
                5 => w.plan(&idea, feature, &branch, ""),
                6 => {
                    if w.state(&idea) == "awaiting_approval" {
                        w.approve(&idea);
                    }
                }
                7 => w.remove(&idea, "plan.yaml"),
                _ => {}
            }
            w.pass();
            for f in &features {
                assert!(
                    w.planning(f) <= 1,
                    "seed {seed}, step {step}: 2 ideas of {f} are in planning"
                );
            }
        }
    }
}
