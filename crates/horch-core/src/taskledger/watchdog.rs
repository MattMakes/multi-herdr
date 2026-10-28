//! The watchdog's checks (section 5.3, 10, #42): no worker works silently.
//!
//! Every `check_every_s` each working worker gets 1 `checks` row. The rules,
//! in order:
//!
//! 1. The agent's process is gone: `dead` at once. A process that does not
//!    exist cannot resume.
//! 2. The agent recorded progress since the last check: `progress`.
//! 3. A proxy checkin: the transcript grew, herdr's state counter moved, or
//!    the working timer ran the whole interval (10m, then 20m, 10 min
//!    apart). The watchdog records progress for the agent.
//! 4. Suspect: the timer ran less than the clock (10m, then 15m, 10 min
//!    apart: it stopped about 5 min ago). Verified at once: herdr still
//!    showing `working` makes it a proxy checkin; else `no_progress`.
//! 5. Nothing moved: `no_progress`, and 1 more stall round. At the run's
//!    `stall_rounds` the worker is `dead`.
//!
//! A worker that waits for something horch knows about (#70) is not
//! stalled while the wait holds and its deadline has not passed: its check
//! is `progress` with a note that starts "waiting", and the stall count
//! starts fresh. The waits, in order: reviewer tasks its `finish` queued, an
//! open question it sent the orchestrator, a `horch ledger progress
//! "waiting: ..."` for a background command. A gone process is still `dead`.
//!
//! This module decides and records; the caller reaps and wakes.

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::Serialize;

use crate::messaging::questions::OpenQuestion;

use super::model::{EventKind, ProgressSource, Verdict};
use super::runs::Run;
use super::store::{optional, Store};
use super::timer;
use super::util;
use super::workers::{self, Worker};

/// What the watchdog sees of 1 worker now.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Observation {
    /// The agent PID runs with its recorded start time.
    pub agent_alive: bool,
    pub herdr_status: Option<String>,
    pub herdr_seq: Option<i64>,
    pub transcript_bytes: Option<i64>,
    pub transcript_mtime: Option<String>,
    /// The visible pane text, for the working timer.
    pub screen: Option<String>,
    /// The worker's open question to the orchestrator, if any.
    pub open_question: Option<OpenQuestion>,
}

/// How long each wait holds at most, from its start.
pub const REVIEW_WAIT_S: i64 = 3 * 3600;
pub const QUESTION_WAIT_S: i64 = 2 * 3600;
pub const BACKGROUND_WAIT_S: i64 = 3600;

/// The `horch ledger progress` text that starts a background wait.
const BACKGROUND_PREFIX: &str = "waiting:";

/// A wait that holds now.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Wait {
    pub reason: String,
    pub since: String,
    pub deadline: String,
}

impl Wait {
    fn new(reason: String, since: &str, limit_s: i64) -> Option<Wait> {
        let at = crate::clock::parse(since)?;
        Some(Wait {
            reason,
            since: since.to_string(),
            deadline: crate::clock::stamp(at + chrono::Duration::seconds(limit_s)),
        })
    }

    fn holds(&self, now: &str) -> bool {
        now < self.deadline.as_str()
    }
}

/// The wait of worker `w` that holds at `now`, if any. `question` is its
/// open question (from [`crate::messaging::questions`]); one asked before
/// this worker claimed its task belongs to an earlier worker of the role.
pub fn wait_of(
    conn: &Connection,
    w: &Worker,
    question: Option<&OpenQuestion>,
    now: &str,
) -> Result<Option<Wait>> {
    let mut waits = Vec::new();
    waits.extend(review_wait(conn, w)?);
    let start = w.claimed_at.as_deref().or(w.spawned_at.as_deref());
    if let Some(q) = question.filter(|q| start.is_none_or(|s| q.asked_at.as_str() >= s)) {
        waits.extend(Wait::new(
            "an answer from the orchestrator".into(),
            &q.asked_at,
            QUESTION_WAIT_S,
        ));
    }
    waits.extend(background_wait(conn, w)?);
    Ok(waits.into_iter().find(|wait| wait.holds(now)))
}

