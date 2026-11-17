//! The task and its reap path (FM-4, FM-22): the process part of I1, I4
//! and I5 (`docs/specs/fleetd/design.md`, "Ledger").
//!
//! Facts in, decisions out. The facts are the rows of the ledger and what
//! part 1 of a pass saw of their processes and panes. No I/O.
//!
//! - I1, process part: a live attempt whose process is dead ends, and the
//!   task that its worker holds is queued again or held ([`dead_attempts`]).
//! - I4: a worker never gets a second attempt ([`held_attempt`]).
//! - I5: a process or pane that an ended row names, and that no live
//!   attempt and no live worker row owns, is an orphan to stop
//!   ([`orphans`]). A live worker row owns its processes and pane in
//!   Phase 1, because `horch spawn` writes no attempt before the claim.
//!
//! The orchestrator row is never stopped (`preservation-04-behavior.md`,
//! S8), and a row of another host is never judged: its PIDs mean nothing
//! here. A pane is closed only when a second fact proves that it is the
//! pane of the row: the process in the pane is a recorded process of the
//! row. Else the pane is kept, and the caller puts it on the needs-you
//! list ([`KeptPane`]).

use std::collections::{BTreeMap, BTreeSet};

/// A process as a row records it. A pid alone can name a later program, so
/// a stop needs the start time (S8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Recorded {
    pub pid: u32,
    pub pid_start: Option<u64>,
}

/// What part 1 of a pass saw of a recorded process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seen {
    /// A process has the pid and, when one is recorded, the start time.
    Alive,
    /// No process has the pid, or a process with another start time has it.
    Dead,
    /// The pass did not read it, or could not tell.
    Unknown,
}

/// The state of an attempt row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttemptState {
    Launching,
    Live,
    Ended,
}

impl AttemptState {
    /// Every state, in the order of the table.
    pub const ALL: [AttemptState; 3] = [
        AttemptState::Launching,
        AttemptState::Live,
        AttemptState::Ended,
    ];

    /// The word of `attempts.state`.
    pub fn as_str(self) -> &'static str {
        match self {
            AttemptState::Launching => "launching",
            AttemptState::Live => "live",
            AttemptState::Ended => "ended",
        }
    }

    /// The state of an `attempts.state` word.
    pub fn parse(word: &str) -> Option<AttemptState> {
        Self::ALL.into_iter().find(|s| s.as_str() == word)
    }

    /// Whether an attempt in this state holds its worker, its processes and
    /// its pane.
    pub fn holds(self) -> bool {
        self != AttemptState::Ended
    }
}

/// The id of attempt `n` of task `task_id`, as the schema v8 backfill
/// writes it.
pub fn attempt_id(task_id: &str, n: i64) -> String {
    format!("{task_id}/A{n}")
}

/// 1 attempt row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttemptRow {
    pub id: String,
    /// `None` for an attempt of the orchestrator.
    pub task_id: Option<String>,
    pub worker_id: String,
    pub state: AttemptState,
    /// The agent process.
    pub process: Option<Recorded>,
    /// The pane of the attempt, else the pane of its worker.
    pub pane: Option<String>,
}

/// 1 worker row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkerRow {
    pub id: String,
    /// The `workers.status` word.
    pub status: String,
    /// Whether the status is a live one.
    pub live: bool,
    pub orchestrator: bool,
    /// The row has the host of this machine, or no host.
    pub here: bool,
    pub agent: Option<Recorded>,
    pub supervisor: Option<Recorded>,
    pub pane: Option<String>,
}

impl WorkerRow {
    /// Whether I1 and I5 may judge the row: a row of this host that is not
    /// the orchestrator.
    fn judged(&self) -> bool {
        self.here && !self.orchestrator
    }

    fn processes(&self) -> impl Iterator<Item = Recorded> {
        self.agent.into_iter().chain(self.supervisor)
    }
}

