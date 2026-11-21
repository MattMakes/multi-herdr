//! The review steps of a code kind (ticket 20 of `docs/tickets/fleetd-p4/`).
//!
//! `docs/specs/fleetd/design.md`, "Done for each kind of task" (Code kind),
//! D6 and RR-18; `preservation-04-behavior.md` rows F1, F2, F5 and F12;
//! `preservation-05-contracts-and-values.md` row V-14.
//!
//! - After the check of a push attempt passed, the verifier runs the steps
//!   of the run's verification profile that come before `land`, in the
//!   run's order ([`verifier_steps`]). The check already ran as the
//!   `task_check` job of ticket 19. `land`, `pr` and `cleanup` are not
//!   steps of the verifier.
//! - Each step is today's code of `taskledger::finish`, called through
//!   [`VerifierAttempt`], not copied. `code-sweep` runs in fleetd: its
//!   reviewer tasks, rounds, `F<n>` ids, answers file and waivers are
//!   today's. Push dispatch dispatches its reviewer tasks. While a review of
//!   its round is open, the step waits and does not run again.
//! - `complexity-sweep` and `recheck` run a command: each is a `task_check`
//!   job run keyed by the tip and the hash of the step and its command
//!   ([`step_input_key`]), so the pass never waits for it.
//! - When each step passed or is not applicable, the task moves to `done`.
//! - A failed `code-sweep` sends the task back for a fix attempt: a review
//!   requeue. It adds 1 to `fix_rounds` (limit [`FIX_ROUND_LIMIT`]) and
//!   does not change the attempt count (RR-18). Past the limit, the attempt
//!   burns 1 attempt by rung 6.
//! - A failed `complexity-sweep` or `recheck` ends the attempt `rejected`
//!   and burns 1 attempt. Its `verdict.json` holds the list, and the next
//!   attempt gets it in its `START.md` ([`start_section`]; Q11 of the MAP).

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::fleetd::fleet_runtime::adapters::jobs_store::JobRow;
use crate::fleetd::fleet_runtime::application::jobs::{self, JobCtx, JobKey, Queued};
use crate::fleetd::fleet_runtime::application::ledger::transition;
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::jobs::Kind;
use crate::fleetd::fleet_runtime::domain::ledger::Entity;
use crate::fleetd::ideas::application::needs_you::NeedsYouList;
use crate::fleetd::task_execution::adapters::verdict_file;
use crate::fleetd::task_execution::application::retry::Policy;
use crate::fleetd::task_execution::application::verifier::{self, check_hash};
use crate::fleetd::task_execution::domain::verification::{input_key, Decision, Verdict, Verified};
use crate::taskledger::finish::{self, Outcome, Tools, VerifierAttempt};
use crate::taskledger::model::{StepName, StepStatus, TaskStatus};
use crate::taskledger::store::Store;
use crate::taskledger::tasks::{self, Task};
use crate::taskledger::{runs, util};

/// The most review requeues of 1 task: fix attempts after a failed
/// code-sweep, counted in `fix_rounds`, not in the attempt count (V-14).
pub const FIX_ROUND_LIMIT: i64 = 2;

/// The command of the complexity-sweep, for its job key: clippy's
/// `cognitive_complexity` lint on changed Rust files, and the analyser of
/// the complexity-sweep skill on changed TypeScript, JavaScript and Python
/// files (`taskledger::finish::complexity`).
pub const COMPLEXITY_COMMAND: &str =
    "cargo clippy -W clippy::cognitive_complexity; complexity-sweep analyser";

/// What the review [`run`] and its callers read.
pub struct Review<'a> {
    pub jobs: &'a JobCtx<'a>,
    /// The fleet clock of the pass.
    pub clock: FleetSeconds,
    /// The programs and places of the finish steps.
    pub tools: &'a Tools,
    /// The numbers of rung 6 of a task.
    pub policy: &'a dyn Fn(&Task) -> Policy,
}