/// The open question of worker `w` in the record under `state_root`: by its
/// role, in its workspace (else the run's).
pub fn open_question(state_root: &std::path::Path, run: &Run, w: &Worker) -> Option<OpenQuestion> {
    let ws = w.workspace_id.as_deref().or(run.workspace_id.as_deref())?;
    let role = w.role.as_deref()?;
    crate::messaging::questions::Questions::under(state_root, ws)
        .open_for(role)
        .ok()
        .flatten()
}

/// The reviewer tasks of the worker's waiting finish step that are not in.
fn review_wait(conn: &Connection, w: &Worker) -> Result<Option<Wait>> {
    let ids: Option<String> = optional(conn.query_row(
        "SELECT review_task_ids FROM task_steps
         WHERE worker_id = ?1 AND status = 'running' AND verdict = 'waiting'
         ORDER BY seq DESC LIMIT 1",
        [&w.worker_id],
        |r| r.get(0),
    ))?
    .flatten();
    let mut pending = Vec::new();
    let mut since: Option<String> = None;
    for id in util::parse_json_list(ids.as_deref()) {
        let Ok(t) = super::tasks::get(conn, &id) else {
            continue;
        };
        if matches!(t.status.as_str(), "done" | "cancelled") {
            continue;
        }
        if since.as_ref().is_none_or(|s| t.created_at < *s) {
            since = Some(t.created_at.clone());
        }
        pending.push(t.short());
    }
    Ok(since.and_then(|s| {
        Wait::new(
            format!("reviewer tasks {}", pending.join(", ")),
            &s,
            REVIEW_WAIT_S,
        )
    }))
}

/// The worker's last own event, when it is a `waiting: ...` progress line.
fn background_wait(conn: &Connection, w: &Worker) -> Result<Option<Wait>> {
    let last: Option<(String, String, String)> = optional(conn.query_row(
        "SELECT at, kind, text FROM events WHERE worker_id = ?1 AND actor = 'worker'
         ORDER BY event_id DESC LIMIT 1",
        [&w.worker_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    ))?;
    Ok(last.and_then(|(at, kind, text)| {
        let t = text.trim_start();
        let waiting = kind == EventKind::Progress.as_str()
            && t.get(..BACKGROUND_PREFIX.len())
                .is_some_and(|h| h.eq_ignore_ascii_case(BACKGROUND_PREFIX));
        waiting.then(|| {
            Wait::new(
                format!(
                    "a background command ({})",
                    t[BACKGROUND_PREFIX.len()..].trim()
                ),
                &at,
                BACKGROUND_WAIT_S,
            )
        })?
    }))
}

/// How the watchdog looks at a worker; the CLI asks herdr and the disk.
pub trait Observer {
    fn observe(&self, w: &Worker) -> Observation;
}

/// The previous check of a worker.
#[derive(Debug, Clone, PartialEq)]
struct Prev {
    at: String,
    herdr_seq: Option<i64>,
    transcript_bytes: Option<i64>,
    transcript_mtime: Option<String>,
    screen_timer_s: Option<i64>,
    verdict: String,
}

fn prev(conn: &Connection, worker_id: &str) -> Result<Option<Prev>> {
    optional(conn.query_row(
        "SELECT at, herdr_seq, transcript_bytes, transcript_mtime, screen_timer_s, verdict
         FROM checks WHERE worker_id = ?1 ORDER BY check_id DESC LIMIT 1",
        [worker_id],
        |r| {
            Ok(Prev {
                at: r.get(0)?,
                herdr_seq: r.get(1)?,
                transcript_bytes: r.get(2)?,
                transcript_mtime: r.get(3)?,
                screen_timer_s: r.get(4)?,
                verdict: r.get(5)?,
            })
        },
    ))
}

/// 1 check's result.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Checked {
    pub worker_id: String,
    pub role: Option<String>,
    pub task: Option<String>,
    pub verdict: String,
    pub round: i64,
    pub note: String,
    /// The verdict differs from the worker's last one: worth a wake.
    pub changed: bool,
    /// The first `no_progress` after progress: ask the worker for an update.
    pub first_stall: bool,
}