/// The facts of 1 task that a failed attempt needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskRow {
    pub id: String,
    /// The worker of a `running` or `landing` task.
    pub held_by: Option<String>,
    /// The attempts since the last operator requeue: `attempts - attempt_floor`.
    pub used: i64,
    /// The `max_attempts` of the run.
    pub max_attempts: i64,
}

/// The rows of the reap path.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rows {
    pub attempts: Vec<AttemptRow>,
    pub workers: BTreeMap<String, WorkerRow>,
    /// The tasks of the live attempts.
    pub tasks: BTreeMap<String, TaskRow>,
}

impl Rows {
    /// Each process that a row of this host names: what part 1 of a pass
    /// reads into [`Facts::processes`].
    pub fn named_processes(&self) -> BTreeSet<Recorded> {
        let workers = self
            .workers
            .values()
            .filter(|w| w.judged())
            .flat_map(WorkerRow::processes);
        let attempts = self
            .attempts
            .iter()
            .filter(|a| self.judged(a))
            .filter_map(|a| a.process);
        workers.chain(attempts).collect()
    }

    /// Whether I1 and I5 may judge the attempt `a`.
    fn judged(&self, a: &AttemptRow) -> bool {
        self.workers
            .get(&a.worker_id)
            .is_some_and(WorkerRow::judged)
    }
}

/// What part 1 of a pass saw.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    /// The recorded processes that the pass read. A process that is not in
    /// the map is [`Seen::Unknown`].
    pub processes: BTreeMap<Recorded, Seen>,
    /// The panes of the herdr snapshot, each with the process in it when
    /// herdr reports one. `None` when the pass has no snapshot.
    pub panes: Option<BTreeMap<String, Option<Recorded>>>,
}

impl Facts {
    fn process(&self, p: Recorded) -> Seen {
        self.processes.get(&p).copied().unwrap_or(Seen::Unknown)
    }

    /// Whether the snapshot has `pane`. `None` with no snapshot.
    fn has_pane(&self, pane: &str) -> Option<bool> {
        self.panes.as_ref().map(|p| p.contains_key(pane))
    }

    /// Whether the process in `pane` is 1 of `recorded`, by pid and start
    /// time: the second fact that proves the pane is the pane of the row.
    fn pane_runs(&self, pane: &str, recorded: &[Recorded]) -> bool {
        let inside = self.panes.as_ref().and_then(|p| p.get(pane)).copied();
        inside
            .flatten()
            .is_some_and(|p| p.pid_start.is_some() && recorded.contains(&p))
    }
}

/// What a failed attempt does to its task (`preservation-04-behavior.md`,
/// L4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AfterFailure {
    /// `ScheduleRetry`: queued again, first in line.
    Retry,
    /// `EscalateTask`: held for the operator.
    Escalate,
}

/// The rule of L4: at `max_attempts` attempts since the last operator
/// requeue, the task is held; else it runs again.
pub fn after_failure(used: i64, max_attempts: i64) -> AfterFailure {
    if used >= max_attempts {
        AfterFailure::Escalate
    } else {
        AfterFailure::Retry
    }
}

/// The row that names a process or a pane: the key of a stop.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RowRef {
    Worker(String),
    Attempt(String),
}

/// What a stop ends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// Stop the process only while it has its pid and start time. With
    /// `pgid`, the stop goes to each process of its group, but only while
    /// `pid` leads that group with `pid_start` at the time of the stop
    /// (FM-22). Else it goes to `pid` alone.
    Process {
        pid: u32,
        pid_start: u64,
        pgid: Option<u32>,
    },
    /// Close the exact pane.
    Pane(String),
}

/// 1 effect to do after the commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stop {
    pub row: RowRef,
    pub target: Target,
}

/// A pane that is not closed: no fact proves that it is the pane of its
/// row, and herdr can give an old pane id to a pane outside the fleet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeptPane {
    pub pane: String,
    pub row: RowRef,
}

/// The stops and the kept panes of the rows that a pass acts on. Each
/// process stop comes before each pane (S8).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cleanup {
    pub stops: Vec<Stop>,
    pub kept: Vec<KeptPane>,
}

