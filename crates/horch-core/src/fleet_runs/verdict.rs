//! The orchestrator's verdict on a record (fleet-dataset §3.3, §5).
//!
//! [`record_verdict`] resolves a role or a record id, appends a
//! `mh.fleet-verdict/1.0.0` row and a `verdict` history event to the ledger
//! record, so `horch sessions` shows it. A refusal ([`VerdictError`]) writes
//! nothing; `horch verdict` maps it to exit code 2.

use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};

use crate::execution::legacy::LedgerRecordV1;
use crate::execution::records::Ledger;
use crate::fleet_runs::rows::{VerdictRow, VERDICT_SCHEMA};
use crate::fleet_runs::store::{self, FleetFile};
use crate::measure::paths::DatasetPaths;

/// The history event a verdict appends.
pub const VERDICT_EVENT: &str = "verdict";

/// The orchestrator's check of a DONE.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The work passed the check as delivered.
    Accepted,
    /// The work needed a fix.
    Rework,
    /// The work was discarded.
    Rejected,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Accepted => "accepted",
            Verdict::Rework => "rework",
            Verdict::Rejected => "rejected",
        }
    }
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Verdict {
    type Err = VerdictError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "accepted" => Ok(Verdict::Accepted),
            "rework" => Ok(Verdict::Rework),
            "rejected" => Ok(Verdict::Rejected),
            other => Err(VerdictError::UnknownVerdict(other.to_string())),
        }
    }
}

/// Why [`record_verdict`] wrote nothing.
#[derive(Debug)]
pub enum VerdictError {
    /// No record has this role or id. A refusal.
    UnknownRecord(String),
    /// The key names an orchestrator record. A refusal.
    OrchestratorRecord(String),
    /// The word is not `accepted`, `rework` or `rejected`. A refusal.
    UnknownVerdict(String),
    /// The ledger or the fleet store failed.
    Failed(anyhow::Error),
}

impl VerdictError {
    /// Whether the caller asked for something wrong (exit 2), not a failure.
    pub fn is_refusal(&self) -> bool {
        !matches!(self, VerdictError::Failed(_))
    }
}

impl fmt::Display for VerdictError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VerdictError::UnknownRecord(key) => {
                write!(f, "no record has the role or record id '{key}'")
            }
            VerdictError::OrchestratorRecord(id) => {
                write!(
                    f,
                    "record {id} is an orchestrator; a verdict judges a worker"
                )
            }
            VerdictError::UnknownVerdict(word) => write!(
                f,
                "'{word}' is not a verdict: use accepted, rework or rejected"
            ),
            VerdictError::Failed(e) => write!(f, "{e:#}"),
        }
    }
}

impl std::error::Error for VerdictError {}

/// The record `key` names: a record id (or session id) names itself; a
/// role names the newest record with that role, in any state.
pub fn resolve<'a>(records: &'a [LedgerRecordV1], key: &str) -> Option<&'a LedgerRecordV1> {
    records.iter().rfind(|r| r.matches(key)).or_else(|| {
        records
            .iter()
            .enumerate()
            .filter(|(_, r)| r.role == key)
            .max_by(|(ia, a), (ib, b)| a.created_at.cmp(&b.created_at).then(ia.cmp(ib)))
            .map(|(_, r)| r)
    })
}

/// Record `verdict` on the record `key` names. `by` is the orchestrator's
/// record id, when known.
pub fn record_verdict(
    ledger: &Ledger,
    paths: &DatasetPaths,
    key: &str,
    verdict: &str,
    note: Option<&str>,
    by: Option<&str>,
    now: DateTime<Utc>,
) -> Result<VerdictRow, VerdictError> {
    let verdict: Verdict = verdict.parse()?;
    let records = ledger.read().map_err(VerdictError::Failed)?;
    let record = resolve(&records, key).ok_or_else(|| VerdictError::UnknownRecord(key.into()))?;
    if record.is_orchestrator() {
        return Err(VerdictError::OrchestratorRecord(record.record_id.clone()));
    }
    let note = note.map(str::trim).filter(|n| !n.is_empty());
    let row = VerdictRow {
        schema: VERDICT_SCHEMA.to_string(),
        record_id: record.record_id.clone(),
        role: record.role.clone(),
        verdict: verdict.to_string(),
        note: note.map(str::to_owned),
        at: crate::clock::stamp(now),
        by: by.map(str::to_owned),
    };
    store::append(paths, FleetFile::Verdicts, &row).map_err(VerdictError::Failed)?;
    let text = match note {
        Some(n) => format!("{verdict}: {n}"),
        None => verdict.to_string(),
    };
    ledger
        .record_event(&record.record_id, VERDICT_EVENT, &text)
        .map_err(VerdictError::Failed)?;
    Ok(row)
}
