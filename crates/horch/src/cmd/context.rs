//! `horch context`: the current context of every live session of the
//! project, its threshold and its state (CTX-01, CTX-04, CTX-06, CTX-09).
//!
//! The rules live in `horch_core::compaction` (design §6.7, review finding
//! 8). This module gathers the inputs of 1 row ([`build_row`]), calls the
//! rules and prints. `horch compact` and the `horch note` check build their
//! rows with the same function. Text mode makes no herdr call; `--json`
//! (CTX-24) makes 1 `herdr pane list` call per workspace of its rows.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::Result;
use chrono::Utc;
use horch_core::compaction::jobfile::{self, LOG_FILE};
use horch_core::compaction::policy::{self, JobLiveness, RowInputs, RowState, EVENT_FAILED};
use horch_core::compaction::window::{self, WindowDecision, WindowSource, HEADROOM_FLOOR};
use horch_core::execution::legacy::Record;
use horch_core::execution::records::Ledger;
use horch_core::harness::codex::PaneMode;
use horch_core::harness::{launch, HarnessKind};
use horch_core::roster::{Roster, Teammate};
use horch_core::routing::decision::merge;
use horch_core::runtime::RuntimeContext;
use horch_core::telemetry::context::{read_current, Reading};
use horch_core::telemetry::readers::Unreadable;
use horch_core::usage::Locations;
use horch_core::workspace::client::WorkspaceClient;
use horch_core::workspace::herdr::Herdr;
use horch_core::workspace::model::Pane;
use serde::Serialize;

/// What `--over` prints when no row is over, requested or lost.
const NONE_OVER: &str = "no session is over its threshold";

/// Everything 1 row is built from. Tests fill it from a temp state dir, a
/// temp ledger and a fixture home.
pub(crate) struct Sources<'a> {
    pub ctx: &'a RuntimeContext,
    pub ledger: &'a Ledger,
    pub roster: &'a Roster,
    pub loc: &'a Locations,
    /// `<state_root>/context`: the marker cache.
    pub cache_dir: &'a Path,
}

/// The marker cache dir of `ctx` (design §6.6).
pub(crate) fn cache_dir(ctx: &RuntimeContext) -> PathBuf {
    ctx.paths.state_root.join("context")
}

/// 1 row: a record, what its transcript says, and its state.
pub(crate) struct Row {
    pub record: Record,
    pub kind: HarnessKind,
    /// The transcript's model, else the record's.
    pub model: String,
    pub reading: Result<Reading, Unreadable>,
    /// The model window: the transcript's, else `model_window`.
    pub window: Option<u64>,
    pub native: Option<u64>,
    pub decision: WindowDecision,
    /// The record has no decision; `decision` was resolved now.
    pub decided_now: bool,
    pub threshold: u64,
    pub job: JobLiveness,
    pub job_dir: PathBuf,
    pub state: RowState,
    /// The mode of a Codex pane with `permission_mode: plan` (X6); None for
    /// every other row.
    pub mode: Option<PaneMode>,
}

impl Row {
    pub fn inputs(&self) -> RowInputs<'_> {
        RowInputs {
            reading: self.reading.as_ref(),
            threshold: self.threshold,
            history: &self.record.history,
            job: &self.job,
        }
    }

    pub fn log_path(&self) -> PathBuf {
        self.job_dir.join(LOG_FILE)
    }

    /// The `reasons:` line of a row that has no number (CTX-04).
    pub fn reason(&self) -> Option<String> {
        let text = self.reason_text()?;
        Some(format!(
            "{}: {}: {text}",
            self.record.role,
            self.state.as_str()
        ))
    }

    /// The reason of [`Row::reason`] without the role and the state.
    fn reason_text(&self) -> Option<String> {
        Some(match (&self.state, &self.job, &self.reading) {
            (RowState::CompactLost, JobLiveness::Lost { step }, _) => format!(
                "the compaction job is lost at step {step}; log {}",
                self.log_path().display()
            ),
            (
                RowState::NoSession
                | RowState::NotRead
                | RowState::NoTranscript
                | RowState::Unknown,
                _,
                Err(why),
            ) => why.to_string(),
            (RowState::Unknown, _, Ok(_)) => "the transcript has no usable response".into(),
            _ => return None,
        })
    }

    /// The token count, when the reading has one.
    pub fn tokens(&self) -> Option<u64> {
        self.reading.as_ref().ok().and_then(|r| r.tokens)
    }

    /// `(native, horch)` compactions of the transcript (CTX-25), when it
    /// reads.
    pub fn compactions(&self) -> Option<(usize, usize)> {
        let reading = self.reading.as_ref().ok()?;
        Some(policy::count_compactions(
            &reading.marks_at,
            &self.record.history,
        ))
    }
}

