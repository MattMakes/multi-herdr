//! Candidate observation (B3, CMP-07, CMP-10, SEC-03).
//!
//! `classify` turns what the execution store and the workspace say about
//! one candidate into what the coordinator records:
//!
//! | Observation | Outcome |
//! |---|---|
//! | the record is `Done` | completed |
//! | the pane is gone while the record is live | Failed(PaneVanished) |
//! | the agent exited (`Failed(AgentExited)`) | Failed(AgentExited) |
//! | the pane shows a trust dialog (first 300 s) | close the pane → Failed(Cancelled{trust_dialog}) |
//! | idle without `horch done` (herdr `agent_status`) | one nudge; still idle after the end period → close the pane → Failed(Cancelled{idle_without_done}) |
//! | the deadline passed | close the pane → Failed(TimedOut) |
//! | the budget ran out | close the pane → Failed(Cancelled{budget}) |
//!
//! The deadline and the budget are the coordinator's decisions; this module
//! gives the facts for them. [`TelemetryUsage`] reads each candidate
//! session's transcript through the telemetry readers, and [`UsageRecord`]
//! is what the dataset keeps of it: a reference and a digest, never the
//! transcript bytes unless `retain_transcripts` asks (SEC-03).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::competition::budget::{UsageMeter, UsageSource};
use crate::execution::{Execution, ExecutionStatus, FailureKind};
use crate::fsx;
use crate::ids::{ExecutionId, ExperimentId, RoundId};
use crate::measure::digest::{sha256_bytes, Digest};
use crate::measure::paths::DatasetPaths;
use crate::usage::money::MicroUsd;
use crate::usage::{read_session, Locations, Tokens, Usage};

/// The reason a budget cancel records.
pub(crate) const CANCELLED_BUDGET: &str = "budget";
/// The reason a candidate that disk pressure kept from launching records.
pub(crate) const CANCELLED_DISK: &str = "disk";
/// The reason a candidate stopped at its harness's trust dialog records.
pub(crate) const CANCELLED_TRUST: &str = "trust_dialog";
/// The reason a candidate that stayed idle after its nudge records.
pub(crate) const CANCELLED_IDLE: &str = "idle_without_done";

/// The one line the coordinator types into a candidate that waits at its
/// prompt without `horch done` (dataset design 4.11.5).
pub(crate) const IDLE_NUDGE: &str = "If you are finished, run: horch done \"<one-paragraph summary of what you changed>\". Do not commit: the coordinator commits your work. If you are not finished, continue the task.";

/// What the coordinator knows about one candidate's idle time. Kept in
/// memory only: a resumed coordinator starts the count again.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct IdleWatch {
    /// When the current idle stretch began; `None` while the agent works.
    pub since: Option<DateTime<Utc>>,
    /// The candidate got its one nudge.
    pub nudged: bool,
}

/// What the idle rule says to do now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IdleAction {
    Wait,
    Nudge,
    End,
}

impl IdleWatch {
    /// Take one look: `idle` is herdr's `agent_status == "idle"`. A
    /// candidate idle for `nudge_after_s` gets the nudge; once nudged, an
    /// idle stretch of `end_after_s` ends it. Work in between restarts the
    /// stretch, but never earns a second nudge. `nudge_after_s == 0` turns
    /// the rule off.
    pub(crate) fn look(
        &mut self,
        idle: bool,
        now: DateTime<Utc>,
        nudge_after_s: u64,
        end_after_s: u64,
    ) -> IdleAction {
        if nudge_after_s == 0 {
            return IdleAction::Wait;
        }
        if !idle {
            self.since = None;
            return IdleAction::Wait;
        }
        let since = *self.since.get_or_insert(now);
        if !self.nudged && deadline_passed(since, now, nudge_after_s) {
            self.nudged = true;
            // The end period counts from the nudge.
            self.since = Some(now);
            return IdleAction::Nudge;
        }
        if self.nudged && deadline_passed(since, now, end_after_s) {
            return IdleAction::End;
        }
        IdleAction::Wait
    }
}

/// How long after its spawn a candidate's pane is read for a trust dialog.
/// The dialog is the first screen; later, the agent may print the same
/// words while it works.
pub(crate) const TRUST_DIALOG_WINDOW_S: u64 = 300;

