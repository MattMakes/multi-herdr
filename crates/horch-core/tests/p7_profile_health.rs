//! The health of a profile (fleetd-p7 ticket 26; FR-8, FM-18, FM-19): a
//! failed conformance run or a dialog at rung 4 marks a profile unhealthy,
//! a passed run marks it healthy again, and dispatch skips a ready task of
//! an unhealthy profile with the hold "unhealthy profile <name>".
//!
//! The health is the last `profile_unhealthy` or `profile_healthy` event of
//! the profile (MAP.md Q10). Each test uses its own temporary ledger. No
//! test starts a process, reads the machine or waits for a time.

use horch_core::fleetd::task_execution::adapters::sqlite as attempts;
use horch_core::fleetd::task_execution::application::dispatch::{choices, CheckSandboxes};
use horch_core::fleetd::task_execution::application::stall;
use horch_core::fleetd::task_execution::domain::dispatch::{Choice, Hold, Holds};
use horch_core::fleetd::task_execution::domain::stall::Decision;
use horch_core::fleetd::worker_profiles::application::health::{
    health, mark_after_conformance, mark_healthy, mark_unhealthy, ConformanceRun,
};
use horch_core::fleetd::worker_profiles::domain::conformance::{
    Check, CheckResult, ConformanceReport,
};
use horch_core::fleetd::worker_profiles::domain::health::{
    Health, Mark, ProfileMarkedHealthy, ProfileMarkedUnhealthy,
};
use horch_core::harness::HarnessKind;
use horch_core::taskledger::model::Actor;
use horch_core::taskledger::runs::{self, NewRun, Run};
use horch_core::taskledger::tasks::{self, NewTask};
use horch_core::taskledger::workers::NewWorker;
use horch_core::taskledger::Store;
use serde_json::Value;

/// Each persona has a check sandbox.
struct AllSandboxes;

impl CheckSandboxes for AllSandboxes {
    fn supported(&self, _persona: &str) -> bool {
        true
    }
}

/// The profiles of the roster of the tests, with their agent.
const PROFILES: [(&str, HarnessKind); 3] = [
    ("opus", HarnessKind::Claude),
    ("sonnet", HarnessKind::Claude),
    ("codex-sol", HarnessKind::Codex),
];

struct World {
    _dir: tempfile::TempDir,
    store: Store,
    run: Run,
}

impl World {
    fn new() -> World {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        let root = dir.path().to_path_buf();
        let run = store
            .immediate(|tx| {
                let new = NewRun {
                    slug: "r1".into(),
                    ..Default::default()
                };
                let run = runs::create(tx, &root, &new)?;
                // FR-12: a task needs an idea in flight of the feature of its run.
                horch_core::taskledger::schema::run_idea(
                    tx,
                    &run.run_id,
                    &horch_core::taskledger::util::now(),
                    true,
                )?;
                Ok(run)
            })
            .unwrap();
        World {
            _dir: dir,
            store,
            run,
        }
    }

    /// 1 ready task of `persona`.
    fn ready(&mut self, title: &str, persona: &str) -> String {
        let run = self.run.clone();
        let new = NewTask::work(title, &format!("docs/{title}.md"), persona);
        self.store
            .immediate(|tx| Ok(tasks::add(tx, &run, &new, Actor::Orchestrator)?.task_id))
            .unwrap()
    }

    fn choices(&self) -> Vec<Choice> {
        choices(self.store.conn(), &Holds::default(), &AllSandboxes, None).unwrap()
    }

    fn health(&self, profile: &str) -> Health {
        health(self.store.conn(), profile).unwrap()
    }

    fn unhealthy(&mut self, profile: &str, reason: &str) -> Option<Mark> {
        self.store
            .immediate(|tx| mark_unhealthy(tx, None, profile, reason))
            .unwrap()
    }

    fn conformance(&mut self, report: &ConformanceReport) -> Vec<Mark> {
        let subject = format!("{} 0123456789abcdef", report.cli);
        self.conformance_run(1, &subject, report)
    }

