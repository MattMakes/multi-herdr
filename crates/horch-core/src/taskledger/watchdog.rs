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

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::Serialize;

use crate::messaging::questions::{OpenQuestion, QuestionState};

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
    /// The worker's last question when it is answered: a wait that may have
    /// opened and closed since the last check.
    pub answered_question: Option<OpenQuestion>,
    /// Why the open-question record could not be read: the worker's wait
    /// state is unknown.
    pub question_error: Option<String>,
}

/// How long each wait holds at most, from its start.
const REVIEW_WAIT_S: i64 = 3 * 3600;
const QUESTION_WAIT_S: i64 = 2 * 3600;
const BACKGROUND_WAIT_S: i64 = 3600;

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
    fn new(reason: String, since: &str, limit_s: i64) -> Result<Wait> {
        let at = crate::clock::parse(since)
            .with_context(|| format!("the wait start {since} is not a time"))?;
        Ok(Wait {
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
/// An error means the wait state cannot be read, not that there is no wait.
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
        waits.push(Wait::new(
            "an answer from the orchestrator".into(),
            &q.asked_at,
            QUESTION_WAIT_S,
        )?);
    }
    waits.extend(background_wait(conn, w)?);
    Ok(waits.into_iter().find(|wait| wait.holds(now)))
}

/// The open question of worker `w` in the record under `state_root`: by its
/// role, in its workspace (else the run's). A worker with no role or
/// workspace has none; a record that cannot be read is an error.
pub fn open_question(
    state_root: &std::path::Path,
    run: &Run,
    w: &Worker,
) -> Result<Option<OpenQuestion>> {
    Ok(match question_state(state_root, run, w)? {
        QuestionState::Open(q) => Some(q),
        _ => None,
    })
}

/// The state of worker `w`'s last question, from 1 read of the record that
/// [`open_question`] reads.
pub fn question_state(
    state_root: &std::path::Path,
    run: &Run,
    w: &Worker,
) -> Result<QuestionState> {
    question_record(state_root, run, w)
        .map_or(Ok(QuestionState::None), |(q, role)| q.state_of(&role))
}

fn question_record(
    state_root: &std::path::Path,
    run: &Run,
    w: &Worker,
) -> Option<(crate::messaging::questions::Questions, String)> {
    let ws = w.workspace_id.as_deref().or(run.workspace_id.as_deref())?;
    let role = w.role.clone()?;
    Some((
        crate::messaging::questions::Questions::under(state_root, ws),
        role,
    ))
}

/// The reviewer tasks of the worker's waiting finish step that are not in.
fn review_wait(conn: &Connection, w: &Worker) -> Result<Option<Wait>> {
    let ids: Option<String> = optional(conn.query_row(
        "SELECT review_task_ids FROM task_steps
         WHERE task_id = ?1 AND worker_id = ?2 AND status = 'running' AND verdict = 'waiting'
         ORDER BY seq DESC LIMIT 1",
        params![w.task_id, w.worker_id],
        |r| r.get(0),
    ))?
    .flatten();
    let Some(ids) = ids else {
        return Ok(None);
    };
    let ids: Vec<String> = serde_json::from_str(&ids)
        .with_context(|| format!("the reviewer task list {ids} does not parse"))?;
    let mut pending = Vec::new();
    let mut since: Option<String> = None;
    for id in ids {
        let t = super::tasks::get(conn, &id)
            .with_context(|| format!("reviewer task {id} cannot be read"))?;
        if matches!(t.status.as_str(), "done" | "cancelled") {
            continue;
        }
        if since.as_ref().is_none_or(|s| t.created_at < *s) {
            since = Some(t.created_at.clone());
        }
        pending.push(t.short());
    }
    let Some(since) = since else {
        return Ok(None);
    };
    Wait::new(
        format!("reviewer tasks {}", pending.join(", ")),
        &since,
        REVIEW_WAIT_S,
    )
    .map(Some)
}

/// The worker's last own event, when it is a `waiting: ...` progress line.
fn background_wait(conn: &Connection, w: &Worker) -> Result<Option<Wait>> {
    let last: Option<(String, String, String)> = optional(conn.query_row(
        "SELECT at, kind, text FROM events WHERE worker_id = ?1 AND actor = 'worker'
         ORDER BY event_id DESC LIMIT 1",
        [&w.worker_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    ))?;
    let Some((at, kind, text)) = last else {
        return Ok(None);
    };
    if kind != EventKind::Progress.as_str() {
        return Ok(None);
    }
    let t = text.trim_start();
    let Some(what) = t
        .get(..BACKGROUND_PREFIX.len())
        .filter(|h| h.eq_ignore_ascii_case(BACKGROUND_PREFIX))
        .map(|h| t[h.len()..].trim())
    else {
        return Ok(None);
    };
    Wait::new(
        format!("a background command ({what})"),
        &at,
        BACKGROUND_WAIT_S,
    )
    .map(Some)
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
    let now = util::now();
    let pass = Pass {
        run,
        observer,
        force,
        now: &now,
    };
    check_run_at(store, &pass)
}

/// 1 watchdog pass over a run: who looks, whether every worker is due, and
/// "now" as a ledger stamp.
struct Pass<'a> {
    run: &'a Run,
    observer: &'a dyn Observer,
    force: bool,
    now: &'a str,
}

/// [`check_run`] as of `pass.now`.
fn check_run_at(store: &mut Store, pass: &Pass<'_>) -> Result<Vec<Checked>> {
    workers::of_run(store.conn(), &pass.run.run_id)?
        .iter()
        .filter(|w| w.kind == "worker" && w.status == "working")
        .filter_map(|w| check_worker(store, pass, w).transpose())
        .collect()
}

/// What 1 check measured, besides the observation.
struct Facts {
    prev: Option<Prev>,
    now: String,
    /// The last check, or the claim.
    since: String,
    elapsed: i64,
    timer_s: Option<i64>,
    timer_delta: Option<i64>,
}

/// 1 verdict before the stall count: the verdict, its note and the proxy
/// progress it records.
type Call = (Verdict, String, Option<ProgressSource>);

/// Check worker `w` when it is due (or `pass.force`).
fn check_worker(store: &mut Store, pass: &Pass<'_>, w: &Worker) -> Result<Option<Checked>> {
    let Pass {
        run,
        observer,
        force,
        now,
    } = *pass;
    let prev = prev(store.conn(), &w.worker_id)?;
    let since = prev
        .as_ref()
        .map(|p| p.at.clone())
        .or(w.claimed_at.clone())
        .unwrap_or_else(|| now.to_string());
    let elapsed = util::seconds_between(&since, now).unwrap_or(0);
    if !force && elapsed < run.check_every_s {
        return Ok(None);
    }
    let o = observer.observe(w);
    let timer_s = o
        .screen
        .as_deref()
        .and_then(|s| timer::working_seconds(s, timer_agent(w.harness.as_deref())))
        .map(|s| s as i64);
    let timer_delta = timer_s
        .zip(prev.as_ref().and_then(|p| p.screen_timer_s))
        .map(|(now_t, before)| now_t - before);
    let f = Facts {
        prev,
        now: now.to_string(),
        since,
        elapsed,
        timer_s,
        timer_delta,
    };
    let call = classify(run, w, &o, &f);
    let call = apply_wait(store.conn(), w, &o, &f, call);
    let (verdict, round, note, source) = stall(run, w, call);
    persist(store, w, now, &o, &f, (verdict, round, &note, source))?;
    let task = match &w.task_id {
        Some(t) => super::tasks::get(store.conn(), t).ok().map(|t| t.short()),
        None => None,
    };
    Ok(Some(Checked {
        worker_id: w.worker_id.clone(),
        role: w.role.clone(),
        task,
        verdict: verdict.as_str().to_string(),
        round,
        first_stall: verdict == Verdict::NoProgress && round == 1,
        note,
        changed: f
            .prev
            .as_ref()
            .is_none_or(|p| p.verdict != verdict.as_str()),
    }))
}

/// The verdict from what moved since the last check (rules 1 to 5).
fn classify(run: &Run, w: &Worker, o: &Observation, f: &Facts) -> Call {
    if !o.agent_alive {
        return (Verdict::Dead, "the agent process is gone".into(), None);
    }
    if w.last_progress_at
        .as_deref()
        .is_some_and(|t| t > f.since.as_str())
        && w.last_progress_source.as_deref() == Some(ProgressSource::Agent.as_str())
    {
        return (
            Verdict::Progress,
            "the agent recorded progress".into(),
            None,
        );
    }
    if let Some(call) = proxy_checkin(run, o, f) {
        return call;
    }
    timer_suspect(run, o, f)
}

/// Rule 3: the transcript, herdr's state counter or the whole-interval timer.
fn proxy_checkin(run: &Run, o: &Observation, f: &Facts) -> Option<Call> {
    let p = f.prev.as_ref();
    let grew = o
        .transcript_bytes
        .zip(p.and_then(|p| p.transcript_bytes))
        .is_some_and(|(b, before)| b > before);
    let touched = o.transcript_mtime.as_deref().is_some_and(|m| {
        m > f.since.as_str() && p.is_none_or(|p| p.transcript_mtime.as_deref() != Some(m))
    });
    if grew || touched {
        return Some((
            Verdict::ProxyCheckin,
            "the transcript grew".into(),
            Some(ProgressSource::Transcript),
        ));
    }
    if o.herdr_seq
        .zip(p.and_then(|p| p.herdr_seq))
        .is_some_and(|(a, b)| a != b)
    {
        return Some((
            Verdict::ProxyCheckin,
            "herdr's state changed".into(),
            Some(ProgressSource::HerdrState),
        ));
    }
    // The timer ran the whole interval: from the last check, or from the
    // claim when it is the first.
    let full = f.elapsed - run.timer_tolerance_s;
    let ran = match (f.timer_delta, f.timer_s) {
        (Some(d), _) => d >= full,
        (None, Some(t)) => p.is_none() && t >= full,
        _ => false,
    };
    ran.then(|| {
        (
            Verdict::ProxyCheckin,
            format!(
                "the working timer ran the whole interval ({}s)",
                f.timer_s.unwrap_or(0)
            ),
            Some(ProgressSource::ScreenTimer),
        )
    })
}

/// Rules 4 and 5: a timer that ran less than the clock, or nothing moved.
fn timer_suspect(run: &Run, o: &Observation, f: &Facts) -> Call {
    let elapsed = f.elapsed;
    let Some(d) = f
        .timer_delta
        .filter(|d| *d < elapsed - run.timer_tolerance_s)
    else {
        return (
            Verdict::NoProgress,
            format!(
                "no ledger call, no transcript growth, no state change, no running timer in {elapsed}s"
            ),
            None,
        );
    };
    let herdr = o.herdr_status.as_deref();
    if herdr == Some("working") {
        return (
            Verdict::ProxyCheckin,
            format!("suspect: the timer moved {d}s in {elapsed}s, but herdr shows working"),
            Some(ProgressSource::HerdrState),
        );
    }
    (
        Verdict::NoProgress,
        format!(
            "suspect: the timer moved {d}s in {elapsed}s (it stopped about {}s ago), herdr shows {}",
            elapsed - d,
            herdr.unwrap_or("unknown")
        ),
        None,
    )
}

/// A `no_progress` while a wait holds is `progress`; while the wait state
/// cannot be read it is `suspect`, which holds the stall count.
fn apply_wait(conn: &Connection, w: &Worker, o: &Observation, f: &Facts, call: Call) -> Call {
    let (verdict, note, source) = call;
    if verdict != Verdict::NoProgress {
        return (verdict, note, source);
    }
    let now = f.now.as_str();
    let wait = match &o.question_error {
        Some(e) => Err(anyhow::anyhow!("{e}")),
        None => wait_of(conn, w, o.open_question.as_ref(), now),
    };
    match wait {
        Ok(None) if answered_since(w, o, &f.since) => (
            Verdict::Progress,
            format!("the worker asked the orchestrator and got its answer since the last check, so the stall count starts fresh: {note}"),
            None,
        ),
        Ok(None) => (verdict, note, source),
        Ok(Some(wait)) => (
            Verdict::Progress,
            format!(
                "waiting on {} for {}s (deadline {}), so the stall count does not run: {note}",
                wait.reason,
                util::seconds_between(&wait.since, now).unwrap_or(0),
                wait.deadline
            ),
            None,
        ),
        Err(e) => (
            Verdict::Suspect,
            format!("{WAIT_UNREADABLE}: {e:#}; the stall count holds: {note}"),
            None,
        ),
    }
}

/// Whether the worker's answered question was asked at or after `since`
/// (and after its claim): a wait that opened and closed between 2 checks.
fn answered_since(w: &Worker, o: &Observation, since: &str) -> bool {
    let start = w.claimed_at.as_deref().unwrap_or(since);
    o.answered_question
        .as_ref()
        .is_some_and(|q| q.asked_at.as_str() >= since && q.asked_at.as_str() >= start)
}

/// The note prefix of a check whose wait state could not be read.
pub const WAIT_UNREADABLE: &str = "wait state unreadable";

/// The stall count: `no_progress` adds 1 round, and the run's last round is
/// `dead`. Returns the verdict, its round, the note and the proxy source.
fn stall(run: &Run, w: &Worker, call: Call) -> (Verdict, i64, String, Option<ProgressSource>) {
    let (verdict, note, source) = call;
    let round = match verdict {
        Verdict::NoProgress => w.stall_rounds + 1,
        Verdict::Dead | Verdict::Suspect => w.stall_rounds,
        _ => 0,
    };
    if verdict == Verdict::NoProgress && round >= run.stall_rounds {
        let note = format!("{note}; {round} checks in a row without progress");
        return (Verdict::Dead, round, note, source);
    }
    (verdict, round, note, source)
}

/// Write the check row and its effect on the worker.
fn persist(
    store: &mut Store,
    w: &Worker,
    now: &str,
    o: &Observation,
    f: &Facts,
    (verdict, round, note, source): (Verdict, i64, &str, Option<ProgressSource>),
) -> Result<()> {
    let checkin_age = w
        .last_checkin_at
        .as_deref()
        .and_then(|t| util::seconds_between(t, now));
    store.immediate(|tx| {
        tx.execute(
            "INSERT INTO checks (worker_id, at, elapsed_s, checkin_age_s, pid_alive, herdr_status,
                herdr_seq, transcript_bytes, transcript_mtime, screen_timer_s, timer_delta_s,
                verdict, round, note)
             SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14",
            params![
                w.worker_id,
                now,
                f.elapsed,
                checkin_age,
                o.agent_alive as i64,
                o.herdr_status,
                o.herdr_seq,
                o.transcript_bytes,
                o.transcript_mtime,
                f.timer_s,
                f.timer_delta,
                verdict.as_str(),
                round,
                note
            ],
        )?;
        if matches!(verdict, Verdict::NoProgress | Verdict::Dead) {
            tx.execute(
                "UPDATE workers SET stall_rounds = ?2 WHERE worker_id = ?1",
                params![w.worker_id, round],
            )?;
        }
        if let Some(src) = source {
            workers::progress(
                tx,
                &w.worker_id,
                src,
                EventKind::Verdict,
                &format!("Proxy checkin: {note}."),
            )?;
        } else if verdict == Verdict::Progress {
            tx.execute(
                "UPDATE workers SET stall_rounds = 0 WHERE worker_id = ?1",
                [&w.worker_id],
            )?;
        }
        Ok(())
    })
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
            let pass = Pass {
                run,
                observer: obs,
                force: false,
                now: &self.0,
            };
            check_run_at(s, &pass).unwrap()
        }
        /// Pass 1 check interval, then check.
        fn next(&mut self, s: &mut Store, run: &Run, obs: &dyn Observer) -> Checked {
            self.pass(run.check_every_s);
            let r = self.check(s, run, obs);
            assert_eq!(r.len(), 1, "{r:?}");
            r.into_iter().next().unwrap()
        }
    }

    /// Record a line from the agent itself.
    fn agent_line(s: &mut Store, w: &str, kind: EventKind, text: &str) {
        s.immediate(|tx| workers::progress(tx, w, ProgressSource::Agent, kind, text))
            .unwrap();
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
        agent_line(&mut s, &w, EventKind::Progress, "x");
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
        waiting_on_reviews(s, run, w, 1).remove(0)
    }

    /// Queue `n` reviewer tasks and a waiting code-sweep step that names
    /// them.
    fn waiting_on_reviews(s: &mut Store, run: &Run, w: &str, n: usize) -> Vec<String> {
        s.immediate(|tx| {
            let mut ids = Vec::new();
            for _ in 0..n {
                let t = tasks::add(
                    tx,
                    run,
                    &NewTask::work("review", "r.md", "codex-terra"),
                    Actor::Worker,
                )?;
                ids.push(t.task_id);
            }
            waiting_step(tx, w, &util::json_list(&ids))?;
            // The tasks are queued at the claim, the test clock's start.
            tx.execute(
                "UPDATE tasks SET created_at = (SELECT claimed_at FROM workers WHERE worker_id = ?1)
                 WHERE kind = 'work' AND title = 'review'",
                [w],
            )?;
            Ok(ids)
        })
        .unwrap()
    }

    /// A waiting code-sweep step of worker `w` whose reviewer list is `ids`.
    fn waiting_step(conn: &Connection, w: &str, ids: &str) -> Result<()> {
        let wk = workers::get(conn, w)?;
        conn.execute(
            "INSERT INTO task_steps (task_id, worker_id, seq, name, status, verdict, review_task_ids)
             VALUES (?1, ?2, 1, 'code-sweep', 'running', 'waiting', ?3)",
            params![wk.task_id, w, ids],
        )?;
        Ok(())
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

    /// A `waiting: <what>` progress line of worker `w`, at `at`.
    fn waiting_line(s: &mut Store, w: &str, what: &str, at: &str) {
        agent_line(s, w, EventKind::Progress, &format!("waiting: {what}"));
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
    }

    #[test]
    fn a_waiting_progress_line_holds_until_its_default_deadline() {
        let (_d, mut s, run, w) = setup();
        let mut c = Clock::at_claim(&s, &w);
        let at = c.0.clone();
        waiting_line(&mut s, &w, "cargo test", &at);
        let obs = Idle(None);
        // The progress line itself, then waits up to 60 min.
        assert_eq!(c.next(&mut s, &run, &obs).verdict, "progress");
        for _ in 0..4 {
            let r = c.next(&mut s, &run, &obs);
            assert_eq!(r.verdict, "progress", "{r:?}");
            let want = "waiting on a background command (cargo test)";
            assert!(r.note.starts_with(want), "{}", r.note);
        }
        let r = c.next(&mut s, &run, &obs);
        assert_eq!(r.verdict, "no_progress", "the deadline passed: {r:?}");
    }

    #[test]
    fn a_later_line_from_the_worker_ends_a_background_wait() {
        let (_d, mut s, run, w) = setup();
        let mut c = Clock::at_claim(&s, &w);
        let at = c.ago(1);
        waiting_line(&mut s, &w, "x", &at);
        agent_line(&mut s, &w, EventKind::Note, "back");
        s.conn()
            .execute(
                "UPDATE workers SET last_progress_at = ?2 WHERE worker_id = ?1",
                params![w, at],
            )
            .unwrap();
        assert_eq!(c.next(&mut s, &run, &Idle(None)).verdict, "no_progress");
    }

    #[test]
    fn a_gone_process_is_dead_at_once_while_it_waits() {
        let (_d, mut s, run, w) = setup();
        let mut c = Clock::at_claim(&s, &w);
        waiting_on_review(&mut s, &run, &w);
        let gone = Script(RefCell::new(vec![Observation::default()]));
        assert_eq!(c.next(&mut s, &run, &gone).verdict, "dead");
    }

    #[test]
    fn the_wait_holds_while_any_reviewer_task_is_not_in() {
        let (_d, mut s, run, w) = setup();
        let mut c = Clock::at_claim(&s, &w);
        let ids = waiting_on_reviews(&mut s, &run, &w, 2);
        s.conn()
            .execute(
                "UPDATE tasks SET status = 'done' WHERE task_id = ?1",
                [&ids[0]],
            )
            .unwrap();
        let r = c.next(&mut s, &run, &Idle(None));
        assert_eq!(r.verdict, "progress", "{r:?}");
        assert!(
            r.note.starts_with("waiting on reviewer tasks T-3 "),
            "{}",
            r.note
        );
        s.conn()
            .execute(
                "UPDATE tasks SET status = 'cancelled' WHERE task_id = ?1",
                [&ids[1]],
            )
            .unwrap();
        assert_eq!(c.next(&mut s, &run, &Idle(None)).verdict, "no_progress");
    }

    /// 4 idle checks of a worker whose wait state cannot be read: each is
    /// `suspect` with the unreadable note, the stall count holds, and only
    /// the first is a change (1 NOTE to the orchestrator).
    fn assert_unreadable_holds(
        c: &mut Clock,
        s: &mut Store,
        run: &Run,
        w: &str,
        obs: &dyn Observer,
        why: &str,
    ) {
        for i in 0..4 {
            let r = c.next(s, run, obs);
            assert_eq!(r.verdict, "suspect", "{r:?}");
            assert!(
                r.note.starts_with(WAIT_UNREADABLE) && r.note.contains(why),
                "{}",
                r.note
            );
            assert_eq!(r.changed, i == 0, "{r:?}");
            assert_eq!(r.round, 0, "{r:?}");
        }
        assert_eq!(workers::get(s.conn(), w).unwrap().stall_rounds, 0);
    }

    #[test]
    fn an_unreadable_question_record_holds_the_stall_count() {
        let (_d, mut s, run, w) = setup();
        let mut c = Clock::at_claim(&s, &w);
        let broken = Script(RefCell::new(
            (0..4)
                .map(|_| Observation {
                    question_error: Some("q.json is not a question record".into()),
                    ..live(None, "idle")
                })
                .collect(),
        ));
        assert_unreadable_holds(&mut c, &mut s, &run, &w, &broken, "not a question record");
    }

    #[test]
    fn a_missing_reviewer_task_holds_the_stall_count() {
        let (_d, mut s, run, w) = setup();
        let mut c = Clock::at_claim(&s, &w);
        s.immediate(|tx| waiting_step(tx, &w, r#"["gone"]"#))
            .unwrap();
        assert_unreadable_holds(&mut c, &mut s, &run, &w, &Idle(None), "reviewer task gone");
    }

    #[test]
    fn a_reviewer_list_that_does_not_parse_holds_the_stall_count() {
        let (_d, mut s, run, w) = setup();
        let mut c = Clock::at_claim(&s, &w);
        s.immediate(|tx| waiting_step(tx, &w, "not json")).unwrap();
        assert_unreadable_holds(&mut c, &mut s, &run, &w, &Idle(None), "does not parse");
    }

    #[test]
    fn a_question_asked_and_answered_between_checks_starts_the_stall_count_fresh() {
        let (_d, mut s, run, w) = setup();
        let mut c = Clock::at_claim(&s, &w);
        for want in ["no_progress", "no_progress"] {
            assert_eq!(c.next(&mut s, &run, &Idle(None)).verdict, want);
        }
        // At round 2 the worker asks; the reply comes before the next check.
        let answered = Script(RefCell::new(vec![Observation {
            answered_question: Some(OpenQuestion {
                role: "r".into(),
                text: "[r] QUESTION: x?".into(),
                asked_at: c.ago(-60),
            }),
            ..live(None, "idle")
        }]));
        let r = c.next(&mut s, &run, &answered);
        assert_eq!(r.verdict, "progress", "{r:?}");
        assert!(
            r.note.contains("got its answer since the last check"),
            "{}",
            r.note
        );
        assert_reaped_after_3_idle_checks(&mut c, &mut s, &run, &Idle(None));
    }

    #[test]
    fn an_answered_question_from_before_the_last_check_is_no_progress() {
        let (_d, mut s, run, w) = setup();
        let mut c = Clock::at_claim(&s, &w);
        c.next(&mut s, &run, &Idle(None));
        let old = Script(RefCell::new(vec![Observation {
            answered_question: Some(OpenQuestion {
                role: "r".into(),
                text: "[r] QUESTION: x?".into(),
                asked_at: c.ago(60),
            }),
            ..live(None, "idle")
        }]));
        assert_eq!(c.next(&mut s, &run, &old).verdict, "no_progress");
    }
}
