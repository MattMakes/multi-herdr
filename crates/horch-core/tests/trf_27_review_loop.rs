//! The bounded, incremental code-sweep loop (trace-fixes ticket 18,
//! TRF-27..TRF-30) on a real git repository in a temp dir: the round cap
//! and its QUESTION, the diff since the last reviewed tree, stable finding
//! ids with carried answers and waivers, and the `Review` modes.

use std::path::{Path, PathBuf};
use std::process::Command;

use horch_core::taskledger::finish::{self, codesweep, CodexSandbox, FinishReport, Step, Tools};
use horch_core::taskledger::git::Git;
use horch_core::taskledger::model::{Actor, ReviewMode, StepName};
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::{self, NewWorker};
use horch_core::taskledger::Store;

const RUN_BRANCH: &str = "fleet/r1";

const REVIEWER_NAMES: &[&str] = &[
    "bug-hunter",
    "vulnerability-scanner",
    "structure-inspector",
    "efficiency-auditor",
    "clarity-checker",
    "coverage-analyst",
    "contract-guardian",
    "waste-detector",
    "reliability-auditor",
    "dependency-examiner",
    "power-of-ten-reviewer",
    "interface-auditor",
];

const NONE: &str = "## Critical\n- none\n## Important\n- none\n## Minor\n- none\n";

fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("LC_ALL", "C")
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {}: {}",
        args.join(" "),
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn commit(dir: &Path, file: &str, body: &str) {
    std::fs::write(dir.join(file), body).unwrap();
    git(dir, &["add", file]);
    git(dir, &["commit", "--quiet", "-m", file]);
}

struct World {
    _tmp: tempfile::TempDir,
    store: Store,
    _run: Run,
    tools: Tools,
    worker: String,
    wt: PathBuf,
}

