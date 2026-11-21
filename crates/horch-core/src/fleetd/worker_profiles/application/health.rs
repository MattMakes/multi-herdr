//! Mark a worker profile unhealthy or healthy, and read its health
//! (`docs/specs/fleetd/design.md`, "Worker profiles and adapters" (The
//! health of a profile); fleetd-p7 MAP.md Q10 and Q15).
//!
//! The health of a profile is its last `profile_unhealthy` or
//! `profile_healthy` event, with the contract of the mark in `data_json`.
//! No event means healthy. Each mark is 1 event row in the caller's
//! transaction, and a mark that does not change the health writes none.
//!
//! A healthy profile whose last conformance run did not prove a check is
//! held (D244-F1): its last event of the 3 kinds is a `note` hold. The hold
//! is not a failed health: it adds no drift idea, and a passed run of its
//! CLI releases it with a `profile_healthy` event.

use std::collections::BTreeMap;

use anyhow::Result;
use rusqlite::Connection;
use serde_json::json;

use crate::fleetd::fleet_runtime::adapters::sqlite::{
    append_named_fact, last_named_facts, NamedFact,
};
use crate::fleetd::fleet_runtime::domain::ledger::Generation;
use crate::fleetd::worker_profiles::application::docs_jobs::exec_harness;
use crate::fleetd::worker_profiles::domain::conformance::ConformanceReport;
use crate::fleetd::worker_profiles::domain::health::{self, Health, Mark, PROFILE};
use crate::harness::HarnessKind;
use crate::taskledger::model::{Actor, EventKind};

const NOT_PROVED: EventKind = EventKind::Note;

const KINDS: [EventKind; 3] = [
    EventKind::ProfileUnhealthy,
    EventKind::ProfileHealthy,
    NOT_PROVED,
];

/// The conformance run of a report: its job id and its job subject.
pub struct ConformanceRun<'a> {
    pub id: i64,
    pub subject: &'a str,
}

/// Each unhealthy profile, with the cause of its mark.
pub fn unhealthy_profiles(conn: &Connection) -> Result<BTreeMap<String, String>> {
    last_of_kind(conn, EventKind::ProfileUnhealthy)
}

/// Each profile that is held because its last conformance run did not
/// prove a check, with the cause of its hold: 1 statement for all profiles.
pub fn not_proved_profiles(conn: &Connection) -> Result<BTreeMap<String, String>> {
    last_of_kind(conn, NOT_PROVED)
}

fn last_of_kind(conn: &Connection, kind: EventKind) -> Result<BTreeMap<String, String>> {
    Ok(last_named_facts(conn, PROFILE, &KINDS)?
        .into_iter()
        .filter(|f| f.kind == kind.as_str())
        .map(|f| (f.id, f.cause))
        .collect())
}

/// The health of `profile` now.
pub fn health(conn: &Connection, profile: &str) -> Result<Health> {
    Ok(match unhealthy_profiles(conn)?.remove(profile) {
        Some(reason) => Health::Unhealthy { reason },
        None => Health::Healthy,
    })
}

/// Mark `profile` unhealthy with `reason`, in `tx`. `None` when it is
/// unhealthy already.
pub fn mark_unhealthy(
    tx: &Connection,
    guard: Option<Generation>,
    profile: &str,
    reason: &str,
) -> Result<Option<Mark>> {
    let mark = health::mark_unhealthy(&health(tx, profile)?, profile, reason);
    write(tx, guard, mark.as_ref(), reason)?;
    Ok(mark)
}

/// Mark `profile` healthy, in `tx`, for `cause`. `None` when it is healthy
/// already.
pub fn mark_healthy(
    tx: &Connection,
    guard: Option<Generation>,
    profile: &str,
    cause: &str,
) -> Result<Option<Mark>> {
    let mark = health::mark_healthy(&health(tx, profile)?, profile);
    write(tx, guard, mark.as_ref(), cause)?;
    Ok(mark)
}