impl Cleanup {
    /// Add the stops and kept panes of 1 more row, with these rules:
    /// - 1 stop for each target: the first row that names a process or a
    ///   pane keeps its stop, and a later row adds none.
    /// - 1 kept pane for each pane id, and none for a pane that a stop of
    ///   any row closes: a row that proves the pane wins.
    /// - Each process stop comes before each pane stop (S8), and each kind
    ///   keeps its order.
    fn merge_row(&mut self, other: Cleanup) {
        for s in other.stops {
            if !self.stops.iter().any(|m| m.target == s.target) {
                self.stops.push(s);
            }
        }
        for k in other.kept {
            if !self.kept.iter().any(|m| m.pane == k.pane) {
                self.kept.push(k);
            }
        }
        let stops = &self.stops;
        self.kept.retain(|k| {
            !stops
                .iter()
                .any(|s| s.target == Target::Pane(k.pane.clone()))
        });
        self.stops
            .sort_by_key(|s| matches!(s.target, Target::Pane(_)));
    }
}

/// I1, process part: 1 live attempt whose process is dead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reap {
    pub attempt: String,
    pub worker: String,
    /// The outcome of the attempt: `no_result` when its worker row is
    /// live, else the end status of the worker row.
    pub outcome: String,
    /// The worker row is live: it ends `incomplete`.
    pub end_worker: bool,
    /// The task that the worker holds, and what happens to it.
    pub task: Option<(String, AfterFailure)>,
    /// The rest of the row: its live processes, then its pane.
    pub cleanup: Cleanup,
}

/// I1, process part: each live attempt of this host whose process is
/// dead. An attempt with no recorded process is judged by its pane, and an
/// attempt with neither is dead: no process or pane can hold it.
pub fn dead_attempts(rows: &Rows, facts: &Facts) -> Vec<Reap> {
    let dead: Vec<&AttemptRow> = rows
        .attempts
        .iter()
        .filter(|a| a.state == AttemptState::Live && a.task_id.is_some())
        .filter(|a| rows.judged(a) && attempt_seen(a, facts) == Seen::Dead)
        .collect();
    let after = ended(rows, &dead);
    let owned = Owned::of(&after);
    dead.into_iter()
        .map(|a| reap(rows, facts, &owned, a))
        .collect()
}

/// The [`Reap`] of the dead attempt `a`; `owned` is what the rows own
/// after the reaps.
fn reap(rows: &Rows, facts: &Facts, owned: &Owned, a: &AttemptRow) -> Reap {
    let w = &rows.workers[&a.worker_id];
    let task = a
        .task_id
        .as_ref()
        .and_then(|t| rows.tasks.get(t))
        .filter(|t| t.held_by.as_deref() == Some(w.id.as_str()))
        .map(|t| (t.id.clone(), after_failure(t.used, t.max_attempts)));
    let mut cleanup = clean_row(facts, owned, &named_by_attempt(a), PaneCheck::Always);
    cleanup.merge_row(clean_row(
        facts,
        owned,
        &named_by_worker(w),
        PaneCheck::Always,
    ));
    Reap {
        attempt: a.id.clone(),
        worker: w.id.clone(),
        outcome: if w.live {
            "no_result".to_string()
        } else {
            w.status.clone()
        },
        end_worker: w.live,
        task,
        cleanup,
    }
}

/// The rows as they are after `dead` end: each attempt `ended`, and each
/// worker of one not live.
fn ended(rows: &Rows, dead: &[&AttemptRow]) -> Rows {
    let mut after = rows.clone();
    for a in dead {
        if let Some(m) = after.attempts.iter_mut().find(|m| m.id == a.id) {
            m.state = AttemptState::Ended;
        }
        if let Some(w) = after.workers.get_mut(&a.worker_id) {
            w.live = false;
        }
    }
    after
}

