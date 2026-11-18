//! The handlers of the run and plan request kinds: `run_new`, `run_close`,
//! `run_gate`, `task_add`, `map_add`, `task_solo`, `plan_workers`,
//! `ledger_import` and `ledger_gc`.
//!
//! Requirements: RR-5, RR-8. Ticket 17 of fleetd-p5 fills it.
//!
//! Each handler writes the rows that its `horch` command writes today,
//! through the functions of `taskledger`, and returns the values that the
//! command prints. It runs no process, no herdr command and no git. The
//! command gathers the facts of git, gh, the gate run and the disk before
//! it sends the request, and the payload carries them. fleetd trusts such a
//! fact only in a request of class `operator` ([`operator_only`]).
//!
//! A ticket or a map travels as a path with its sha256 (Q15): the handler
//! reads the file with [`INPUT_MAX_BYTES`] and refuses the request when the
//! sha256 differs.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use rusqlite::Transaction;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::fleetd::fleet_runtime::application::requests::Handlers;
use crate::fleetd::fleet_runtime::domain::requests::{Class, Kind};
use crate::taskledger::git::Git;
use crate::taskledger::import::{self, ImportFacts};
use crate::taskledger::map::{self, TicketFacts};
use crate::taskledger::model::{Actor, StepName, TaskKind, WorktreeMode};
use crate::taskledger::runs::{self, CloseGuards, NewRun, PrCheck, Seat, Thresholds};
use crate::taskledger::status;
use crate::taskledger::tasks::{self, NewTask, Task, ONE_OFF_TICKET};

/// The size limit of a ticket or a map that a request names, in bytes.
pub const INPUT_MAX_BYTES: u64 = 1 << 20;

/// The kinds of this file.
pub const KINDS: [Kind; 9] = [
    Kind::RunNew,
    Kind::RunClose,
    Kind::RunGate,
    Kind::TaskAdd,
    Kind::MapAdd,
    Kind::TaskSolo,
    Kind::PlanWorkers,
    Kind::LedgerImport,
    Kind::LedgerGc,
];