    /// The marks after the conformance run `run` of the job subject
    /// `subject`.
    fn conformance_run(
        &mut self,
        run: i64,
        subject: &str,
        report: &ConformanceReport,
    ) -> Vec<Mark> {
        let ran = ConformanceRun { id: run, subject };
        self.store
            .immediate(|tx| mark_after_conformance(tx, None, &ran, report, &PROFILES))
            .unwrap()
    }

    /// The same ledger, opened again: a restart of fleetd.
    fn restart(&mut self) {
        let path = self._dir.path().join("ledger.sqlite3");
        self.store = Store::open(&path).unwrap();
    }

    /// The health events, oldest first: kind, entity, entity id, cause and
    /// the data.
    fn events(&self) -> Vec<(String, String, String, String, Value)> {
        let conn = self.store.conn();
        let mut stmt = conn
            .prepare(
                "SELECT kind, entity, entity_id, cause, data_json FROM events
                 WHERE kind IN ('profile_unhealthy', 'profile_healthy') ORDER BY event_id",
            )
            .unwrap();
        stmt.query_map([], |r| {
            let data: String = r.get(4)?;
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                serde_json::from_str(&data).unwrap(),
            ))
        })
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
    }
}

/// A conformance run of `cli` in which `failed` fail and Stop gate passes.
fn report(cli: &str, failed: &[Check]) -> ConformanceReport {
    let mut checks = vec![CheckResult::pass(Check::StopGate, "repaired")];
    checks.extend(failed.iter().map(|c| CheckResult::fail(*c, "no event")));
    ConformanceReport {
        cli: cli.into(),
        version: "9.8.7".into(),
        adapter_hash: "sha256:0123".into(),
        checks,
    }
}

fn hold_of(c: &[Choice], task_id: &str) -> Option<Hold> {
    c.iter().find_map(|c| match c {
        Choice::Held { task_id: t, hold } if t == task_id => Some(hold.clone()),
        _ => None,
    })
}

fn is_bound(c: &[Choice], task_id: &str) -> bool {
    c.iter()
        .any(|c| matches!(c, Choice::Bind { task_id: t, .. } if t == task_id))
}

fn keys(v: &Value) -> Vec<&str> {
    let mut k: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    k.sort_unstable();
    k
}

#[test]
fn unhealthy_profile_is_skipped_by_dispatch() {
    let mut w = World::new();
    let sick = w.ready("sick", "opus");
    let well = w.ready("well", "codex-sol");
    w.unhealthy("opus", "a check failed");

    let c = w.choices();
    let hold = hold_of(&c, &sick).expect("the task of the unhealthy profile waits");
    assert_eq!(
        hold,
        Hold::UnhealthyProfile {
            profile: "opus".into()
        }
    );
    assert_eq!(hold.to_string(), "unhealthy profile opus");
    // The task waits for its own profile: no fallback takes it.
    assert!(!c
        .iter()
        .any(|c| matches!(c, Choice::Bind { task_id, .. } if *task_id == sick)));
    assert!(is_bound(&c, &well), "{c:?}");
    assert_eq!(tasks::get(w.store.conn(), &sick).unwrap().status, "ready");
}

#[test]
fn profile_is_marked_healthy_again_when_conformance_passes() {
    let mut w = World::new();
    let task = w.ready("t", "opus");
    w.conformance(&report("claude", &[Check::LaunchHeadless]));
    assert!(matches!(w.health("opus"), Health::Unhealthy { .. }));
    assert!(hold_of(&w.choices(), &task).is_some());

    let marks = w.conformance(&report("claude", &[]));
    assert_eq!(
        marks,
        vec![
            Mark::Healthy(ProfileMarkedHealthy {
                profile_id: "opus".into()
            }),
            Mark::Healthy(ProfileMarkedHealthy {
                profile_id: "sonnet".into()
            }),
        ]
    );
    assert_eq!(w.health("opus"), Health::Healthy);
    assert_eq!(w.health("sonnet"), Health::Healthy);
    assert!(is_bound(&w.choices(), &task));
}

