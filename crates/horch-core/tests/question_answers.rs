//! The 4 answer cases of a worker question and the decision log of the
//! feature (docs/specs/fleetd/design.md, "Ideas": How a worker's question
//! is resolved, steps 4 to 6; D13; RR-12;
//! docs/tickets/fleetd-p6/32-answers.md).
//!
//! The sub-orchestrator answers with an `answer` note: `covered` with a
//! citation that fleetd checks, `revise` to you, `replan` through the
//! replan file, or `own_decision`, a new assumption that needs no new
//! approval. Each answer goes to `features/F-NNNN/decisions.md`.
//!
//! Each test makes its own temporary ledger and idea folder, and gives the
//! fleet clock. No test reads the clock, the load or the limits of the
//! machine, and no test starts a process.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use horch_core::fleetd::fleet_runtime::application::jobs::JobCtx;
use horch_core::fleetd::fleet_runtime::application::ledger::{transition, Cause};
use horch_core::fleetd::fleet_runtime::domain::budget::PassBudget;
use horch_core::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use horch_core::fleetd::fleet_runtime::domain::config::FleetConfig;
use horch_core::fleetd::fleet_runtime::domain::ledger::Entity;
use horch_core::fleetd::ideas::adapters::idea_folder_files::IdeaFolder;
use horch_core::fleetd::ideas::adapters::idea_notes_reader::{AnswerCase, Note, NoteBody};
use horch_core::fleetd::ideas::adapters::ideas_store::{self, NewIdea};
use horch_core::fleetd::ideas::adapters::questions_store;
use horch_core::fleetd::ideas::application::answers::{
    answer_question, pending_replans, settle_revisions, Answered, Answering, Recorded,
};
use horch_core::fleetd::ideas::application::approval::{self, record_approval, Approved};
use horch_core::fleetd::ideas::application::needs_you::NeedsYouList;
use horch_core::fleetd::ideas::application::plan_commit::{
    validate_plan, PlanCommit, Verdict, PLAN_YAML,
};
use horch_core::fleetd::ideas::application::question_flow::{self, needs_you_key, Flow};
use horch_core::fleetd::ideas::application::questions::{self, Ask};
use horch_core::fleetd::ideas::application::replan::{replan_file, replan_task, Replan, Replanned};
use horch_core::fleetd::ideas::domain::approval::Approval;
use horch_core::fleetd::ideas::domain::idea_folder::Mark;
use horch_core::fleetd::ideas::domain::plan_validator::{DeclaredGates, RosterProfile};
use horch_core::fleetd::ideas::domain::questions::{Question, State};
use horch_core::fleetd::integration::adapters::sqlite::{self as rows, NewFeature};
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun};
use horch_core::taskledger::Store;
use horch_platform::channel::User;
use rusqlite::{params, Connection};

const IDEA: &str = "I-0007";
/// The feature of the run of the test: `F-<run rowid>` (MAP.md, Q5).
const FEATURE: &str = "F-0001";
/// The fleet clock when the test starts.
const START: i64 = 1000;
/// `question_limit_s` of the design: 15 min.
const LIMIT_S: u64 = 900;
/// The live sub-orchestrator attempt of the idea, and its pane.
const OWNER: &str = "I-0007/supervising/1";
const PANE: &str = "w1:p1";

const ASSUMPTIONS: &str = "# Assumptions of I-0007

feature: retry-sync
1. [confirmed] The client retries a failed request.
2. [assumed] The retry log goes to docs/sync.md.
";

const SPEC: &str = "# Spec of I-0007

The client retries each failed sync request 5 times with backoff.
";

const REQUIREMENTS: &str = "idea: I-0007
requirements:
  - id: R1
    text: The client retries with backoff.
    cites: [A1]
  - id: R2
    text: The retry behaviour is documented.
    cites: [A2]
";