/// The directory a record's agent runs in: its `workdir`, else the project.
pub(crate) fn record_workdir(ctx: &RuntimeContext, record: &Record) -> PathBuf {
    record
        .workdir
        .as_deref()
        .map(PathBuf::from)
        .or_else(|| ctx.paths.project().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// The job dir of a record: its workspace, else `workspace` (the caller's).
pub(crate) fn record_job_dir(
    ctx: &RuntimeContext,
    record: &Record,
    workspace: Option<&str>,
) -> PathBuf {
    let ws = record
        .workspace_id
        .as_deref()
        .or(workspace)
        .unwrap_or("none");
    jobfile::job_dir(&ctx.paths.state_root, ws, &record.role)
}

/// The harness of a record; an agent name this binary does not know reads
/// as `none` (no reader, no command).
pub(crate) fn record_kind(record: &Record) -> HarnessKind {
    record.agent.parse().unwrap_or(HarnessKind::None)
}

/// Build the row of `record`: reading, window decision, native trigger,
/// threshold, job liveness and state. For a lost job it claims the loss;
/// the 1 caller that claims it records the `compact-failed` event (design
/// §6.8a). No herdr call.
pub(crate) fn build_row(src: &Sources, record: Record, workspace: Option<&str>) -> Row {
    let kind = record_kind(&record);
    let caps = kind.capabilities().compaction;
    let reading = read_current(
        src.loc,
        src.cache_dir,
        &record.agent,
        record.session_id.as_deref(),
    );
    let model = reading
        .as_ref()
        .ok()
        .and_then(|r| r.model.clone())
        .unwrap_or_else(|| record.model.clone());

    let recorded = record
        .compact_window
        .as_ref()
        .and_then(WindowDecision::from_ledger);
    let decided_now = recorded.is_none();
    let decision = recorded.unwrap_or_else(|| decide_now(src, &record));

    let transcript_window = reading.as_ref().ok().and_then(|r| r.window);
    let window =
        transcript_window.or_else(|| window::model_window(kind.as_str(), &model, caps.trigger));
    let native = window::native_trigger(
        caps.trigger,
        kind.as_str(),
        &model,
        decision.tokens,
        transcript_window,
    );
    // CTX-21: the `compact_at` of the teammate that ran. `horch context`,
    // `horch compact` and the `horch note` check all build the row here.
    let compact_at = launched_teammate(src, &record).and_then(|t| t.compact_at);
    let threshold = window::threshold(policy::watch_base(compact_at), native);

    let job_dir = record_job_dir(src.ctx, &record, workspace);
    let job = jobfile::liveness(&job_dir, Utc::now());
    if let JobLiveness::Lost { step } = &job {
        if let Ok(Some(_)) = jobfile::claim_lost(&job_dir) {
            let text = format!(
                "step {step}: the job process is lost; log {}",
                job_dir.join(LOG_FILE).display()
            );
            // Best effort: the row shows the loss whatever the ledger says.
            let _ = src
                .ledger
                .record_event(&record.record_id, EVENT_FAILED, &text);
        }
    }

    let mode = super::mode::is_plan_pane(src.roster, &record)
        .then(|| super::mode::current_mode(&record.history));
    let mut row = Row {
        mode,
        record,
        kind,
        model,
        reading,
        window,
        native,
        decision,
        decided_now,
        threshold,
        job,
        job_dir,
        state: RowState::Ok,
    };
    row.state = policy::classify(&row.inputs());
    row
}

/// The teammate whose settings the launch of `record` ran with. After a
/// substitution that is the requested teammate merged with the fallback
/// (`routing.resolved`, or `via` in a record from before routing
/// provenance), as the spawn gate built it: the requested name, the
/// fallback's harness, model, `compact_window` and `compact_at`.
fn launched_teammate(src: &Sources, record: &Record) -> Option<Teammate> {
    let fallback = record
        .routing
        .as_ref()
        .map(|r| r.resolved.as_str())
        .filter(|name| *name != record.tier)
        .or(record.via.as_deref())
        .and_then(|name| src.roster.get(name));
    match (src.roster.get(&record.tier), fallback) {
        (Some(requested), Some(fallback)) => Some(merge(requested, fallback)),
        (requested, fallback) => fallback.or(requested).cloned(),
    }
}

/// The window decision a launch of `record` would make now.
fn decide_now(src: &Sources, record: &Record) -> WindowDecision {
    match launched_teammate(src, record) {
        Some(t) => launch::window_decision(
            src.ctx,
            &t,
            &record.model,
            &record_workdir(src.ctx, record),
            src.roster.fleet_window(&t, &record.model),
        ),
        None => window::decide(None, None),
    }
}

/// The records `horch context` shows: live orchestrators, newest first, then
/// 1 live record per worker role (the newest `updated_at`), by role.
pub(crate) fn shown_records(records: &[Record]) -> Vec<Record> {
    let live = || records.iter().filter(|r| r.execution_status().is_live());
    let mut orchestrators: Vec<&Record> = live().filter(|r| r.is_orchestrator()).collect();
    orchestrators.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    let mut workers: std::collections::BTreeMap<&str, &Record> = Default::default();
    for r in live().filter(|r| !r.is_orchestrator()) {
        let slot = workers.entry(r.role.as_str()).or_insert(r);
        if r.updated_at > slot.updated_at {
            *slot = r;
        }
    }
    orchestrators
        .into_iter()
        .chain(workers.into_values())
        .cloned()
        .collect()
}

/// `horch context [--over] [--windows] [--json]`.
pub fn context(ctx: &RuntimeContext, over: bool, windows: bool, json: bool) -> Result<ExitCode> {
    let roster = match super::load_roster(ctx, None) {
        Ok(r) => r,
        Err(_) => Roster::builtin()?,
    };
    if windows {
        let workdir = ctx.paths.current_dir()?;
        crate::output::print(&windows_table(ctx, &roster, &workdir));
        return Ok(ExitCode::SUCCESS);
    }
    let ledger = Ledger::open_in(ctx)?;
    let loc = Locations::from_context(ctx);
    let cache = cache_dir(ctx);
    let src = Sources {
        ctx,
        ledger: &ledger,
        roster: &roster,
        loc: &loc,
        cache_dir: &cache,
    };
    let herdr = Herdr::with_bin(&ctx.bins.harness.herdr);
    let out = if json {
        context_json(&src, &herdr)
    } else {
        context_text(&src, over)
    };
    match out {
        Ok(text) => {
            crate::output::print(&text);
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => {
            eprintln!("horch context: {e:#}");
            Ok(ExitCode::FAILURE)
        }
    }
}

/// The text of `horch context` (`--over`: only rows in `over`, `requested`
/// or `compact-lost`). Err only when the ledger does not read.
pub(crate) fn context_text(src: &Sources, over: bool) -> Result<String> {
    let rows: Vec<Row> = shown_rows(src)?
        .into_iter()
        .filter(|row| {
            !over
                || matches!(
                    row.state,
                    RowState::Over | RowState::Requested | RowState::CompactLost
                )
        })
        .collect();
    if over && rows.is_empty() {
        return Ok(format!("{NONE_OVER}\n"));
    }
    Ok(rows_table(&rows))
}

/// The rows of every shown record. Err only when the ledger does not read.
fn shown_rows(src: &Sources) -> Result<Vec<Row>> {
    let records = src.ledger.read()?;
    let workspace = caller_workspace(src.ctx);
    Ok(shown_records(&records)
        .into_iter()
        .map(|r| build_row(src, r, workspace.as_deref()))
        .collect())
}

/// The herdr workspace of the caller, if it runs in one.
fn caller_workspace(ctx: &RuntimeContext) -> Option<String> {
    ctx.herdr.workspace.as_ref().map(|w| w.to_string())
}

/// 1 object of `horch context --json` (CTX-24, design §6.10). The field
/// order is the key order. Every unknown value is `null`.
#[derive(Serialize)]
pub(crate) struct JsonRow {
    pub role: String,
    pub record_id: String,
    pub harness: &'static str,
    pub model: String,
    pub session_id: Option<String>,
    pub transcript: Option<PathBuf>,
    /// The record's pane, when herdr lists it in the record's workspace.
    pub pane: Option<String>,
    /// herdr's `agent_status` of that pane.
    pub pane_status: Option<String>,
    pub tokens: Option<u64>,
    /// None when the transcript does not read.
    pub provisional: Option<bool>,
    pub pending: Option<bool>,
    pub window: Option<u64>,
    pub native_trigger: Option<u64>,
    /// The window setting of the decision (Claude
    /// `CLAUDE_CODE_AUTO_COMPACT_WINDOW`, Codex
    /// `model_auto_compact_token_limit`, Prime and pi `contextWindow`).
    pub native_setting: Option<u64>,
    /// `operator`, `fleet` or `harness`.
    pub native_source: &'static str,
    pub native_detail: String,
    pub threshold: u64,
    pub last_compaction: Option<JsonCompaction>,
    /// Markers no `compacted` event matches (CTX-25); None when the
    /// transcript does not read.
    pub native_compactions: Option<usize>,
    /// Markers a `compacted` event matches within 15 minutes (CTX-25).
    pub horch_compactions: Option<usize>,
    pub state: &'static str,
    /// The text of the row's `reasons:` line, without the role and state.
    pub reason: Option<String>,
    /// `in-place` or `fresh`.
    pub route: &'static str,
    /// The handoff file, resolved against the record's workdir.
    pub handoff: PathBuf,
    /// `plan` or `write` for a Codex pane with `permission_mode: plan`.
    pub mode: Option<&'static str>,
}

/// `last_compaction` of a [`JsonRow`]: every key, `null` when unknown.
#[derive(Serialize)]
pub(crate) struct JsonCompaction {
    pub at: String,
    pub trigger: Option<String>,
    pub pre_tokens: Option<u64>,
    pub post_tokens: Option<u64>,
}

/// The `--json` array: 1 object per row, in the order of the text table.
/// Err only when the ledger does not read.
pub(crate) fn context_json(src: &Sources, ws: &dyn WorkspaceClient) -> Result<String> {
    let rows = shown_rows(src)?;
    let caller = caller_workspace(src.ctx);
    // 1 `pane list` per workspace; None: herdr did not answer.
    let mut panes: BTreeMap<String, Option<Vec<Pane>>> = BTreeMap::new();
    let out: Vec<JsonRow> = rows
        .iter()
        .map(|row| {
            let workspace = row.record.workspace_id.clone().or_else(|| caller.clone());
            let pane = match (workspace, row.record.pane_id.as_deref()) {
                (Some(w), Some(id)) => panes
                    .entry(w)
                    .or_insert_with_key(|w| ws.pane_list(w).ok())
                    .as_ref()
                    .and_then(|list| list.iter().find(|p| p.pane_id == id)),
                _ => None,
            };
            json_row(src, row, pane)
        })
        .collect();
    Ok(format!("{}\n", serde_json::to_string_pretty(&out)?))
}

/// The [`JsonRow`] of `row`; `pane` is herdr's entry for its pane.
fn json_row(src: &Sources, row: &Row, pane: Option<&Pane>) -> JsonRow {
    let reading = row.reading.as_ref().ok();
    JsonRow {
        role: row.record.role.clone(),
        record_id: row.record.record_id.clone(),
        harness: row.kind.as_str(),
        model: row.model.clone(),
        session_id: row.record.session_id.clone(),
        transcript: reading.map(|r| r.transcript.clone()),
        pane: pane.map(|p| p.pane_id.clone()),
        pane_status: pane.and_then(|p| p.agent_status.clone()),
        tokens: row.tokens(),
        provisional: reading.map(|r| r.provisional),
        pending: reading.map(|r| r.pending),
        window: row.window,
        native_trigger: row.native,
        native_setting: row.decision.tokens,
        native_source: source_name(row.decision.source),
        native_detail: row.decision.detail.clone(),
        threshold: row.threshold,
        last_compaction: reading
            .and_then(|r| r.last_compaction.as_ref())
            .map(|m| JsonCompaction {
                at: m.at.clone(),
                trigger: m.trigger.clone(),
                pre_tokens: m.pre_tokens,
                post_tokens: m.post_tokens,
            }),
        native_compactions: row.compactions().map(|c| c.0),
        horch_compactions: row.compactions().map(|c| c.1),
        state: row.state.as_str(),
        reason: row.reason_text(),
        route: if src.roster.compacts_in_place(row.kind) {
            "in-place"
        } else {
            "fresh"
        },
        handoff: record_workdir(src.ctx, &row.record).join(policy::handoff_path(&row.record.role)),
        mode: row.mode.map(PaneMode::as_str),
    }
}

/// STATE of the text table: a plan pane left in write mode gets ` [write]`,
/// so the orchestrator sees a pane to switch back (X6).
fn state_text(row: &Row) -> String {
    match row.mode {
        Some(PaneMode::Write) => format!("{} [write]", row.state.as_str()),
        _ => row.state.as_str().into(),
    }
}

/// The table of rows and its `reasons:` block.
fn rows_table(rows: &[Row]) -> String {
    let mut t = Table::new(&[
        ("ROLE", Align::Left),
        ("HARNESS", Align::Left),
        ("MODEL", Align::Left),
        ("CONTEXT", Align::Right),
        ("WINDOW", Align::Right),
        ("NATIVE", Align::Right),
        ("SOURCE", Align::Left),
        ("THRESHOLD", Align::Right),
        ("LAST COMPACT", Align::Left),
        ("COMPACTIONS", Align::Left),
        ("STATE", Align::Left),
    ]);
    for row in rows {
        let provisional = row.reading.as_ref().is_ok_and(|r| r.provisional);
        let context = match row.tokens() {
            // `*` = provisional; the space keeps the digits in 1 column.
            Some(n) => format!("{}{}", grouped(n), if provisional { "*" } else { " " }),
            None => "- ".into(),
        };
        let last = row
            .reading
            .as_ref()
            .ok()
            .and_then(|r| r.last_compaction.as_ref())
            .map_or_else(|| "-".to_string(), |m| m.at.clone());
        let compactions = row
            .compactions()
            .map_or_else(|| "-".to_string(), |(n, h)| format!("{n}n/{h}h"));
        t.push(vec![
            row.record.role.clone(),
            row.kind.as_str().into(),
            row.model.clone(),
            context,
            number(row.window),
            number(row.native),
            source_text(row.decision.source, row.decided_now),
            grouped(row.threshold),
            last,
            compactions,
            state_text(row),
        ]);
    }
    let mut out = t.render();
    let reasons: Vec<String> = rows.iter().filter_map(Row::reason).collect();
    if !reasons.is_empty() {
        out.push_str("reasons:\n");
        for r in reasons {
            out.push_str(&format!("  {r}\n"));
        }
    }
    out
}

/// `--windows`: 1 row per roster teammate on a harness with a known trigger
/// rule, as a launch from `workdir` would decide now (CTX-06), with the
/// watch base and its source: the teammate's `compact_at` or the fleet base.
pub(crate) fn windows_table(ctx: &RuntimeContext, roster: &Roster, workdir: &Path) -> String {
    let mut t = Table::new(&[
        ("TEAMMATE", Align::Left),
        ("HARNESS", Align::Left),
        ("MODEL", Align::Left),
        ("SETTING", Align::Right),
        ("SOURCE", Align::Left),
        ("NATIVE", Align::Right),
        ("COMPACT_AT", Align::Right),
        ("FROM", Align::Left),
        ("THRESHOLD", Align::Right),
        ("HEADROOM", Align::Right),
        ("DETAIL", Align::Left),
    ]);
    let mut names = roster.names();
    names.sort_unstable();
    for name in names {
        let Some(teammate) = roster.get(name) else {
            continue;
        };
        let kind = teammate.agent;
        let trigger = kind.capabilities().compaction.trigger;
        if trigger == horch_core::harness::capabilities::TriggerRule::Unknown {
            continue;
        }
        let model = teammate.model.clone().unwrap_or_default();
        let decision = launch::window_decision(
            ctx,
            teammate,
            &model,
            workdir,
            roster.fleet_window(teammate, &model),
        );
        let native = window::native_trigger(trigger, kind.as_str(), &model, decision.tokens, None);
        let threshold = window::threshold(policy::watch_base(teammate.compact_at), native);
        let headroom = native.map_or_else(
            || "-".to_string(),
            |n| {
                let h = window::headroom(n, threshold);
                if h < HEADROOM_FLOOR {
                    format!("{} WARN", grouped(h))
                } else {
                    grouped(h)
                }
            },
        );
        t.push(vec![
            teammate.name.clone(),
            kind.as_str().into(),
            if model.is_empty() { "-".into() } else { model },
            number(decision.tokens),
            source_text(decision.source, false),
            number(native),
            grouped(policy::watch_base(teammate.compact_at)),
            if teammate.compact_at.is_some() {
                "teammate"
            } else {
                "fleet base"
            }
            .into(),
            grouped(threshold),
            headroom,
            home_relative(&decision.detail, &ctx.paths.home),
        ]);
    }
    t.render()
}

/// The SOURCE column: `operator`, `fleet` or `harness`, with ` (now)` when
/// the record has no recorded decision.
fn source_text(source: WindowSource, now: bool) -> String {
    let name = source_name(source);
    if now {
        format!("{name} (now)")
    } else {
        name.into()
    }
}

/// `operator`, `fleet` or `harness`.
fn source_name(source: WindowSource) -> &'static str {
    match source {
        WindowSource::Operator => "operator",
        WindowSource::Fleet => "fleet",
        WindowSource::Harness => "harness",
    }
}

/// A detail that names a file under `home`, with `~` for the home.
fn home_relative(detail: &str, home: &Path) -> String {
    let home = home.to_string_lossy();
    match detail.strip_prefix(home.as_ref()) {
        Some(rest) if !home.is_empty() && rest.starts_with('/') => format!("~{rest}"),
        _ => detail.into(),
    }
}

/// `1,000,000`, or `-` when unknown.
fn number(n: Option<u64>) -> String {
    n.map_or_else(|| "-".into(), grouped)
}

/// `n` with `,` between groups of 3 digits.
pub(crate) fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[derive(Clone, Copy)]
enum Align {
    Left,
    Right,
}

/// Columns 2 spaces apart, each as wide as its widest cell.
struct Table {
    head: Vec<(&'static str, Align)>,
    rows: Vec<Vec<String>>,
}

impl Table {
    fn new(head: &[(&'static str, Align)]) -> Self {
        Table {
            head: head.to_vec(),
            rows: Vec::new(),
        }
    }

    fn push(&mut self, row: Vec<String>) {
        self.rows.push(row);
    }

    fn render(&self) -> String {
        let widths: Vec<usize> = self
            .head
            .iter()
            .enumerate()
            .map(|(i, (h, _))| {
                self.rows
                    .iter()
                    .map(|r| r[i].chars().count())
                    .chain([h.len()])
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        let line = |cells: Vec<&str>| {
            let mut s = String::new();
            for (i, cell) in cells.into_iter().enumerate() {
                if i > 0 {
                    s.push_str("  ");
                }
                let w = widths[i];
                match self.head[i].1 {
                    Align::Left => s.push_str(&format!("{cell:<w$}")),
                    Align::Right => s.push_str(&format!("{cell:>w$}")),
                }
            }
            format!("{}\n", s.trim_end())
        };
        let mut out = line(self.head.iter().map(|(h, _)| *h).collect());
        for r in &self.rows {
            out.push_str(&line(r.iter().map(String::as_str).collect()));
        }
        out
    }
}

#[cfg(test)]
pub(crate) mod testkit {
    //! A temp world for the context-policy commands: a state dir, a project
    //! dir, the fixture home of `horch-core`, the built-in roster.

    use super::*;
    use horch_core::execution::legacy::{HistoryEntry, KIND_ORCHESTRATOR};
    use horch_core::runtime::MapEnv;

    /// The fixture session ids (`horch-core/tests/fixtures/context/home`).
    pub const OVER: &str = "11111111-1111-4111-8111-111111111111"; // 311,225 tokens
    pub const PENDING: &str = "22222222-2222-4222-8222-222222222222";
    pub const SMALL: &str = "33333333-3333-4333-8333-333333333333"; // 33,352 tokens
    pub const CODEX: &str = "01a0de6e-0000-7000-8000-000000000001";

    pub fn fixture_home() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../horch-core/tests/fixtures/context/home")
    }

    pub struct World {
        pub tmp: tempfile::TempDir,
        pub ctx: RuntimeContext,
        pub ledger: Ledger,
        pub roster: Roster,
        pub loc: Locations,
        pub cache: PathBuf,
    }

    impl World {
        /// HOME is the fixture home; state, project and the managed
        /// settings file are in a temp dir. The managed settings set the
        /// Claude window to 500,000, as the operator's Mac does (design
        /// §8.1): a Claude threshold is then 300,000.
        pub fn new() -> World {
            Self::with_home(&fixture_home())
        }

        pub fn with_home(home: &Path) -> World {
            Self::with_env(home, |e| e)
        }

        pub fn with_env(home: &Path, f: impl FnOnce(MapEnv) -> MapEnv) -> World {
            let tmp = tempfile::tempdir().unwrap();
            let project = tmp.path().join("project");
            let state = tmp.path().join("state");
            std::fs::create_dir_all(&project).unwrap();
            let env = MapEnv::new(&project)
                .with("HOME", &home.to_string_lossy())
                .with("HORCH_STATE_DIR", &state.to_string_lossy())
                .with("HORCH_PROJECT_DIR", &project.to_string_lossy())
                .with("HORCH_WORKSPACE_ID", "w1")
                .with(
                    "HORCH_CLAUDE_MANAGED_SETTINGS",
                    &tmp.path().join("managed.json").to_string_lossy(),
                );
            std::fs::write(
                tmp.path().join("managed.json"),
                r#"{"env":{"CLAUDE_CODE_AUTO_COMPACT_WINDOW":"500000"}}"#,
            )
            .unwrap();
            let ctx = RuntimeContext::from_env(&f(env)).unwrap();
            let ledger = Ledger::open_in(&ctx).unwrap();
            let loc = Locations::from_context(&ctx);
            let cache = cache_dir(&ctx);
            World {
                tmp,
                ctx,
                ledger,
                roster: Roster::builtin().unwrap(),
                loc,
                cache,
            }
        }

        pub fn sources(&self) -> Sources<'_> {
            Sources {
                ctx: &self.ctx,
                ledger: &self.ledger,
                roster: &self.roster,
                loc: &self.loc,
                cache_dir: &self.cache,
            }
        }

        pub fn project(&self) -> PathBuf {
            self.ctx.paths.project().unwrap()
        }

        /// A live worker record of teammate `tier` on `agent`.
        pub fn worker(&self, id: &str, role: &str, agent: &str, tier: &str, sid: Option<&str>) {
            self.ledger
                .insert(Record {
                    record_id: id.into(),
                    session_id: sid.map(Into::into),
                    agent: agent.into(),
                    tier: tier.into(),
                    model: tier.into(),
                    role: role.into(),
                    task: "work".into(),
                    workspace_id: Some("w1".into()),
                    ..Record::default()
                })
                .unwrap();
        }

        /// A live orchestrator record.
        pub fn orchestrator(&self, id: &str, sid: Option<&str>) {
            self.ledger
                .insert(Record {
                    record_id: id.into(),
                    session_id: sid.map(Into::into),
                    agent: "claude".into(),
                    tier: "orchestrator".into(),
                    model: "opus".into(),
                    role: "orchestrator".into(),
                    kind: KIND_ORCHESTRATOR.into(),
                    task: "(orchestrating)".into(),
                    workspace_id: Some("w1".into()),
                    ..Record::default()
                })
                .unwrap();
        }

        /// Append a history entry with an explicit time.
        pub fn event_at(&self, id: &str, at: &str, event: &str, text: &str) {
            self.ledger
                .store()
                .update(|records| {
                    for r in records.iter_mut().filter(|r| r.record_id == id) {
                        r.history.push(HistoryEntry {
                            at: at.into(),
                            event: event.into(),
                            text: text.into(),
                        });
                    }
                    Ok(())
                })
                .unwrap();
        }

        pub fn history(&self, id: &str) -> Vec<(String, String)> {
            self.ledger
                .get(id)
                .unwrap()
                .history
                .into_iter()
                .map(|h| (h.event, h.text))
                .collect()
        }

        pub fn events(&self, id: &str, event: &str) -> Vec<String> {
            self.history(id)
                .into_iter()
                .filter(|(e, _)| e == event)
                .map(|(_, t)| t)
                .collect()
        }

        /// A job dir of `role` in `w1` whose child is dead: `job.json` at
        /// `step` and a heartbeat of a pid that does not run.
        pub fn lost_job(&self, role: &str, step: &str) -> PathBuf {
            let dir = jobfile::job_dir(&self.ctx.paths.state_root, "w1", role);
            std::fs::create_dir_all(&dir).unwrap();
            let job = jobfile::JobRecord {
                v: jobfile::JOB_VERSION,
                role: role.into(),
                record_id: "x".into(),
                started_at: "2026-10-06T10:00:00Z".into(),
                step: step.into(),
            };
            jobfile::create_job(&dir, &job).unwrap();
            let hb = horch_core::heartbeat::Heartbeat {
                pid: dead_pid(),
                at: "2026-10-06T10:00:00Z".into(),
                started: Some(1),
            };
            std::fs::write(
                dir.join(horch_core::heartbeat::HEARTBEAT_FILE),
                serde_json::to_vec(&hb).unwrap(),
            )
            .unwrap();
            dir
        }
    }

    /// A pid that ran and has exited.
    pub fn dead_pid() -> u32 {
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        pid
    }
}

#[cfg(test)]
mod tests {
    use super::testkit::*;
    use super::*;

    fn text(w: &World, over: bool) -> String {
        context_text(&w.sources(), over).unwrap()
    }

    /// CTX-01: the orchestrator first, then 1 row per live worker, with
    /// every column.
    #[test]
    fn ctx_01_lists_orchestrator_first_then_live_workers() {
        let w = World::new();
        w.worker("r-s", "sonnet-1", "claude", "sonnet", Some(SMALL));
        w.worker("r-c", "codex-sol-1", "codex", "codex-sol", Some(CODEX));
        w.orchestrator("r-o", Some(OVER));
        let out = text(&w, false);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 4, "{out}");
        let head: Vec<&str> = lines[0]
            .split("  ")
            .filter(|s| !s.is_empty())
            .map(str::trim)
            .collect();
        assert_eq!(
            head,
            [
                "ROLE",
                "HARNESS",
                "MODEL",
                "CONTEXT",
                "WINDOW",
                "NATIVE",
                "SOURCE",
                "THRESHOLD",
                "LAST COMPACT",
                "COMPACTIONS",
                "STATE"
            ],
            "{out}"
        );
        assert!(lines[1].starts_with("orchestrator  claude"), "{out}");
        assert!(lines[1].contains("311,225"), "{out}");
        assert!(lines[1].ends_with("over"), "{out}");
        assert!(lines[2].starts_with("codex-sol-1"), "{out}");
        assert!(lines[3].starts_with("sonnet-1"), "{out}");
        assert!(lines[3].contains("33,352"), "{out}");
        for l in &lines[1..] {
            assert!(l.split_whitespace().count() >= 10, "{l}");
        }
    }

    /// X6: a Codex pane with `permission_mode: plan` shows its mode: STATE
    /// gets ` [write]` while it is in write mode, and `--json` has `mode`
    /// (`plan`, `write`, or `null` for every other row).
    #[test]
    fn x6_a_plan_pane_shows_its_mode() {
        let mut w = World::new();
        w.roster.insert_for_test(Teammate {
            name: "codex-plan".into(),
            agent: HarnessKind::Codex,
            permission_mode: Some(horch_core::roster::PermissionMode::Plan),
            ..Default::default()
        });
        w.worker("r-p", "codex-plan-1", "codex", "codex-plan", Some(CODEX));
        w.worker("r-s", "sonnet-1", "claude", "sonnet", Some(SMALL));
        let mode_of = |w: &World| -> Vec<serde_json::Value> {
            let ws = horch_core::workspace::testing::FakeWorkspace::new();
            let rows: Vec<serde_json::Value> =
                serde_json::from_str(&context_json(&w.sources(), &ws).unwrap()).unwrap();
            rows.iter().map(|r| r["mode"].clone()).collect()
        };
        assert_eq!(
            mode_of(&w),
            [serde_json::json!("plan"), serde_json::Value::Null]
        );
        let out = text(&w, false);
        assert!(!out.contains("[write]"), "{out}");

        w.ledger
            .record_event("r-p", super::super::mode::EVENT_MODE_CHANGED, "write")
            .unwrap();
        assert_eq!(
            mode_of(&w),
            [serde_json::json!("write"), serde_json::Value::Null]
        );
        let out = text(&w, false);
        let line = out.lines().find(|l| l.starts_with("codex-plan-1")).unwrap();
        assert!(line.ends_with(" [write]"), "{out}");
        assert!(!out
            .lines()
            .any(|l| l.starts_with("sonnet-1") && l.contains("[write]")));
    }

    /// CTX-01: a done worker has no row.
    #[test]
    fn ctx_01_done_worker_has_no_row() {
        let w = World::new();
        w.worker("r-1", "sonnet-1", "claude", "sonnet", Some(SMALL));
        w.worker("r-2", "sonnet-2", "claude", "sonnet", Some(SMALL));
        w.ledger.done("r-2", "finished").unwrap();
        let out = text(&w, false);
        assert_eq!(out.lines().count(), 2, "{out}");
        assert!(!out.contains("sonnet-2"), "{out}");
    }

    /// CTX-04: a row with no number has 1 `reasons:` line.
    #[test]
    fn ctx_04_states_print_a_reason_line() {
        let w = World::new();
        w.worker(
            "r-1",
            "sonnet-1",
            "claude",
            "sonnet",
            Some("99999999-9999-4999-8999-999999999999"),
        );
        w.worker("r-2", "ag-1", "antigravity", "antigravity", Some("s-ag"));
        let out = text(&w, false);
        let reasons: Vec<&str> = out
            .lines()
            .skip_while(|l| *l != "reasons:")
            .skip(1)
            .collect();
        assert_eq!(reasons.len(), 2, "{out}");
        assert!(reasons[0].starts_with("  ag-1: not-read: "), "{out}");
        assert_eq!(
            reasons[1], "  sonnet-1: no-transcript: no transcript found",
            "{out}"
        );
    }

    /// CTX-06: the windows view with an operator window of 500,000.
    #[test]
    fn ctx_06_windows_view_lists_claude_and_codex_teammates() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".claude")).unwrap();
        std::fs::write(
            home.path().join(".claude/settings.json"),
            r#"{"env":{"CLAUDE_CODE_AUTO_COMPACT_WINDOW":"500000"}}"#,
        )
        .unwrap();
        // No managed settings: the user file is the operator's value.
        let w = World::with_env(home.path(), |e| {
            e.with("HORCH_CLAUDE_MANAGED_SETTINGS", "/nonexistent/managed.json")
        });
        let out = windows_table(&w.ctx, &w.roster, &w.project());
        println!("{out}");
        let row = |name: &str| {
            out.lines()
                .find(|l| l.split_whitespace().next() == Some(name))
                .unwrap_or_else(|| panic!("no row {name}:\n{out}"))
                .to_string()
        };
        let mut claude = 0;
        for t in w.roster.names() {
            let tm = w.roster.get(t).unwrap();
            if tm.agent == HarnessKind::Claude {
                claude += 1;
                assert!(row(t).contains("500,000  operator"), "{}", row(t));
            }
        }
        assert!(claude > 0);
        assert!(
            row("codex-sol").contains("200,000  fleet"),
            "{}",
            row("codex-sol")
        );
        assert!(
            row("codex-luna").contains("harness"),
            "{}",
            row("codex-luna")
        );
        assert!(
            row("orchestrator").ends_with("~/.claude/settings.json"),
            "{}",
            row("orchestrator")
        );
    }

    /// CTX-09: `--over` shows only over and requested rows.
    #[test]
    fn ctx_09_over_prints_only_over_and_requested() {
        let w = World::new();
        w.worker("r-over", "sonnet-1", "claude", "sonnet", Some(OVER));
        w.worker("r-ok", "sonnet-2", "claude", "sonnet", Some(SMALL));
        w.worker("r-asked", "sonnet-3", "claude", "sonnet", Some(SMALL));
        w.ledger
            .record_event("r-asked", policy::EVENT_REQUESTED, "tokens 1 threshold 2")
            .unwrap();
        w.worker("r-none", "sonnet-4", "claude", "sonnet", None);
        let out = text(&w, true);
        let roles: Vec<&str> = out
            .lines()
            .skip(1)
            .map(|l| l.split_whitespace().next().unwrap())
            .collect();
        assert_eq!(roles, ["sonnet-1", "sonnet-3"], "{out}");
        assert!(out.lines().nth(1).unwrap().ends_with("over"));
        assert!(out.lines().nth(2).unwrap().ends_with("requested"));
    }

    /// CTX-09, CTX-15: a lost job shows in `--over` with its reason.
    #[test]
    fn ctx_09_over_prints_compact_lost() {
        let w = World::new();
        w.worker("r-1", "sonnet-1", "claude", "sonnet", Some(SMALL));
        let dir = w.lost_job("sonnet-1", "wait-marker");
        let out = text(&w, true);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 4, "{out}");
        assert!(lines[1].ends_with("compact-lost"), "{out}");
        assert_eq!(lines[2], "reasons:");
        assert_eq!(
            lines[3],
            format!(
                "  sonnet-1: compact-lost: the compaction job is lost at step wait-marker; log {}",
                dir.join("job.log").display()
            )
        );
    }

    /// CTX-09: nothing over prints exactly 1 line.
    #[test]
    fn ctx_09_over_with_none_prints_exact_line() {
        let w = World::new();
        w.worker("r-1", "sonnet-1", "claude", "sonnet", Some(SMALL));
        assert_eq!(text(&w, true), "no session is over its threshold\n");
        let empty = World::new();
        assert_eq!(text(&empty, true), "no session is over its threshold\n");
    }

    /// CTX-15: of 2 readers of 1 lost job, 1 records the event.
    #[test]
    fn ctx_15_first_reader_of_a_lost_job_records_one_event() {
        let w = World::new();
        w.worker("r-1", "sonnet-1", "claude", "sonnet", Some(SMALL));
        let dir = w.lost_job("sonnet-1", "wait-idle");
        let first = text(&w, false);
        let second = text(&w, false);
        let _ = crate::cmd::messaging::note_check(&w.sources(), "r-1");
        let failed = w.events("r-1", EVENT_FAILED);
        assert_eq!(
            failed,
            [format!(
                "step wait-idle: the job process is lost; log {}",
                dir.join("job.log").display()
            )]
        );
        for out in [first, second, text(&w, false)] {
            assert!(
                out.lines().nth(1).unwrap().ends_with("compact-lost"),
                "{out}"
            );
        }
    }

    /// CTX-24: `pane` and `pane_status` come from 1 `pane list` call per
    /// workspace; when herdr does not answer, both are `null`.
    #[test]
    fn ctx_24_json_pane_from_one_pane_list() {
        use horch_core::workspace::testing::FakeWorkspace;
        let w = World::new();
        let ws = FakeWorkspace::new();
        let new = ws.workspace_create("fleet", None, false).unwrap();
        assert_eq!(new.workspace_id, "w1");
        ws.set_agent_states(&new.root_pane_id, &[(Some("claude"), Some("idle"))]);
        for (id, role, pane) in [
            ("r-1", "sonnet-1", Some(new.root_pane_id.clone())),
            ("r-2", "sonnet-2", None),
        ] {
            w.ledger
                .insert(Record {
                    record_id: id.into(),
                    session_id: Some(SMALL.into()),
                    agent: "claude".into(),
                    tier: "sonnet".into(),
                    model: "sonnet".into(),
                    role: role.into(),
                    task: "work".into(),
                    workspace_id: Some("w1".into()),
                    pane_id: pane,
                    ..Record::default()
                })
                .unwrap();
        }
        let rows: Vec<serde_json::Value> =
            serde_json::from_str(&context_json(&w.sources(), &ws).unwrap()).unwrap();
        assert_eq!(rows[0]["pane"], new.root_pane_id.as_str());
        assert_eq!(rows[0]["pane_status"], "idle");
        assert_eq!(rows[1]["pane"], serde_json::Value::Null);
        let lists = ws
            .calls()
            .iter()
            .filter(|c| c.method == "pane_list")
            .count();
        assert_eq!(lists, 1);

        ws.set_reachable(false);
        let rows: Vec<serde_json::Value> =
            serde_json::from_str(&context_json(&w.sources(), &ws).unwrap()).unwrap();
        assert_eq!(rows[0]["pane"], serde_json::Value::Null);
        assert_eq!(rows[0]["pane_status"], serde_json::Value::Null);
    }

    #[test]
    fn grouped_puts_commas_between_thousands() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1_000), "1,000");
        assert_eq!(grouped(311_225), "311,225");
        assert_eq!(grouped(1_000_000), "1,000,000");
    }
}