/// 1 handler of this file.
type HandlerFn = fn(&Transaction<'_>, &Value) -> Result<Value>;

/// Put the handler of each kind of this file in `handlers`.
pub fn register(handlers: &mut Handlers) {
    let all: [(Kind, HandlerFn); 9] = [
        (Kind::RunNew, run_new),
        (Kind::RunClose, run_close),
        (Kind::RunGate, run_gate),
        (Kind::TaskAdd, task_add),
        (Kind::MapAdd, map_add),
        (Kind::TaskSolo, task_solo),
        (Kind::PlanWorkers, plan_workers),
        (Kind::LedgerImport, ledger_import),
        (Kind::LedgerGc, ledger_gc),
    ];
    for (kind, handler) in all {
        handlers.insert(kind, operator_only(kind, handler));
    }
}

/// `handler` for `kind`, which refuses each request when `kind` is not of
/// class `operator`: the payload of these kinds carries facts that the
/// command gathered, and only the operator is trusted to give them.
pub fn operator_only(
    kind: Kind,
    handler: impl Fn(&Transaction<'_>, &Value) -> Result<Value> + Send + Sync + 'static,
) -> impl Fn(&Transaction<'_>, &Value) -> Result<Value> + Send + Sync + 'static {
    move |tx, payload| {
        if kind.class() != Class::Operator {
            bail!(
                "`{}` is not an operator kind, so fleetd does not trust the facts of its payload",
                kind.as_str()
            );
        }
        handler(tx, payload)
    }
}

/// The payload as `T`. An unknown or missing field refuses the request.
fn args<T: DeserializeOwned>(payload: &Value) -> Result<T> {
    serde_json::from_value(payload.clone()).context("the payload does not fit its kind")
}

/// The orchestrator that sends a request that can create a run.
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct SeatArgs {
    #[serde(default)]
    pane_id: Option<String>,
    /// The harness and model of its session record, when it names them.
    #[serde(default)]
    harness: Option<(Option<String>, Option<String>)>,
}

impl SeatArgs {
    fn seat(&self) -> Seat {
        Seat {
            pane_id: self.pane_id.clone(),
            harness: self.harness.clone(),
        }
    }
}

// ─── run_new ────────────────────────────────────────────────────────────

/// `horch ledger run new`. The command made the run branch (TRF-34) and
/// chose the steps; the payload names them.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunNewArgs {
    project_root: PathBuf,
    slug: String,
    #[serde(default)]
    workspace_id: Option<String>,
    #[serde(default)]
    base_branch: Option<String>,
    #[serde(default)]
    run_branch: Option<String>,
    #[serde(default)]
    remote: Option<String>,
    finish_steps: Vec<String>,
    #[serde(default)]
    thresholds: ThresholdArgs,
    #[serde(default)]
    check: Option<String>,
    #[serde(default)]
    seat: SeatArgs,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ThresholdArgs {
    #[serde(default)]
    check_every_s: Option<i64>,
    #[serde(default)]
    stall_rounds: Option<i64>,
    #[serde(default)]
    heartbeat_every_s: Option<i64>,
    #[serde(default)]
    heartbeat_stale_s: Option<i64>,
    #[serde(default)]
    timer_tolerance_s: Option<i64>,
    #[serde(default)]
    max_attempts: Option<i64>,
}

fn run_new(tx: &Transaction<'_>, payload: &Value) -> Result<Value> {
    let a: RunNewArgs = args(payload)?;
    let finish_steps = a
        .finish_steps
        .iter()
        .map(|s| StepName::parse(s))
        .collect::<Result<Vec<_>>>()?;
    let t = a.thresholds;
    let new = NewRun {
        slug: a.slug,
        workspace_id: a.workspace_id,
        base_branch: a.base_branch,
        run_branch: a.run_branch,
        remote: a.remote,
        finish_steps,
        thresholds: Thresholds {
            check_every_s: t.check_every_s,
            stall_rounds: t.stall_rounds,
            heartbeat_every_s: t.heartbeat_every_s,
            heartbeat_stale_s: t.heartbeat_stale_s,
            timer_tolerance_s: t.timer_tolerance_s,
            max_attempts: t.max_attempts,
        },
    };
    let check = a.check.as_deref().filter(|c| !c.trim().is_empty());
    let run = runs::create_with_orchestrator(tx, &a.project_root, &new, check, &a.seat.seat())?;
    Ok(json!({ "run": run }))
}

// ─── run_close ──────────────────────────────────────────────────────────

/// `horch ledger run close`. The command read the C5 facts (the tip of the
/// run branch and the PR) for a close as `done` of a settled run.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunCloseArgs {
    run_id: String,
    #[serde(default)]
    abandon: Option<String>,
    #[serde(default)]
    guards: Option<GuardArgs>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GuardArgs {
    #[serde(default)]
    tip: Option<String>,
    pr: PrArgs,
    #[serde(default)]
    pr_declined: Option<String>,
}

/// [`PrCheck`] on the wire.
#[derive(Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
enum PrArgs {
    NotApplicable { why: String },
    Merged { number: i64 },
    Unmerged { number: i64, pr_state: String },
    Missing,
}

impl GuardArgs {
    fn guards(self) -> CloseGuards {
        CloseGuards {
            tip: self.tip,
            pr: match self.pr {
                PrArgs::NotApplicable { why } => PrCheck::NotApplicable(why),
                PrArgs::Merged { number } => PrCheck::Merged(number),
                PrArgs::Unmerged { number, pr_state } => PrCheck::Unmerged {
                    number,
                    state: pr_state,
                },
                PrArgs::Missing => PrCheck::Missing,
            },
            pr_declined: self.pr_declined.filter(|p| !p.trim().is_empty()),
        }
    }
}

/// The rows of the close. A close with `--abandon` keeps its cancels when a
/// parked worktree keeps the run active: the result names the parked trees,
/// and the command refuses with them as today. The gate folder and the stop
/// of each worker of a cancelled task are effects after the commit; the
/// result names them (`cancelled`), and this handler does neither.
fn run_close(tx: &Transaction<'_>, payload: &Value) -> Result<Value> {
    let a: RunCloseArgs = args(payload)?;
    let run = runs::get(tx, &a.run_id)?;
    let abandon = a.abandon.as_deref();
    // `close_run` and `close_run_with` do not read their git.
    let unread = Git::new("git");
    let closed = match (abandon, a.guards) {
        (Some(_), _) => runs::close_run(tx, &unread, &run, abandon)?,
        (None, Some(g)) => runs::close_run_with(tx, &unread, &run, None, &g.guards())?,
        (None, None) => {
            // C6 first: an unsettled run gets the refusal of the settled rule.
            if status::unsettled(tx, &run.run_id)?.is_empty() {
                bail!(
                    "run {} is settled, and a close as done needs the facts of the tip and the PR",
                    run.slug
                );
            }
            runs::close_run(tx, &unread, &run, None)?
        }
    };
    Ok(json!({ "run": runs::get(tx, &run.run_id)?, "closed": closed }))
}

// ─── run_gate ───────────────────────────────────────────────────────────

/// `horch ledger run gate`: the command ran the check on the tip in the
/// gate folder, and the payload is its result.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunGateArgs {
    run_id: String,
    tip_sha: String,
    command: String,
    #[serde(default)]
    exit_code: Option<i64>,
    #[serde(default)]
    log_path: Option<String>,
    started_at: String,
    #[serde(default)]
    ended_at: Option<String>,
}

fn run_gate(tx: &Transaction<'_>, payload: &Value) -> Result<Value> {
    let a: RunGateArgs = args(payload)?;
    runs::get(tx, &a.run_id)?;
    let gate = runs::record_gate(
        tx,
        &runs::Gate {
            gate_id: 0,
            run_id: a.run_id,
            tip_sha: a.tip_sha,
            command: a.command,
            exit_code: a.exit_code,
            log_path: a.log_path,
            started_at: a.started_at,
            ended_at: a.ended_at,
        },
    )?;
    Ok(json!({ "gate": gate, "passed": gate.passed() }))
}

// ─── task_add ───────────────────────────────────────────────────────────

/// `horch ledger add`. `task_file` is as the worker reads it: relative to
/// the project root when it is inside it. `ticket_sha256` is the sha256 of
/// the ticket file, `None` when the command could not read it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskAddArgs {
    run_id: String,
    project_root: PathBuf,
    task_file: String,
    #[serde(default)]
    ticket_sha256: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    issue: Option<String>,
    #[serde(default)]
    spec: Option<String>,
    primary: String,
    #[serde(default)]
    fallback: Option<String>,
    #[serde(default)]
    phase: Option<String>,
    #[serde(default)]
    exclusive: bool,
    priority: i64,
    #[serde(default)]
    shared: bool,
    #[serde(default)]
    check: Option<String>,
    #[serde(default)]
    blocked_by: Vec<String>,
    #[serde(default)]
    own: Vec<String>,
    #[serde(default)]
    no_touch: Vec<String>,
}

/// The task that `add` queues: what a flag names, else what the ticket
/// says. As today, `add` keeps no ticket snapshot.
fn task_add(tx: &Transaction<'_>, payload: &Value) -> Result<Value> {
    let a: TaskAddArgs = args(payload)?;
    let run = runs::get(tx, &a.run_id)?;
    let path = a.project_root.join(&a.task_file);
    let text = a
        .ticket_sha256
        .as_deref()
        .map(|sha| read_checked(&path, sha))
        .transpose()?;
    let facts = text.as_deref().map(map::ticket_facts).unwrap_or_default();
    let (check_none, land_check) = resolve_check(a.check, facts.check_none, facts.check.clone());
    let new = NewTask {
        title: a.title.unwrap_or_else(|| title_of(&facts, &path)),
        task_file: a.task_file,
        issue_url: a.issue,
        spec_ref: a.spec,
        primary: a.primary,
        fallback: a.fallback,
        phase: a.phase,
        parallel: !a.exclusive,
        priority: a.priority,
        wave: None,
        worktree_mode: if a.shared {
            WorktreeMode::Shared
        } else {
            WorktreeMode::Worktree
        },
        check_none,
        land_check,
        kind: TaskKind::Work,
        parent_task_id: None,
        blocked_by: a.blocked_by,
        own: if a.own.is_empty() {
            facts.own.clone()
        } else {
            a.own
        },
        no_touch: if a.no_touch.is_empty() {
            facts.no_touch.clone()
        } else {
            a.no_touch
        },
        work_kind: facts.kind,
        review_mode: facts.review_mode(),
        ticket_snapshot: None,
    };
    let t = tasks::add(tx, &run, &new, Actor::Orchestrator)?;
    Ok(json!({ "task": queued(&t) }))
}

/// The ticket's first heading, or its file stem.
fn title_of(facts: &TicketFacts, path: &Path) -> String {
    facts.title.clone().unwrap_or_else(|| {
        path.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "task".into())
    })
}