impl World {
    /// A run with only code-sweep, the reviewer prompts, and a claimed task
    /// of `mode` whose worktree has 1 commit of `feature.txt`.
    fn new(mode: Option<ReviewMode>) -> World {
        let tmp = tempfile::tempdir().unwrap();
        let top = tmp.path().canonicalize().unwrap();
        let root = top.join("repo");
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "--quiet", "-b", "main"]);
        for (k, v) in [
            ("user.name", "t"),
            ("user.email", "t@example.com"),
            ("commit.gpgsign", "false"),
        ] {
            git(&root, &["config", k, v]);
        }
        commit(&root, "README.md", "base\n");
        git(&root, &["branch", RUN_BRANCH]);
        let mut store = Store::open(&top.join("ledger.sqlite3")).unwrap();
        let run = store
            .immediate(|tx| {
                runs::create(
                    tx,
                    &root,
                    &NewRun {
                        slug: "r1".into(),
                        base_branch: Some("main".into()),
                        run_branch: Some(RUN_BRANCH.into()),
                        finish_steps: vec![StepName::CodeSweep],
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        let sweep = top.join("code-sweep");
        std::fs::create_dir_all(sweep.join("reviewers")).unwrap();
        for r in REVIEWER_NAMES {
            std::fs::write(sweep.join("reviewers").join(format!("{r}.md")), "x\n").unwrap();
        }
        let tools = Tools {
            git: Git::new("git"),
            gh: top.join("gh"),
            node: "/bin/sh".into(),
            complexity_cli: None,
            code_sweep_dir: Some(sweep),
            evidence_root: top.join("evidence"),
            shell: "/bin/sh".into(),
            reviewers_down: Vec::new(),
            codex: CodexSandbox::default(),
        };
        let mut t = NewTask::work("feature", "t.md", "opus");
        t.review_mode = mode;
        let r = run.clone();
        let worker = store
            .immediate(|tx| {
                tasks::add(tx, &r, &t, Actor::Orchestrator)?;
                let id = workers::insert(
                    tx,
                    &NewWorker {
                        run_id: r.run_id.clone(),
                        persona: "opus".into(),
                        role: Some("opus-1".into()),
                        ..Default::default()
                    },
                )?;
                workers::supervisor_started(tx, &id, 4242, Some(1))?;
                workers::agent_started(tx, &id, 4243, Some(2))?;
                tasks::claim(tx, &id)?;
                Ok(id)
            })
            .unwrap();
        let wt = finish::worktree_new(&mut store, &tools.git, &worker, "feat").unwrap();
        let wt = PathBuf::from(&wt.path);
        commit(&wt, "feature.txt", "work\n");
        World {
            _tmp: tmp,
            store,
            _run: run,
            tools,
            worker,
            wt,
        }
    }

    fn finish(&mut self) -> FinishReport {
        finish::finish(&mut self.store, &self.tools, &self.worker).unwrap()
    }

    fn task(&self) -> tasks::Task {
        let id: String = self
            .store
            .conn()
            .query_row("SELECT task_id FROM tasks WHERE seq = 1", [], |r| r.get(0))
            .unwrap();
        tasks::get(self.store.conn(), &id).unwrap()
    }

    /// The review tasks not yet done: (persona, fallback, findings file).
    fn open_reviews(&self) -> Vec<(String, String, String)> {
        let mut stmt = self
            .store
            .conn()
            .prepare(
                "SELECT primary_persona, fallback_persona, spec_ref FROM tasks
                 WHERE kind = 'review' AND status != 'done' ORDER BY seq",
            )
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    }

    fn review_count(&self) -> i64 {
        self.store
            .conn()
            .query_row(
                "SELECT count(*) FROM tasks WHERE kind = 'review'",
                [],
                |r| r.get(0),
            )
            .unwrap()
    }

    /// Every open review is in: the first writes `first`, the others none.
    fn round_in(&self, first: &str) {
        for (n, (_, _, file)) in self.open_reviews().iter().enumerate() {
            std::fs::write(file, if n == 0 { first } else { NONE }).unwrap();
        }
        self.store
            .conn()
            .execute("UPDATE tasks SET status = 'done' WHERE kind = 'review'", [])
            .unwrap();
    }

    /// The folder of review round `round`.
    fn round_dir(&self, round: u32) -> PathBuf {
        let task = self.task();
        self.wt
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join(".horch/reviews")
            .join(format!("T{}-{}", task.seq, &task.task_id[..8]))
            .join(format!("round-{round}"))
    }

    fn questions(&self) -> Vec<String> {
        let mut stmt = self
            .store
            .conn()
            .prepare("SELECT text FROM outbox WHERE event = 'question' ORDER BY created_at")
            .unwrap();
        stmt.query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    }
}

fn step<'a>(r: &'a FinishReport, name: &str) -> &'a Step {
    r.steps.iter().find(|s| s.name == name).unwrap()
}

fn note(r: &FinishReport) -> String {
    step(r, "code-sweep").note.clone().unwrap_or_default()
}

fn verdict(r: &FinishReport) -> String {
    step(r, "code-sweep").verdict.clone().unwrap_or_default()
}

const MEH: &str =
    "## Critical\n- none\n## Important\n- a.rs:2 meh. Fix: check it\n## Minor\n- none\n";