fn not_proved(cli: &str, failed: &[Check]) -> ConformanceReport {
    let mut r = report(cli, failed);
    r.checks.push(CheckResult::not_proved(
        Check::OperatorBoundary,
        "not proved: no fleetd serves the command channel",
    ));
    r
}

/// The hold of the profile `profile` whose last conformance run did not
/// prove a check: its persona hold, with the cause.
fn not_proved_hold(c: &[Choice], task_id: &str, profile: &str) -> String {
    match hold_of(c, task_id) {
        Some(Hold::Persona { persona, cause }) if persona == profile => cause,
        other => panic!("the task of {profile} is not held as not proved: {other:?}"),
    }
}

/// A check that is not proved because fleetd was not ready changes the
/// health of no profile: a healthy profile stays healthy, and an unhealthy
/// one stays unhealthy. A failed check of the same run still marks.
#[test]
fn not_proved_check_changes_no_health() {
    let mut w = World::new();
    w.ready("t", "opus");
    assert_eq!(w.conformance(&not_proved("claude", &[])), Vec::new());
    assert_eq!(w.health("opus"), Health::Healthy);
    assert!(w.events().is_empty(), "{:?}", w.events());

    w.conformance(&report("claude", &[Check::LaunchHeadless]));
    assert_eq!(w.conformance(&not_proved("claude", &[])), Vec::new());
    assert!(matches!(w.health("opus"), Health::Unhealthy { .. }));

    let mut w = World::new();
    let marks = w.conformance(&not_proved("claude", &[Check::SharedWrite]));
    assert_eq!(marks.len(), 2, "{marks:?}");
    let Health::Unhealthy { reason } = w.health("opus") else {
        panic!("opus is healthy");
    };
    assert!(reason.contains(Check::SharedWrite.name()), "{reason}");
    assert!(!reason.contains(Check::OperatorBoundary.name()), "{reason}");
}

/// D244-F1: a first conformance run whose check is not proved holds each
/// profile of the CLI, and a later passed run of the same key releases it.
/// A profile of another CLI is not held.
#[test]
fn first_not_proved_run_holds_the_profile_until_a_passed_run() {
    let mut w = World::new();
    let task = w.ready("t", "opus");
    let other = w.ready("other", "codex-sol");
    w.conformance_run(4, "claude 0123456789abcdef", &not_proved("claude", &[]));

    let c = w.choices();
    not_proved_hold(&c, &task, "opus");
    assert!(is_bound(&c, &other), "{c:?}");
    assert_eq!(tasks::get(w.store.conn(), &task).unwrap().status, "ready");

    let marks = w.conformance_run(5, "claude 0123456789abcdef", &report("claude", &[]));
    assert_eq!(marks.len(), 2, "{marks:?}");
    assert!(is_bound(&w.choices(), &task));
}

/// The hold text names the run that did not prove, the check and its
/// evidence, and the run that is awaited.
#[test]
fn not_proved_hold_names_its_cause_and_the_awaited_run() {
    let mut w = World::new();
    let task = w.ready("t", "opus");
    w.conformance_run(4, "claude 0123456789abcdef", &not_proved("claude", &[]));

    let cause = not_proved_hold(&w.choices(), &task, "opus");
    assert_eq!(
        cause,
        "conformance run 4 of claude 9.8.7 did not prove Operator boundary: \
         not proved: no fleetd serves the command channel; dispatch waits for a \
         passed conformance run of claude 0123456789abcdef, which a fleetd with \
         a channel server queues"
    );
    let hold = hold_of(&w.choices(), &task).unwrap();
    assert_eq!(hold.to_string(), format!("persona opus: {cause}"));
}

