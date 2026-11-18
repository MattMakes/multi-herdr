//! The judge pair in the verifier (ticket 41 of fleetd-p6).
//!
//! `docs/specs/fleetd/design.md`, "Architecture and roles" (The judge
//! pair, steps 1 to 7; the rank table), RR-3, FM-9, FM-33 and decision D4;
//! `docs/tickets/fleetd-p6/MAP.md`, Q18.
//!
//! - A task of a judgment kind waits in `verifying` after its structural
//!   check passed. Only [`judge_done`] lets the verifier move it to `done`.
//! - The verifier gives the verdict of each judge task that passed its
//!   structural check to [`settle`], in the transaction that records the
//!   structural verdict. The verdict stands only when the judge branch
//!   holds the tip that the newest verdict of the judged task judged.
//! - No open CRITICAL item: the judge task moves to `done`, then the judged
//!   task. The landing then moves each task branch to `ready`.
//! - An open CRITICAL item: the judge attempt ends `critical_open`, which
//!   burns no attempt. Below [`FIX_ROUND_LIMIT`] fix rounds, the judged
//!   task gets 1 more fix round and moves to `ready` for a fix attempt that
//!   takes no attempt of `max_attempts`, and the judge task moves to
//!   `pending` until the fix attempt passes its structural check. At the
//!   limit, both tasks move to `escalated` with 1 needs-you item.
//! - A HIGH item never blocks. Each HIGH item gets the id
//!   `<judge attempt>:H<n>`, goes to the decision log of the feature, and
//!   rings the live sub-orchestrator of the feature. It fixes the item with
//!   a replan or dismisses it with `horch idea note --dismiss`.
//! - The judge of a fix round checks only the earlier CRITICAL items and
//!   the change, so it cannot raise a MEDIUM or LOW item: [`settle`] drops
//!   them and logs them in 1 `note` event of the judge task.

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection};
use serde_json::json;

use crate::fleetd::fleet_runtime::adapters::sqlite as door;
use crate::fleetd::fleet_runtime::application::ledger::{record, transition, Cause};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::fleetd::ideas::application::answers::{live_owner, DecisionLog};
use crate::fleetd::ideas::application::needs_you::{self, NeedsYouList};
use crate::fleetd::ideas::application::question_flow::{feature_of_task, Ring};
use crate::fleetd::ideas::domain::answer::one_line;
use crate::fleetd::ideas::domain::needs_you::Write;
use crate::fleetd::task_execution::application::verifier;
use crate::fleetd::task_execution::domain::judge_pair::{
    parse_verdict, Finding, Rank, Verdict, VerdictWord,
};
use crate::fleetd::task_execution::domain::retry::Outcome;
use crate::fleetd::task_execution::domain::structural::Output;
use crate::fleetd::task_execution::domain::task::judged_task;
use crate::fleetd::task_execution::domain::verification::Verified;
use crate::taskledger::model::{Actor, EventKind, TaskKind, TaskStatus};
use crate::taskledger::tasks::{self, Task};
use crate::taskledger::util;

/// The most fix rounds of a judge pair (fixed contract, D4).
pub const FIX_ROUND_LIMIT: i64 = 2;

/// The kind of the needs-you item of a judge pair past its fix rounds.
pub const ESCALATED_KIND: &str = "judge_escalated";

/// The kind of the needs-you item of the HIGH items of a judged task with
/// no feature: no sub-orchestrator owns them.
pub const HIGH_KIND: &str = "judge_high";

/// The key of the needs-you item of the judged task `judged` past its fix
/// rounds.
pub fn escalated_key(judged: &str) -> String {
    format!("{ESCALATED_KIND}:{judged}")
}

/// What [`settle`] did with the pair.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// No open CRITICAL item: the judge task and the judged task are
    /// `done`.
    Done,
    /// An open CRITICAL item started the fix round `n`.
    FixRound(i64),
    /// An open CRITICAL item after [`FIX_ROUND_LIMIT`] fix rounds: both
    /// tasks are `escalated`.
    Escalated,
    /// The verdict does not stand for the judged task now: nothing moved.
    Waits,
}