fn timer_agent(harness: Option<&str>) -> timer::Agent {
    match harness {
        Some("codex") => timer::Agent::Codex,
        Some("claude") => timer::Agent::Claude,
        _ => timer::Agent::Any,
    }
}

/// Check every due working worker of `run`. `force` checks every one now,
/// due or not.
pub fn check_run(
    store: &mut Store,
    run: &Run,
    observer: &dyn Observer,
    force: bool,
) -> Result<Vec<Checked>> {
    check_run_at(store, run, observer, force, &util::now())
}

/// [`check_run`] as of `now`, a ledger stamp.
pub fn check_run_at(
    store: &mut Store,
    run: &Run,
    observer: &dyn Observer,
    force: bool,
    now: &str,
) -> Result<Vec<Checked>> {
    let now = now.to_string();
    let mut out = Vec::new();
    for w in workers::of_run(store.conn(), &run.run_id)? {
        if w.kind != "worker" || w.status != "working" {
            continue;
        }
        let p = prev(store.conn(), &w.worker_id)?;
        let since = p
            .as_ref()
            .map(|p| p.at.clone())
            .or(w.claimed_at.clone())
            .unwrap_or_else(|| now.clone());
        let elapsed = util::seconds_between(&since, &now).unwrap_or(0);
        if !force && elapsed < run.check_every_s {
            continue;
        }
        let o = observer.observe(&w);
        let checkin_age = w
            .last_checkin_at
            .as_deref()
            .and_then(|t| util::seconds_between(t, &now));
        let timer_s = o
            .screen
            .as_deref()
            .and_then(|s| timer::working_seconds(s, timer_agent(w.harness.as_deref())))
            .map(|s| s as i64);
        let timer_delta = match (timer_s, p.as_ref().and_then(|p| p.screen_timer_s)) {
            (Some(now_t), Some(before)) => Some(now_t - before),
            _ => None,
        };
        let tol = run.timer_tolerance_s;
        let agent_moved = w
            .last_progress_at
            .as_deref()
            .is_some_and(|t| t > since.as_str())
            && w.last_progress_source.as_deref() == Some(ProgressSource::Agent.as_str());
        let transcript_moved = match (
            &o.transcript_bytes,
            p.as_ref().and_then(|p| p.transcript_bytes),
        ) {
            (Some(b), Some(before)) => *b > before,
            _ => false,
        } || o.transcript_mtime.as_deref().is_some_and(|m| {
            m > since.as_str()
                && p.as_ref()
                    .is_none_or(|p| p.transcript_mtime.as_deref() != Some(m))
        });
        let seq_moved = match (o.herdr_seq, p.as_ref().and_then(|p| p.herdr_seq)) {
            (Some(a), Some(b)) => a != b,
            _ => false,
        };
        // The timer ran the whole interval: from the last check, or from the
        // claim when it is the first.
        let timer_ran = match (timer_delta, timer_s) {
            (Some(d), _) => d >= elapsed - tol,
            (None, Some(t)) if p.is_none() => t >= elapsed - tol,
            _ => false,
        };
        let timer_short = timer_delta.is_some_and(|d| d < elapsed - tol);
        let (mut verdict, mut note, source) = if !o.agent_alive {
            (Verdict::Dead, "the agent process is gone".to_string(), None)
        } else if agent_moved {
            (
                Verdict::Progress,
                "the agent recorded progress".to_string(),
                None,
            )
        } else if transcript_moved {
            (
                Verdict::ProxyCheckin,
                "the transcript grew".to_string(),
                Some(ProgressSource::Transcript),
            )
        } else if seq_moved {
            (
                Verdict::ProxyCheckin,
                "herdr's state changed".to_string(),
                Some(ProgressSource::HerdrState),
            )
        } else if timer_ran {
            (
                Verdict::ProxyCheckin,
                format!(
                    "the working timer ran the whole interval ({}s)",
                    timer_s.unwrap_or(0)
                ),
                Some(ProgressSource::ScreenTimer),
            )
        } else if timer_short && o.herdr_status.as_deref() == Some("working") {
            (
                Verdict::ProxyCheckin,
                format!(
                    "suspect: the timer moved {}s in {elapsed}s, but herdr shows working",
                    timer_delta.unwrap_or(0)
                ),
                Some(ProgressSource::HerdrState),
            )
        } else if timer_short {
            (
                Verdict::NoProgress,
                format!(
                    "suspect: the timer moved {}s in {elapsed}s (it stopped about {}s ago), herdr shows {}",
                    timer_delta.unwrap_or(0),
                    elapsed - timer_delta.unwrap_or(0),
                    o.herdr_status.as_deref().unwrap_or("unknown")
                ),
                None,
            )
        } else {
            (
                Verdict::NoProgress,
                format!(
                    "no ledger call, no transcript growth, no state change, no running timer in {elapsed}s"
                ),
                None,
            )
        };
        if verdict == Verdict::NoProgress {
            if let Some(wait) = wait_of(store.conn(), &w, o.open_question.as_ref(), &now)? {
                verdict = Verdict::Progress;
                note = format!(
                    "waiting on {} for {}s (deadline {}), so the stall count does not run: {note}",
                    wait.reason,
                    util::seconds_between(&wait.since, &now).unwrap_or(0),
                    wait.deadline
                );
            }
        }
        let mut round = match verdict {
            Verdict::NoProgress => w.stall_rounds + 1,
            Verdict::Dead => w.stall_rounds,
            _ => 0,
        };
        if verdict == Verdict::NoProgress && round >= run.stall_rounds {
            verdict = Verdict::Dead;
            note = format!("{note}; {round} checks in a row without progress");
            round = w.stall_rounds + 1;
        }
        let task_short = match &w.task_id {
            Some(t) => super::tasks::get(store.conn(), t).ok().map(|t| t.short()),
            None => None,
        };
        store.immediate(|tx| {
            tx.execute(
                "INSERT INTO checks (worker_id, at, elapsed_s, checkin_age_s, pid_alive, herdr_status,
                    herdr_seq, transcript_bytes, transcript_mtime, screen_timer_s, timer_delta_s,
                    verdict, round, note)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                params![
                    w.worker_id,
                    now,
                    elapsed,
                    checkin_age,
                    o.agent_alive as i64,
                    o.herdr_status,
                    o.herdr_seq,
                    o.transcript_bytes,
                    o.transcript_mtime,
                    timer_s,
                    timer_delta,
                    verdict.as_str(),
                    round,
                    note
                ],
            )?;
            match verdict {
                Verdict::NoProgress | Verdict::Dead => {
                    tx.execute(
                        "UPDATE workers SET stall_rounds = ?2 WHERE worker_id = ?1",
                        params![w.worker_id, round],
                    )?;
                }
                _ => {}
            }
            if let Some(src) = source {
                workers::progress(tx, &w.worker_id, src, EventKind::Verdict, &format!("Proxy checkin: {note}."))?;
            } else if verdict == Verdict::Progress {
                tx.execute(
                    "UPDATE workers SET stall_rounds = 0 WHERE worker_id = ?1",
                    [&w.worker_id],
                )?;
            }
            Ok(())
        })?;
        let changed = p.as_ref().is_none_or(|p| p.verdict != verdict.as_str());
        out.push(Checked {
            worker_id: w.worker_id.clone(),
            role: w.role.clone(),
            task: task_short,
            verdict: verdict.as_str().to_string(),
            round,
            first_stall: verdict == Verdict::NoProgress && round == 1,
            note,
            changed,
        });
    }
    Ok(out)
}

