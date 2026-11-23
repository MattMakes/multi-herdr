use std::io::{Read as _, Seek as _, SeekFrom, Write as _};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use rusqlite::{params, Connection, OptionalExtension};

use crate::execution::lifecycle::TRANSCRIPT_JSONL;
use crate::fleetd::fleet_runtime::adapters::sqlite::fact_data;
use crate::fleetd::fleet_runtime::application::ledger::{record, transition, Cause};
use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;
use crate::fleetd::fleet_runtime::domain::config::FleetConfig;
use crate::fleetd::fleet_runtime::domain::ledger::{Entity, Generation};
use crate::fleetd::ideas::application::needs_you::{self, NeedsYouList};
use crate::fleetd::ideas::domain::needs_you::Write;
use crate::fleetd::task_execution::adapters::outbox_reader::progress_path;
use crate::fleetd::task_execution::adapters::result_reader::AttemptRef;
use crate::fleetd::task_execution::adapters::sqlite as attempts_store;
use crate::fleetd::task_execution::adapters::task_files::attempt_dir;
use crate::fleetd::task_execution::domain::context_limit::{loops, End, CONTEXT_LIMIT};
use crate::fleetd::task_execution::domain::retry::{
    handoff_text, replan_overdue, HandoffFacts, Outcome,
};
use crate::fleetd::task_execution::domain::task::AttemptState;
use crate::fleetd::worker_profiles::domain::profile::Retry;
use crate::taskledger::model::{Actor, EventKind, TaskStatus, WorkerStatus};
use crate::taskledger::tasks::{self, Requeue};
use crate::taskledger::{util, workers};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Policy {
    pub retry: Retry,
    pub replan_limit_s: u64,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            retry: Retry::default(),
            replan_limit_s: FleetConfig::default().replan_limit_s,
        }
    }
}

pub fn burn_attempt(
    tx: &Connection,
    task_id: &str,
    outcome: Outcome,
    policy: &Policy,
    why: &str,
) -> Result<Requeue> {
    if tx.is_autocommit() {
        bail!("rung 6 writes its rows in 1 open transaction of the ledger");
    }
    if !outcome.burns_attempt() {
        bail!(
            "the outcome {} burns no attempt; rung 6 does not apply",
            outcome.as_str()
        );
    }
    tasks::burn_attempt(
        tx,
        task_id,
        &format!("{why} ({})", outcome.as_str()),
        policy,
    )
}

pub const CONTEXT_LIMIT_END: &str = "context_limit";

pub const CONTEXT_LIMIT_LOOP: &str = "context_limit_loop";

const HANDOFF_MD: &str = "handoff.md";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TreeAtEnd {
    pub head: Option<String>,
    pub diff: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContextLimitMove {
    Ready { handoff: PathBuf },
    Held,
}

pub fn on_context_limit(
    tx: &Connection,
    guard: Option<Generation>,
    fleet_root: &Path,
    tail_bytes: u64,
    attempt: &AttemptRef,
    tree: &TreeAtEnd,
) -> Result<Option<ContextLimitMove>> {
    if tx.is_autocommit() {
        bail!("the context limit writes its rows in 1 open transaction of the ledger");
    }
    let word = Outcome::ContextLimit.as_str();
    let (worker, state, outcome) = attempt_row(tx, &attempt.attempt_id)?;
    let ended = state == AttemptState::Ended.as_str();
    if ended && outcome.as_deref() != Some(word) {
        return Ok(None);
    }
    let task = tasks::get(tx, &attempt.task_id)?;
    if task.status != TaskStatus::Running.as_str()
        || task.current_worker_id.as_deref() != Some(worker.as_str())
    {
        return Ok(None);
    }
    end_at_limit(tx, guard, attempt, &worker, ended, tree.head.as_deref())?;
    let previous = previous_end(tx, attempt)?;
    tx.execute(
        "UPDATE tasks SET attempt_floor = attempt_floor + 1, current_worker_id = NULL,
            not_before = NULL, updated_at = ?2
         WHERE task_id = ?1",
        params![attempt.task_id, util::now()],
    )?;
    let this = End {
        outcome: Some(word),
        head: tree.head.as_deref(),
    };
    if let Some(p) = &previous {
        let before = End {
            outcome: p.outcome.as_deref(),
            head: p.head.as_deref(),
        };
        if loops(Some(&before), &this) {
            hold_loop(tx, guard, attempt, &p.id)?;
            return Ok(Some(ContextLimitMove::Held));
        }
    }
    let handoff = write_handoff(fleet_root, tail_bytes, attempt, &tree.diff)?;
    let path = handoff.to_string_lossy();
    tx.execute(
        "UPDATE workers SET handoff_path = ?2 WHERE worker_id = ?1",
        params![worker, path],
    )?;
    let text = format!(
        "context limit: attempt {} ended at {CONTEXT_LIMIT} tokens; handoff file {path}",
        attempt.attempt_id
    );
    let ready = TaskStatus::Ready.as_str();
    transition(
        tx,
        guard,
        Entity::Task,
        &attempt.task_id,
        ready,
        by_fleetd(&text),
        &NeedsYouList,
    )?;
    Ok(Some(ContextLimitMove::Ready { handoff }))
}

fn by_fleetd(text: &str) -> Cause<'_> {
    Cause {
        actor: Actor::Horch,
        text,
    }
}