/// What [`settle`] did with 1 verdict of a judge task.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settled {
    /// The judged task.
    pub judged: String,
    pub step: Step,
    /// The entry of the HIGH items in the decision log of the feature, to
    /// write after the commit.
    pub decision: Option<DecisionLog>,
    /// The doorbell of the HIGH items to the live sub-orchestrator of the
    /// feature, to type after the commit.
    pub ring: Option<Ring>,
    /// The MEDIUM and LOW items of the judge of a fix round, dropped.
    pub dropped: Vec<Finding>,
}

/// What [`settle`] reads besides the ledger.
pub struct Judging<'a> {
    /// The generation of the fleetd that writes.
    pub guard: Option<Generation>,
    /// The fleet clock of the pass, for a doorbell.
    pub clock: FleetSeconds,
    /// The structural verdict of the judge attempt, recorded in the same
    /// transaction.
    pub judge: &'a Verified,
    /// The verdict file of the judge attempt.
    pub verdict: &'a Verdict,
    /// The tip of the judged branch that the judge branch holds: the tip of
    /// the newest verdict of the judged task, when the judge branch holds
    /// it.
    pub held: Option<&'a str>,
}

/// The verdict of the outputs of a judge task whose structural check
/// passed: the findings of each verdict file, `critical_open` when 1 file
/// has it. `None` when no output is a verdict file.
pub fn verdict_of(outputs: &[Output]) -> Option<Verdict> {
    let mut all: Option<Verdict> = None;
    let texts = outputs
        .iter()
        .filter(|o| o.path.ends_with(".md"))
        .filter_map(|o| o.text.as_deref());
    for v in texts.filter_map(|t| parse_verdict(t).ok()) {
        all = Some(match all {
            None => v,
            Some(mut a) => {
                if v.word == VerdictWord::CriticalOpen {
                    a.word = VerdictWord::CriticalOpen;
                }
                a.findings.extend(v.findings);
                a
            }
        });
    }
    all
}

/// The judge tasks of the judged task `judged` that are not cancelled
/// (the rule of `judged_task`), with their status.
fn judges_of(conn: &Connection, judged: &str) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT task_id, status FROM tasks
         WHERE parent_task_id = ?1 AND kind = ?2 AND status <> ?3 ORDER BY rowid",
    )?;
    let rows = stmt
        .query_map(
            params![
                judged,
                TaskKind::Work.as_str(),
                TaskStatus::Cancelled.as_str()
            ],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

/// Whether the judge task of `judged` is `done`: only then does the
/// judged task leave `verifying` for `done`. `false` with no judge task.
pub fn judge_done(conn: &Connection, judged: &str) -> Result<bool> {
    let judges = judges_of(conn, judged)?;
    Ok(!judges.is_empty() && judges.iter().all(|(_, s)| s == TaskStatus::Done.as_str()))
}

/// The judge pair of the judge task `judge`, after its verdict file passed
/// its structural check, in the open transaction `tx`.
pub fn settle(tx: &Connection, j: &Judging<'_>, judge: &Task) -> Result<Settled> {
    let judged_id = judged_task(&judge.kind, judge.parent_task_id.as_deref())
        .with_context(|| format!("{} is not a judge task", judge.short()))?;
    let judged = tasks::get(tx, judged_id)?;
    let mut s = Settled {
        judged: judged.task_id.clone(),
        step: Step::Waits,
        decision: None,
        ring: None,
        dropped: Vec::new(),
    };
    let Some(held) = j.held else {
        return Ok(s);
    };
    let Some(result) = verifier::standing(tx, &judged.task_id, held)? else {
        return Ok(s);
    };
    let rounds = fix_rounds(tx, &judged.task_id)?;
    let (kept, dropped): (Vec<Finding>, Vec<Finding>) = j
        .verdict
        .findings
        .iter()
        .cloned()
        .partition(|f| rounds == 0 || matches!(f.rank, Rank::Critical | Rank::High));
    if !dropped.is_empty() {
        log_dropped(tx, j, judge, &dropped)?;
    }
    s.dropped = dropped;
    high(tx, j, judge, &judged, &kept, &mut s)?;
    s.step = match j.verdict.word {
        VerdictWord::Pass => {
            done(tx, j.guard, j.judge.clone(), judge)?;
            done(tx, j.guard, result, &judged)?;
            Step::Done
        }
        VerdictWord::CriticalOpen if rounds < FIX_ROUND_LIMIT => {
            fix_round(tx, j, judge, &judged, &result)?;
            Step::FixRound(rounds + 1)
        }
        VerdictWord::CriticalOpen => {
            escalate(tx, j, judge, &judged, &result)?;
            Step::Escalated
        }
    };
    Ok(s)
}

/// The verifier moves `task` to `done` for `verified`. The facts were read
/// in this transaction, so a refusal is a fault.
fn done(tx: &Connection, guard: Option<Generation>, verified: Verified, task: &Task) -> Result<()> {
    let tip = verified.tip().to_string();
    if !verifier::done(tx, guard, verified, &tip)? {
        bail!("the judge pair cannot move {} to done", task.short());
    }
    Ok(())
}

fn fix_rounds(conn: &Connection, task_id: &str) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT fix_rounds FROM tasks WHERE task_id = ?1",
        [task_id],
        |r| r.get(0),
    )?)
}