/// A held profile whose next run fails is unhealthy, as after any failed
/// run, and a passed run marks it healthy again.
#[test]
fn held_profile_with_a_failed_run_is_unhealthy() {
    let mut w = World::new();
    let task = w.ready("t", "opus");
    w.conformance_run(4, "claude 0123456789abcdef", &not_proved("claude", &[]));
    not_proved_hold(&w.choices(), &task, "opus");
    w.conformance_run(
        5,
        "claude 0123456789abcdef",
        &report("claude", &[Check::StopGate]),
    );

    assert!(matches!(w.health("opus"), Health::Unhealthy { .. }));
    assert_eq!(
        hold_of(&w.choices(), &task),
        Some(Hold::UnhealthyProfile {
            profile: "opus".into()
        })
    );
    w.conformance_run(6, "claude 0123456789abcdef", &report("claude", &[]));
    assert!(is_bound(&w.choices(), &task));
}

/// A new CLI binary or a new horch binary is a new key: a run of the new
/// key that does not prove a check holds the profile again, after a passed
/// run of the old key, until a passed run of the new key.
#[test]
fn not_proved_run_of_a_new_key_holds_again_until_its_proof() {
    let mut w = World::new();
    let task = w.ready("t", "opus");
    w.conformance_run(4, "claude 0123456789abcdef", &report("claude", &[]));
    assert!(is_bound(&w.choices(), &task));

    let mut new = not_proved("claude", &[]);
    new.version = "9.9.0".into();
    new.adapter_hash = "sha256:4567".into();
    w.conformance_run(7, "claude fedcba9876543210", &new);
    let cause = not_proved_hold(&w.choices(), &task, "opus");
    assert!(cause.contains("run 7 of claude 9.9.0"), "{cause}");
    assert!(cause.contains("claude fedcba9876543210"), "{cause}");

    let mut proof = report("claude", &[]);
    proof.version = "9.9.0".into();
    w.conformance_run(8, "claude fedcba9876543210", &proof);
    assert!(is_bound(&w.choices(), &task));
}

/// The hold and its release are rows: a restart of fleetd keeps each.
#[test]
fn restart_keeps_the_hold_and_the_proof() {
    let mut w = World::new();
    let task = w.ready("t", "opus");
    w.conformance_run(4, "claude 0123456789abcdef", &not_proved("claude", &[]));
    w.restart();
    not_proved_hold(&w.choices(), &task, "opus");

    w.conformance_run(5, "claude 0123456789abcdef", &report("claude", &[]));
    w.restart();
    assert!(is_bound(&w.choices(), &task));
}

#[test]
fn failed_conformance_marks_each_profile_of_the_cli() {
    let mut w = World::new();
    let failed = [Check::LaunchHeadless, Check::ContextLimit];
    let marks = w.conformance(&report("claude", &failed));
    assert_eq!(marks.len(), 2, "{marks:?}");
    for profile in ["opus", "sonnet"] {
        let Health::Unhealthy { reason } = w.health(profile) else {
            panic!("{profile} is healthy");
        };
        for check in failed {
            assert!(reason.contains(check.name()), "{reason}");
        }
        assert!(!reason.contains(Check::StopGate.name()), "{reason}");
    }
    // A profile of another CLI keeps its health.
    assert_eq!(w.health("codex-sol"), Health::Healthy);
}

#[test]
fn dialog_at_rung_4_marks_the_profile_unhealthy() {
    let mut w = World::new();
    let run = w.run.clone();
    let worker = w
        .store
        .immediate(|tx| {
            // The claim made no tree: the task works in the shared checkout.
            let mut a = NewTask::work("a", "docs/a.md", "opus");
            a.worktree_mode = horch_core::taskledger::model::WorktreeMode::Shared;
            let task = tasks::add(tx, &run, &a, Actor::Orchestrator)?.task_id;
            let new = NewWorker {
                run_id: run.run_id.clone(),
                persona: "opus".into(),
                pane_id: Some("w0:p1".into()),
                ..Default::default()
            };
            // fleetd binds the task to the worker and its live attempt (FR-5).
            let agent = horch_core::fleetd::task_execution::domain::task::Recorded {
                pid: 4_242,
                pid_start: Some(77),
            };
            Ok(horch_core::taskledger::testing::bind_live(tx, &task, &new, Some(agent))?.worker_id)
        })
        .unwrap();
    let attempt = attempts::attempts_of_worker(w.store.conn(), &worker)
        .unwrap()
        .remove(0);
    let next = w.ready("b", "opus");

    let decision = Decision::NeedsYou {
        excerpt: "Do you trust the files in this folder? (y/n)".into(),
    };
    w.store
        .immediate(|tx| stall::apply(tx, None, &attempt, 1_300, &decision))
        .unwrap();

    let Health::Unhealthy { reason } = w.health("opus") else {
        panic!("a dialog marks the profile unhealthy");
    };
    assert!(reason.contains(&attempt.id), "{reason}");
    assert_eq!(
        hold_of(&w.choices(), &next),
        Some(Hold::UnhealthyProfile {
            profile: "opus".into()
        })
    );
    // Only a passed conformance run marks it healthy again.
    w.conformance(&report("claude", &[]));
    assert_eq!(w.health("opus"), Health::Healthy);
}