/// What 1 [`run`] did with 1 task.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reviewed {
    /// Each step passed or is not applicable: the task is `done`.
    Done,
    /// The verified value no longer stands: nothing moved.
    Stale,
    /// The step waits: for its reviewer tasks, or for its job.
    Waiting(StepName),
    /// The job of the step is held at `abandoned_run_limit`.
    Held(StepName),
    /// A failed code-sweep: the task is `ready` for a fix attempt.
    FixAttempt(StepName),
    /// The step failed: the attempt ended `rejected` and burned 1 attempt.
    Rejected(StepName),
}

/// The steps that the verifier runs after the check, in the run's order
/// `steps`: each step before `land`, except the check, which ran as the
/// `task_check` job. `land`, `pr` and `cleanup` are never verifier steps.
pub fn verifier_steps(steps: &[StepName]) -> Vec<StepName> {
    steps
        .iter()
        .copied()
        .take_while(|n| *n != StepName::Land)
        .filter(|n| {
            !matches!(
                n,
                StepName::Check
                    | StepName::CommentSweep
                    | StepName::Land
                    | StepName::Pr
                    | StepName::Cleanup
            )
        })
        .collect()
}

/// The input key of the `task_check` job of step `step` at `tip`: the tip,
/// and the hash of the step and its command.
pub fn step_input_key(tip: &str, step: StepName, command: &str) -> String {
    input_key(tip, &step_hash(step, command))
}

fn step_hash(step: StepName, command: &str) -> String {
    check_hash(Some(&format!("{}\n{command}", step.as_str())))
}

/// Whether the `task_check` job with `input_key` is a complexity-sweep.
pub fn is_complexity_key(input_key: &str) -> bool {
    let hash = step_hash(StepName::ComplexitySweep, COMPLEXITY_COMMAND);
    input_key.rsplit_once(':').is_some_and(|(_, h)| h == hash)
}

/// The step outcome `o` as the `data` of a job result.
pub fn step_data(o: &Outcome) -> serde_json::Value {
    serde_json::to_value(StepData {
        status: o.status.as_str().into(),
        verdict: o.verdict.clone(),
        note: o.note.clone(),
        evidence: o.evidence.as_ref().map(|p| p.display().to_string()),
        judged_rev: o.judged_rev.clone(),
    })
    .unwrap_or_default()
}

/// The step outcome of a job result.
#[derive(Serialize, Deserialize)]
struct StepData {
    status: String,
    verdict: Option<String>,
    note: String,
    evidence: Option<String>,
    judged_rev: Option<String>,
}

/// The review steps of the attempt of `verified`, at its tip, in the store
/// `store` with no transaction open. Each call goes on from the step where
/// the last call stopped. A step that passed and stands for the tree now
/// does not run again (F5).
pub fn run(r: &Review<'_>, store: &mut Store, verified: Verified) -> Result<Reviewed> {
    let task = tasks::get(store.conn(), verified.task_id())?;
    if task.status != TaskStatus::Verifying.as_str() {
        return Ok(Reviewed::Stale);
    }
    let worker_id = verifier::attempt(store.conn(), verified.attempt())?.worker_id;
    let att = VerifierAttempt::open(store.conn(), r.tools, &task.task_id, &worker_id)?;
    att.seed(store)?;
    for name in verifier_steps(att.steps()) {
        if att.stands(store, name)? {
            continue;
        }
        let ran = match name {
            StepName::CodeSweep => code_sweep(store, &att, &task)?,
            _ => job_step(r, store, &att, &task, verified.tip(), name)?,
        };
        let o = match ran {
            Step::Through => continue,
            Step::Waiting => return Ok(Reviewed::Waiting(name)),
            Step::Held => return Ok(Reviewed::Held(name)),
            Step::Failed(o, job) => (o, job),
        };
        return fail(r, store, &task, &verified, name, o);
    }
    let guard = r.jobs.guard;
    let tip = verified.tip().to_string();
    let moved = store.immediate(|tx| verifier::done(tx, guard, verified, &tip))?;
    Ok(if moved {
        Reviewed::Done
    } else {
        Reviewed::Stale
    })
}