fn attempt_row(tx: &Connection, id: &str) -> Result<(String, String, Option<String>)> {
    tx.query_row(
        "SELECT worker_id, state, outcome FROM attempts WHERE id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .optional()?
    .with_context(|| format!("no attempt {id}"))
}

fn end_at_limit(
    tx: &Connection,
    guard: Option<Generation>,
    a: &AttemptRef,
    worker: &str,
    ended: bool,
    head: Option<&str>,
) -> Result<()> {
    let word = Outcome::ContextLimit.as_str();
    let text =
        format!("the wrapper ended it at the context limit of {CONTEXT_LIMIT} tokens: {word}");
    if !ended {
        attempts_store::set_ended(tx, &a.attempt_id, word, &util::now())?;
        let to = AttemptState::Ended.as_str();
        transition(
            tx,
            guard,
            Entity::Attempt,
            &a.attempt_id,
            to,
            by_fleetd(&text),
            &NeedsYouList,
        )?;
    }
    let reason = format!("{word}: attempt {}", a.attempt_id);
    workers::end(tx, worker, WorkerStatus::Incomplete, &reason, None)?;
    let data = serde_json::json!({ "outcome": word, "head": head });
    record(
        tx,
        guard,
        EventKind::Exited,
        Entity::Attempt,
        &a.attempt_id,
        Cause {
            actor: Actor::Horch,
            text: CONTEXT_LIMIT_END,
        },
        &data,
    )?;
    Ok(())
}

struct PreviousEnd {
    id: String,
    outcome: Option<String>,
    head: Option<String>,
}

fn previous_end(tx: &Connection, a: &AttemptRef) -> Result<Option<PreviousEnd>> {
    let row: Option<(String, Option<String>)> = tx
        .query_row(
            "SELECT id, outcome FROM attempts
             WHERE task_id = ?1 AND state = ?3
               AND rowid < (SELECT rowid FROM attempts WHERE id = ?2)
             ORDER BY rowid DESC LIMIT 1",
            params![a.task_id, a.attempt_id, AttemptState::Ended.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((id, outcome)) = row else {
        return Ok(None);
    };
    let data = fact_data(
        tx,
        Entity::Attempt,
        &id,
        EventKind::Exited,
        CONTEXT_LIMIT_END,
    )?;
    let head = data
        .and_then(|d| serde_json::from_str::<serde_json::Value>(&d).ok())
        .and_then(|v| v.get("head")?.as_str().map(str::to_string));
    Ok(Some(PreviousEnd { id, outcome, head }))
}

fn hold_loop(
    tx: &Connection,
    guard: Option<Generation>,
    a: &AttemptRef,
    previous: &str,
) -> Result<()> {
    let task = &a.task_id;
    let text = format!(
        "Task {task} reached the context limit of {CONTEXT_LIMIT} tokens in 2 attempts in a row, \
         {previous} and {}, with no new commit between them. The task is held, so it does not \
         loop. Split the task, or requeue it with horch task requeue {task}.",
        a.attempt_id
    );
    for to in [TaskStatus::Failed, TaskStatus::Escalated] {
        transition(
            tx,
            guard,
            Entity::Task,
            task,
            to.as_str(),
            by_fleetd(&text),
            &NeedsYouList,
        )?;
    }
    tx.execute(
        "UPDATE tasks SET held_reason = ?2, due_at = NULL, updated_at = ?3 WHERE task_id = ?1",
        params![task, "context limit loop", util::now()],
    )?;
    let key = format!("{CONTEXT_LIMIT_LOOP}:{task}");
    needs_you::open_item(
        tx,
        &Write {
            key: &key,
            kind: CONTEXT_LIMIT_LOOP,
            entity: Entity::Task.as_str(),
            entity_id: task,
            text: &text,
        },
    )
}

fn write_handoff(
    fleet_root: &Path,
    tail_bytes: u64,
    a: &AttemptRef,
    diff: &str,
) -> Result<PathBuf> {
    let dir = attempt_dir(fleet_root, &a.task_id, a.attempt());
    let progress = tail(&progress_path(fleet_root, a), tail_bytes)?;
    let transcript = tail(&dir.join(TRANSCRIPT_JSONL), tail_bytes)?;
    let why = format!(
        "The attempt ended because its context reached the limit of {CONTEXT_LIMIT} tokens."
    );
    let text = handoff_text(&HandoffFacts {
        attempt: &a.attempt_id,
        why: &why,
        progress: &progress,
        diff,
        transcript_tail: &transcript,
    });
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = dir.join(HANDOFF_MD);
    let mut options = std::fs::OpenOptions::new();
    options.create(true).write(true).truncate(true);
    let _ = horch_platform::fs::create_mode(&mut options, 0o600);
    options
        .open(&path)
        .and_then(|mut f| f.write_all(text.as_bytes()))
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

fn tail(path: &Path, bytes: u64) -> Result<String> {
    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
        Err(e) => return Err(e).with_context(|| format!("opening {}", path.display())),
    };
    let len = file.metadata()?.len();
    let start = len.saturating_sub(bytes);
    file.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    let text = String::from_utf8_lossy(&buf);
    if start == 0 {
        return Ok(text.into_owned());
    }
    Ok(text
        .split_once('\n')
        .map_or("", |(_, rest)| rest)
        .to_string())
}

pub fn escalate_overdue(
    tx: &Connection,
    clock: FleetSeconds,
    replan_limit_s: u64,
) -> Result<Vec<String>> {
    if tx.is_autocommit() {
        bail!("rung 6 writes its rows in 1 open transaction of the ledger");
    }
    let mut out = Vec::new();
    for t in tasks::failed(tx)? {
        match t.due_at {
            None => tasks::set_replan_due(tx, &t.task_id, clock, replan_limit_s)?,
            Some(d) if replan_overdue(Some(d), clock.0) => {
                tasks::escalate(tx, &t.task_id)?;
                out.push(t.task_id);
            }
            Some(_) => {}
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::fleetd::fleet_runtime::adapters::clock_store::ClockStore;
    use crate::fleetd::fleet_runtime::domain::clock::AwakeReading;
    use crate::fleetd::ideas::application::needs_you;
    use crate::fleetd::task_execution::application::commands;
    use crate::fleetd::task_execution::domain::retry::{rung6, Numbers, Rung6};
    use crate::fleetd::worker_profiles::domain::profile::Retry;
    use crate::taskledger::model::WorktreeMode;
    use crate::taskledger::model::{Actor, WorkerStatus};
    use crate::taskledger::runs::{self, NewRun, Run};
    use crate::taskledger::tasks::{NewTask, Task};
    use crate::taskledger::workers::{self, NewWorker};
    use crate::taskledger::Store;

    struct Fixture {
        _dir: tempfile::TempDir,
        store: Store,
        run: Run,
        task: String,
    }

    fn fixture(clock: i64) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(&dir.path().join("ledger.sqlite3")).unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
                 VALUES (1, 1, 1, 1, 'then', ?1, 'then')",
                [clock],
            )
            .unwrap();
        let new_run = NewRun {
            slug: "r1".into(),
            ..Default::default()
        };
        let run = store
            .immediate(|tx| {
                let run = runs::create(tx, dir.path(), &new_run)?;
                crate::taskledger::schema::run_idea(
                    tx,
                    &run.run_id,
                    &crate::taskledger::util::now(),
                    true,
                )?;
                Ok(run)
            })
            .unwrap();
        let mut new = NewTask::work("a", "tickets/a.md", "dev");
        new.worktree_mode = WorktreeMode::Shared;
        let task = store
            .immediate(|tx| tasks::add(tx, &run, &new, Actor::Orchestrator))
            .unwrap()
            .task_id;
        Fixture {
            _dir: dir,
            store,
            run,
            task,
        }
    }

    impl Fixture {
        fn set_clock(&mut self, clock: i64) {
            self.store
                .conn()
                .execute("UPDATE fleetd SET fleet_clock_s = ?1 WHERE id = 1", [clock])
                .unwrap();
        }

        fn task(&self) -> Task {
            tasks::get(self.store.conn(), &self.task).unwrap()
        }

        fn claim(&mut self) -> Option<String> {
            let conn = self.store.conn();
            let startable = tasks::startable(conn, &self.run.run_id).unwrap();
            startable.iter().find(|t| t.task_id == self.task)?;
            let new = NewWorker {
                run_id: self.run.run_id.clone(),
                persona: "dev".into(),
                ..Default::default()
            };
            crate::taskledger::testing::bind_live(conn, &self.task, &new, None).unwrap();
            Some(self.task.clone())
        }

        fn fail(&mut self, outcome: Outcome) -> Requeue {
            let task = self.task.clone();
            let worker = self.task().current_worker_id.unwrap();
            self.store
                .immediate(|tx| {
                    commands::end_own_attempt(tx, &worker, "crashed")?;
                    workers::end(tx, &worker, WorkerStatus::Incomplete, "test", None)?;
                    burn_attempt(tx, &task, outcome, &Policy::default(), "the worker ended")
                })
                .unwrap()
        }

        fn claim_and_fail(&mut self, clock: i64) -> Requeue {
            self.set_clock(clock);
            assert_eq!(self.claim().as_deref(), Some(self.task.as_str()));
            self.fail(Outcome::Crashed)
        }

        fn replan(&mut self) {
            let t = self.task();
            self.store
                .immediate(|tx| tasks::requeue_by_operator(tx, &t, "replan: the ticket is fixed"))
                .unwrap();
        }

        fn overdue(&mut self, clock: i64) -> Vec<String> {
            self.store
                .immediate(|tx| escalate_overdue(tx, FleetSeconds(clock), 1800))
                .unwrap()
        }

        fn replan_wakes(&self) -> Vec<String> {
            let mut stmt = self
                .store
                .conn()
                .prepare("SELECT text FROM outbox WHERE key LIKE '%:replan:%' ORDER BY rowid")
                .unwrap();
            stmt.query_map([], |r| r.get(0))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        }

        fn escalated_items(&self) -> Vec<String> {
            needs_you::open_items(self.store.conn())
                .unwrap()
                .into_iter()
                .filter(|i| i.key.starts_with("task_escalated:"))
                .map(|i| i.key)
                .collect()
        }
    }

    #[test]
    fn ready_task_is_not_claimed_before_not_before() {
        let mut l = fixture(100);
        assert_eq!(l.claim_and_fail(100), Requeue::Queued);
        let t = l.task();
        assert_eq!(t.status, "ready");
        assert_eq!(t.not_before, Some(130), "30 s on the fleet clock");
        l.set_clock(129);
        assert_eq!(l.claim(), None, "the claim waits for the backoff");
        let waits = tasks::waits(l.store.conn(), &l.run.run_id).unwrap();
        assert_eq!(waits.len(), 1);
        assert_eq!(waits[0].1.to_string(), "retry after 1 s");
        l.set_clock(130);
        assert_eq!(l.claim().as_deref(), Some(l.task.as_str()));
    }

    #[test]
    fn sleep_past_a_backoff_fires_it_only_after_the_rest_of_its_fleet_time() {
        let mut l = fixture(0);
        let boot = |s: u64| AwakeReading {
            boot_id: "b1".into(),
            awake_ms: s * 1000,
        };
        let mut clock = ClockStore::new();
        let mut tick = |l: &mut Fixture, now: AwakeReading| {
            let t = l.store.immediate(|tx| clock.tick(tx, &now, 5)).unwrap();
            clock.committed(t.clone());
            t.clock.0
        };
        tick(&mut l, boot(0));
        assert_eq!(tick(&mut l, boot(5)), 5);
        assert_eq!(l.claim_and_fail(5), Requeue::Queued);
        assert_eq!(l.task().not_before, Some(35));
        assert_eq!(tick(&mut l, boot(5)), 5);
        assert_eq!(l.claim(), None);
        let mut now = 5;
        while now < 30 {
            now += 5;
            assert_eq!(tick(&mut l, boot(now)), i64::try_from(now).unwrap());
            assert_eq!(l.claim(), None, "fleet clock {now}");
        }
        assert_eq!(tick(&mut l, boot(35)), 35);
        assert_eq!(l.claim().as_deref(), Some(l.task.as_str()));
    }

    #[test]
    fn failed_task_wakes_the_orchestrator_once() {
        let mut l = fixture(0);
        assert_eq!(l.claim_and_fail(0), Requeue::Queued);
        assert_eq!(l.claim_and_fail(30), Requeue::Queued);
        assert_eq!(l.claim_and_fail(150), Requeue::Failed);
        let t = l.task();
        assert_eq!(t.status, "failed");
        assert_eq!(t.due_at, Some(150 + 1800));
        assert_eq!(t.not_before, None);
        let wakes = l.replan_wakes();
        assert_eq!(wakes.len(), 1, "{wakes:?}");
        let short = t.short();
        assert!(
            wakes[0].contains(&format!("replan {short}: requeue it 1 time, or cancel it")),
            "{}",
            wakes[0]
        );
        assert!(wakes[0].contains(&format!("horch ledger requeue {short}")));
        assert!(wakes[0].contains(&format!("horch ledger cancel {short}")));
        assert_eq!(l.overdue(1000), Vec::<String>::new());
        assert_eq!(l.replan_wakes().len(), 1);
        assert_eq!(l.escalated_items(), Vec::<String>::new());
        assert_eq!(l.claim(), None, "a failed task is not claimed");
    }

    #[test]
    fn requeue_of_a_failed_task_is_the_1_replan() {
        let mut l = fixture(0);
        for clock in [0, 30, 150] {
            l.claim_and_fail(clock);
        }
        l.replan();
        let t = l.task();
        assert_eq!(t.status, "ready");
        assert_eq!(t.replans, 1);
        assert_eq!(t.attempt_floor, t.attempts, "the attempt count is 0 again");
        assert_eq!((t.due_at, t.not_before), (None, None));
        assert_eq!(l.claim().as_deref(), Some(l.task.as_str()));
    }

    #[test]
    fn second_failure_after_the_replan_escalates_with_1_needs_you_item() {
        let mut l = fixture(0);
        for clock in [0, 30, 150] {
            l.claim_and_fail(clock);
        }
        l.replan();
        assert_eq!(l.claim_and_fail(200), Requeue::Queued);
        assert_eq!(l.claim_and_fail(230), Requeue::Queued);
        assert_eq!(l.claim_and_fail(350), Requeue::Escalated);
        let t = l.task();
        assert_eq!(t.status, "escalated");
        assert_eq!(t.held_reason.as_deref(), Some("3 attempts failed"));
        assert_eq!(l.replan_wakes().len(), 1, "no second replan wake");
        assert_eq!(l.escalated_items(), [format!("task_escalated:{}", l.task)]);
        let moves: Vec<(String, String)> = {
            let mut stmt = l
                .store
                .conn()
                .prepare(
                    "SELECT from_state, to_state FROM events
                     WHERE entity = 'task' AND entity_id = ?1
                       AND to_state IN ('failed', 'escalated')
                     ORDER BY event_id",
                )
                .unwrap();
            stmt.query_map([&l.task], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        };
        let moves: Vec<(&str, &str)> = moves
            .iter()
            .map(|(f, t)| (f.as_str(), t.as_str()))
            .collect();
        assert_eq!(
            moves,
            [
                ("running", "failed"),
                ("running", "failed"),
                ("failed", "escalated")
            ],
            "the task reaches escalated through failed"
        );
        l.replan();
        assert_eq!(l.task().status, "ready");
        assert_eq!(l.escalated_items(), Vec::<String>::new());
    }

    #[test]
    fn cancel_of_an_escalated_task_resolves_its_needs_you_item() {
        let mut l = fixture(0);
        for clock in [0, 30, 150] {
            l.claim_and_fail(clock);
        }
        l.overdue(150 + 1800);
        assert_eq!(l.escalated_items().len(), 1);
        let t = l.task();
        l.store
            .immediate(|tx| tasks::cancel(tx, &t, "the plan drops it"))
            .unwrap();
        assert_eq!(l.task().status, "cancelled");
        assert_eq!(l.escalated_items(), Vec::<String>::new());
    }

    #[test]
    fn failed_task_past_replan_limit_escalates() {
        let mut l = fixture(0);
        for clock in [0, 30, 150] {
            l.claim_and_fail(clock);
        }
        assert_eq!(l.overdue(150 + 1799), Vec::<String>::new());
        assert_eq!(l.overdue(150 + 1800), [l.task.clone()]);
        let t = l.task();
        assert_eq!(t.status, "escalated");
        assert_eq!(t.held_reason.as_deref(), Some("3 attempts failed"));
        assert_eq!(l.escalated_items(), [format!("task_escalated:{}", l.task)]);
        assert_eq!(l.overdue(9999), Vec::<String>::new(), "1 escalation");
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    enum State {
        Ready,
        Running,
        Failed,
        Escalated,
    }

    #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
    struct Node {
        state: State,
        used: i64,
        replans: i64,
    }

    fn moves(n: &Node, policy: &Policy, run_max: i64) -> Vec<(bool, Node)> {
        let limit =
            crate::fleetd::task_execution::domain::retry::limit(policy.retry.max_attempts, run_max);
        let to = |state, used, replans| Node {
            state,
            used,
            replans,
        };
        match n.state {
            State::Ready => vec![(false, to(State::Running, n.used + 1, n.replans))],
            State::Running => Outcome::ALL
                .into_iter()
                .filter(|o| o.burns_attempt())
                .map(|_| {
                    let nums = Numbers {
                        used: n.used,
                        limit,
                        replans: n.replans,
                    };
                    let next = match rung6(&nums, &policy.retry.backoff_s, 1, 0) {
                        Rung6::Retry { .. } => to(State::Ready, n.used, n.replans),
                        Rung6::Fail { .. } => to(State::Failed, n.used, n.replans),
                        Rung6::Escalate => to(State::Escalated, n.used, n.replans),
                    };
                    (true, next)
                })
                .collect(),
            State::Failed => {
                let mut out = vec![(false, to(State::Escalated, n.used, n.replans))];
                if n.replans == 0 {
                    out.push((false, to(State::Ready, 0, 1)));
                }
                out
            }
            State::Escalated => Vec::new(),
        }
    }

    #[test]
    fn rr6_no_sequence_of_outcomes_retries_forever() {
        for (max, run_max) in [(None, 1), (None, 3), (Some(2), 3), (Some(5), 1)] {
            let policy = Policy {
                retry: Retry {
                    max_attempts: max,
                    ..Retry::default()
                },
                ..Policy::default()
            };
            let limit = crate::fleetd::task_execution::domain::retry::limit(
                policy.retry.max_attempts,
                run_max,
            );
            let start = Node {
                state: State::Ready,
                used: 0,
                replans: 0,
            };
            let mut most = 0;
            let mut stack = vec![(start, 0_i64, BTreeSet::new())];
            while let Some((n, burned, seen)) = stack.pop() {
                assert!(burned <= 2 * limit, "{n:?}: {burned} burned attempts");
                let next: BTreeSet<(bool, Node)> =
                    moves(&n, &policy, run_max).into_iter().collect();
                if next.is_empty() {
                    assert_eq!(n.state, State::Escalated, "{n:?} has no exit");
                    most = most.max(burned);
                }
                for (burns, m) in next {
                    let mut path: BTreeSet<Node> = seen.clone();
                    assert!(path.insert(n.clone()), "{n:?} repeats: a retry loop");
                    stack.push((m, burned + i64::from(burns), path));
                }
            }
            assert_eq!(most, 2 * limit, "max {max:?}, run {run_max}");
        }
    }
}