/// The task's (check_none, land_check) from a `--check` flag and the
/// ticket's `**Check:**` line. The flag value `none` means no check.
fn resolve_check(
    flag: Option<String>,
    ticket_none: bool,
    ticket_check: Option<String>,
) -> (bool, Option<String>) {
    match flag {
        Some(c) if c.trim() == "none" => (true, None),
        Some(c) => (false, Some(c)),
        None => (ticket_none, ticket_check),
    }
}

/// What the command prints of a queued task.
fn queued(t: &Task) -> Value {
    json!({
        "task_id": t.task_id,
        "short": t.short(),
        "claimable_by": t.claimable_by,
        "title": t.title,
    })
}

// ─── map_add ────────────────────────────────────────────────────────────

/// `horch ledger add-map`. `map_path` is absolute. `tickets` holds the
/// sha256 of each ticket of the map, by its task file as the map gives it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MapAddArgs {
    run_id: String,
    project_root: PathBuf,
    map_path: PathBuf,
    map_sha256: String,
    tickets: BTreeMap<String, String>,
    #[serde(default)]
    again: bool,
}

/// Each ticket keeps its snapshot and the sha256 of it, as today. Each
/// snapshot must have the sha256 of the payload.
fn map_add(tx: &Transaction<'_>, payload: &Value) -> Result<Value> {
    let a: MapAddArgs = args(payload)?;
    if !a.map_path.is_absolute() {
        bail!("the map path {} is not absolute", a.map_path.display());
    }
    read_checked(&a.map_path, &a.map_sha256)?;
    let parsed = map::read(&a.map_path, &a.project_root)?;
    let files: BTreeSet<&str> = parsed
        .items
        .iter()
        .map(|i| i.task.task_file.as_str())
        .collect();
    if files != a.tickets.keys().map(String::as_str).collect() {
        bail!(
            "the tickets of {} are not the tickets of the request",
            a.map_path.display()
        );
    }
    for item in &parsed.items {
        let text = item.task.ticket_snapshot.as_deref().unwrap_or_default();
        if tasks::ticket_sha(text) != a.tickets[&item.task.task_file] {
            bail!(
                "the ticket {} changed after the request was sent",
                item.task.task_file
            );
        }
        if text.len() as u64 > INPUT_MAX_BYTES {
            bail!(
                "the ticket {} is over {INPUT_MAX_BYTES} bytes",
                item.task.task_file
            );
        }
    }
    let run = runs::get(tx, &a.run_id)?;
    let items: Vec<(String, NewTask)> = parsed.items.into_iter().map(|i| (i.key, i.task)).collect();
    if !a.again {
        tasks::refuse_queued(tx, &run.run_id, &items)?;
    }
    let added = tasks::add_many(tx, &run, &items, Actor::Orchestrator)?;
    Ok(json!({
        "tasks": added.iter().map(queued).collect::<Vec<_>>(),
        "map": a.map_path,
    }))
}