/// What 1 step did in 1 [`run`].
enum Step {
    Through,
    Waiting,
    Held,
    /// The outcome, with the job run that gave it.
    Failed(Outcome, Option<i64>),
}

/// The code-sweep in this process, as `finish` runs it. While a review of
/// its round is open, it waits and does not run.
fn code_sweep(store: &mut Store, att: &VerifierAttempt<'_>, task: &Task) -> Result<Step> {
    let running = att
        .row(store.conn(), StepName::CodeSweep)?
        .is_some_and(|row| row.status == StepStatus::Running.as_str());
    if running && finish::open_reviews(store.conn(), &task.task_id)? > 0 {
        return Ok(Step::Waiting);
    }
    let o = att.run(store, StepName::CodeSweep)?;
    Ok(match o.status {
        s if s.is_through() => Step::Through,
        StepStatus::Failed => Step::Failed(o, None),
        _ => Step::Waiting,
    })
}

/// A step that runs a command: its `task_check` job run at `tip`. A pass
/// queues it and never waits. A later pass records its result.
fn job_step(
    r: &Review<'_>,
    store: &mut Store,
    att: &VerifierAttempt<'_>,
    task: &Task,
    tip: &str,
    name: StepName,
) -> Result<Step> {
    let tree = tip_tree(r.tools, att, tip)?;
    let command = match name {
        StepName::ComplexitySweep => Some(COMPLEXITY_COMMAND.to_string()),
        _ => check_of(store.conn(), task)?,
    };
    let Some(command) = command else {
        // F2: horch decides `not_applicable` from the facts alone.
        let o = Outcome::not_applicable("the ticket says `**Check:** none`").judged(Some(tree));
        att.record(store, name, &o)?;
        return Ok(Step::Through);
    };
    let key = step_input_key(tip, name, &command);
    let (jobs_ctx, clock) = (r.jobs, r.clock);
    let queued = store.immediate(|tx| {
        let key = JobKey {
            kind: Kind::TaskCheck,
            subject: &task.task_id,
            input_key: &key,
        };
        jobs::queue(jobs_ctx, tx, &key, None, clock)
    })?;
    let row = match queued {
        Queued::New(_) | Queued::Active(_) => {
            let pending = att
                .row(store.conn(), name)?
                .is_none_or(|row| row.status != StepStatus::Running.as_str());
            if pending {
                att.start(store, name)?;
            }
            return Ok(Step::Waiting);
        }
        Queued::Held(_) => return Ok(Step::Held),
        Queued::Reused(row) => row,
    };
    let o = job_outcome(&row, &tree)?;
    att.record(store, name, &o)?;
    Ok(if o.status.is_through() {
        Step::Through
    } else {
        Step::Failed(o, Some(row.id))
    })
}

/// The check command of `task`: its own, else the run's default.
fn check_of(conn: &Connection, task: &Task) -> Result<Option<String>> {
    Ok(task
        .land_check
        .clone()
        .or(finish::default_check(conn, &task.run_id)?)
        .filter(|c| !c.trim().is_empty()))
}

/// The tree of `tip`: what a step of a job judged.
fn tip_tree(tools: &Tools, att: &VerifierAttempt<'_>, tip: &str) -> Result<String> {
    tools
        .git
        .run(
            att.dir(),
            &["rev-parse", "--verify", &format!("{tip}^{{tree}}")],
        )
        .map(|t| t.trim().to_string())
        .with_context(|| format!("the tree of the tip {tip}"))
}

/// The step outcome of the ended job run `row`. A run with step data gives
/// it. Else, the run of a check: its exit, judged at `tree`.
fn job_outcome(row: &JobRow, tree: &str) -> Result<Outcome> {
    let o = jobs::outcome(row).with_context(|| format!("the job run {} has no result", row.id))?;
    if let Some(d) = jobs::result_data(row).and_then(|d| serde_json::from_value::<StepData>(d).ok())
    {
        let status = StepStatus::parse(&d.status)?;
        return Ok(Outcome {
            status,
            verdict: d.verdict,
            evidence: d.evidence.map(Into::into),
            note: d.note,
            judged_rev: d.judged_rev.or(Some(tree.to_string())),
            review_task_ids: None,
            stop_workers: Vec::new(),
        });
    }
    let o = if o.passed {
        Outcome::pass("exit 0", None, &o.text)
    } else {
        Outcome::fail("failed", None, &o.text)
    };
    Ok(o.judged(Some(tree.to_string())))
}