/// Mark each profile of `profiles` whose agent is the harness of the CLI of
/// the conformance run `report` (the docs source `agy` is the harness
/// `antigravity`), in `tx`: unhealthy after a failed run, with
/// the failed checks as the cause, and healthy again after a passed run.
/// A run with no failed check and a check that is not proved (fleetd was
/// not ready) does not prove the adapter: it holds each healthy profile,
/// and an unhealthy one stays unhealthy. Gives the health marks.
pub fn mark_after_conformance(
    tx: &Connection,
    guard: Option<Generation>,
    run: &ConformanceRun<'_>,
    report: &ConformanceReport,
    profiles: &[(&str, HarnessKind)],
) -> Result<Vec<Mark>> {
    let last: BTreeMap<String, (String, String)> = last_named_facts(tx, PROFILE, &KINDS)?
        .into_iter()
        .map(|f| (f.id, (f.kind, f.cause)))
        .collect();
    let of_cli = profiles
        .iter()
        .filter(|(_, agent)| exec_harness(&report.cli) == Some(*agent))
        .map(|(name, _)| (*name, last.get(*name)));
    if let Some(cause) = report.not_proved_cause(run.id, run.subject) {
        let healthy = of_cli
            .filter(|(_, l)| l.is_none_or(|(kind, _)| kind == EventKind::ProfileHealthy.as_str()));
        for (name, _) in healthy {
            hold(tx, guard, name, &cause)?;
        }
        return Ok(Vec::new());
    }
    let failed = report.failure().is_some();
    let of_cli: Vec<(String, Health)> = of_cli
        .map(|(name, l)| {
            let now = match l {
                Some((kind, reason)) if kind == EventKind::ProfileUnhealthy.as_str() => {
                    Health::Unhealthy {
                        reason: reason.clone(),
                    }
                }
                Some((kind, reason)) if kind == NOT_PROVED.as_str() && !failed => {
                    Health::Unhealthy {
                        reason: reason.clone(),
                    }
                }
                _ => Health::Healthy,
            };
            (name.to_string(), now)
        })
        .collect();
    let passed = format!("conformance of {} {} passed", report.cli, report.version);
    let marks = health::after_conformance(report, &of_cli);
    for mark in &marks {
        let cause = match mark {
            Mark::Unhealthy(m) => m.reason.as_str(),
            Mark::Healthy(_) => passed.as_str(),
        };
        write(tx, guard, Some(mark), cause)?;
    }
    Ok(marks)
}

/// Mark each profile of `profiles` whose agent is the harness of the CLI
/// `cli` unhealthy with `reason`, in `tx`: a conformance run of `cli` that
/// gave no report that can be read fails closed (FM-19).
pub fn mark_cli_unhealthy(
    tx: &Connection,
    guard: Option<Generation>,
    cli: &str,
    reason: &str,
    profiles: &[(&str, HarnessKind)],
) -> Result<Vec<Mark>> {
    let mut marks = Vec::new();
    for (name, _) in profiles
        .iter()
        .filter(|(_, agent)| exec_harness(cli) == Some(*agent))
    {
        marks.extend(mark_unhealthy(tx, guard, name, reason)?);
    }
    Ok(marks)
}

fn hold(tx: &Connection, guard: Option<Generation>, profile: &str, cause: &str) -> Result<()> {
    let data = json!({ "profile_id": profile, "hold": "not proved" }).to_string();
    append_named_fact(
        tx,
        guard,
        &NamedFact {
            kind: NOT_PROVED,
            about: PROFILE,
            id: profile,
            cause,
            actor: Actor::Horch,
            data_json: &data,
        },
    )
}

/// The event of `mark`, with `cause`, in `tx`.
fn write(
    tx: &Connection,
    guard: Option<Generation>,
    mark: Option<&Mark>,
    cause: &str,
) -> Result<()> {
    let (kind, id, data) = match mark {
        None => return Ok(()),
        Some(Mark::Unhealthy(m)) => (
            EventKind::ProfileUnhealthy,
            &m.profile_id,
            json!({ "profile_id": m.profile_id, "reason": m.reason }),
        ),
        Some(Mark::Healthy(m)) => (
            EventKind::ProfileHealthy,
            &m.profile_id,
            json!({ "profile_id": m.profile_id }),
        ),
    };
    let data = data.to_string();
    append_named_fact(
        tx,
        guard,
        &NamedFact {
            kind,
            about: PROFILE,
            id,
            cause,
            actor: Actor::Horch,
            data_json: &data,
        },
    )
}