#[test]
fn contract_profile_marked_unhealthy() {
    let mut w = World::new();
    let mark = w.unhealthy("opus", "a check failed");
    let want = ProfileMarkedUnhealthy {
        profile_id: "opus".into(),
        reason: "a check failed".into(),
    };
    assert_eq!(mark, Some(Mark::Unhealthy(want.clone())));
    assert_eq!(ProfileMarkedUnhealthy::NAME, "profile-marked-unhealthy");

    let events = w.events();
    assert_eq!(events.len(), 1, "{events:?}");
    let (kind, entity, id, cause, data) = &events[0];
    assert_eq!(kind, "profile_unhealthy");
    assert_eq!(entity, "profile");
    assert_eq!(id, "opus");
    assert_eq!(cause, "a check failed");
    // The data is the contract: exactly its profile_id and its reason.
    assert_eq!(keys(data), ["profile_id", "reason"]);
    assert_eq!(data["profile_id"], want.profile_id.as_str());
    assert_eq!(data["reason"], want.reason.as_str());
}

#[test]
fn contract_profile_marked_healthy() {
    let mut w = World::new();
    w.conformance(&report("codex", &[Check::LaunchHeadless]));
    let mark = w
        .store
        .immediate(|tx| mark_healthy(tx, None, "codex-sol", "a passed run"))
        .unwrap();
    let want = ProfileMarkedHealthy {
        profile_id: "codex-sol".into(),
    };
    assert_eq!(mark, Some(Mark::Healthy(want.clone())));
    assert_eq!(ProfileMarkedHealthy::NAME, "profile-marked-healthy");

    let events = w.events();
    assert_eq!(events.len(), 2, "{events:?}");
    let (kind, entity, id, _, data) = &events[1];
    assert_eq!(kind, "profile_healthy");
    assert_eq!(entity, "profile");
    assert_eq!(id, "codex-sol");
    assert_eq!(keys(data), ["profile_id"]);
    assert_eq!(data["profile_id"], want.profile_id.as_str());
}

#[test]
fn same_health_writes_no_event() {
    let mut w = World::new();
    // A healthy profile marked healthy: no event.
    let mark = w
        .store
        .immediate(|tx| mark_healthy(tx, None, "opus", "a passed run"))
        .unwrap();
    assert_eq!(mark, None);
    assert_eq!(w.conformance(&report("claude", &[])), Vec::new());
    assert_eq!(w.events().len(), 0);

    // An unhealthy profile marked unhealthy again: no event, and the
    // first cause stays.
    assert!(w.unhealthy("opus", "first").is_some());
    assert_eq!(w.unhealthy("opus", "second"), None);
    assert_eq!(
        w.health("opus"),
        Health::Unhealthy {
            reason: "first".into()
        }
    );
    // A second failed run marks only the profile that is healthy.
    let marks = w.conformance(&report("claude", &[Check::LaunchHeadless]));
    assert_eq!(marks.len(), 1, "{marks:?}");
    assert_eq!(
        w.conformance(&report("claude", &[Check::LaunchHeadless])),
        Vec::new()
    );
    assert_eq!(w.events().len(), 2);
}