/// The prefix of the reason of the verdict of a failed review step.
fn reason_of(name: StepName, o: &Outcome) -> String {
    let verdict = o.verdict.as_deref().unwrap_or(o.status.as_str());
    format!("{}: {verdict}: {}", name.as_str(), o.note)
}

/// Step `name` failed with `o`: record the failed verdict, then the review
/// requeue of a code-sweep, or the rejection of each other step.
fn fail(
    r: &Review<'_>,
    store: &mut Store,
    task: &Task,
    verified: &Verified,
    name: StepName,
    (o, job): (Outcome, Option<i64>),
) -> Result<Reviewed> {
    let guard = r.jobs.guard;
    store.immediate(|tx| {
        let passed =
            verdict_file::newest(tx, &task.task_id)?.context("a verified task has its verdict")?;
        let failed = Verdict {
            n: 0,
            decision: Decision::Failed,
            job,
            reason: reason_of(name, &o),
            attempt: verified.attempt().into(),
            tip: verified.tip().into(),
            ..passed
        };
        let verdict = verdict_file::record(tx, guard, failed)?;
        let a = verifier::attempt(tx, verified.attempt())?;
        if name == StepName::CodeSweep && fix_rounds(tx, &task.task_id)? < FIX_ROUND_LIMIT {
            verifier::end_attempt(tx, guard, &a, &verdict)?;
            requeue_for_fix(tx, guard, task, &verdict)?;
            return Ok(Reviewed::FixAttempt(name));
        }
        verifier::reject(tx, guard, r.policy, task, &a, &verdict)?;
        Ok(Reviewed::Rejected(name))
    })
}

fn fix_rounds(conn: &Connection, task_id: &str) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT fix_rounds FROM tasks WHERE task_id = ?1",
        [task_id],
        |r| r.get(0),
    )?)
}

/// The review requeue of `task` (RR-18): 1 more fix round, `verifying` to
/// `ready`, and the attempt count stays. The next attempt adds 1 to
/// `attempts`, and the floor moves with it.
fn requeue_for_fix(
    tx: &Connection,
    guard: Option<crate::fleetd::fleet_runtime::domain::ledger::Generation>,
    task: &Task,
    verdict: &Verdict,
) -> Result<()> {
    tx.execute(
        "UPDATE tasks SET fix_rounds = fix_rounds + 1, attempt_floor = attempt_floor + 1,
            current_worker_id = NULL, not_before = NULL, updated_at = ?2
         WHERE task_id = ?1",
        params![task.task_id, util::now()],
    )?;
    let text = format!(
        "the review requeues it for a fix attempt: {}",
        verdict_file::file_name(verdict.n)
    );
    transition(
        tx,
        guard,
        Entity::Task,
        &task.task_id,
        TaskStatus::Ready.as_str(),
        crate::fleetd::fleet_runtime::application::ledger::Cause {
            actor: crate::taskledger::model::Actor::Horch,
            text: &text,
        },
        &NeedsYouList,
    )?;
    Ok(())
}

/// Whether each verifier step of the attempt `attempt_id` of `task` passed
/// or is not applicable. Only then does a code kind move to `done`.
pub fn steps_through(conn: &Connection, task: &Task, attempt_id: &str) -> Result<bool> {
    let worker_id = verifier::attempt(conn, attempt_id)?.worker_id;
    let run = runs::get(conn, &task.run_id)?;
    let rows = finish::steps_of(conn, &task.task_id, &worker_id)?;
    for name in verifier_steps(&run.steps()?) {
        let through = rows.iter().any(|row| {
            row.name == name.as_str()
                && StepStatus::parse(&row.status).is_ok_and(|s| s.is_through())
        });
        if !through {
            return Ok(false);
        }
    }
    Ok(true)
}