/// `a`, `b` after `a`, the judgment task `doc` and its judge `doc-judge`.
const PLAN: &str = "idea: I-0007
repo: /fleet/repos/app
feature: F-0001
base_branch: fleet/retry-sync
tasks:
  - key: a
    title: Add backoff
    kind: feature
    profile: coder-claude
    covers: [R1]
    acceptance:
      - run: cargo test
  - key: b
    title: Log each retry
    kind: feature
    profile: coder-codex
    covers: [R1]
    after: [a]
    acceptance:
      - run: cargo test -p x
  - key: doc
    title: Document the retries
    kind: docs
    profile: coder-claude
    covers: [R2]
    outputs: [docs/sync.md]
    acceptance:
      - sections: [docs/sync.md, \"## Retries\"]
  - key: doc-judge
    title: Judge docs/sync.md
    kind: judge
    judges: doc
    profile: coder-codex
    outputs: [docs/sync.review.md]
";

/// The replan of `a`: a new `a`, and `a-log` after it.
const REPLACE_A: &str = "replace:
  - key: a
    title: Add backoff, in 2 steps
    kind: feature
    profile: coder-codex
    covers: [R1]
    acceptance:
      - run: cargo test -p sync
  - key: a-log
    title: Log the backoff
    kind: feature
    profile: coder-claude
    covers: [R1]
    after: [a]
    acceptance:
      - run: cargo test -p log
";

// ─── the world of a test ────────────────────────────────────────────────

struct World {
    dir: tempfile::TempDir,
    store: Store,
    roster: Vec<RosterProfile>,
    gates: DeclaredGates,
    config: FleetConfig,
}

fn cause(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

fn owner() -> User {
    User::current().unwrap()
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

/// A ledger at fleet clock [`START`] with 1 run, its feature, the idea
/// [`IDEA`] in `in_progress` with the committed plan [`PLAN`], and the live
/// sub-orchestrator [`OWNER`] of the idea in [`PANE`].
fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
    store
        .conn()
        .execute(
            "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
             VALUES (1, 1, 1, 1, 'then', ?1, 'then')",
            [START],
        )
        .unwrap();
    let root = dir.path().to_path_buf();
    let run = store
        .immediate(|tx| {
            runs::create(
                tx,
                &root,
                &NewRun {
                    slug: "retry-sync".into(),
                    ..Default::default()
                },
            )
        })
        .unwrap();
    let feature = NewFeature {
        id: FEATURE,
        name: Some("retry-sync"),
        branch: Some("fleet/retry-sync"),
        primary_branch: "main",
    };
    store
        .immediate(|tx| rows::insert_feature(tx, None, &feature, cause("feature opened")))
        .unwrap();
    let idea = NewIdea {
        id: IDEA,
        feature_id: FEATURE,
        priority: 0,
    };
    store
        .immediate(|tx| {
            ideas_store::insert_idea(tx, None, &idea, cause("idea added"))?;
            transition(
                tx,
                None,
                Entity::Idea,
                IDEA,
                "ticketing",
                cause("the test starts in ticketing"),
                &NeedsYouList,
            )?;
            Ok(())
        })
        .unwrap();
    let mut w = World {
        dir,
        store,
        roster: vec![
            profile("coder-claude", "claude", "claude-opus-5-5", "coder-codex"),
            profile("coder-codex", "codex", "gpt-5", "coder-claude"),
        ],
        gates: DeclaredGates {
            fast: Some("cargo test".into()),
            full: Some("cargo test --workspace".into()),
        },
        config: FleetConfig::default(),
    };
    std::fs::create_dir_all(w.folder()).unwrap();
    for folder in [w.dir.path().to_path_buf(), w.ideas(), w.folder()] {
        std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    w.write("assumptions.md", ASSUMPTIONS);
    w.write("spec.md", SPEC);
    w.write("requirements.yaml", REQUIREMENTS);
    w.write(PLAN_YAML, PLAN);
    let verdict = w.commit_plan();
    assert!(matches!(verdict, Verdict::Committed { .. }), "{verdict:?}");
    assert_eq!(w.idea_state(), "in_progress");
    let worker = "W-idea-lead-1";
    w.conn()
        .execute(
            "INSERT INTO workers (worker_id, run_id, persona, status, spawned_at, host)
             VALUES (?1, ?2, 'idea-lead', 'working', 'then', ?3)",
            params![
                worker,
                run.run_id,
                horch_core::taskledger::util::host_name()
            ],
        )
        .unwrap();
    w.conn()
        .execute(
            "INSERT INTO attempts (id, worker_id, idea_id, mux_handle, pid, pid_start, state, started_at)
             VALUES (?1, ?2, ?3, ?4, 4101, 11, 'live', 'then')",
            params![OWNER, worker, IDEA, PANE],
        )
        .unwrap();
    w
}

impl World {
    fn conn(&self) -> &Connection {
        self.store.conn()
    }

    fn root(&self) -> PathBuf {
        self.dir.path().to_path_buf()
    }

    fn ideas(&self) -> PathBuf {
        self.root().join("ideas")
    }

    fn folder(&self) -> PathBuf {
        self.ideas().join(IDEA)
    }

    fn idea_folder(&self) -> IdeaFolder {
        IdeaFolder::new(&self.ideas(), IDEA, owner()).unwrap()
    }

    /// Write the file `name` of the idea folder, mode 0600.
    fn write(&self, name: &str, text: &str) {
        let path = self.folder().join(name);
        std::fs::write(&path, text).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.folder().join(name)).unwrap()
    }

    fn commit_plan(&mut self) -> Verdict {
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
        validate_plan(&mut self.store, &ctx, IDEA).unwrap()
    }

    fn idea_state(&self) -> String {
        ideas_store::idea(self.conn(), IDEA).unwrap().unwrap().state
    }

    /// The id and the status of the task of `key` that is not `cancelled`.
    fn live(&self, key: &str) -> (String, String) {
        let live: Vec<(String, String)> = self
            .tasks(key)
            .into_iter()
            .filter(|(_, s, _)| s != "cancelled")
            .map(|(id, s, _)| (id, s))
            .collect();
        assert_eq!(live.len(), 1, "1 live task of {key}: {live:?}");
        live.into_iter().next().unwrap()
    }

    /// (id, status, replans) of each task of `key`, oldest first.
    fn tasks(&self, key: &str) -> Vec<(String, String, i64)> {
        let mut stmt = self
            .conn()
            .prepare(
                "SELECT task_id, status, replans FROM tasks
                 WHERE idea_id = ?1 AND json_extract(spec_json, '$.key') = ?2 ORDER BY rowid",
            )
            .unwrap();
        stmt.query_map([IDEA, key], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    fn status(&self, key: &str) -> String {
        self.live(key).1
    }

    /// The worker of the task of `key` asks question `n` and exits: the
    /// task waits in `blocked_on_user`.
    fn ask(&mut self, key: &str, n: u32) -> Question {
        let (task, status) = self.live(key);
        if status != "blocked_on_user" {
            self.store
                .immediate(|tx| {
                    for to in ["running", "blocked_on_user"] {
                        transition(
                            tx,
                            None,
                            Entity::Task,
                            &task,
                            to,
                            cause("test"),
                            &NeedsYouList,
                        )?;
                    }
                    Ok(())
                })
                .unwrap();
        }
        let attempt = format!("{task}/A1");
        let mail = format!("mail-{key}-{n}");
        let a = Ask {
            task_id: &task,
            attempt_id: &attempt,
            mail_id: Some(&mail),
            text: "QUESTION: how many retries?",
            asked_at: "2026-11-18T00:00:00Z",
        };
        questions::ask(self.conn(), &a, FleetSeconds(START), LIMIT_S).unwrap()
    }

    /// The sub-orchestrator answers question `q` with the case `case`.
    fn answer(
        &mut self,
        q: &Question,
        case: AnswerCase,
        cite: Option<&str>,
        text: &str,
    ) -> Answered {
        let note = Note {
            idea: IDEA.into(),
            attempt: OWNER.into(),
            body: NoteBody::Answer {
                question: q.id.clone(),
                case,
                cite: cite.map(str::to_string),
                text: text.into(),
            },
        };
        let root = self.root();
        let owner = owner();
        let ctx = Answering {
            guard: None,
            clock: FleetSeconds(START + 10),
            root: &root,
            owner: &owner,
        };
        self.store
            .immediate(|tx| answer_question(tx, &ctx, &note))
            .unwrap()
    }

    /// [`World::answer`], which must be recorded; its files are written
    /// after the commit.
    fn recorded(
        &mut self,
        q: &Question,
        case: AnswerCase,
        cite: Option<&str>,
        text: &str,
    ) -> Recorded {
        let Answered::Recorded(r) = self.answer(q, case, cite, text) else {
            panic!("the {case:?} answer of {} is refused", q.id);
        };
        r.write(&self.root(), &owner()).unwrap();
        r
    }

    fn question(&self, id: &str) -> Question {
        questions_store::get(self.conn(), id).unwrap().unwrap()
    }

    /// The `doorbell` events of the sub-orchestrator attempt.
    fn doorbells(&self) -> i64 {
        self.conn()
            .query_row(
                "SELECT count(*) FROM events WHERE kind = 'doorbell' AND entity = 'attempt'
                   AND entity_id = ?1",
                [OWNER],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn decisions(&self) -> Option<String> {
        let path = self
            .root()
            .join("features")
            .join(FEATURE)
            .join("decisions.md");
        std::fs::read_to_string(path).ok()
    }

    /// You approved the assumptions.md of the idea as it is now.
    fn approved(&mut self) {
        let folder = self.idea_folder();
        let sha = folder.sha256_of("assumptions.md").unwrap();
        let approval = Approval::of_assumptions(IDEA, &sha, "2026-11-18T00:00:00Z");
        let file = self
            .store
            .immediate(|tx| record_approval(tx, None, &approval, &sha, cause("you approved")))
            .unwrap();
        file.write(&folder).unwrap();
    }

    /// I7 for the idea: `true` when it holds.
    fn i7_holds(&mut self) -> bool {
        let folder = self.idea_folder();
        self.store
            .immediate(|tx| approval::check_i7(tx, None, &folder, IDEA))
            .unwrap()
    }

    fn replan(&mut self, task: &str, clock: i64) -> Replanned {
        let ideas = self.ideas();
        let owner = owner();
        let ctx = Replan {
            plan: PlanCommit {
                ideas: &ideas,
                owner: &owner,
                roster: &self.roster,
                gates: &self.gates,
                guard: None,
                refusals: &NeedsYouList,
            },
            jobs: JobCtx {
                guard: None,
                config: &self.config,
                needs_you: &NeedsYouList,
            },
        };
        replan_task(&mut self.store, &ctx, task, FleetSeconds(clock)).unwrap()
    }
}

// ─── covered ────────────────────────────────────────────────────────────

#[test]
fn covered_answer_with_a_missing_citation_is_refused() {
    let mut w = world();
    let q = w.ask("a", 1);
    let before = w.doorbells();

    // Line 40 of assumptions.md does not exist, and spec.md of an idea of
    // another feature is not a citation of this feature.
    for cite in ["assumptions.md:40", "I-0099/spec.md:3", "notes.md:1"] {
        let Answered::Refused(r) = w.answer(&q, AnswerCase::Covered, Some(cite), "5 retries")
        else {
            panic!("the citation {cite} is accepted");
        };
        assert!(r.reason.contains(cite), "{}", r.reason);
        // fleetd rings the owner again, so it answers again.
        let ring = r.ring.expect("a ring to the live owner");
        assert_eq!((ring.attempt.as_str(), ring.pane.as_str()), (OWNER, PANE));
        assert_eq!(ring.questions, vec![q.id.clone()]);
    }
    assert_eq!(w.doorbells(), before + 3);

    // The question stays open, and the task waits.
    assert_eq!(w.question(&q.id).state, State::Open);
    assert_eq!(w.status("a"), "blocked_on_user");
    assert_eq!(w.decisions(), None);
}

#[test]
fn covered_answer_moves_the_task_to_ready() {
    let mut w = world();
    let q1 = w.ask("a", 1);
    let r = w.recorded(
        &q1,
        AnswerCase::Covered,
        Some("spec.md:3"),
        "5 retries, as the spec says",
    );
    assert_eq!(r.question, q1.id);
    assert_eq!(w.question(&q1.id).state, State::Answered);
    assert_eq!(w.status("a"), "ready");

    // A citation of the assumptions of the idea, with its idea named.
    let q2 = w.ask("doc", 1);
    w.recorded(
        &q2,
        AnswerCase::Covered,
        Some("I-0007/assumptions.md:4"),
        "docs/sync.md",
    );
    assert_eq!(w.status("doc"), "ready");

    // An answered question is not answered again.
    let Answered::Refused(r) = w.answer(&q1, AnswerCase::Covered, Some("spec.md:3"), "again")
    else {
        panic!("an answered question is answered again");
    };
    assert!(r.ring.is_none(), "{r:?}");
}

// ─── revise ─────────────────────────────────────────────────────────────

#[test]
fn revise_answer_pauses_only_the_named_tasks() {
    let mut w = world();
    let q = w.ask("a", 1);
    let doc = w.status("doc");
    let b = w.status("b");

    w.recorded(
        &q,
        AnswerCase::Revise,
        None,
        "The retries need a config option: assumption 2 changes.",
    );

    // The idea goes to you; the question waits for your approval.
    assert_eq!(w.idea_state(), "awaiting_approval");
    let waiting = w.question(&q.id);
    assert_ne!(waiting.state, State::Answered);
    assert_eq!(w.status("a"), "blocked_on_user");
    // The other tasks of the idea do not pause.
    assert_eq!((w.status("doc"), w.status("b")), (doc, b));

    // The question flow does not ring or escalate it while it waits for
    // you.
    let flowed = w
        .store
        .immediate(|tx| {
            question_flow::run(
                tx,
                &Flow {
                    guard: None,
                    clock: FleetSeconds(START + 10 * LIMIT_S as i64),
                    elapsed_s: 0,
                    stamp: "2026-11-18T00:00:00Z",
                    budget: PassBudget::new(100),
                },
            )
        })
        .unwrap();
    assert!(
        flowed.rings.is_empty() && flowed.escalated.is_empty(),
        "{flowed:?}"
    );

    // Before your approval, nothing settles.
    let settled = w.store.immediate(|tx| settle_revisions(tx)).unwrap();
    assert!(settled.is_empty());
    assert_eq!(w.status("a"), "blocked_on_user");

    // Your approval answers the question, and the task goes on.
    let folder = w.idea_folder();
    let seen = folder.sha256_of("assumptions.md").unwrap();
    let approved = w
        .store
        .immediate(|tx| approval::approve(tx, None, &folder, IDEA, &seen))
        .unwrap();
    assert!(matches!(approved, Approved::Approved(_)), "{approved:?}");
    let settled = w.store.immediate(|tx| settle_revisions(tx)).unwrap();
    assert_eq!(settled, vec![q.id.clone()]);
    assert_eq!(w.question(&q.id).state, State::Answered);
    assert_eq!(w.status("a"), "ready");
}

// ─── own decision ───────────────────────────────────────────────────────

#[test]
fn own_decision_is_appended_as_an_assumption() {
    let mut w = world();
    w.approved();
    assert!(w.i7_holds());
    let q = w.ask("a", 1);

    w.recorded(
        &q,
        AnswerCase::OwnDecision,
        None,
        "Use the backoff helper of src/net.rs.",
    );

    let text = w.read("assumptions.md");
    let line = format!(
        "3. [assumed, own decision] Use the backoff helper of src/net.rs. ({})",
        q.id
    );
    assert!(text.ends_with(&format!("{line}\n")), "{text}");
    let parsed = w.idea_folder().read_assumptions().unwrap();
    assert_eq!(parsed.items.len(), 3);
    assert_eq!(parsed.items[2].mark, Mark::OwnDecision);

    // No new approval: the approval follows the decision, so I7 holds and
    // the idea stays in progress.
    assert!(w.i7_holds());
    assert_eq!(w.idea_state(), "in_progress");
    assert_eq!(w.question(&q.id).state, State::Answered);
    assert_eq!(w.status("a"), "ready");
}

// ─── the decision log ───────────────────────────────────────────────────

#[test]
fn each_answer_goes_to_the_decisions_file() {
    let mut w = world();
    let q1 = w.ask("a", 1);
    let q2 = w.ask("doc", 1);
    let r1 = w.recorded(&q1, AnswerCase::Covered, Some("spec.md:3"), "5 retries.");
    w.recorded(
        &q2,
        AnswerCase::OwnDecision,
        None,
        "One page in docs/sync.md.",
    );
    // The resumed worker of `a` asks again.
    let q3 = w.ask("a", 2);
    w.recorded(&q3, AnswerCase::Replan, None, "Task a needs a split.");

    let file = w.decisions().expect("decisions.md of the feature");
    assert!(
        file.starts_with(&format!("# {FEATURE} decisions\n")),
        "{file}"
    );
    for (q, case, answer) in [
        (&q1, "covered", "5 retries."),
        (&q2, "own_decision", "One page in docs/sync.md."),
        (&q3, "replan", "Task a needs a split."),
    ] {
        assert!(file.contains(&format!("## {}: {case}", q.id)), "{file}");
        assert!(file.contains(answer), "{file}");
    }
    assert!(file.contains("spec.md:3"), "{file}");
    assert!(file.contains("QUESTION: how many retries?"), "{file}");

    // A second write of the same decision adds nothing.
    r1.write(&w.root(), &owner()).unwrap();
    assert_eq!(w.decisions().unwrap(), file);
    let mode = std::fs::metadata(w.root().join("features").join(FEATURE).join("decisions.md"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

// ─── replan ─────────────────────────────────────────────────────────────

#[test]
fn replan_answer_with_a_retry_file_moves_the_task_to_ready() {
    let mut w = world();
    let q = w.ask("a", 1);
    w.recorded(&q, AnswerCase::Replan, None, "Do the backoff first.");

    // The question waits for the replan file.
    let (task, status) = w.live("a");
    assert_eq!(status, "blocked_on_user");
    assert_eq!(w.question(&q.id).state, State::Open);
    assert_eq!(pending_replans(w.conn()).unwrap(), vec![task.clone()]);
    assert_eq!(w.replan(&task, START + 20), Replanned::Waiting);

    w.write(&replan_file("a"), "retry: the backoff comes first\n");
    assert_eq!(w.replan(&task, START + 30), Replanned::Retried);
    let all = w.tasks("a");
    assert_eq!(all, vec![(task.clone(), "ready".to_string(), 1)]);
    assert_eq!(w.question(&q.id).state, State::Answered);
    assert!(pending_replans(w.conn()).unwrap().is_empty());
}

#[test]
fn replan_answer_with_a_replace_file_cancels_the_task_and_adds_the_new_tasks() {
    let mut w = world();
    let q = w.ask("a", 1);
    w.recorded(&q, AnswerCase::Replan, None, "Split a in 2 tasks.");
    let (old, _) = w.live("a");

    w.write(&replan_file("a"), REPLACE_A);
    let Replanned::Replaced { tasks } = w.replan(&old, START + 20) else {
        panic!("the replan of a is not a replace");
    };
    let keys: Vec<&str> = tasks.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["a", "a-log"]);

    // The old task follows A cancel from `blocked_on_user`, and its
    // question is answered.
    let all = w.tasks("a");
    assert_eq!(
        (all[0].0.as_str(), all[0].1.as_str(), all[0].2),
        (old.as_str(), "cancelled", 1)
    );
    assert_eq!(w.question(&q.id).state, State::Answered);
    let (new_a, _) = w.live("a");
    assert_ne!(new_a, old);
    assert!(pending_replans(w.conn()).unwrap().is_empty());
}

#[test]
fn replan_answer_with_no_replan_file_escalates_at_the_question_limit() {
    let mut w = world();
    let q = w.ask("a", 1);
    w.recorded(&q, AnswerCase::Replan, None, "Split a in 2 tasks.");
    let (task, _) = w.live("a");
    assert_eq!(w.replan(&task, START + 20), Replanned::Waiting);

    // The owner runs, and no replan file comes within the question limit.
    let flow = |clock: i64| Flow {
        guard: None,
        clock: FleetSeconds(clock),
        elapsed_s: 0,
        stamp: "2026-11-18T00:00:00Z",
        budget: PassBudget::new(100),
    };
    let before = w
        .store
        .immediate(|tx| question_flow::run(tx, &flow(START + LIMIT_S as i64 - 1)))
        .unwrap();
    assert!(before.escalated.is_empty(), "{before:?}");
    let at = w
        .store
        .immediate(|tx| question_flow::run(tx, &flow(START + LIMIT_S as i64)))
        .unwrap();
    assert_eq!(at.escalated, vec![q.id.clone()]);

    // The question goes to you; the task still waits.
    assert_eq!(w.question(&q.id).state, State::Escalated);
    assert_eq!(w.status("a"), "blocked_on_user");
    let open: i64 = w
        .conn()
        .query_row(
            "SELECT count(*) FROM needs_you WHERE key = ?1 AND state = 'open'",
            [needs_you_key(&q.id)],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(open, 1);
}
