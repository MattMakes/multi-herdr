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

pub const FIX_ROUND_LIMIT: i64 = 2;

pub const ESCALATED_KIND: &str = "judge_escalated";

pub const HIGH_KIND: &str = "judge_high";

pub fn escalated_key(judged: &str) -> String {
    format!("{ESCALATED_KIND}:{judged}")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Done,
    FixRound(i64),
    Escalated,
    Waits,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settled {
    pub judged: String,
    pub step: Step,
    pub decision: Option<DecisionLog>,
    pub ring: Option<Ring>,
    pub dropped: Vec<Finding>,
}

pub struct Judging<'a> {
    pub guard: Option<Generation>,
    pub clock: FleetSeconds,
    pub judge: &'a Verified,
    pub verdict: &'a Verdict,
    pub held: Option<&'a str>,
}

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

pub fn judge_done(conn: &Connection, judged: &str) -> Result<bool> {
    let judges = judges_of(conn, judged)?;
    Ok(!judges.is_empty() && judges.iter().all(|(_, s)| s == TaskStatus::Done.as_str()))
}

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

fn done(tx: &Connection, guard: Option<Generation>, verified: Verified, task: &Task) -> Result<()> {
    let tip = verified.tip().to_string();
    if !verifier::done(tx, guard, verified, &tip)? {
        bail!("the judge pair cannot move {} to done", task.short());
    }
    Ok(())
}

pub fn in_fix_round(conn: &Connection, task_id: &str) -> Result<bool> {
    Ok(fix_rounds(conn, task_id)? > 0)
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

fn end_attempts(tx: &Connection, j: &Judging<'_>, result: &Verified, text: &str) -> Result<()> {
    for id in [j.judge.attempt(), result.attempt()] {
        let a = verifier::attempt(tx, id)?;
        verifier::end_with(tx, j.guard, &a, Outcome::CriticalOpen, text)?;
    }
    Ok(())
}

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

fn item(f: &Finding) -> String {
    let rank = match f.rank {
        Rank::Critical => "CRITICAL",
        Rank::High => "HIGH",
        Rank::Medium => "MEDIUM",
        Rank::Low => "LOW",
    };
    format!("[{rank}] {}", one_line(&f.text))
}

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