/// The trust dialog texts, per harness, normalized (see [`normalize_screen`]).
/// Each one is a whole line of the dialog, so a wrapped pane still has it.
/// Seen on claude 2.1.289 and codex-cli 0.160.0; agy's from its teammate
/// note. PRE-14 refuses an untrusted repository before this can happen.
pub(crate) const TRUST_DIALOGS: &[(&str, &str)] = &[
    ("claude", "is this a project you created or one you trust"),
    ("claude", "yes, i trust this folder"),
    ("codex", "trusting will apply to the repository root"),
    (
        "codex",
        "trust this folder? codex can read, edit, and run files here",
    ),
    ("antigravity", "do you trust the contents of this project"),
];

/// Lower case, every run of characters other than letters, digits and
/// `, ? '` as one space: box borders and line wraps do not split a phrase.
fn normalize_screen(screen: &str) -> String {
    let mut out = String::with_capacity(screen.len());
    let mut space = true;
    for c in screen.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() || matches!(c, ',' | '?' | '\'') {
            out.push(c);
            space = false;
        } else if !space {
            out.push(' ');
            space = true;
        }
    }
    out
}

/// The harness whose trust dialog `screen` shows, if any.
pub(crate) fn trust_dialog(screen: &str) -> Option<&'static str> {
    let screen = normalize_screen(screen);
    TRUST_DIALOGS
        .iter()
        .find(|(_, text)| screen.contains(text))
        .map(|(harness, _)| *harness)
}

/// What one look at a candidate found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Observed {
    /// The agent may still be working.
    Live,
    Completed {
        exit_code: Option<i32>,
    },
    Failed(FailureKind),
}

/// Classify a candidate's execution. `pane_alive` is `None` when the record
/// has no pane to look at. A record that is not live yet (`Planned`) is
/// `Live`: its spawn is in flight.
pub(crate) fn classify(record: &Execution, pane_alive: Option<bool>) -> Observed {
    match &record.status {
        ExecutionStatus::Done => Observed::Completed {
            exit_code: record.exit_code,
        },
        ExecutionStatus::Failed { failure } => Observed::Failed(failure.clone()),
        // The launch never ran the agent.
        ExecutionStatus::LaunchFailed { .. } => Observed::Failed(FailureKind::Crashed),
        ExecutionStatus::Starting | ExecutionStatus::Running if pane_alive == Some(false) => {
            Observed::Failed(FailureKind::PaneVanished)
        }
        _ => Observed::Live,
    }
}

/// Whether a candidate spawned at `spawned_at` is past its deadline.
pub(crate) fn deadline_passed(
    spawned_at: DateTime<Utc>,
    now: DateTime<Utc>,
    deadline_s: u64,
) -> bool {
    let deadline = i64::try_from(deadline_s).unwrap_or(i64::MAX);
    now.signed_duration_since(spawned_at).num_seconds() >= deadline
}

/// Candidate usage from the harness transcripts, through the same readers
/// as `horch cost` (TEL-10).
pub struct TelemetryUsage {
    pub locations: Locations,
}

impl UsageSource for TelemetryUsage {
    fn usage(&self, harness: &str, session_id: Option<&str>) -> Option<Usage> {
        read_session(&self.locations, harness, session_id)
            .ok()
            .map(|(_, usage)| usage)
    }
}

impl TelemetryUsage {
    /// The main transcript file of a session, when one is found.
    pub fn transcript(&self, harness: &str, session_id: Option<&str>) -> Option<PathBuf> {
        read_session(&self.locations, harness, session_id)
            .ok()
            .map(|(path, _)| path)
    }
}

/// What the dataset keeps of one candidate's usage:
/// `experiments/<exp>/artifacts/<round>/usage/<label>.json`. Export reads it
/// back for the WorkerRun facts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageRecord {
    pub execution_id: ExecutionId,
    pub label: String,
    pub tokens: Tokens,
    pub cost_microusd: MicroUsd,
    /// `CostSource` as text: `price_table@<date>` or `unpriced`.
    pub cost_source: String,
    /// The transcript's path on this machine. A reference, not a copy.
    pub transcript_ref: Option<String>,
    pub transcript_digest: Option<Digest>,
    /// The copy under the dataset dir, only with `retain_transcripts`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retained: Option<String>,
}

/// The usage directory of a round.
pub(crate) fn usage_dir(
    paths: &DatasetPaths,
    exp: &ExperimentId,
    round: &RoundId,
) -> Result<PathBuf> {
    Ok(paths.artifacts_dir(exp, round)?.join("usage"))
}

