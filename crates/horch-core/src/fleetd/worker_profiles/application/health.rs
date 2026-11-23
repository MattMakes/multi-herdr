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

pub struct ConformanceRun<'a> {
    pub id: i64,
    pub subject: &'a str,
}

pub fn unhealthy_profiles(conn: &Connection) -> Result<BTreeMap<String, String>> {
    last_of_kind(conn, EventKind::ProfileUnhealthy)
}

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

pub fn health(conn: &Connection, profile: &str) -> Result<Health> {
    Ok(match unhealthy_profiles(conn)?.remove(profile) {
        Some(reason) => Health::Unhealthy { reason },
        None => Health::Healthy,
    })
}

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