/// The section of `START.md` of the next attempt of `task_id`, when the
/// newest verdict of the task is a failed review step: its verdict file
/// and its reason, which holds the list of findings or functions.
pub fn start_section(conn: &Connection, task_id: &str) -> Result<Option<String>> {
    Ok(verdict_file::newest(conn, task_id)?.and_then(|v| section_of(&v)))
}

/// [`start_section`] of each task of `task_ids`, by task: 1 statement for
/// any count of tasks. A task with no such section is not in the map.
pub fn start_sections<'a>(
    conn: &Connection,
    task_ids: impl IntoIterator<Item = &'a str>,
) -> Result<std::collections::BTreeMap<String, String>> {
    Ok(verdict_file::newest_of(conn, task_ids)?
        .into_iter()
        .filter_map(|(task_id, v)| section_of(&v).map(|s| (task_id, s)))
        .collect())
}

fn section_of(v: &crate::fleetd::task_execution::domain::verification::Verdict) -> Option<String> {
    let review_step = [
        StepName::CodeSweep,
        StepName::ComplexitySweep,
        StepName::Recheck,
    ]
    .iter()
    .any(|n| v.reason.starts_with(&format!("{}: ", n.as_str())));
    if v.decision != Decision::Failed || !review_step {
        return None;
    }
    Some(format!(
        "## The review of the last attempt\n\n\
         `{}` of attempt {}: {}\n",
        verdict_file::file_name(v.n),
        v.attempt,
        v.reason
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taskledger::model::VerifyProfile;

    #[test]
    fn verifier_steps_are_the_steps_before_land_without_the_check() {
        use StepName::*;
        assert_eq!(
            verifier_steps(&VerifyProfile::Full.steps()),
            [CodeSweep, ComplexitySweep, Recheck]
        );
        assert_eq!(
            verifier_steps(&VerifyProfile::Light.steps()),
            [ComplexitySweep]
        );
        assert!(verifier_steps(&VerifyProfile::Minimal.steps()).is_empty());
        // A step after land is never a verifier step.
        assert_eq!(verifier_steps(&[Check, Land, Recheck, Pr]), []);
    }

    #[test]
    fn the_job_key_of_a_step_is_the_tip_and_the_hash_of_the_step_and_its_command() {
        let complexity = step_input_key("tip1", StepName::ComplexitySweep, COMPLEXITY_COMMAND);
        assert!(complexity.starts_with("tip1:"));
        assert!(is_complexity_key(&complexity));
        let recheck = step_input_key("tip1", StepName::Recheck, "cargo test");
        assert!(!is_complexity_key(&recheck));
        // The recheck is not the check: the same command has another key.
        assert_ne!(recheck, input_key("tip1", &check_hash(Some("cargo test"))));
        assert_ne!(
            recheck,
            step_input_key("tip2", StepName::Recheck, "cargo test")
        );
        assert_ne!(
            recheck,
            step_input_key("tip1", StepName::Recheck, "cargo t")
        );
    }

    #[test]
    fn step_data_round_trips_through_a_job_result() {
        let o = Outcome::fail("2 violations", Some("/e/1.json".into()), "a.ts:1 f")
            .judged(Some("tree1".into()));
        let data = step_data(&o);
        let row_json = serde_json::json!({
            "passed": false, "verdict": true, "text": "x", "data": data,
        });
        let row = JobRow {
            id: 1,
            kind: Kind::TaskCheck,
            subject: "T-1".into(),
            repo: None,
            input_key: "tip1:h".into(),
            run: 1,
            abandoned_runs: 0,
            not_before: None,
            state: crate::fleetd::fleet_runtime::domain::jobs::State::Failed,
            pid: None,
            pid_start: None,
            time_limit_s: 0,
            deadline: None,
            result_json: Some(row_json.to_string()),
            created_at: String::new(),
            started_at: None,
            ended_at: None,
        };
        assert_eq!(job_outcome(&row, "tip-tree").unwrap(), o);
    }
}