/// I5: the processes and panes that ended rows of this host name, and that
/// no live attempt and no live worker row owns. A pane is looked at only
/// for a row with a process to stop: an ended row whose processes are
/// gone names a pane id that herdr can have given to another pane.
pub fn orphans(rows: &Rows, facts: &Facts) -> Cleanup {
    let owned = Owned::of(rows);
    let mut out = Cleanup::default();
    for w in rows.workers.values().filter(|w| w.judged() && !w.live) {
        let named = named_by_worker(w);
        out.merge_row(clean_row(
            facts,
            &owned,
            &named,
            PaneCheck::AfterProcessStop,
        ));
    }
    let ended = rows
        .attempts
        .iter()
        .filter(|a| a.state == AttemptState::Ended && rows.judged(a));
    for a in ended {
        let named = named_by_attempt(a);
        out.merge_row(clean_row(
            facts,
            &owned,
            &named,
            PaneCheck::AfterProcessStop,
        ));
    }
    out
}

/// I4: the attempt that holds worker `worker_id`, if one does. A worker
/// with one gets no second attempt.
pub fn held_attempt<'a>(worker_id: &str, attempts: &'a [AttemptRow]) -> Option<&'a AttemptRow> {
    attempts
        .iter()
        .find(|a| a.worker_id == worker_id && a.state.holds())
}

/// Whether the process or the pane of `a` is alive.
fn attempt_seen(a: &AttemptRow, facts: &Facts) -> Seen {
    match (a.process, &a.pane) {
        (Some(p), _) => facts.process(p),
        (None, Some(pane)) => match facts.has_pane(pane) {
            Some(true) => Seen::Alive,
            Some(false) => Seen::Dead,
            None => Seen::Unknown,
        },
        (None, None) => Seen::Dead,
    }
}

/// What 1 row names.
struct Named {
    row: RowRef,
    processes: Vec<Recorded>,
    pane: Option<String>,
}

fn named_by_worker(w: &WorkerRow) -> Named {
    Named {
        row: RowRef::Worker(w.id.clone()),
        processes: w.processes().collect(),
        pane: w.pane.clone(),
    }
}

fn named_by_attempt(a: &AttemptRow) -> Named {
    Named {
        row: RowRef::Attempt(a.id.clone()),
        processes: a.process.into_iter().collect(),
        pane: a.pane.clone(),
    }
}

/// The processes and panes that live rows of this host own.
struct Owned {
    pids: BTreeSet<u32>,
    processes: BTreeSet<Recorded>,
    panes: BTreeSet<String>,
}

impl Owned {
    /// What each attempt that holds and each live worker row of this host
    /// owns, the orchestrator row too. The worker of an attempt that holds
    /// owns its processes and pane too. A row of another host owns nothing
    /// here: its PIDs and pane ids name nothing on this machine.
    fn of(rows: &Rows) -> Owned {
        let mut owned = Owned {
            pids: BTreeSet::new(),
            processes: BTreeSet::new(),
            panes: BTreeSet::new(),
        };
        for a in rows.attempts.iter().filter(|a| a.state.holds()) {
            if let Some(w) = rows.workers.get(&a.worker_id).filter(|w| w.here) {
                owned.add(&named_by_attempt(a));
                owned.add(&named_by_worker(w));
            }
        }
        for w in rows.workers.values().filter(|w| w.live && w.here) {
            owned.add(&named_by_worker(w));
        }
        owned
    }

    fn add(&mut self, n: &Named) {
        for p in &n.processes {
            match p.pid_start {
                Some(_) => self.processes.insert(*p),
                // A pid with no start time can be any process with that
                // pid: each one stays.
                None => self.pids.insert(p.pid),
            };
        }
        self.panes.extend(n.pane.clone());
    }

    fn has(&self, p: Recorded) -> bool {
        self.pids.contains(&p.pid) || self.processes.contains(&p)
    }
}

/// When [`clean_row`] looks at the pane of a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PaneCheck {
    /// Always: the pass ends this row now (I1).
    Always,
    /// Only when a process of the row is stopped (I5). An ended row whose
    /// processes are gone names a pane id that herdr can have given to
    /// another pane.
    AfterProcessStop,
}