#[test]
fn trf_27_the_review_stops_after_round_3_and_asks_the_orchestrator() {
    let mut w = World::new(None);
    for round in 1..=3 {
        let r = w.finish();
        assert!(
            note(&r).starts_with(&format!("round {round}: 3 reviewer tasks queued")),
            "{}",
            note(&r)
        );
        w.round_in(MEH);
        let r = w.finish();
        assert_eq!(verdict(&r), "WARN");
        if round < 3 {
            assert!(
                note(&r).contains("the change is reviewed again"),
                "{}",
                note(&r)
            );
            assert!(w.questions().is_empty());
            commit(&w.wt, &format!("fix{round}.txt"), "fix\n");
        }
    }
    let r = w.finish();
    let answers = w.round_dir(3).join("answers.md");
    assert!(
        note(&r).starts_with(&format!(
            "1 Important findings without an answer (F1) in round 3, the last review round (Review: full). horch asked the orchestrator to rule on them. Answer each one in {} as `F<n>: <reason>`",
            answers.display()
        )),
        "{}",
        note(&r)
    );
    let q = w.questions();
    assert_eq!(q.len(), 1, "1 row, even after 2 finish calls: {q:?}");
    assert!(
        q[0].starts_with("QUESTION: T-1 code-sweep round 3 is the last review round, and 1 findings still block (F1). Waive each one that does not block: horch ledger waive T-1 F1"),
        "{}",
        q[0]
    );
    // A new commit queues no round 4.
    commit(&w.wt, "fix3.txt", "fix\n");
    let before = w.review_count();
    let r = w.finish();
    assert_eq!(w.review_count(), before, "no round 4");
    assert_eq!(verdict(&r), "WARN");
    assert!(note(&r).contains("in round 3, the last review round"));
    // An answer resolves it; the pass says the change since round 3 is not reviewed.
    std::fs::write(&answers, "F1: fixed in fix3.txt\n").unwrap();
    let r = w.finish();
    assert!(r.complete, "{r:?}");
    assert_eq!(
        note(&r),
        "round 3: WARN. Round 3 is the last review round (Review: full): the change since it is not reviewed"
    );
}

#[test]
fn trf_28_rounds_2_and_3_review_only_the_change_since_the_last_round() {
    let mut w = World::new(None);
    w.finish();
    let d1 = std::fs::read_to_string(w.round_dir(1).join("diff.patch")).unwrap();
    assert!(d1.contains("feature.txt"));
    w.round_in(MEH);
    w.finish();
    commit(&w.wt, "second.txt", "second\n");
    w.finish();
    let d2 = std::fs::read_to_string(w.round_dir(2).join("diff.patch")).unwrap();
    assert!(d2.contains("second.txt"), "{d2}");
    assert!(
        !d2.contains("feature.txt"),
        "round 2 reviews only the new change: {d2}"
    );
    let ticket = std::fs::read_to_string(w.round_dir(2).join("bug-hunter.md")).unwrap();
    assert!(
        ticket.contains("- The changed files: second.txt\n"),
        "{ticket}"
    );
    w.round_in(MEH);
    w.finish();
    commit(&w.wt, "third.txt", "third\n");
    w.finish();
    let d3 = std::fs::read_to_string(w.round_dir(3).join("diff.patch")).unwrap();
    assert!(
        d3.contains("third.txt") && !d3.contains("second.txt"),
        "{d3}"
    );
}