/// Build the usage record of one finished candidate, and copy its transcript
/// into the dataset only when `retain` is set.
#[allow(clippy::too_many_arguments)]
pub(crate) fn usage_record(
    source: &TelemetryUsage,
    meter: &UsageMeter,
    paths: &DatasetPaths,
    exp: &ExperimentId,
    round: &RoundId,
    record: &Execution,
    label: &str,
    retain: bool,
) -> Result<UsageRecord> {
    let session = session_of(record);
    let harness = record.harness.as_str();
    let usage = source
        .usage(harness, session.as_deref())
        .unwrap_or_default();
    let (cost, cost_source) = meter.price(&usage);
    let transcript = source.transcript(harness, session.as_deref());
    let bytes = transcript.as_ref().and_then(|p| std::fs::read(p).ok());
    let mut retained = None;
    if let (true, Some(path), Some(bytes)) = (retain, &transcript, &bytes) {
        let dir = paths.artifacts_dir(exp, round)?.join("transcripts");
        ensure_dirs(paths, &dir)?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "transcript".into());
        let dest = dir.join(format!("{label}-{name}"));
        fsx::write_atomic(&dest, bytes, 0o600)
            .with_context(|| format!("writing {}", dest.display()))?;
        retained = Some(dest.to_string_lossy().into_owned());
    }
    Ok(UsageRecord {
        execution_id: record.id.clone(),
        label: label.to_string(),
        tokens: usage.tokens(),
        cost_microusd: cost,
        cost_source: cost_source.to_string(),
        transcript_ref: transcript.map(|p| p.to_string_lossy().into_owned()),
        transcript_digest: bytes.as_deref().map(sha256_bytes),
        retained,
    })
}

/// Write `record` under the round's usage dir (0600). A restart rewrites
/// it with the same facts.
pub(crate) fn write_usage_record(
    paths: &DatasetPaths,
    exp: &ExperimentId,
    round: &RoundId,
    record: &UsageRecord,
) -> Result<()> {
    let dir = usage_dir(paths, exp, round)?;
    ensure_dirs(paths, &dir)?;
    let file = dir.join(format!(
        "{}.json",
        crate::measure::paths::component("label", &record.label)?
    ));
    let json = serde_json::to_vec_pretty(record)?;
    fsx::write_atomic(&file, &json, 0o600).with_context(|| format!("writing {}", file.display()))
}

/// Every usage record in the dataset, by execution id. Unreadable files are
/// skipped: export reports such a run as unpriced.
pub fn load_usage_records(paths: &DatasetPaths) -> BTreeMap<String, UsageRecord> {
    let mut out = BTreeMap::new();
    let dirs = |p: &Path| -> Vec<PathBuf> {
        std::fs::read_dir(p)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .collect()
    };
    for exp in dirs(&paths.experiments_dir()) {
        for round in dirs(&exp.join("artifacts")) {
            for file in dirs(&round.join("usage")) {
                let record = std::fs::read(&file)
                    .ok()
                    .and_then(|b| serde_json::from_slice::<UsageRecord>(&b).ok());
                if let Some(r) = record {
                    out.insert(r.execution_id.to_string(), r);
                }
            }
        }
    }
    out
}

/// The session id of a record, when it is known.
pub(crate) fn session_of(record: &Execution) -> Option<String> {
    match &record.session {
        crate::execution::SessionState::Known(id) => Some(id.to_string()),
        _ => None,
    }
}

