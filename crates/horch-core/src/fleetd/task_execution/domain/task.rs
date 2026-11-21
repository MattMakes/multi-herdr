use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Recorded {
    pub pid: u32,
    pub pid_start: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seen {
    Alive,
    Dead,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttemptState {
    Launching,
    Live,
    Ended,
}

impl AttemptState {
    pub const ALL: [AttemptState; 3] = [
        AttemptState::Launching,
        AttemptState::Live,
        AttemptState::Ended,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            AttemptState::Launching => "launching",
            AttemptState::Live => "live",
            AttemptState::Ended => "ended",
        }
    }

    pub fn parse(word: &str) -> Option<AttemptState> {
        Self::ALL.into_iter().find(|s| s.as_str() == word)
    }

    pub fn holds(self) -> bool {
        self != AttemptState::Ended
    }
}

pub fn attempt_id(task_id: &str, n: i64) -> String {
    format!("{task_id}/A{n}")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttemptRow {
    pub id: String,
    pub task_id: Option<String>,
    pub worker_id: String,
    pub state: AttemptState,
    pub process: Option<Recorded>,
    pub pane: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkerRow {
    pub id: String,
    pub status: String,
    pub live: bool,
    pub orchestrator: bool,
    pub here: bool,
    pub agent: Option<Recorded>,
    pub supervisor: Option<Recorded>,
    pub pane: Option<String>,
}

impl WorkerRow {
    fn judged(&self) -> bool {
        self.here && !self.orchestrator
    }

    fn processes(&self) -> impl Iterator<Item = Recorded> {
        self.agent.into_iter().chain(self.supervisor)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskRow {
    pub id: String,
    pub held_by: Option<String>,
    pub used: i64,
    pub max_attempts: i64,
}

pub fn promotable(
    statuses: &BTreeMap<String, String>,
    deps: &[(String, String)],
    landed: &BTreeMap<String, bool>,
) -> Vec<String> {
    let deps: Vec<Dep> = deps
        .iter()
        .map(|(task, on)| Dep {
            task: task.clone(),
            on: on.clone(),
            kind: DepKind::After,
        })
        .collect();
    promotable_deps(statuses, &deps, landed, &BTreeSet::new())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DepKind {
    After,
    Judges,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dep {
    pub task: String,
    pub on: String,
    pub kind: DepKind,
}

pub fn judged_task<'a>(kind: &str, parent_task_id: Option<&'a str>) -> Option<&'a str> {
    parent_task_id.filter(|_| kind == "work")
}

pub fn promotable_deps(
    statuses: &BTreeMap<String, String>,
    deps: &[Dep],
    landed: &BTreeMap<String, bool>,
    structural_passed: &BTreeSet<String>,
) -> Vec<String> {
    let met = |d: &Dep| {
        let status = statuses.get(&d.on).map(String::as_str);
        match d.kind {
            DepKind::After => status == Some("done") && landed.get(&d.on).copied().unwrap_or(true),
            DepKind::Judges => status == Some("verifying") && structural_passed.contains(&d.on),
        }
    };
    let unmet: BTreeSet<&String> = deps.iter().filter(|d| !met(d)).map(|d| &d.task).collect();
    statuses
        .iter()
        .filter(|(id, status)| *status == "pending" && !unmet.contains(id))
        .map(|(id, _)| id.clone())
        .collect()
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rows {
    pub attempts: Vec<AttemptRow>,
    pub workers: BTreeMap<String, WorkerRow>,
    pub tasks: BTreeMap<String, TaskRow>,
}

impl Rows {
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

    fn judged(&self, a: &AttemptRow) -> bool {
        self.workers
            .get(&a.worker_id)
            .is_some_and(WorkerRow::judged)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    pub processes: BTreeMap<Recorded, Seen>,
    pub panes: Option<BTreeMap<String, InPane>>,
    pub stopped: Vec<Stopped>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InPane {
    Unknown,
    Runs { pid: u32, pid_start: u64 },
    Gone(u32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stopped {
    pub process: Recorded,
    pub proof: PaneProof,
    pub group: GroupLeft,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GroupLeft {
    #[default]
    None,
    Members,
    NewLeader,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaneProof {
    None,
    Pane(String),
    Unreadable,
}

impl Facts {
    fn process(&self, p: Recorded) -> Seen {
        self.processes.get(&p).copied().unwrap_or(Seen::Unknown)
    }

    fn has_pane(&self, pane: &str) -> Option<bool> {
        self.panes.as_ref().map(|p| p.contains_key(pane))
    }

    fn in_pane(&self, pane: &str) -> Option<InPane> {
        self.panes.as_ref().and_then(|p| p.get(pane)).copied()
    }

    fn pane_runs(&self, pane: &str, recorded: &[Recorded]) -> Option<Recorded> {
        let Some(InPane::Runs { pid, pid_start }) = self.in_pane(pane) else {
            return None;
        };
        let p = Recorded {
            pid,
            pid_start: Some(pid_start),
        };
        recorded.contains(&p).then_some(p)
    }

    fn pane_names_gone(&self, pane: &str, shell: Recorded) -> bool {
        self.in_pane(pane) == Some(InPane::Gone(shell.pid)) && self.process(shell) == Seen::Dead
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AfterFailure {
    Retry,
    Escalate,
}

pub fn after_failure(used: i64, max_attempts: i64) -> AfterFailure {
    if used >= max_attempts {
        AfterFailure::Escalate
    } else {
        AfterFailure::Retry
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RowRef {
    Worker(String),
    Attempt(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessScope {
    Alone,
    LeaderGroup,
    OrphanGroup,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Process {
        pid: u32,
        pid_start: u64,
        scope: ProcessScope,
    },
    Pane {
        pane: String,
        shell: Recorded,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stop {
    pub row: RowRef,
    pub target: Target,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeptPane {
    pub pane: String,
    pub row: RowRef,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeptGroup {
    pub group: u32,
    pub row: RowRef,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cleanup {
    pub stops: Vec<Stop>,
    pub kept: Vec<KeptPane>,
    pub kept_groups: Vec<KeptGroup>,
}

impl Cleanup {
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
        for g in other.kept_groups {
            if !self.kept_groups.iter().any(|m| m.group == g.group) {
                self.kept_groups.push(g);
            }
        }
        let stops = &self.stops;
        self.kept.retain(|k| {
            !stops
                .iter()
                .any(|s| matches!(&s.target, Target::Pane { pane, .. } if *pane == k.pane))
        });
        self.stops
            .sort_by_key(|s| matches!(s.target, Target::Pane { .. }));
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reap {
    pub attempt: String,
    pub worker: String,
    pub outcome: String,
    pub end_worker: bool,
    pub task: Option<(String, AfterFailure)>,
    pub cleanup: Cleanup,
}

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
    let naming = naming(rows, &facts.stopped);
    for s in &facts.stopped {
        let named = &naming[&s.process];
        out.merge_row(group_left(named, &owned, s));
        out.merge_row(after_polite_stop(named, facts, &owned, s));
    }
    out
}

fn group_left(named: &[Named], owned: &Owned, s: &Stopped) -> Cleanup {
    let mut out = Cleanup::default();
    let (Some(pid_start), false) = (s.process.pid_start, owned.has(s.process)) else {
        return out;
    };
    for n in named {
        match s.group {
            GroupLeft::None => {}
            GroupLeft::Members => out.stops.push(Stop {
                row: n.row.clone(),
                target: Target::Process {
                    pid: s.process.pid,
                    pid_start,
                    scope: ProcessScope::OrphanGroup,
                },
            }),
            GroupLeft::NewLeader => out.kept_groups.push(KeptGroup {
                group: s.process.pid,
                row: n.row.clone(),
            }),
        }
    }
    out
}

fn naming(rows: &Rows, stopped: &[Stopped]) -> BTreeMap<Recorded, Vec<Named>> {
    let mut by: BTreeMap<Recorded, Vec<Named>> =
        stopped.iter().map(|s| (s.process, Vec::new())).collect();
    let workers = rows
        .workers
        .values()
        .filter(|w| w.judged())
        .map(named_by_worker);
    let attempts = rows
        .attempts
        .iter()
        .filter(|a| rows.judged(a))
        .map(named_by_attempt);
    for n in workers.chain(attempts) {
        for p in &n.processes {
            if let Some(list) = by.get_mut(p) {
                list.push(n.clone());
            }
        }
    }
    by
}

pub fn needs_snapshot(rows: &Rows, stopped: &[Stopped]) -> BTreeSet<Recorded> {
    naming(rows, stopped)
        .into_iter()
        .filter(|(_, named)| named.iter().any(|n| n.pane.is_some()))
        .map(|(p, _)| p)
        .collect()
}

fn after_polite_stop(named: &[Named], facts: &Facts, owned: &Owned, s: &Stopped) -> Cleanup {
    let mut out = Cleanup::default();
    for n in named {
        let Some(pane) = &n.pane else {
            continue;
        };
        let all_gone = || n.processes.iter().all(|p| facts.process(*p) == Seen::Dead);
        let proven = match &s.proof {
            PaneProof::None if !all_gone() => continue,
            PaneProof::Pane(p) if p != pane => continue,
            PaneProof::Pane(_) => true,
            PaneProof::None | PaneProof::Unreadable => false,
        };
        if owned.panes.contains(pane) || facts.has_pane(pane) != Some(true) {
            continue;
        }
        if proven && facts.pane_names_gone(pane, s.process) {
            out.stops.push(Stop {
                row: n.row.clone(),
                target: Target::Pane {
                    pane: pane.clone(),
                    shell: s.process,
                },
            });
        } else {
            out.kept.push(KeptPane {
                pane: pane.clone(),
                row: n.row.clone(),
            });
        }
    }
    out
}

pub fn held_attempt<'a>(worker_id: &str, attempts: &'a [AttemptRow]) -> Option<&'a AttemptRow> {
    attempts
        .iter()
        .find(|a| a.worker_id == worker_id && a.state.holds())
}

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

#[derive(Clone)]
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

struct Owned {
    pids: BTreeSet<u32>,
    processes: BTreeSet<Recorded>,
    panes: BTreeSet<String>,
}

impl Owned {
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
                None => self.pids.insert(p.pid),
            };
        }
        self.panes.extend(n.pane.clone());
    }

    fn has(&self, p: Recorded) -> bool {
        self.pids.contains(&p.pid) || self.processes.contains(&p)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PaneCheck {
    Always,
    AfterProcessStop,
}

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
                    scope: ProcessScope::LeaderGroup,
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
    if let Some(shell) = facts.pane_runs(pane, &n.processes) {
        out.stops.push(Stop {
            row: n.row.clone(),
            target: Target::Pane {
                pane: pane.clone(),
                shell,
            },
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

    fn runs(p: Recorded) -> InPane {
        InPane::Runs {
            pid: p.pid,
            pid_start: p.pid_start.unwrap(),
        }
    }

    fn facts(seen: &[(Recorded, Seen)], panes: &[(&str, InPane)]) -> Facts {
        Facts {
            processes: seen.iter().copied().collect(),
            panes: Some(panes.iter().map(|(p, r)| (p.to_string(), *r)).collect()),
            stopped: Vec::new(),
        }
    }

    fn process_stop(row: RowRef, p: Recorded) -> Stop {
        Stop {
            row,
            target: Target::Process {
                pid: p.pid,
                pid_start: p.pid_start.unwrap(),
                scope: ProcessScope::LeaderGroup,
            },
        }
    }

    fn statuses(rows: &[(&str, &str)]) -> BTreeMap<String, String> {
        rows.iter()
            .map(|(id, s)| (id.to_string(), s.to_string()))
            .collect()
    }

    fn deps(rows: &[(&str, &str)]) -> Vec<(String, String)> {
        rows.iter()
            .map(|(t, d)| (t.to_string(), d.to_string()))
            .collect()
    }

    #[test]
    fn pending_task_with_each_dependency_done_is_promotable() {
        let tasks = statuses(&[
            ("T-1", "done"),
            ("T-2", "done"),
            ("T-3", "pending"),
            ("T-4", "pending"),
            ("T-5", "ready"),
        ]);
        let deps = deps(&[("T-3", "T-1"), ("T-3", "T-2"), ("T-5", "T-1")]);
        assert_eq!(
            promotable(&tasks, &deps, &BTreeMap::new()),
            vec!["T-3", "T-4"]
        );
    }

    #[test]
    fn pending_task_with_a_dependency_not_done_is_not_promotable() {
        let tasks = statuses(&[
            ("T-1", "done"),
            ("T-2", "verifying"),
            ("T-3", "pending"),
            ("T-4", "pending"),
            ("T-5", "cancelled"),
            ("T-6", "pending"),
        ]);
        let deps = deps(&[
            ("T-3", "T-1"),
            ("T-3", "T-2"),
            ("T-4", "T-5"),
            ("T-6", "T-9"),
        ]);
        assert_eq!(
            promotable(&tasks, &deps, &BTreeMap::new()),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_done_dependency_with_a_branch_row_is_met_only_when_landed() {
        let tasks = statuses(&[
            ("T-1", "done"),
            ("T-2", "done"),
            ("T-3", "done"),
            ("T-4", "pending"),
            ("T-5", "pending"),
            ("T-6", "pending"),
        ]);
        let deps = deps(&[("T-4", "T-1"), ("T-5", "T-2"), ("T-6", "T-3")]);
        let landed = BTreeMap::from([("T-1".to_string(), false), ("T-2".to_string(), true)]);
        assert_eq!(promotable(&tasks, &deps, &landed), vec!["T-5", "T-6"]);
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
            &[("w0:p1", InPane::Unknown)],
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
        assert_eq!(
            dead_attempts(&r, &facts(&[], &[("w0:p1", InPane::Unknown)])),
            vec![]
        );
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
            &[("w0:p1", runs(AGENT))],
        );
        let out = orphans(&r, &f);
        assert_eq!(
            out.stops,
            vec![
                process_stop(RowRef::Worker("W".into()), AGENT),
                process_stop(RowRef::Worker("W".into()), SUPERVISOR),
                Stop {
                    row: RowRef::Worker("W".into()),
                    target: Target::Pane {
                        pane: "w0:p1".into(),
                        shell: AGENT,
                    },
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
        let f = facts(
            &[(AGENT, Seen::Alive), (SUPERVISOR, Seen::Alive)],
            &[("w0:p1", runs(SUPERVISOR))],
        );
        let out = orphans(&r, &f);
        assert_eq!(out.kept, vec![]);
        assert_eq!(
            out.stops.last().map(|s| &s.target),
            Some(&Target::Pane {
                pane: "w0:p1".into(),
                shell: SUPERVISOR,
            })
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
                InPane::Runs {
                    pid: 7,
                    pid_start: 7,
                },
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

    fn after_stop(proof: PaneProof, inside: InPane) -> (Rows, Facts) {
        let r = rows(worker("W", "stopped", false), None, None);
        let mut f = facts(
            &[(AGENT, Seen::Dead), (SUPERVISOR, Seen::Dead)],
            &[("w0:p1", inside)],
        );
        f.stopped = vec![Stopped {
            process: SUPERVISOR,
            proof,
            group: GroupLeft::None,
        }];
        (r, f)
    }

    fn group_after_stop(group: GroupLeft) -> (Rows, Facts) {
        let (r, mut f) = after_stop(PaneProof::None, InPane::Unknown);
        f.stopped[0].group = group;
        (r, f)
    }

    #[test]
    fn a_group_whose_leader_is_gone_is_stopped_by_its_id() {
        let (r, f) = group_after_stop(GroupLeft::Members);
        let out = orphans(&r, &f);
        assert_eq!(
            out.stops,
            [Stop {
                row: RowRef::Worker("W".into()),
                target: Target::Process {
                    pid: SUPERVISOR.pid,
                    pid_start: SUPERVISOR.pid_start.unwrap(),
                    scope: ProcessScope::OrphanGroup,
                },
            }]
        );
        assert!(out.kept_groups.is_empty());
    }

    #[test]
    fn a_new_leader_with_the_pid_of_the_gone_leader_keeps_its_group() {
        let (r, f) = group_after_stop(GroupLeft::NewLeader);
        let out = orphans(&r, &f);
        assert!(out.stops.is_empty(), "{:?}", out.stops);
        assert_eq!(
            out.kept_groups,
            [KeptGroup {
                group: SUPERVISOR.pid,
                row: RowRef::Worker("W".into()),
            }]
        );
    }

    #[test]
    fn a_gone_leader_with_no_group_left_gets_no_stop() {
        let (r, f) = group_after_stop(GroupLeft::None);
        let out = orphans(&r, &f);
        assert!(out.stops.is_empty(), "{:?}", out.stops);
        assert!(out.kept_groups.is_empty(), "{:?}", out.kept_groups);
    }

    const GONE: InPane = InPane::Gone(101);

    fn kept_pane() -> Cleanup {
        Cleanup {
            stops: vec![],
            kept: vec![KeptPane {
                pane: "w0:p1".into(),
                row: RowRef::Worker("W".into()),
            }],
            kept_groups: vec![],
        }
    }

    #[test]
    fn a_pane_proven_at_the_polite_stop_closes_after_the_process_ends() {
        let (r, f) = after_stop(PaneProof::Pane("w0:p1".into()), GONE);
        assert_eq!(
            orphans(&r, &f).stops,
            vec![Stop {
                row: RowRef::Worker("W".into()),
                target: Target::Pane {
                    pane: "w0:p1".into(),
                    shell: SUPERVISOR,
                },
            }]
        );
    }

    #[test]
    fn a_pane_with_another_process_or_none_after_the_polite_stop_is_kept() {
        let other = Recorded {
            pid: 300,
            pid_start: Some(9),
        };
        let reused = Recorded {
            pid: 101,
            pid_start: Some(9),
        };
        for inside in [runs(other), runs(reused), InPane::Unknown] {
            let (r, f) = after_stop(PaneProof::Pane("w0:p1".into()), inside);
            assert_eq!(orphans(&r, &f), kept_pane(), "{inside:?}");
        }
    }

    #[test]
    fn an_unreadable_record_keeps_the_pane_of_the_row() {
        let (r, f) = after_stop(PaneProof::Unreadable, GONE);
        assert_eq!(orphans(&r, &f), kept_pane());
    }

    #[test]
    fn a_record_of_another_pane_does_nothing() {
        let (r, f) = after_stop(PaneProof::Pane("w0:p9".into()), GONE);
        assert_eq!(orphans(&r, &f), Cleanup::default());
    }

    #[test]
    fn a_record_with_no_pane_keeps_the_pane_once_each_process_of_the_row_is_gone() {
        let (r, f) = after_stop(PaneProof::None, GONE);
        assert_eq!(orphans(&r, &f), kept_pane());
        let (r, mut f) = after_stop(PaneProof::None, GONE);
        f.processes.insert(AGENT, Seen::Unknown);
        assert_eq!(orphans(&r, &f), Cleanup::default());
    }

    #[test]
    fn a_record_waits_for_a_snapshot_only_when_a_row_names_its_process_and_a_pane() {
        let (mut r, f) = after_stop(PaneProof::None, GONE);
        assert_eq!(needs_snapshot(&r, &f.stopped), BTreeSet::from([SUPERVISOR]));
        r.workers.get_mut("W").unwrap().pane = None;
        assert_eq!(needs_snapshot(&r, &f.stopped), BTreeSet::new());
    }

    #[test]
    fn a_gone_pane_no_snapshot_or_a_live_owner_does_nothing_after_the_polite_stop() {
        let proof = || PaneProof::Pane("w0:p1".into());
        let (r, mut f) = after_stop(proof(), GONE);
        f.panes = Some(BTreeMap::new());
        assert_eq!(orphans(&r, &f), Cleanup::default());
        f.panes = None;
        assert_eq!(orphans(&r, &f), Cleanup::default());
        let (mut r, f) = after_stop(proof(), GONE);
        let mut other = worker("V", "working", true);
        other.agent = None;
        other.supervisor = None;
        r.workers.insert("V".into(), other);
        assert_eq!(orphans(&r, &f), Cleanup::default());
    }
}