// ─── task_solo ──────────────────────────────────────────────────────────

/// `horch ledger task`: 1 one-off task in the solo run of the workspace.
/// `solo` names the run branch that the command made, for a solo run that
/// does not exist yet.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskSoloArgs {
    project_root: PathBuf,
    workspace_id: String,
    title: String,
    persona: String,
    #[serde(default)]
    task_file: Option<String>,
    #[serde(default)]
    ticket_sha256: Option<String>,
    #[serde(default)]
    check: Option<String>,
    #[serde(default)]
    solo: SoloArgs,
    #[serde(default)]
    seat: SeatArgs,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct SoloArgs {
    #[serde(default)]
    run_branch: Option<String>,
    #[serde(default)]
    base_branch: Option<String>,
    #[serde(default)]
    remote: Option<String>,
}

fn task_solo(tx: &Transaction<'_>, payload: &Value) -> Result<Value> {
    let a: TaskSoloArgs = args(payload)?;
    let new = one_off_task(&a)?;
    let root = &a.project_root;
    let (run, created) = match runs::active_solo(tx, root, &a.workspace_id)? {
        Some(run) => (run, false),
        None => {
            let mut solo = runs::solo_new_run(&a.workspace_id);
            solo.run_branch = a.solo.run_branch.clone();
            solo.base_branch = a.solo.base_branch.clone();
            solo.remote = a.solo.remote.clone();
            let run = runs::create_with_orchestrator(tx, root, &solo, None, &a.seat.seat())?;
            (run, true)
        }
    };
    let t = tasks::add(tx, &run, &new, Actor::Orchestrator)?;
    Ok(json!({
        "run": run,
        "created_run": created,
        "task": queued(&t),
        "primary": new.primary,
        "no_check": new.land_check.is_none() && !new.check_none,
    }))
}