/// Create `dir` and every missing level below the dataset root, 0700.
pub fn ensure_dirs(paths: &DatasetPaths, dir: &Path) -> Result<()> {
    let mut missing = Vec::new();
    let mut at = dir;
    while !at.exists() && at.starts_with(paths.root()) {
        missing.push(at.to_path_buf());
        match at.parent() {
            Some(p) => at = p,
            None => break,
        }
    }
    for d in missing.iter().rev() {
        fsx::ensure_private_dir(d).with_context(|| format!("creating {}", d.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::LaunchStage;

    fn record(status: ExecutionStatus) -> Execution {
        let json = serde_json::json!({
            "record_id": "e-1", "agent": "claude", "tier": "sonnet", "model": "m",
            "role": "candidate-A", "status": "working", "task": "t", "history": [],
            "created_at": "2026-10-02T00:00:00Z", "updated_at": "2026-10-02T00:00:00Z",
            "kind": "worker", "experiment_id": "x", "round_id": "r", "label": "A",
        });
        let r: crate::execution::legacy::LedgerRecordV1 = serde_json::from_value(json).unwrap();
        let mut e = crate::execution::store::to_execution(&r).unwrap();
        e.status = status;
        e
    }

    #[test]
    fn classify_follows_the_observation_table() {
        let done = record(ExecutionStatus::Done);
        assert_eq!(
            classify(&done, Some(true)),
            Observed::Completed { exit_code: None }
        );
        let running = record(ExecutionStatus::Running);
        assert_eq!(classify(&running, Some(true)), Observed::Live);
        assert_eq!(
            classify(&running, Some(false)),
            Observed::Failed(FailureKind::PaneVanished)
        );
        let exited = record(ExecutionStatus::Failed {
            failure: FailureKind::AgentExited { code: Some(3) },
        });
        assert_eq!(
            classify(&exited, Some(true)),
            Observed::Failed(FailureKind::AgentExited { code: Some(3) })
        );
        let launch = record(ExecutionStatus::LaunchFailed {
            stage: LaunchStage::Run,
            reason: "x".into(),
        });
        assert_eq!(
            classify(&launch, None),
            Observed::Failed(FailureKind::Crashed)
        );
        // A planned record has no pane yet; it is not vanished.
        assert_eq!(
            classify(&record(ExecutionStatus::Planned), Some(false)),
            Observed::Live
        );
    }

    #[test]
    fn deadline_counts_whole_seconds() {
        let t0 = DateTime::parse_from_rfc3339("2026-10-02T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(!deadline_passed(t0, t0 + chrono::Duration::seconds(9), 10));
        assert!(deadline_passed(t0, t0 + chrono::Duration::seconds(10), 10));
    }

    /// The dialog texts as panes showed them in LA-7 (L1), wrapped and
    /// boxed, match; ordinary agent output does not.
    #[test]
    fn trust_dialog_matches_the_harness_dialogs() {
        let claude = "│ Accessing workspace:\n│ /x/wt/A\n│ Quick safety check: Is this a project you created or one you\n│ trust? (Like your own code)\n│ ❯ 1. Yes, I trust this folder\n│   2. No, exit\n";
        assert_eq!(trust_dialog(claude), Some("claude"));
        let codex = "  Note: You\u{2019}re in a subdirectory of a Git project. Trusting will apply to the\n  repository root: /x/proj\n\n  Trust this folder? Codex can read, edit, and run files here,\n  › 1. Trust and continue\n    2. Quit\n";
        assert_eq!(trust_dialog(codex), Some("codex"));
        assert_eq!(
            trust_dialog("Do you trust the contents of this project?"),
            Some("antigravity")
        );
        for quiet in [
            "",
            "running cargo test\nok",
            "I trust this plan",
            "trust_level = \"trusted\"",
        ] {
            assert_eq!(trust_dialog(quiet), None, "{quiet:?}");
        }
        // Every dialog text is already normalized, so it can match.
        for (_, text) in TRUST_DIALOGS {
            assert_eq!(normalize_screen(text), *text);
        }
    }

    /// The idle rule: one nudge after the nudge period, an end after the
    /// end period counted from the nudge, work restarts the stretch, and 0
    /// turns it off.
    #[test]
    fn idle_watch_nudges_once_then_ends() {
        let t0 = crate::clock::parse("2026-10-04T12:00:00Z").unwrap();
        let at = |s: i64| t0 + chrono::Duration::seconds(s);
        let mut w = IdleWatch::default();
        assert_eq!(w.look(true, at(0), 120, 180), IdleAction::Wait);
        assert_eq!(w.look(true, at(119), 120, 180), IdleAction::Wait);
        assert_eq!(w.look(true, at(120), 120, 180), IdleAction::Nudge);
        // It works for a while, then waits at the prompt again.
        assert_eq!(w.look(false, at(150), 120, 180), IdleAction::Wait);
        assert_eq!(w.look(true, at(200), 120, 180), IdleAction::Wait);
        assert_eq!(w.look(true, at(379), 120, 180), IdleAction::Wait);
        assert_eq!(w.look(true, at(380), 120, 180), IdleAction::End);

        // A working agent is never nudged.
        let mut busy = IdleWatch::default();
        for s in (0..1000).step_by(60) {
            assert_eq!(busy.look(false, at(s), 120, 180), IdleAction::Wait);
        }
        // Off.
        let mut off = IdleWatch::default();
        assert_eq!(off.look(true, at(0), 0, 180), IdleAction::Wait);
        assert_eq!(off.look(true, at(10_000), 0, 180), IdleAction::Wait);
    }
}