#[test]
fn trf_29_finding_ids_carry_and_reviewers_rule_on_the_answers() {
    let mut w = World::new(None);
    w.finish();
    w.round_in(
        "## Critical\n- b.rs:1 boom\n## Important\n- a.rs:2 meh. Fix: check it\n## Minor\n- none\n",
    );
    let r = w.finish();
    assert!(
        note(&r).starts_with("1 Critical findings (F1)"),
        "{}",
        note(&r)
    );
    let task = w.task();
    codesweep::waive(&w.store, &w.tools.evidence_root, &task, "F1", "a test").unwrap();
    std::fs::write(
        w.round_dir(1).join("answers.md"),
        "F2: the caller checks it\n",
    )
    .unwrap();
    assert!(w.finish().complete);

    // Round 2: the ticket names answers.md, which carries the answer.
    commit(&w.wt, "second.txt", "second\n");
    w.finish();
    let answers = w.round_dir(2).join("answers.md");
    assert_eq!(
        std::fs::read_to_string(&answers).unwrap(),
        "F2: the caller checks it\n"
    );
    let ticket = std::fs::read_to_string(w.round_dir(2).join("bug-hunter.md")).unwrap();
    assert!(
        ticket.contains(&format!("The author's answers: {}.", answers.display())),
        "{ticket}"
    );
    assert!(
        ticket.contains("`- F<n> accepted` or `- F<n> rejected: <why>`"),
        "{ticket}"
    );
    assert!(ticket.contains("## Answers\n- none\n```"), "{ticket}");

    // The same findings at moved lines keep their ids; the waiver and the
    // accepted answer carry.
    w.round_in(
        "## Critical\n- b.rs:7 boom\n## Important\n- a.rs:9 meh. Fix: check it\n## Minor\n- c.rs:1 new\n## Answers\n- F2 accepted\n",
    );
    let r = w.finish();
    assert!(r.complete, "{}", note(&r));
    let report = std::fs::read_to_string(w.round_dir(2).join("report.md")).unwrap();
    assert!(
        report.contains("- F1 [bug-hunter] b.rs:7 boom — waived by the orchestrator: a test\n"),
        "{report}"
    );
    assert!(
        report.contains(
            "- F2 [bug-hunter] a.rs:9 meh. Fix: check it — answered: the caller checks it\n"
        ),
        "{report}"
    );
    assert!(
        report.contains("- F3 [bug-hunter] c.rs:1 new\n"),
        "{report}"
    );

    // Round 3: a reviewer rejects the answer and writes the finding again
    // by id. It blocks until the author changes the answer.
    commit(&w.wt, "third.txt", "third\n");
    w.finish();
    w.round_in(
        "## Critical\n- none\n## Important\n- F2 a.rs:12 still unchecked\n## Minor\n- none\n## Answers\n- F2 rejected: the caller does not check it\n",
    );
    let r = w.finish();
    assert!(
        note(&r).starts_with("1 Important findings without an answer (F2) in round 3"),
        "{}",
        note(&r)
    );
    let report = std::fs::read_to_string(w.round_dir(3).join("report.md")).unwrap();
    assert!(
        report.contains("- F2 [bug-hunter] a.rs:12 still unchecked — answer rejected by bug-hunter: the caller does not check it\n"),
        "{report}"
    );
    std::fs::write(
        w.round_dir(3).join("answers.md"),
        "F2: fixed in third.txt\n",
    )
    .unwrap();
    let r = w.finish();
    assert!(r.complete, "{}", note(&r));
}

#[test]
fn trf_30_review_modes_reviewer_limits_and_the_cheaper_rounds() {
    // none: the step does not apply.
    let mut w = World::new(Some(ReviewMode::NoReview));
    let r = w.finish();
    assert_eq!(step(&r, "code-sweep").status, "not_applicable");
    assert_eq!(note(&r), "the ticket says Review: none");
    assert_eq!(w.review_count(), 0);

    // light: 1 round of 2 reviewers; a new commit queues no round 2.
    let mut w = World::new(Some(ReviewMode::Light));
    let r = w.finish();
    assert!(
        note(&r).starts_with("round 1: 2 reviewer tasks queued (T-2, T-3)"),
        "{}",
        note(&r)
    );
    w.round_in(MEH);
    let r = w.finish();
    assert!(
        note(&r).contains("in round 1, the last review round (Review: light)"),
        "{}",
        note(&r)
    );
    commit(&w.wt, "fix.txt", "fix\n");
    w.finish();
    assert_eq!(w.review_count(), 2);

    // full: at most 3 reviewers, though the triage picks more; round 2
    // goes to the cheaper persona, with the full one as its fallback.
    let mut w = World::new(Some(ReviewMode::Full));
    commit(
        &w.wt,
        "lib.rs",
        "pub struct A;\nlet password = 1;\nfor x in y {}\npub fn f() {}\n",
    );
    w.finish();
    let round1 = w.open_reviews();
    assert_eq!(round1.len(), 3);
    assert!(round1
        .iter()
        .all(|(p, f, _)| p == "codex-reviewer" && f == "architect-reviewer"));
    w.round_in(MEH);
    w.finish();
    commit(&w.wt, "fix.txt", "fix\n");
    w.finish();
    let round2 = w.open_reviews();
    assert!(round2.len() <= 3 && !round2.is_empty());
    assert!(
        round2
            .iter()
            .all(|(p, f, _)| p == "codex-reviewer-light" && f == "codex-reviewer"),
        "{round2:?}"
    );
}