/// Take the run's single-instance watchdog lock in `dir`, or `None` when a
/// live watchdog holds it. A lock whose owner died is broken.
pub fn lock(dir: &std::path::Path, run_id: &str) -> Result<Option<crate::fsx::DirLockGuard>> {
    match crate::fsx::DirLock::acquire(
        dir,
        &format!("watchdog-{run_id}"),
        std::time::Duration::from_secs(365 * 24 * 3600),
        std::time::Duration::ZERO,
    ) {
        Ok(g) => Ok(Some(g)),
        Err(crate::fsx::FsxError::LockTimeout { .. }) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Record the watchdog's own heartbeat on the run.
pub fn checkin(conn: &Connection, run_id: &str, pid: u32, started: Option<u64>) -> Result<()> {
    conn.execute(
        "UPDATE runs SET watchdog_pid = ?2, watchdog_started = ?3, watchdog_checkin_at = ?4
         WHERE run_id = ?1",
        params![run_id, pid as i64, started.map(|s| s as i64), util::now()],
    )?;
    Ok(())
}

/// Whether the run's watchdog has gone quiet while a task runs: no checkin
/// in 2 check intervals (or never), and its recorded process is not alive.
pub fn needs_restart(conn: &Connection, run: &Run) -> Result<bool> {
    let running: i64 = conn.query_row(
        "SELECT count(*) FROM tasks WHERE run_id = ?1 AND status IN ('running', 'landing')",
        [&run.run_id],
        |r| r.get(0),
    )?;
    if running == 0 {
        return Ok(false);
    }
    let alive = run
        .watchdog_pid
        .is_some_and(|p| crate::procid::alive(p as u32, run.watchdog_started.map(|s| s as u64)));
    let stale = match &run.watchdog_checkin_at {
        None => true,
        Some(t) => {
            util::seconds_between(t, &util::now()).is_none_or(|age| age > 2 * run.check_every_s)
        }
    };
    Ok(stale && !alive)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::Path;

    use super::*;
    use crate::taskledger::model::Actor;
    use crate::taskledger::runs::{self, NewRun};
    use crate::taskledger::tasks::{self, NewTask};
    use crate::taskledger::workers::NewWorker;

    struct Script(RefCell<Vec<Observation>>);
    impl Observer for Script {
        fn observe(&self, _: &Worker) -> Observation {
            self.0.borrow_mut().remove(0)
        }
    }

    fn setup() -> (tempfile::TempDir, Store, Run, String) {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(&dir.path().join("l.sqlite3")).unwrap();
        let (run, w) = s
            .immediate(|tx| {
                let run = runs::create(
                    tx,
                    Path::new("/r"),
                    &NewRun {
                        slug: "s".into(),
                        ..Default::default()
                    },
                )?;
                tasks::add(
                    tx,
                    &run,
                    &NewTask::work("a", "a.md", "codex-terra"),
                    Actor::Orchestrator,
                )?;
                let w = workers::insert(
                    tx,
                    &NewWorker {
                        run_id: run.run_id.clone(),
                        persona: "codex-terra".into(),
                        harness: Some("codex".into()),
                        ..Default::default()
                    },
                )?;
                workers::supervisor_started(tx, &w, 1, Some(1))?;
                workers::agent_started(tx, &w, 2, Some(2))?;
                tasks::claim(tx, &w)?;
                Ok((run, w))
            })
            .unwrap();
        (dir, s, run, w)
    }

    fn screen(mins: u64) -> Option<String> {
        Some(format!("• Working ({mins}m 00s • esc to interrupt)\n"))
    }

    fn live(timer: Option<String>, status: &str) -> Observation {
        Observation {
            agent_alive: true,
            herdr_status: Some(status.into()),
            screen: timer,
            ..Default::default()
        }
    }

    /// The test clock: it starts at the claim and moves only by
    /// [`Clock::pass`]. A check reads it, not the wall clock, so a second
    /// that ticks during a test changes no elapsed time.
    struct Clock(String);
    impl Clock {
        fn at_claim(s: &Store, w: &str) -> Clock {
            Clock(workers::get(s.conn(), w).unwrap().claimed_at.unwrap())
        }
        fn pass(&mut self, secs: i64) {
            let t = crate::clock::parse(&self.0).unwrap() + chrono::Duration::seconds(secs);
            self.0 = crate::clock::stamp(t);
        }
        /// A stamp `secs` before now (after it, for a negative `secs`).
        fn ago(&self, secs: i64) -> String {
            crate::clock::stamp(
                crate::clock::parse(&self.0).unwrap() - chrono::Duration::seconds(secs),
            )
        }
        fn check(&mut self, s: &mut Store, run: &Run, obs: &dyn Observer) -> Vec<Checked> {
            check_run_at(s, run, obs, false, &self.0).unwrap()
        }
        /// Pass 1 check interval, then check.
        fn next(&mut self, s: &mut Store, run: &Run, obs: &dyn Observer) -> Checked {
            self.pass(run.check_every_s);
            let r = self.check(s, run, obs);
            assert_eq!(r.len(), 1, "{r:?}");
            r.into_iter().next().unwrap()
        }
    }

    /// An observer that sees the same live, idle pane every time.
    struct Idle(Option<OpenQuestion>);
    impl Observer for Idle {
        fn observe(&self, _: &Worker) -> Observation {
            Observation {
                open_question: self.0.clone(),
                ..live(None, "idle")
            }
        }
    }

    #[test]
    fn a_timer_that_ran_the_whole_interval_is_a_proxy_checkin() {
        let (_d, mut s, run, w) = setup();
        let mut c = Clock::at_claim(&s, &w);
        let obs = Script(RefCell::new(vec![
            live(screen(10), "working"),
            live(screen(20), "working"),
        ]));
        let r = c.next(&mut s, &run, &obs);
        assert_eq!(r.verdict, "proxy_checkin", "{r:?}");
        let r = c.next(&mut s, &run, &obs);
        assert_eq!(r.verdict, "proxy_checkin", "{r:?}");
        assert_eq!(workers::get(s.conn(), &w).unwrap().stall_rounds, 0);
    }

    #[test]
    fn a_timer_that_ran_less_than_the_clock_and_an_idle_pane_is_no_progress() {
        let (_d, mut s, run, w) = setup();
        let mut c = Clock::at_claim(&s, &w);
        let obs = Script(RefCell::new(vec![
            live(screen(10), "working"),
            live(screen(15), "idle"),
        ]));
        c.next(&mut s, &run, &obs);
        let r = c.next(&mut s, &run, &obs);
        assert_eq!(r.verdict, "no_progress", "{r:?}");
        assert!(r.note.contains("stopped about 300s ago"), "{}", r.note);
        assert!(r.first_stall);
    }

    #[test]
    fn a_gone_process_is_dead_at_once() {
        let (_d, mut s, run, w) = setup();
        let mut c = Clock::at_claim(&s, &w);
        let obs = Script(RefCell::new(vec![Observation::default()]));
        assert_eq!(c.next(&mut s, &run, &obs).verdict, "dead");
    }

    #[test]
    fn three_stalled_checks_are_dead_and_progress_resets_the_count() {
        let (_d, mut s, run, w) = setup();
        let mut c = Clock::at_claim(&s, &w);
        let obs = Idle(None);
        for want in ["no_progress", "no_progress"] {
            assert_eq!(c.next(&mut s, &run, &obs).verdict, want);
        }
        // The agent records progress: the count resets.
        s.immediate(|tx| {
            workers::progress(tx, &w, ProgressSource::Agent, EventKind::Progress, "x")
        })
        .unwrap();
        c.pass(run.check_every_s);
        s.conn()
            .execute(
                "UPDATE workers SET last_progress_at = ?2 WHERE worker_id = ?1",
                params![w, c.ago(10)],
            )
            .unwrap();
        assert_eq!(c.check(&mut s, &run, &obs)[0].verdict, "progress");
        for want in ["no_progress", "no_progress", "dead"] {
            assert_eq!(c.next(&mut s, &run, &obs).verdict, want);
        }
    }

    #[test]
    fn a_worker_not_due_is_not_checked() {
        let (_d, mut s, run, w) = setup();
        let mut c = Clock::at_claim(&s, &w);
        let obs = Script(RefCell::new(vec![]));
        assert!(c.check(&mut s, &run, &obs).is_empty());
    }

    /// Queue 1 reviewer task and a waiting code-sweep step that names it.
    fn waiting_on_review(s: &mut Store, run: &Run, w: &str) -> String {
        s.immediate(|tx| {
            let t = tasks::add(
                tx,
                run,
                &NewTask::work("review", "r.md", "codex-terra"),
                Actor::Worker,
            )?;
            let wk = workers::get(tx, w)?;
            tx.execute(
                "INSERT INTO task_steps (task_id, worker_id, seq, name, status, verdict, review_task_ids)
                 VALUES (?1, ?2, 1, 'code-sweep', 'running', 'waiting', ?3)",
                params![wk.task_id, w, util::json_list(std::slice::from_ref(&t.task_id))],
            )?;
            Ok(t.task_id)
        })
        .unwrap()
    }

    /// The stall count starts fresh: 3 idle checks reap the worker, as for
    /// one that never waited.
    fn assert_reaped_after_3_idle_checks(
        c: &mut Clock,
        s: &mut Store,
        run: &Run,
        obs: &dyn Observer,
    ) {
        for (i, want) in ["no_progress", "no_progress", "dead"]
            .into_iter()
            .enumerate()
        {
            let r = c.next(s, run, obs);
            assert_eq!(r.verdict, want, "{r:?}");
            assert_eq!(r.first_stall, i == 0, "{r:?}");
        }
    }

    #[test]
    fn a_worker_waiting_on_reviewer_tasks_for_60_minutes_is_not_reaped_and_is_after_they_are_in() {
        let (_d, mut s, run, w) = setup();
        let mut c = Clock::at_claim(&s, &w);
        let review = waiting_on_review(&mut s, &run, &w);
        let obs = Idle(None);
        for _ in 0..6 {
            let r = c.next(&mut s, &run, &obs);
            assert_eq!(r.verdict, "progress", "{r:?}");
            assert!(
                r.note.starts_with("waiting on reviewer tasks T-2"),
                "{}",
                r.note
            );
            assert_eq!(r.round, 0);
        }
        assert_eq!(workers::get(s.conn(), &w).unwrap().stall_rounds, 0);
        s.conn()
            .execute(
                "UPDATE tasks SET status = 'done' WHERE task_id = ?1",
                [&review],
            )
            .unwrap();
        assert_reaped_after_3_idle_checks(&mut c, &mut s, &run, &obs);
    }

    #[test]
    fn a_review_wait_ends_at_its_deadline() {
        let (_d, mut s, run, w) = setup();
        let mut c = Clock::at_claim(&s, &w);
        waiting_on_review(&mut s, &run, &w);
        let obs = Idle(None);
        c.pass(REVIEW_WAIT_S - run.check_every_s);
        assert_eq!(c.next(&mut s, &run, &obs).verdict, "no_progress");
    }

    #[test]
    fn a_worker_with_an_open_question_is_not_reaped_until_the_reply_then_3_idle_checks_reap_it() {
        let (_d, mut s, run, w) = setup();
        let mut c = Clock::at_claim(&s, &w);
        let q = OpenQuestion {
            role: "codex-terra-1".into(),
            text: "[codex-terra-1] QUESTION: x?".into(),
            asked_at: c.ago(-60),
        };
        let asking = Idle(Some(q));
        for _ in 0..6 {
            let r = c.next(&mut s, &run, &asking);
            assert_eq!(r.verdict, "progress", "{r:?}");
            assert!(r.note.starts_with("waiting on an answer"), "{}", r.note);
        }
        // The orchestrator replied: the record shows no open question.
        assert_reaped_after_3_idle_checks(&mut c, &mut s, &run, &Idle(None));
    }

    #[test]
    fn a_question_wait_ends_at_its_deadline() {
        let (_d, mut s, run, w) = setup();
        let mut c = Clock::at_claim(&s, &w);
        let asking = Idle(Some(OpenQuestion {
            role: "r".into(),
            text: "[r] QUESTION: x?".into(),
            asked_at: c.0.clone(),
        }));
        c.pass(QUESTION_WAIT_S - run.check_every_s);
        assert_eq!(c.next(&mut s, &run, &asking).verdict, "no_progress");
    }

    #[test]
    fn a_question_from_before_the_claim_is_not_this_workers_wait() {
        let (_d, mut s, run, w) = setup();
        let mut c = Clock::at_claim(&s, &w);
        let asking = Idle(Some(OpenQuestion {
            role: "r".into(),
            text: "[r] QUESTION: x?".into(),
            asked_at: c.ago(60),
        }));
        assert_eq!(c.next(&mut s, &run, &asking).verdict, "no_progress");
    }

    #[test]
    fn a_waiting_progress_line_holds_for_the_default_deadline_and_any_later_line_ends_it() {
        let (_d, mut s, run, w) = setup();
        let mut c = Clock::at_claim(&s, &w);
        s.immediate(|tx| {
            workers::progress(
                tx,
                &w,
                ProgressSource::Agent,
                EventKind::Progress,
                "waiting: cargo test",
            )
        })
        .unwrap();
        let at = c.0.clone();
        s.conn()
            .execute(
                "UPDATE events SET at = ?2 WHERE worker_id = ?1",
                params![w, at],
            )
            .unwrap();
        s.conn()
            .execute(
                "UPDATE workers SET last_progress_at = ?2 WHERE worker_id = ?1",
                params![w, at],
            )
            .unwrap();
        let obs = Idle(None);
        // The progress line itself, then waits up to 60 min.
        assert_eq!(c.next(&mut s, &run, &obs).verdict, "progress");
        for _ in 0..4 {
            let r = c.next(&mut s, &run, &obs);
            assert_eq!(r.verdict, "progress", "{r:?}");
            assert!(
                r.note
                    .starts_with("waiting on a background command (cargo test)"),
                "{}",
                r.note
            );
        }
        assert_eq!(
            c.next(&mut s, &run, &obs).verdict,
            "no_progress",
            "the deadline passed"
        );

        // A later note from the worker ends the wait.
        let (_d, mut s, run, w) = setup();
        let mut c = Clock::at_claim(&s, &w);
        s.immediate(|tx| {
            workers::progress(
                tx,
                &w,
                ProgressSource::Agent,
                EventKind::Progress,
                "waiting: x",
            )?;
            workers::progress(tx, &w, ProgressSource::Agent, EventKind::Note, "back")
        })
        .unwrap();
        s.conn()
            .execute(
                "UPDATE workers SET last_progress_at = ?2 WHERE worker_id = ?1",
                params![w, c.ago(1)],
            )
            .unwrap();
        assert_eq!(c.next(&mut s, &run, &obs).verdict, "no_progress");
    }

    #[test]
    fn a_gone_process_is_dead_at_once_while_it_waits() {
        let (_d, mut s, run, w) = setup();
        let mut c = Clock::at_claim(&s, &w);
        waiting_on_review(&mut s, &run, &w);
        let gone = Script(RefCell::new(vec![Observation {
            open_question: Some(OpenQuestion {
                role: "r".into(),
                text: "[r] QUESTION: x?".into(),
                asked_at: c.0.clone(),
            }),
            ..Observation::default()
        }]));
        assert_eq!(c.next(&mut s, &run, &gone).verdict, "dead");
    }
}