fn by_fleetd(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

/// The judge attempt and the attempt of the judged result end with the
/// outcome `critical_open`, which burns no attempt.
fn end_attempts(tx: &Connection, j: &Judging<'_>, result: &Verified, text: &str) -> Result<()> {
    for id in [j.judge.attempt(), result.attempt()] {
        let a = verifier::attempt(tx, id)?;
        verifier::end_with(tx, j.guard, &a, Outcome::CriticalOpen, text)?;
    }
    Ok(())
}

/// No attempt is burned: the next attempt adds 1 to `attempts`, and the
/// floor moves with it. A fix round of `judged` adds 1 to `fix_rounds`.
fn give_back(tx: &Connection, task_id: &str, fix_round: bool) -> Result<()> {
    tx.execute(
        "UPDATE tasks SET attempt_floor = attempt_floor + 1, fix_rounds = fix_rounds + ?2,
            current_worker_id = NULL, not_before = NULL, updated_at = ?3
         WHERE task_id = ?1",
        params![task_id, i64::from(fix_round), util::now()],
    )?;
    Ok(())
}

fn move_task(
    tx: &Connection,
    guard: Option<Generation>,
    task: &str,
    to: TaskStatus,
    text: &str,
) -> Result<()> {
    transition(
        tx,
        guard,
        Entity::Task,
        task,
        to.as_str(),
        by_fleetd(text),
        &NeedsYouList,
    )?;
    Ok(())
}

/// Step 5: the judged task moves to `ready` for its fix attempt, and the
/// judge task to `pending` until the fix attempt passes its structural
/// check.
fn fix_round(
    tx: &Connection,
    j: &Judging<'_>,
    judge: &Task,
    judged: &Task,
    result: &Verified,
) -> Result<()> {
    let text = format!(
        "the judge {} reports an open CRITICAL item on {}: fix round {}",
        judge.short(),
        judged.short(),
        fix_rounds(tx, &judged.task_id)? + 1
    );
    end_attempts(tx, j, result, &text)?;
    give_back(tx, &judged.task_id, true)?;
    give_back(tx, &judge.task_id, false)?;
    move_task(tx, j.guard, &judged.task_id, TaskStatus::Ready, &text)?;
    move_task(tx, j.guard, &judge.task_id, TaskStatus::Pending, &text)
}

/// Step 7: after [`FIX_ROUND_LIMIT`] fix rounds both tasks move to
/// `escalated`, and 1 needs-you item goes to you.
fn escalate(
    tx: &Connection,
    j: &Judging<'_>,
    judge: &Task,
    judged: &Task,
    result: &Verified,
) -> Result<()> {
    let text = format!(
        "the judge {} reports an open CRITICAL item on {} after {FIX_ROUND_LIMIT} fix rounds",
        judge.short(),
        judged.short()
    );
    end_attempts(tx, j, result, &text)?;
    for task in [&judged.task_id, &judge.task_id] {
        tx.execute(
            "UPDATE tasks SET current_worker_id = NULL, updated_at = ?2 WHERE task_id = ?1",
            params![task, util::now()],
        )?;
        move_task(tx, j.guard, task, TaskStatus::Escalated, &text)?;
    }
    needs_you::open_item(
        tx,
        &Write {
            key: &escalated_key(&judged.task_id),
            kind: ESCALATED_KIND,
            entity: Entity::Task.as_str(),
            entity_id: &judged.task_id,
            text: &text,
        },
    )
}

/// 1 `note` event of the judge task logs the `dropped` items.
fn log_dropped(tx: &Connection, j: &Judging<'_>, judge: &Task, dropped: &[Finding]) -> Result<()> {
    let items: Vec<String> = dropped.iter().map(item).collect();
    let text = format!(
        "the judge {} of a fix round raised {} MEDIUM or LOW item(s); the verifier drops them: {}",
        judge.short(),
        dropped.len(),
        items.join("; ")
    );
    let data = json!({ "attempt": j.judge.attempt(), "dropped": items });
    record(
        tx,
        j.guard,
        EventKind::Note,
        Entity::Task,
        &judge.task_id,
        by_fleetd(&text),
        &data,
    )?;
    Ok(())
}

/// `[RANK] text` of `f`, on 1 line.
fn item(f: &Finding) -> String {
    let rank = match f.rank {
        Rank::Critical => "CRITICAL",
        Rank::High => "HIGH",
        Rank::Medium => "MEDIUM",
        Rank::Low => "LOW",
    };
    format!("[{rank}] {}", one_line(&f.text))
}

/// The HIGH items of `kept`: 1 `note` event of the judge task, then 1
/// entry of the decision log of the feature and 1 doorbell to its live
/// sub-orchestrator. A judged task with no feature gives 1 needs-you item.
fn high(
    tx: &Connection,
    j: &Judging<'_>,
    judge: &Task,
    judged: &Task,
    kept: &[Finding],
    s: &mut Settled,
) -> Result<()> {
    let attempt = j.judge.attempt();
    let items: Vec<String> = kept
        .iter()
        .filter(|f| f.rank == Rank::High)
        .enumerate()
        .map(|(i, f)| format!("{attempt}:H{} {}", i + 1, item(f)))
        .collect();
    if items.is_empty() {
        return Ok(());
    }
    let text = format!(
        "the judge {} reports {} HIGH item(s) on {}: {}",
        judge.short(),
        items.len(),
        judged.short(),
        items.join("; ")
    );
    let data = json!({ "attempt": attempt, "high": items });
    record(
        tx,
        j.guard,
        EventKind::Note,
        Entity::Task,
        &judge.task_id,
        by_fleetd(&text),
        &data,
    )?;
    let Some(feature) = feature_of_task(tx, &judged.task_id)? else {
        return needs_you::open_item(
            tx,
            &Write {
                key: &format!("{HIGH_KIND}:{attempt}"),
                kind: HIGH_KIND,
                entity: Entity::Task.as_str(),
                entity_id: &judged.task_id,
                text: &text,
            },
        );
    };
    let heading = format!("## {attempt}: HIGH");
    let mut entry = format!(
        "\n{heading}\n\n- Judge task: {}\n- Judged task: {}\n",
        judge.short(),
        judged.short()
    );
    for i in &items {
        entry.push_str(&format!("- {i}\n"));
    }
    s.decision = Some(DecisionLog {
        feature: feature.clone(),
        heading,
        text: entry,
    });
    s.ring = ring(tx, j, &feature, &text)?;
    Ok(())
}

/// 1 doorbell event of the live sub-orchestrator of `feature`, typed after
/// the commit. `None` when no process of the feature runs: the decision
/// log holds the items for its next start.
fn ring(tx: &Connection, j: &Judging<'_>, feature: &str, text: &str) -> Result<Option<Ring>> {
    let Some((attempt, pane)) = live_owner(tx, feature)? else {
        return Ok(None);
    };
    let data = json!({ "fleet_clock": j.clock.0, "judge_attempt": j.judge.attempt() });
    record(
        tx,
        j.guard,
        EventKind::Doorbell,
        Entity::Attempt,
        &attempt,
        by_fleetd(text),
        &data,
    )?;
    let event = door::facts_of(tx, Entity::Attempt, &attempt, EventKind::Doorbell)?
        .last()
        .map(|f| f.event_id)
        .context("the doorbell event that the judge pair recorded")?;
    Ok(Some(Ring {
        attempt,
        pane,
        event,
        questions: Vec::new(),
    }))
}