/// The stops and kept pane of the row `n`. A process is stopped when it is
/// alive with its recorded start time and no live row owns it. `check`
/// says when the pane is looked at. A pane that is looked at is closed
/// when the process in it is a recorded process of the row, else it is
/// kept.
fn clean_row(facts: &Facts, owned: &Owned, n: &Named, check: PaneCheck) -> Cleanup {
    let mut out = Cleanup::default();
    for p in &n.processes {
        let Some(pid_start) = p.pid_start else {
            continue;
        };
        if facts.process(*p) == Seen::Alive && !owned.has(*p) {
            out.stops.push(Stop {
                row: n.row.clone(),
                target: Target::Process {
                    pid: p.pid,
                    pid_start,
                    pgid: Some(p.pid),
                },
            });
        }
    }
    let Some(pane) = &n.pane else {
        return out;
    };
    let looked_at = check == PaneCheck::Always || !out.stops.is_empty();
    if !looked_at || owned.panes.contains(pane) || facts.has_pane(pane) != Some(true) {
        return out;
    }
    if facts.pane_runs(pane, &n.processes) {
        out.stops.push(Stop {
            row: n.row.clone(),
            target: Target::Pane(pane.clone()),
        });
    } else {
        out.kept.push(KeptPane {
            pane: pane.clone(),
            row: n.row.clone(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const AGENT: Recorded = Recorded {
        pid: 100,
        pid_start: Some(1),
    };
    const SUPERVISOR: Recorded = Recorded {
        pid: 101,
        pid_start: Some(2),
    };

    fn worker(id: &str, status: &str, live: bool) -> WorkerRow {
        WorkerRow {
            id: id.into(),
            status: status.into(),
            live,
            orchestrator: false,
            here: true,
            agent: Some(AGENT),
            supervisor: Some(SUPERVISOR),
            pane: Some("w0:p1".into()),
        }
    }

    fn attempt(state: AttemptState) -> AttemptRow {
        AttemptRow {
            id: "T-1/A1".into(),
            task_id: Some("T-1".into()),
            worker_id: "W".into(),
            state,
            process: Some(AGENT),
            pane: Some("w0:p1".into()),
        }
    }

    fn task(used: i64) -> TaskRow {
        TaskRow {
            id: "T-1".into(),
            held_by: Some("W".into()),
            used,
            max_attempts: 3,
        }
    }

    fn rows(w: WorkerRow, a: Option<AttemptRow>, t: Option<TaskRow>) -> Rows {
        Rows {
            attempts: a.into_iter().collect(),
            workers: [(w.id.clone(), w)].into(),
            tasks: t.into_iter().map(|t| (t.id.clone(), t)).collect(),
        }
    }

    fn facts(seen: &[(Recorded, Seen)], panes: &[(&str, Option<Recorded>)]) -> Facts {
        Facts {
            processes: seen.iter().copied().collect(),
            panes: Some(panes.iter().map(|(p, r)| (p.to_string(), *r)).collect()),
        }
    }

    fn process_stop(row: RowRef, p: Recorded) -> Stop {
        Stop {
            row,
            target: Target::Process {
                pid: p.pid,
                pid_start: p.pid_start.unwrap(),
                pgid: Some(p.pid),
            },
        }
    }

    #[test]
    fn at_max_attempts_the_task_escalates_and_below_it_retries() {
        assert_eq!(after_failure(2, 3), AfterFailure::Retry);
        assert_eq!(after_failure(3, 3), AfterFailure::Escalate);
        assert_eq!(after_failure(4, 3), AfterFailure::Escalate);
    }

    #[test]
    fn a_dead_agent_reaps_its_attempt_requeues_the_task_and_stops_the_rest() {
        let r = rows(
            worker("W", "working", true),
            Some(attempt(AttemptState::Live)),
            Some(task(1)),
        );
        let f = facts(
            &[(AGENT, Seen::Dead), (SUPERVISOR, Seen::Alive)],
            &[("w0:p1", None)],
        );
        let reaps = dead_attempts(&r, &f);
        assert_eq!(reaps.len(), 1);
        let reap = &reaps[0];
        assert_eq!(reap.attempt, "T-1/A1");
        assert_eq!(reap.outcome, "no_result");
        assert!(reap.end_worker);
        assert_eq!(reap.task, Some(("T-1".into(), AfterFailure::Retry)));
        assert_eq!(
            reap.cleanup.stops,
            vec![process_stop(RowRef::Worker("W".into()), SUPERVISOR)]
        );
        assert_eq!(
            reap.cleanup.kept,
            vec![KeptPane {
                pane: "w0:p1".into(),
                row: RowRef::Attempt("T-1/A1".into()),
            }]
        );
    }

    #[test]
    fn a_live_or_unknown_process_reaps_nothing() {
        let r = rows(
            worker("W", "working", true),
            Some(attempt(AttemptState::Live)),
            Some(task(1)),
        );
        for seen in [Seen::Alive, Seen::Unknown] {
            assert_eq!(dead_attempts(&r, &facts(&[(AGENT, seen)], &[])), vec![]);
        }
    }

    #[test]
    fn an_attempt_with_no_process_is_judged_by_its_pane() {
        let mut a = attempt(AttemptState::Live);
        a.process = None;
        let r = rows(worker("W", "working", true), Some(a), Some(task(1)));
        assert_eq!(dead_attempts(&r, &facts(&[], &[("w0:p1", None)])), vec![]);
        let no_snapshot = Facts::default();
        assert_eq!(dead_attempts(&r, &no_snapshot), vec![]);
        assert_eq!(dead_attempts(&r, &facts(&[], &[])).len(), 1);
    }

    #[test]
    fn a_task_held_by_another_worker_is_not_touched() {
        let mut t = task(1);
        t.held_by = Some("other".into());
        let r = rows(
            worker("W", "done", false),
            Some(attempt(AttemptState::Live)),
            Some(t),
        );
        let reaps = dead_attempts(&r, &facts(&[(AGENT, Seen::Dead)], &[]));
        assert_eq!(reaps[0].task, None);
        assert!(!reaps[0].end_worker);
        assert_eq!(reaps[0].outcome, "done");
    }

    #[test]
    fn a_task_at_max_attempts_is_escalated() {
        let r = rows(
            worker("W", "working", true),
            Some(attempt(AttemptState::Live)),
            Some(task(3)),
        );
        let reaps = dead_attempts(&r, &facts(&[(AGENT, Seen::Dead)], &[]));
        assert_eq!(reaps[0].task, Some(("T-1".into(), AfterFailure::Escalate)));
    }

    #[test]
    fn the_orchestrator_and_a_row_of_another_host_are_never_judged() {
        for change in [
            |w: &mut WorkerRow| w.orchestrator = true,
            |w: &mut WorkerRow| w.here = false,
        ] {
            let mut w = worker("W", "stopped", false);
            change(&mut w);
            let r = rows(w, Some(attempt(AttemptState::Live)), Some(task(1)));
            let f = facts(&[(AGENT, Seen::Dead), (SUPERVISOR, Seen::Alive)], &[]);
            assert_eq!(dead_attempts(&r, &f), vec![]);
            assert_eq!(orphans(&r, &f), Cleanup::default());
            assert!(r.named_processes().is_empty());
        }
    }

    #[test]
    fn a_live_worker_row_or_a_live_attempt_owns_its_processes() {
        let f = facts(&[(AGENT, Seen::Alive), (SUPERVISOR, Seen::Alive)], &[]);
        let live = rows(worker("W", "claiming", true), None, None);
        assert_eq!(orphans(&live, &f), Cleanup::default());
        let held = rows(
            worker("W", "done", false),
            Some(attempt(AttemptState::Live)),
            None,
        );
        assert_eq!(orphans(&held, &f), Cleanup::default());
    }

    #[test]
    fn an_ended_row_with_a_live_process_gives_stops_processes_first() {
        let r = rows(
            worker("W", "stopped", false),
            Some(attempt(AttemptState::Ended)),
            None,
        );
        let f = facts(
            &[(AGENT, Seen::Alive), (SUPERVISOR, Seen::Alive)],
            &[("w0:p1", Some(AGENT))],
        );
        let out = orphans(&r, &f);
        assert_eq!(
            out.stops,
            vec![
                process_stop(RowRef::Worker("W".into()), AGENT),
                process_stop(RowRef::Worker("W".into()), SUPERVISOR),
                Stop {
                    row: RowRef::Worker("W".into()),
                    target: Target::Pane("w0:p1".into()),
                },
            ]
        );
        assert_eq!(out.kept, vec![]);
    }

    #[test]
    fn a_pane_that_1_row_proves_is_closed_and_not_kept() {
        let r = rows(
            worker("W", "stopped", false),
            Some(attempt(AttemptState::Ended)),
            None,
        );
        // The pane runs the supervisor: the worker row proves it, the
        // attempt row, which names only the agent, does not.
        let f = facts(
            &[(AGENT, Seen::Alive), (SUPERVISOR, Seen::Alive)],
            &[("w0:p1", Some(SUPERVISOR))],
        );
        let out = orphans(&r, &f);
        assert_eq!(out.kept, vec![]);
        assert_eq!(
            out.stops.last().map(|s| &s.target),
            Some(&Target::Pane("w0:p1".into()))
        );
    }

    #[test]
    fn a_live_row_of_another_host_does_not_own_a_local_orphan() {
        let mut r = rows(worker("W", "stopped", false), None, None);
        let mut remote = worker("R", "working", true);
        remote.here = false;
        r.workers.insert("R".into(), remote);
        let f = facts(&[(AGENT, Seen::Alive), (SUPERVISOR, Seen::Dead)], &[]);
        assert_eq!(
            orphans(&r, &f).stops,
            vec![process_stop(RowRef::Worker("W".into()), AGENT)]
        );
    }

    #[test]
    fn a_pid_with_no_start_time_is_never_stopped() {
        let bare = Recorded {
            pid: 100,
            pid_start: None,
        };
        let mut w = worker("W", "stopped", false);
        w.agent = Some(bare);
        w.supervisor = None;
        let out = orphans(&rows(w, None, None), &facts(&[(bare, Seen::Alive)], &[]));
        assert_eq!(out, Cleanup::default());
    }

    #[test]
    fn a_pane_of_an_ended_row_with_no_live_process_is_left_alone() {
        let r = rows(worker("W", "done", false), None, None);
        let f = facts(
            &[(AGENT, Seen::Dead), (SUPERVISOR, Seen::Dead)],
            &[(
                "w0:p1",
                Some(Recorded {
                    pid: 7,
                    pid_start: Some(7),
                }),
            )],
        );
        assert_eq!(orphans(&r, &f), Cleanup::default());
    }

    #[test]
    fn a_held_attempt_blocks_a_second_one() {
        let live = [attempt(AttemptState::Live)];
        assert_eq!(
            held_attempt("W", &live).map(|a| a.id.as_str()),
            Some("T-1/A1")
        );
        assert_eq!(held_attempt("X", &live), None);
        assert_eq!(held_attempt("W", &[attempt(AttemptState::Ended)]), None);
        assert!(held_attempt("W", &[attempt(AttemptState::Launching)]).is_some());
    }

    #[test]
    fn named_processes_are_those_of_rows_of_this_host() {
        let r = rows(
            worker("W", "working", true),
            Some(attempt(AttemptState::Live)),
            None,
        );
        assert_eq!(r.named_processes(), [AGENT, SUPERVISOR].into());
    }

    #[test]
    fn attempt_states_round_trip_their_words() {
        for s in AttemptState::ALL {
            assert_eq!(AttemptState::parse(s.as_str()), Some(s));
        }
        assert_eq!(attempt_id("T-1", 2), "T-1/A2");
    }
}