/// The one-off task: the ticket file's facts when `task_file` names one,
/// else a snapshot that holds only the title.
fn one_off_task(a: &TaskSoloArgs) -> Result<NewTask> {
    if a.title.trim().is_empty() {
        bail!("a task needs a title");
    }
    let Some(file) = &a.task_file else {
        let mut new = NewTask::work(&a.title, ONE_OFF_TICKET, &a.persona);
        new.ticket_snapshot = Some(format!("# {}\n", a.title.trim()));
        (new.check_none, new.land_check) = resolve_check(a.check.clone(), false, None);
        return Ok(new);
    };
    let text = a
        .ticket_sha256
        .as_deref()
        .map(|sha| read_checked(&a.project_root.join(file), sha))
        .transpose()?;
    let facts = text.as_deref().map(map::ticket_facts).unwrap_or_default();
    let mut new = NewTask::work(&a.title, file, &a.persona);
    new.review_mode = facts.review_mode();
    (new.check_none, new.land_check) =
        resolve_check(a.check.clone(), facts.check_none, facts.check);
    new.work_kind = facts.kind;
    new.own = facts.own;
    new.no_touch = facts.no_touch;
    Ok(new)
}

// ─── plan_workers ───────────────────────────────────────────────────────

/// `horch ledger plan-workers`. `workspace_id` is set when the command runs
/// in the pane of the orchestrator: a run with no workspace adopts it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanWorkersArgs {
    run_id: String,
    #[serde(default)]
    workspace_id: Option<String>,
}

fn plan_workers(tx: &Transaction<'_>, payload: &Value) -> Result<Value> {
    let a: PlanWorkersArgs = args(payload)?;
    let run = runs::get(tx, &a.run_id)?;
    let adopted = match &a.workspace_id {
        Some(ws) => runs::adopt_workspace(tx, &run.run_id, ws)?,
        None => false,
    };
    let plan = tasks::plan_workers(tx, &run.run_id)?;
    Ok(json!({ "run": runs::get(tx, &run.run_id)?, "adopted": adopted, "plan": plan }))
}

// ─── ledger_import ──────────────────────────────────────────────────────

/// `horch ledger import` (not `--dry-run`): the old files under
/// `state_root`, and the facts of git and the disk that the command read.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportArgs {
    state_root: PathBuf,
    facts: ImportFacts,
}

fn ledger_import(tx: &Transaction<'_>, payload: &Value) -> Result<Value> {
    let a: ImportArgs = args(payload)?;
    let report = import::import_from(tx, &a.state_root, &a.facts, false)?;
    Ok(json!({ "report": report }))
}

// ─── ledger_gc ──────────────────────────────────────────────────────────

/// `horch ledger gc`: the command deleted the trash refs, which are git
/// refs and not ledger rows. The request records what it deleted.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GcArgs {
    deleted: Vec<String>,
}

fn ledger_gc(_tx: &Transaction<'_>, payload: &Value) -> Result<Value> {
    let a: GcArgs = args(payload)?;
    Ok(json!({ "deleted": a.deleted }))
}

// ─── files ──────────────────────────────────────────────────────────────

/// The text of the file `path`, which must have the sha256 `sha256` and at
/// most [`INPUT_MAX_BYTES`].
fn read_checked(path: &Path, sha256: &str) -> Result<String> {
    let file = std::fs::File::open(path).with_context(|| format!("reading {}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(INPUT_MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("reading {}", path.display()))?;
    if bytes.len() as u64 > INPUT_MAX_BYTES {
        bail!("{} is over {INPUT_MAX_BYTES} bytes", path.display());
    }
    let text =
        String::from_utf8(bytes).with_context(|| format!("{} is not UTF-8", path.display()))?;
    if tasks::ticket_sha(&text) != sha256 {
        bail!("{} changed after the request was sent", path.display());
    }
    Ok(text)
}
