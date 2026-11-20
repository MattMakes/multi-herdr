//! Mark a worker profile unhealthy or healthy, and read its health
//! (`docs/specs/fleetd/design.md`, "Worker profiles and adapters" (The
//! health of a profile); fleetd-p7 MAP.md Q10 and Q15).
//!
//! The health of a profile is its last `profile_unhealthy` or
//! `profile_healthy` event, with the contract of the mark in `data_json`.
//! No event means healthy. Each mark is 1 event row in the caller's
//! transaction, and a mark that does not change the health writes none.

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

const KINDS: [EventKind; 2] = [EventKind::ProfileUnhealthy, EventKind::ProfileHealthy];

/// Each unhealthy profile, with the cause of its mark.
pub fn unhealthy_profiles(conn: &Connection) -> Result<BTreeMap<String, String>> {
    Ok(last_named_facts(conn, PROFILE, &KINDS)?
        .into_iter()
        .filter(|f| f.kind == EventKind::ProfileUnhealthy.as_str())
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
pub fn mark_after_conformance(
    tx: &Connection,
    guard: Option<Generation>,
    report: &ConformanceReport,
    profiles: &[(&str, HarnessKind)],
) -> Result<Vec<Mark>> {
    let unhealthy = unhealthy_profiles(tx)?;
    let of_cli: Vec<(String, Health)> = profiles
        .iter()
        .filter(|(_, agent)| exec_harness(&report.cli) == Some(*agent))
        .map(|(name, _)| {
            let now = match unhealthy.get(*name) {
                Some(reason) => Health::Unhealthy {
                    reason: reason.clone(),
                },
                None => Health::Healthy,
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
