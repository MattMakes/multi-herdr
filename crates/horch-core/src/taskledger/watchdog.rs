use anyhow::Result;
use rusqlite::{params, Connection};
use serde::Serialize;

use super::model::{EventKind, OutboxEvent, ProgressSource, RunStatus, Verdict};
use super::outbox::{self, Wake};
use super::runs::Run;
use super::sql_words::{self, word};
use super::store::{optional, Store};
use super::tasks;
use super::timer;
use super::util;
use super::workers::{self, Worker};
use crate::exeid::ExeIdentity;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Observation {
    pub agent_alive: bool,
    pub herdr_status: Option<String>,
    pub herdr_seq: Option<i64>,
    pub transcript_bytes: Option<i64>,
    pub transcript_mtime: Option<String>,
    pub screen: Option<String>,
}

pub trait Observer {
    fn observe(&self, w: &Worker) -> Observation;
}

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

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Checked {
    pub worker_id: String,
    pub role: Option<String>,
    pub task: Option<String>,
    pub verdict: String,
    pub round: i64,
    pub note: String,
    pub changed: bool,
    pub first_stall: bool,
    pub pane_id: Option<String>,
}

const HERDR_ONLY: &str = "herdr shows working";

fn missing_signals(w: &Worker, o: &Observation, timer_s: Option<i64>) -> String {
    let mut out = Vec::new();
    match w.harness.as_deref() {
        Some(h) if timer::Agent::of_harness(h).is_none() => {
            out.push(format!("no timer format for harness {h}"))
        }
        _ if timer_s.is_none() => out.push("no running timer".to_string()),
        _ => {}
    }
    if o.transcript_bytes.is_none() {
        out.push("no transcript".to_string());
    }
    if out.is_empty() {
        String::new()
    } else {
        format!(" ({})", out.join("; "))
    }
}

fn herdr_only_streak(conn: &Connection, worker_id: &str, cap: i64) -> Result<i64> {
    let mut stmt = conn
        .prepare("SELECT note FROM checks WHERE worker_id = ?1 ORDER BY check_id DESC LIMIT ?2")?;
    let notes = stmt
        .query_map(params![worker_id, cap + 1], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(notes
        .iter()
        .take_while(|n| n.starts_with(HERDR_ONLY))
        .count() as i64)
}

fn herdr_fallback(
    conn: &Connection,
    w: &Worker,
    run: &Run,
    prev_verdict: Option<&str>,
    detail: &str,
    missing: &str,
) -> Result<(Verdict, String, Option<ProgressSource>)> {
    let cap = 2 * run.stall_rounds.max(1);
    let streak = herdr_only_streak(conn, &w.worker_id, cap)?;
    let after_herdr_suspect = streak > 0 && prev_verdict == Some(Verdict::Suspect.as_str());
    Ok(if after_herdr_suspect {
        (
            Verdict::Dead,
            format!(
                "{HERDR_ONLY}, but nothing else moved for {} checks in a row{missing}",
                streak + 1
            ),
            None,
        )
    } else if streak >= cap {
        let pane = w.pane_id.as_deref().unwrap_or("unknown");
        (
                Verdict::Suspect,
                format!(
                    "{HERDR_ONLY}, but nothing else moved for {cap} checks in a row{missing}; pane {pane}"
                ),
                None,
            )
    } else {
        (
            Verdict::ProxyCheckin,
            format!("{HERDR_ONLY}, {detail}{missing}"),
            Some(ProgressSource::HerdrState),
        )
    })
}

fn timer_agent(harness: Option<&str>) -> timer::Agent {
    harness
        .and_then(timer::Agent::of_harness)
        .unwrap_or(timer::Agent::Any)
}

pub fn check_run(
    store: &mut Store,
    run: &Run,
    observer: &dyn Observer,
    force: bool,
) -> Result<Vec<Checked>> {
    check_run_at(store, run, observer, force, util::now())
}

fn check_run_at(
    store: &mut Store,
    run: &Run,
    observer: &dyn Observer,
    force: bool,
    now: String,
) -> Result<Vec<Checked>> {
    workers::of_run(store.conn(), &run.run_id)?
        .iter()
        .filter(|w| w.kind == "worker" && w.status == "working")
        .filter_map(|w| check_worker(store, run, observer, w, force, &now).transpose())
        .collect()
}

struct Look<'a> {
    w: &'a Worker,
    run: &'a Run,
    o: &'a Observation,
    now: &'a str,
    prev_verdict: Option<&'a str>,
    waits: &'a [workers::Wait],
    elapsed: i64,
    timer_s: Option<i64>,
    timer_delta: Option<i64>,
}

fn check_worker(
    store: &mut Store,
    run: &Run,
    observer: &dyn Observer,
    w: &Worker,
    force: bool,
    now: &str,
) -> Result<Option<Checked>> {
    let p = prev(store.conn(), &w.worker_id)?;
    let since = since(w, p.as_ref(), now);
    let elapsed = util::seconds_between(&since, now).unwrap_or(0);
    if !force && elapsed < run.check_every_s {
        return Ok(None);
    }
    let o = observer.observe(w);
    let waits = workers::waiting(store.conn(), &w.worker_id)?;
    let timer_s = timer_seconds(w, &o);
    let s = signals(
        w,
        &o,
        p.as_ref(),
        &since,
        elapsed,
        run.timer_tolerance_s,
        timer_s,
    );
    let look = Look {
        w,
        run,
        o: &o,
        now,
        prev_verdict: p.as_ref().map(|p| p.verdict.as_str()),
        waits: &waits,
        elapsed,
        timer_s,
        timer_delta: s.timer_delta,
    };
    let (verdict, note, source) = judge(store.conn(), &look, &s)?;
    let (verdict, note, round) = stall_round(store.conn(), verdict, note, w, run)?;
    let task = task_short(store.conn(), w);
    record_check(store, &look, verdict, &note, source, round)?;
    Ok(Some(checked(w, p.as_ref(), task, verdict, note, round)))
}

fn checked(
    w: &Worker,
    p: Option<&Prev>,
    task: Option<String>,
    verdict: Verdict,
    note: String,
    round: i64,
) -> Checked {
    Checked {
        worker_id: w.worker_id.clone(),
        role: w.role.clone(),
        task,
        verdict: verdict.as_str().to_string(),
        round,
        first_stall: verdict == Verdict::NoProgress && round == 1,
        note,
        changed: p.is_none_or(|p| p.verdict != verdict.as_str()),
        pane_id: w.pane_id.clone(),
    }
}

fn since(w: &Worker, p: Option<&Prev>, now: &str) -> String {
    p.map(|p| p.at.clone())
        .or(w.claimed_at.clone())
        .unwrap_or_else(|| now.to_string())
}

fn timer_seconds(w: &Worker, o: &Observation) -> Option<i64> {
    o.screen
        .as_deref()
        .and_then(|s| timer::working_seconds(s, timer_agent(w.harness.as_deref())))
        .map(|s| s as i64)
}

struct Signals {
    agent_moved: bool,
    transcript_moved: bool,
    seq_moved: bool,
    timer_ran: bool,
    timer_short: bool,
    timer_delta: Option<i64>,
}

fn signals(
    w: &Worker,
    o: &Observation,
    p: Option<&Prev>,
    since: &str,
    elapsed: i64,
    tol: i64,
    timer_s: Option<i64>,
) -> Signals {
    let timer_delta = match (timer_s, p.and_then(|p| p.screen_timer_s)) {
        (Some(now_t), Some(before)) => Some(now_t - before),
        _ => None,
    };
    Signals {
        agent_moved: agent_moved(w, since),
        transcript_moved: transcript_moved(o, p, since),
        seq_moved: seq_moved(o, p),
        timer_ran: timer_ran(timer_delta, timer_s, p.is_none(), elapsed - tol),
        timer_short: timer_delta.is_some_and(|d| d < elapsed - tol),
        timer_delta,
    }
}

fn seq_moved(o: &Observation, p: Option<&Prev>) -> bool {
    matches!((o.herdr_seq, p.and_then(|p| p.herdr_seq)), (Some(a), Some(b)) if a != b)
}

fn timer_ran(delta: Option<i64>, timer_s: Option<i64>, first: bool, need: i64) -> bool {
    match (delta, timer_s) {
        (Some(d), _) => d >= need,
        (None, Some(t)) if first => t >= need,
        _ => false,
    }
}

fn agent_moved(w: &Worker, since: &str) -> bool {
    w.last_progress_at.as_deref().is_some_and(|t| t > since)
        && w.last_progress_source.as_deref() == Some(ProgressSource::Agent.as_str())
}

fn transcript_moved(o: &Observation, p: Option<&Prev>, since: &str) -> bool {
    let grew = match (&o.transcript_bytes, p.and_then(|p| p.transcript_bytes)) {
        (Some(b), Some(before)) => *b > before,
        _ => false,
    };
    grew || o
        .transcript_mtime
        .as_deref()
        .is_some_and(|m| m > since && p.is_none_or(|p| p.transcript_mtime.as_deref() != Some(m)))
}

type Judged = (Verdict, String, Option<ProgressSource>);

fn judge(conn: &Connection, look: &Look, s: &Signals) -> Result<Judged> {
    match proof(look, s) {
        Some(judged) => Ok(judged),
        None => stalled(conn, look, s),
    }
}

fn proof(look: &Look, s: &Signals) -> Option<Judged> {
    Some(if !look.o.agent_alive {
        (Verdict::Dead, "the agent process is gone".to_string(), None)
    } else if !look.waits.is_empty() {
        (Verdict::Progress, waiting_note(look.waits), None)
    } else if s.agent_moved {
        (
            Verdict::Progress,
            "the agent recorded progress".to_string(),
            None,
        )
    } else if s.transcript_moved {
        (
            Verdict::ProxyCheckin,
            "the transcript grew".to_string(),
            Some(ProgressSource::Transcript),
        )
    } else if s.seq_moved {
        (
            Verdict::ProxyCheckin,
            "herdr's state changed".to_string(),
            Some(ProgressSource::HerdrState),
        )
    } else if s.timer_ran {
        (
            Verdict::ProxyCheckin,
            format!(
                "the working timer ran the whole interval ({}s)",
                look.timer_s.unwrap_or(0)
            ),
            Some(ProgressSource::ScreenTimer),
        )
    } else {
        return None;
    })
}

fn waiting_note(waits: &[workers::Wait]) -> String {
    let reasons: Vec<&str> = waits.iter().map(|w| w.reason.as_str()).collect();
    format!("waiting: {}", reasons.join("; "))
}

fn stalled(conn: &Connection, look: &Look, s: &Signals) -> Result<Judged> {
    let herdr_working = look.o.herdr_status.as_deref() == Some("working");
    let delta = look.timer_delta.unwrap_or(0);
    let missing = missing_signals(look.w, look.o, look.timer_s);
    let fallback = |detail: String| {
        herdr_fallback(conn, look.w, look.run, look.prev_verdict, &detail, &missing)
    };
    if s.timer_short && herdr_working {
        return fallback(format!("but the timer moved {delta}s in {}s", look.elapsed));
    }
    if s.timer_short {
        return Ok((
            Verdict::NoProgress,
            format!(
                "suspect: the timer moved {delta}s in {}s (it stopped about {}s ago), herdr shows {}",
                look.elapsed,
                look.elapsed - delta,
                look.o.herdr_status.as_deref().unwrap_or("unknown")
            ),
            None,
        ));
    }
    if herdr_working {
        return fallback(format!("and nothing else moved in {}s", look.elapsed));
    }
    Ok((
        Verdict::NoProgress,
        format!(
            "no ledger call, no transcript growth, no state change, no running timer in {}s{missing}",
            look.elapsed
        ),
        None,
    ))
}

fn stall_round(
    conn: &Connection,
    verdict: Verdict,
    note: String,
    w: &Worker,
    run: &Run,
) -> Result<(Verdict, String, i64)> {
    let round = match verdict {
        Verdict::NoProgress => workers::get(conn, &w.worker_id)?.stall_rounds + 1,
        Verdict::Dead => w.stall_rounds,
        _ => 0,
    };
    if verdict == Verdict::NoProgress && round >= run.stall_rounds {
        let note = format!("{note}; {round} checks in a row without progress");
        return Ok((Verdict::Dead, note, w.stall_rounds + 1));
    }
    Ok((verdict, note, round))
}

fn task_short(conn: &Connection, w: &Worker) -> Option<String> {
    let task = w.task_id.as_ref()?;
    super::tasks::get(conn, task).ok().map(|t| t.short())
}

fn record_check(
    store: &mut Store,
    look: &Look,
    verdict: Verdict,
    note: &str,
    source: Option<ProgressSource>,
    round: i64,
) -> Result<()> {
    store.immediate(|tx| {
        insert_check(tx, look, verdict, note, round)?;
        update_worker(tx, &look.w.worker_id, verdict, note, source, round)
    })
}

fn insert_check(
    tx: &Connection,
    look: &Look,
    verdict: Verdict,
    note: &str,
    round: i64,
) -> Result<()> {
    let (w, o) = (look.w, look.o);
    let checkin_age = w
        .last_checkin_at
        .as_deref()
        .and_then(|t| util::seconds_between(t, look.now));
    tx.execute(
        "INSERT INTO checks (worker_id, at, elapsed_s, checkin_age_s, pid_alive, herdr_status,
            herdr_seq, transcript_bytes, transcript_mtime, screen_timer_s, timer_delta_s,
            verdict, round, note)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            w.worker_id,
            look.now,
            look.elapsed,
            checkin_age,
            o.agent_alive as i64,
            o.herdr_status,
            o.herdr_seq,
            o.transcript_bytes,
            o.transcript_mtime,
            look.timer_s,
            look.timer_delta,
            verdict.as_str(),
            round,
            note
        ],
    )?;
    Ok(())
}

fn update_worker(
    tx: &Connection,
    worker_id: &str,
    verdict: Verdict,
    note: &str,
    source: Option<ProgressSource>,
    round: i64,
) -> Result<()> {
    if matches!(verdict, Verdict::NoProgress | Verdict::Dead) {
        tx.execute(
            "UPDATE workers SET stall_rounds = ?2 WHERE worker_id = ?1",
            params![worker_id, round],
        )?;
    }
    if let Some(src) = source {
        workers::progress(
            tx,
            worker_id,
            src,
            EventKind::Verdict,
            &format!("Proxy checkin: {note}."),
        )?;
    } else if verdict == Verdict::Progress {
        tx.execute(
            "UPDATE workers SET stall_rounds = 0 WHERE worker_id = ?1",
            [worker_id],
        )?;
    }
    Ok(())
}

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

const EXE_FILE: &str = "exe";

const EXE_RECORD_GRACE_S: i64 = 10;

pub fn record_exe(guard: &crate::fsx::DirLockGuard, exe: Option<&ExeIdentity>) -> Result<()> {
    if let Some(exe) = exe {
        crate::fsx::write_atomic(
            &guard.path().join(EXE_FILE),
            &serde_json::to_vec(exe)?,
            crate::fsx::PRIVATE_FILE,
        )?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq)]
pub struct LockHolder {
    pub pid: u32,
    pub started: Option<u64>,
    pub exe: Option<ExeIdentity>,
    pub recent: bool,
}

impl LockHolder {
    fn unrecorded(&self) -> bool {
        self.exe.is_none() && !self.recent
    }

    pub fn runs_other_binary(&self, current: Option<&ExeIdentity>) -> bool {
        current.is_some_and(|want| match &self.exe {
            Some(exe) => exe != want,
            None => self.unrecorded(),
        })
    }

    pub fn binary_replaced(&self) -> bool {
        match &self.exe {
            Some(exe) => {
                crate::exeid::exe_identity(std::path::Path::new(&exe.path)).as_ref() != Some(exe)
            }
            None => self.unrecorded(),
        }
    }

    #[cfg(unix)]
    pub fn stop(&self, wait: std::time::Duration) -> Result<()> {
        let Some(started) = self.started else {
            anyhow::bail!(
                "watchdog PID {} recorded no start time, so horch cannot prove it is the watchdog",
                self.pid
            );
        };
        if !crate::procid::signal_same(self.pid, started, libc::SIGTERM) {
            anyhow::bail!(
                "watchdog PID {} no longer runs with its recorded start time; nothing was signalled",
                self.pid
            );
        }
        let until = std::time::Instant::now() + wait;
        while std::time::Instant::now() < until {
            if !crate::procid::alive(self.pid, Some(started)) {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        anyhow::bail!(
            "watchdog PID {} did not stop within {}s of SIGTERM",
            self.pid,
            wait.as_secs()
        )
    }

    #[cfg(not(unix))]
    pub fn stop(&self, _wait: std::time::Duration) -> Result<()> {
        anyhow::bail!(
            "watchdog PID {} runs another binary; stop it by hand on this platform",
            self.pid
        )
    }
}

pub fn holder(dir: &std::path::Path, run_id: &str) -> Option<LockHolder> {
    let lock = dir.join(format!("watchdog-{run_id}.lock"));
    let v: serde_json::Value =
        serde_json::from_slice(&std::fs::read(lock.join("owner")).ok()?).ok()?;
    let pid = u32::try_from(v["pid"].as_u64()?).ok()?;
    let started = v["started"].as_u64();
    if !crate::procid::alive(pid, started) {
        return None;
    }
    let exe = std::fs::read(lock.join(EXE_FILE))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());
    let recent = v["acquired_at"]
        .as_str()
        .and_then(crate::clock::parse)
        .is_some_and(|t| (chrono::Utc::now() - t).num_seconds() < EXE_RECORD_GRACE_S);
    Some(LockHolder {
        pid,
        started,
        exe,
        recent,
    })
}

pub fn checkin(conn: &Connection, run_id: &str, pid: u32, started: Option<u64>) -> Result<()> {
    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| crate::exeid::exe_identity(&p))
        .and_then(|e| serde_json::to_string(&e).ok());
    conn.execute(
        "UPDATE runs SET watchdog_pid = ?2, watchdog_started = ?3, watchdog_checkin_at = ?4,
            watchdog_version = ?5, watchdog_exe = ?6
         WHERE run_id = ?1",
        params![
            run_id,
            pid as i64,
            started.map(|s| s as i64),
            util::now(),
            env!("CARGO_PKG_VERSION"),
            exe
        ],
    )?;
    Ok(())
}

pub fn record_start(conn: &Connection, run_id: &str, pid: u32, started: Option<u64>) -> Result<()> {
    conn.execute(
        "UPDATE runs SET watchdog_pid = ?2, watchdog_started = ?3 WHERE run_id = ?1",
        params![run_id, pid as i64, started.map(|s| s as i64)],
    )?;
    Ok(())
}

pub fn active_runs(conn: &Connection) -> Result<Vec<Run>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT run_id FROM runs WHERE status = {active}",
        active = word(RunStatus::Active)
    ))?;
    let ids = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ids.iter().map(|id| super::runs::get(conn, id)).collect()
}

pub fn needs_restart(conn: &Connection, run: &Run) -> Result<bool> {
    let running: i64 = conn.query_row(
        &format!(
            "SELECT count(*) FROM tasks WHERE run_id = ?1 AND status IN ({in_flight})",
            in_flight = sql_words::list(sql_words::TASKS_IN_FLIGHT)
        ),
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

pub const RECONCILE_S: u64 = 30;

pub const REWAKE_AFTER_S: i64 = 120;

pub const UNCLAIMED_AFTER_S: i64 = 180;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sighting {
    board: String,
    since: String,
}

pub fn startable_rewake(
    conn: &Connection,
    run: &Run,
    now: &str,
    seen: &mut Option<Sighting>,
) -> Result<Option<Wake>> {
    let plan: Vec<tasks::PersonaPlan> = tasks::plan_workers(conn, &run.run_id)?
        .into_iter()
        .filter(|p| p.spawn > 0)
        .collect();
    if plan.is_empty() {
        *seen = None;
        return Ok(None);
    }
    let board = outbox::board(conn, &run.run_id, true)?;
    if seen.as_ref().is_none_or(|s| s.board != board) {
        *seen = Some(Sighting {
            board: board.clone(),
            since: now.to_string(),
        });
    }
    let ready: Vec<String> = tasks::startable(conn, &run.run_id)?
        .iter()
        .map(|t| t.short())
        .collect();
    let ready = ready.join(",");
    let mut since = seen.as_ref().map(|s| s.since.clone()).unwrap_or_default();
    if let Some(first) = first_wake_at(conn, run, &ready, &board)? {
        since = since.min(first);
    }
    if util::seconds_between(&since, now).is_none_or(|age| age < REWAKE_AFTER_S) {
        return Ok(None);
    }
    let spawn: Vec<String> = plan
        .iter()
        .map(|p| {
            if p.replaces.is_empty() {
                format!("{} x{}", p.persona, p.spawn)
            } else {
                format!(
                    "{} x{} (--replaces {})",
                    p.persona,
                    p.spawn,
                    p.replaces.join(", ")
                )
            }
        })
        .collect();
    let text = format!(
        "[horch] NOTE: Tasks can start, and no worker is on its way to them for {REWAKE_AFTER_S} s: {}. Spawn: {}.",
        ready.replace(',', ", "),
        spawn.join(", ")
    );
    Ok(Some(
        Wake::new(OutboxEvent::Startable, format!("rewake:{ready}"), text).on_board(board),
    ))
}

fn first_wake_at(conn: &Connection, run: &Run, ready: &str, board: &str) -> Result<Option<String>> {
    let first = Wake::new(OutboxEvent::Startable, ready, "").on_board(board.to_string());
    optional(conn.query_row(
        "SELECT created_at FROM outbox WHERE key = ?1",
        [outbox::key(&run.run_id, &first)],
        |r| r.get(0),
    ))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Unclaimed {
    pub worker: Worker,
    pub waited_s: i64,
}

const NO_CLAIM: &str = "No claim:";

pub fn unclaimed_spawns(conn: &Connection, run: &Run, now: &str) -> Result<Vec<Unclaimed>> {
    let mut out = Vec::new();
    for w in workers::of_run(conn, &run.run_id)? {
        if w.kind != "worker"
            || !matches!(w.status.as_str(), "spawning" | "starting" | "claiming")
            || workers::is_stale(&w, run.heartbeat_stale_s, now)
        {
            continue;
        }
        let waited_s = w
            .spawned_at
            .as_deref()
            .or(w.started_at.as_deref())
            .and_then(|t| util::seconds_between(t, now))
            .unwrap_or(0);
        if waited_s < UNCLAIMED_AFTER_S || flagged(conn, &w.worker_id)? {
            continue;
        }
        out.push(Unclaimed {
            worker: w,
            waited_s,
        });
    }
    Ok(out)
}

fn flagged(conn: &Connection, worker_id: &str) -> Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT count(*) FROM events WHERE worker_id = ?1 AND kind = ?2 AND text LIKE ?3",
        params![
            worker_id,
            EventKind::Verdict.as_str(),
            format!("{NO_CLAIM}%")
        ],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

pub fn flag_unclaimed(
    conn: &Connection,
    run: &Run,
    u: &Unclaimed,
    herdr_status: Option<&str>,
) -> Result<Option<Wake>> {
    let w = &u.worker;
    if flagged(conn, &w.worker_id)? {
        return Ok(None);
    }
    let note = format!("{} has not claimed a task in {}s", w.label(), u.waited_s);
    util::event(
        conn,
        &util::About {
            run: Some(&run.run_id),
            task: None,
            worker: Some(&w.worker_id),
        },
        super::model::Actor::Watchdog,
        EventKind::Verdict,
        &format!("{NO_CLAIM} {note}."),
    )?;
    let text = format!(
        "[horch] NOTE: {note}. Look at its pane {} (herdr status {}): it may wait at a login, trust or quota prompt.",
        w.pane_id.as_deref().unwrap_or("(none)"),
        herdr_status.unwrap_or("unknown")
    );
    Ok(Some(Wake::new(
        OutboxEvent::UnclaimedSpawn,
        format!("asked:{}", w.worker_id),
        text,
    )))
}

pub const REVIEWS_IN_AFTER_S: i64 = 60;

pub fn stalled_blockers(conn: &Connection, run: &Run) -> Result<Vec<tasks::Task>> {
    let mut out = Vec::new();
    for t in tasks::of_run(conn, &run.run_id)? {
        let blocker = t.status == super::model::TaskStatus::Cancelled.as_str() || t.is_parked();
        if blocker && !tasks::dependents(conn, &t.task_id)?.is_empty() {
            out.push(t);
        }
    }
    Ok(out)
}

pub fn blocker_state(t: &tasks::Task) -> String {
    if t.is_parked() {
        format!("parked:{}", t.attempts)
    } else {
        format!(
            "{}:{}",
            t.status,
            t.finished_at.as_deref().unwrap_or_default()
        )
    }
}

pub fn attempt_held(conn: &Connection, run: &Run) -> Result<Vec<tasks::Task>> {
    Ok(tasks::of_run(conn, &run.run_id)?
        .into_iter()
        .filter(|t| {
            t.status == super::model::TaskStatus::Queued.as_str()
                && t.held_reason.is_some()
                && !t.is_parked()
        })
        .collect())
}

pub fn held_state(t: &tasks::Task) -> String {
    format!("held:{}", t.attempts)
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReviewsIn {
    pub parent: tasks::Task,
    pub author: Worker,
}

pub fn reviews_waiting(conn: &Connection, run: &Run, now: &str) -> Result<Vec<ReviewsIn>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT t.task_id, t.current_worker_id, max(r.finished_at)
           FROM tasks t
           JOIN task_steps s ON s.task_id = t.task_id AND s.worker_id = t.current_worker_id
           JOIN tasks r ON r.parent_task_id = t.task_id AND r.kind = 'review'
          WHERE t.run_id = ?1 AND t.status IN ({in_flight})
            AND s.name IN ({sweeps}) AND s.status = {running}
          GROUP BY t.task_id, t.current_worker_id
         HAVING sum(r.status NOT IN ({ended})) = 0",
        in_flight = sql_words::list(sql_words::TASKS_IN_FLIGHT),
        sweeps = sql_words::list(&[
            super::model::StepName::CodeSweep,
            super::model::StepName::CommentSweep,
        ]),
        running = word(super::model::StepStatus::Running),
        ended = sql_words::list(sql_words::TASKS_ENDED),
    ))?;
    let rows = stmt
        .query_map([&run.run_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut out = Vec::new();
    for (task_id, worker_id, last) in rows {
        let settled = last
            .as_deref()
            .and_then(|t| util::seconds_between(t, now))
            .is_some_and(|age| age >= REVIEWS_IN_AFTER_S);
        let author = workers::get(conn, &worker_id)?;
        if settled && author.status().is_ok_and(|s| s.is_live()) {
            out.push(ReviewsIn {
                parent: tasks::get(conn, &task_id)?,
                author,
            });
        }
    }
    Ok(out)
}

pub fn review_round(conn: &Connection, parent_id: &str) -> Result<String> {
    Ok(conn.query_row(
        "SELECT coalesce(group_concat(task_id, ','), '') FROM (
           SELECT task_id FROM tasks WHERE parent_task_id = ?1 AND kind = 'review'
            ORDER BY task_id)",
        [parent_id],
        |r| r.get(0),
    )?)
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

    fn pass_time(s: &Store, w: &str, secs: i64) -> String {
        let now: String = s
            .conn()
            .query_row(
                "SELECT max(t) FROM (SELECT claimed_at AS t FROM workers WHERE worker_id = ?1
                 UNION ALL SELECT at FROM checks WHERE worker_id = ?1)",
                [w],
                |row| row.get(0),
            )
            .unwrap();
        let t = |col: &str| {
            format!(
            "UPDATE {col} SET at = strftime('%Y-%m-%dT%H:%M:%SZ', at, '-{secs} seconds') WHERE worker_id = ?1"
        )
        };
        s.conn().execute(&t("checks"), [w]).unwrap();
        s.conn()
            .execute(
                &format!("UPDATE workers SET claimed_at = strftime('%Y-%m-%dT%H:%M:%SZ', claimed_at, '-{secs} seconds'), last_progress_at = strftime('%Y-%m-%dT%H:%M:%SZ', last_progress_at, '-{secs} seconds') WHERE worker_id = ?1"),
                [w],
            )
            .unwrap();
        now
    }

    #[test]
    fn a_timer_that_ran_the_whole_interval_is_a_proxy_checkin() {
        let (_d, mut s, run, w) = setup();
        let now = pass_time(&s, &w, 600);
        let obs = Script(RefCell::new(vec![
            live(screen(10), "working"),
            live(screen(20), "working"),
        ]));
        let r = check_run_at(&mut s, &run, &obs, false, now).unwrap();
        assert_eq!(r[0].verdict, "proxy_checkin", "{r:?}");
        let now = pass_time(&s, &w, 600);
        let r = check_run_at(&mut s, &run, &obs, false, now).unwrap();
        assert_eq!(r[0].verdict, "proxy_checkin", "{r:?}");
        assert_eq!(workers::get(s.conn(), &w).unwrap().stall_rounds, 0);
    }

    #[test]
    fn a_timer_that_ran_less_than_the_clock_and_an_idle_pane_is_no_progress() {
        let (_d, mut s, run, w) = setup();
        let now = pass_time(&s, &w, 600);
        let obs = Script(RefCell::new(vec![
            live(screen(10), "working"),
            live(screen(15), "idle"),
        ]));
        check_run_at(&mut s, &run, &obs, false, now).unwrap();
        let now = pass_time(&s, &w, 600);
        let r = check_run_at(&mut s, &run, &obs, false, now).unwrap();
        assert_eq!(r[0].verdict, "no_progress", "{r:?}");
        assert!(
            r[0].note.contains("stopped about 300s ago"),
            "{}",
            r[0].note
        );
        assert!(r[0].first_stall);
    }

    #[test]
    fn a_gone_process_is_dead_at_once() {
        let (_d, mut s, run, w) = setup();
        let now = pass_time(&s, &w, 600);
        let obs = Script(RefCell::new(vec![Observation::default()]));
        let r = check_run_at(&mut s, &run, &obs, false, now).unwrap();
        assert_eq!(r[0].verdict, "dead");
    }

    #[test]
    fn three_stalled_checks_are_dead_and_progress_resets_the_count() {
        let (_d, mut s, run, w) = setup();
        let idle = || live(None, "idle");
        let obs = Script(RefCell::new(vec![idle(), idle(), idle(), idle(), idle()]));
        for want in ["no_progress", "no_progress"] {
            let now = pass_time(&s, &w, 600);
            assert_eq!(
                check_run_at(&mut s, &run, &obs, false, now).unwrap()[0].verdict,
                want
            );
        }
        s.immediate(|tx| {
            workers::progress(tx, &w, ProgressSource::Agent, EventKind::Progress, "x")
        })
        .unwrap();
        let now = pass_time(&s, &w, 600);
        s.conn().execute("UPDATE workers SET last_progress_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-10 seconds') WHERE worker_id = ?1", [&w]).unwrap();
        let obs2 = Script(RefCell::new(vec![idle()]));
        assert_eq!(
            check_run_at(&mut s, &run, &obs2, false, now).unwrap()[0].verdict,
            "progress"
        );
        for want in ["no_progress", "no_progress", "dead"] {
            let now = pass_time(&s, &w, 600);
            assert_eq!(
                check_run_at(&mut s, &run, &obs, false, now).unwrap()[0].verdict,
                want
            );
        }
    }

    #[test]
    fn herdr_working_with_no_timer_and_no_transcript_is_a_proxy_checkin() {
        let (_d, mut s, run, w) = setup();
        let busy = || live(None, "working");
        let obs = Script(RefCell::new(vec![busy(), busy(), busy()]));
        for _ in 0..3 {
            let now = pass_time(&s, &w, 600);
            let r = check_run_at(&mut s, &run, &obs, false, now).unwrap();
            assert_eq!(r[0].verdict, "proxy_checkin", "{r:?}");
            assert!(r[0].note.contains("herdr shows working"), "{}", r[0].note);
            assert!(r[0].note.contains("no transcript"), "{}", r[0].note);
        }
        let worker = workers::get(s.conn(), &w).unwrap();
        assert_eq!(worker.status, "working");
        assert_eq!(worker.stall_rounds, 0);
        assert_eq!(worker.last_progress_source.as_deref(), Some("herdr_state"));
    }

    #[test]
    fn herdr_working_past_the_cap_is_one_suspect_then_dead() {
        let (_d, mut s, run, w) = setup();
        assert_eq!(run.stall_rounds, 3);
        let busy = || live(None, "working");
        let obs = Script(RefCell::new((0..8).map(|_| busy()).collect()));
        let mut got = Vec::new();
        for _ in 0..8 {
            let now = pass_time(&s, &w, 600);
            got.push(
                check_run_at(&mut s, &run, &obs, false, now)
                    .unwrap()
                    .remove(0),
            );
        }
        let verdicts: Vec<&str> = got.iter().map(|c| c.verdict.as_str()).collect();
        assert_eq!(
            verdicts,
            [
                "proxy_checkin",
                "proxy_checkin",
                "proxy_checkin",
                "proxy_checkin",
                "proxy_checkin",
                "proxy_checkin",
                "suspect",
                "dead"
            ]
        );
        assert!(got[6].changed, "the suspect check is a change: 1 wake");
        assert!(got[6].note.contains("6 checks"), "{}", got[6].note);
        assert!(
            got[7].note.contains("herdr shows working"),
            "{}",
            got[7].note
        );
    }

    #[test]
    fn a_frozen_timer_under_a_working_spinner_is_capped_too() {
        let (_d, mut s, run, w) = setup();
        let frozen = || live(screen(10), "working");
        let obs = Script(RefCell::new((0..9).map(|_| frozen()).collect()));
        let mut got = Vec::new();
        for _ in 0..9 {
            let now = pass_time(&s, &w, 600);
            got.push(
                check_run_at(&mut s, &run, &obs, false, now)
                    .unwrap()
                    .remove(0),
            );
        }
        let verdicts: Vec<&str> = got.iter().map(|c| c.verdict.as_str()).collect();
        assert_eq!(
            verdicts,
            [
                "proxy_checkin",
                "proxy_checkin",
                "proxy_checkin",
                "proxy_checkin",
                "proxy_checkin",
                "proxy_checkin",
                "proxy_checkin",
                "suspect",
                "dead"
            ]
        );
        assert!(
            got[1]
                .note
                .starts_with("herdr shows working, but the timer moved 0s in "),
            "{}",
            got[1].note
        );
        assert!(got[7].note.contains("6 checks"), "{}", got[7].note);
    }

    #[test]
    fn a_run_with_no_stall_rounds_still_gets_one_round_of_herdr_checkins() {
        let (_d, mut s, mut run, w) = setup();
        s.conn()
            .execute(
                "UPDATE runs SET stall_rounds = 0 WHERE run_id = ?1",
                [&run.run_id],
            )
            .unwrap();
        run.stall_rounds = 0;
        let busy = || live(None, "working");
        let obs = Script(RefCell::new((0..4).map(|_| busy()).collect()));
        let mut verdicts = Vec::new();
        for _ in 0..4 {
            let now = pass_time(&s, &w, 600);
            verdicts.push(
                check_run_at(&mut s, &run, &obs, false, now)
                    .unwrap()
                    .remove(0)
                    .verdict,
            );
        }
        assert_eq!(
            verdicts,
            ["proxy_checkin", "proxy_checkin", "suspect", "dead"]
        );
    }

    #[test]
    fn a_check_with_progress_ends_the_herdr_streak() {
        let (_d, mut s, run, w) = setup();
        let busy = || live(None, "working");
        let mut script: Vec<Observation> = (0..5).map(|_| busy()).collect();
        script.push(Observation {
            transcript_bytes: Some(10),
            ..busy()
        });
        script.push(Observation {
            transcript_bytes: Some(20),
            ..busy()
        });
        script.extend((0..6).map(|_| busy()));
        let obs = Script(RefCell::new(script));
        let mut verdicts = Vec::new();
        for _ in 0..13 {
            let now = pass_time(&s, &w, 600);
            verdicts.push(
                check_run_at(&mut s, &run, &obs, false, now)
                    .unwrap()
                    .remove(0)
                    .verdict,
            );
        }
        assert!(
            verdicts.iter().all(|v| v == "proxy_checkin"),
            "{verdicts:?}"
        );
    }

    #[test]
    fn a_harness_with_no_timer_format_is_named_in_the_note() {
        let (_d, mut s, run, w) = setup();
        s.conn()
            .execute(
                "UPDATE workers SET harness = 'pi' WHERE worker_id = ?1",
                [&w],
            )
            .unwrap();
        let now = pass_time(&s, &w, 600);
        let obs = Script(RefCell::new(vec![live(None, "idle")]));
        let r = check_run_at(&mut s, &run, &obs, false, now).unwrap();
        assert_eq!(r[0].verdict, "no_progress");
        assert!(
            r[0].note
                .contains("no timer format for harness pi; no transcript"),
            "{}",
            r[0].note
        );
    }

    #[test]
    fn the_lock_holder_names_the_binary_it_recorded() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(holder(dir.path(), "r1"), None, "no lock: no holder");
        let guard = lock(dir.path(), "r1").unwrap().unwrap();
        let me = std::process::id();
        let new = holder(dir.path(), "r1").unwrap();
        assert_eq!(new.pid, me);
        assert_eq!(new.exe, None);
        let current = crate::exeid::exe_identity(&std::env::current_exe().unwrap()).unwrap();
        assert!(new.recent);
        assert!(!new.runs_other_binary(Some(&current)));
        assert!(!new.binary_replaced());
        let owner = guard.path().join("owner");
        let mut v: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&owner).unwrap()).unwrap();
        v["acquired_at"] =
            crate::clock::stamp(chrono::Utc::now() - chrono::Duration::seconds(60)).into();
        std::fs::write(&owner, serde_json::to_vec(&v).unwrap()).unwrap();
        let old = holder(dir.path(), "r1").unwrap();
        assert!(!old.recent);
        assert!(old.runs_other_binary(Some(&current)));
        assert!(!old.runs_other_binary(None));
        assert!(old.binary_replaced());
        record_exe(&guard, Some(&current)).unwrap();
        let now = holder(dir.path(), "r1").unwrap();
        assert_eq!(now.exe.as_ref(), Some(&current));
        assert!(!now.runs_other_binary(Some(&current)));
        assert!(!now.binary_replaced(), "its binary file is unchanged");
        let rebuilt = ExeIdentity {
            len: current.len + 1,
            ..current.clone()
        };
        assert!(now.runs_other_binary(Some(&rebuilt)));
        let other = dir.path().join("other-horch");
        std::fs::write(&other, b"v1").unwrap();
        record_exe(&guard, crate::exeid::exe_identity(&other).as_ref()).unwrap();
        let elsewhere = holder(dir.path(), "r1").unwrap();
        assert!(elsewhere.runs_other_binary(Some(&current)));
        assert!(!elsewhere.binary_replaced());
        std::fs::write(&other, b"v2 updated").unwrap();
        assert!(elsewhere.binary_replaced(), "the file it runs was replaced");
        drop(guard);
        assert_eq!(
            holder(dir.path(), "r1"),
            None,
            "a released lock has no holder"
        );
    }

    #[test]
    fn a_recorded_start_holds_off_a_restart_while_that_process_runs() {
        let (_d, s, run, _w) = setup();
        let get = || runs::get(s.conn(), &run.run_id).unwrap();
        assert!(needs_restart(s.conn(), &get()).unwrap(), "never checked in");
        let me = std::process::id();
        record_start(s.conn(), &run.run_id, me, crate::procid::start_time(me)).unwrap();
        assert!(!needs_restart(s.conn(), &get()).unwrap(), "it is starting");
        assert_eq!(get().watchdog_checkin_at, None, "a start is no checkin");
        let Some(started) = crate::procid::start_time(me) else {
            return;
        };
        record_start(s.conn(), &run.run_id, me, Some(started + 1)).unwrap();
        assert!(needs_restart(s.conn(), &get()).unwrap(), "it has gone");
    }

    #[cfg(unix)]
    #[test]
    fn stop_ends_only_the_process_with_the_recorded_start_time() {
        let mut child = std::process::Command::new("sleep")
            .arg("60")
            .spawn()
            .unwrap();
        let pid = child.id();
        let Some(started) = crate::procid::start_time(pid) else {
            let _ = child.kill();
            let _ = child.wait();
            return;
        };
        let reaper = std::thread::spawn(move || child.wait().unwrap());
        let wrong = LockHolder {
            pid,
            started: Some(started + 1),
            exe: None,
            recent: false,
        };
        let e = wrong.stop(std::time::Duration::from_secs(1)).unwrap_err();
        assert!(format!("{e:#}").contains("nothing was signalled"), "{e:#}");
        assert!(crate::procid::alive(pid, Some(started)));
        let unknown = LockHolder {
            pid,
            started: None,
            exe: None,
            recent: false,
        };
        assert!(unknown.stop(std::time::Duration::from_secs(1)).is_err());
        assert!(crate::procid::alive(pid, Some(started)));
        let right = LockHolder {
            pid,
            started: Some(started),
            exe: None,
            recent: false,
        };
        right.stop(std::time::Duration::from_secs(5)).unwrap();
        assert!(!reaper.join().unwrap().success(), "ended by SIGTERM");
    }

    fn one_check(s: &mut Store, run: &Run, w: &str, o: Observation) -> Checked {
        let now = pass_time(s, w, 600);
        let obs = Script(RefCell::new(vec![o]));
        check_run_at(s, run, &obs, false, now).unwrap().remove(0)
    }

    #[test]
    fn a_check_names_the_workers_task_role_and_pane() {
        let (_d, mut s, run, w) = setup();
        s.conn()
            .execute(
                "UPDATE workers SET role = 'builder', pane_id = 'p-7' WHERE worker_id = ?1",
                [&w],
            )
            .unwrap();
        let c = one_check(&mut s, &run, &w, live(None, "idle"));
        assert_eq!(c.task.as_deref(), Some("T-1"));
        assert_eq!(c.role.as_deref(), Some("builder"));
        assert_eq!(c.pane_id.as_deref(), Some("p-7"));
        assert_eq!(c.worker_id, w);
        assert!(c.changed);
        assert_eq!(
            c.note,
            "no ledger call, no transcript growth, no state change, no running timer in 600s (no running timer; no transcript)"
        );
    }

    #[test]
    fn a_worker_that_waits_is_progress_and_names_the_reasons() {
        let (_d, mut s, run, w) = setup();
        workers::set_wait(s.conn(), &w, "question", "reason b", &[], 3600).unwrap();
        workers::set_wait(s.conn(), &w, "background", "reason a", &[], 3600).unwrap();
        let c = one_check(&mut s, &run, &w, live(None, "idle"));
        assert_eq!(c.verdict, "progress");
        assert_eq!(c.note, "waiting: reason a; reason b");
        assert_eq!(c.round, 0);
    }

    #[test]
    fn an_agent_that_recorded_progress_since_the_last_check_is_progress() {
        let (_d, mut s, run, w) = setup();
        let now = pass_time(&s, &w, 600);
        s.immediate(|tx| {
            workers::progress(tx, &w, ProgressSource::Agent, EventKind::Progress, "x")
        })
        .unwrap();
        let obs = Script(RefCell::new(vec![live(None, "idle")]));
        let c = check_run_at(&mut s, &run, &obs, false, now)
            .unwrap()
            .remove(0);
        assert_eq!(c.verdict, "progress");
        assert_eq!(c.note, "the agent recorded progress");
    }

    #[test]
    fn transcript_growth_is_a_proxy_checkin_and_records_the_source() {
        let (_d, mut s, run, w) = setup();
        let grown = |bytes| Observation {
            transcript_bytes: Some(bytes),
            ..live(None, "idle")
        };
        let first = one_check(&mut s, &run, &w, grown(10));
        assert_eq!(first.verdict, "no_progress", "{first:?}");
        let c = one_check(&mut s, &run, &w, grown(20));
        assert_eq!(c.verdict, "proxy_checkin");
        assert_eq!(c.note, "the transcript grew");
        let src: String = s
            .conn()
            .query_row(
                "SELECT last_progress_source FROM workers WHERE worker_id = ?1",
                [&w],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(src, ProgressSource::Transcript.as_str());
        let same = one_check(&mut s, &run, &w, grown(20));
        assert_eq!(same.verdict, "no_progress", "{same:?}");
    }

    #[test]
    fn a_transcript_written_since_the_check_is_a_proxy_checkin_once() {
        let (_d, mut s, run, w) = setup();
        let written = || Observation {
            transcript_mtime: Some("2999-01-01T00:00:00Z".into()),
            ..live(None, "idle")
        };
        let c = one_check(&mut s, &run, &w, written());
        assert_eq!(c.verdict, "proxy_checkin", "{c:?}");
        assert_eq!(c.note, "the transcript grew");
        let again = one_check(&mut s, &run, &w, written());
        assert_eq!(again.verdict, "no_progress", "{again:?}");
    }

    #[test]
    fn a_moved_herdr_counter_is_a_proxy_checkin() {
        let (_d, mut s, run, w) = setup();
        let seq = |n| Observation {
            herdr_seq: Some(n),
            ..live(None, "idle")
        };
        let first = one_check(&mut s, &run, &w, seq(1));
        assert_eq!(first.verdict, "no_progress", "{first:?}");
        let c = one_check(&mut s, &run, &w, seq(2));
        assert_eq!(c.verdict, "proxy_checkin");
        assert_eq!(c.note, "herdr's state changed");
        let same = one_check(&mut s, &run, &w, seq(2));
        assert_eq!(same.verdict, "no_progress", "{same:?}");
    }

    #[test]
    fn a_timer_that_ran_names_its_seconds_in_the_note() {
        let (_d, mut s, run, w) = setup();
        let c = one_check(&mut s, &run, &w, live(screen(10), "idle"));
        assert_eq!(c.verdict, "proxy_checkin", "{c:?}");
        assert_eq!(c.note, "the working timer ran the whole interval (600s)");
    }

    #[test]
    fn a_short_timer_under_an_idle_pane_names_what_herdr_shows() {
        let (_d, mut s, run, w) = setup();
        one_check(&mut s, &run, &w, live(screen(10), "working"));
        let c = one_check(&mut s, &run, &w, live(screen(12), "blocked"));
        assert_eq!(c.verdict, "no_progress");
        assert_eq!(
            c.note,
            "suspect: the timer moved 120s in 600s (it stopped about 480s ago), herdr shows blocked"
        );
    }

    #[test]
    fn a_worker_not_due_is_not_checked() {
        let (_d, mut s, run, _w) = setup();
        let obs = Script(RefCell::new(vec![]));
        assert!(check_run(&mut s, &run, &obs, false).unwrap().is_empty());
    }
}

#[cfg(test)]
mod reconcile_tests {
    use std::path::Path;

    use super::*;
    use crate::taskledger::model::Actor;
    use crate::taskledger::runs::{self, NewRun};
    use crate::taskledger::tasks::NewTask;
    use crate::taskledger::workers::NewWorker;

    fn world() -> (tempfile::TempDir, Store, Run) {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(&dir.path().join("l.sqlite3")).unwrap();
        let run = s
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
                    &NewTask::work("a", "a.md", "dev"),
                    Actor::Orchestrator,
                )?;
                Ok(run)
            })
            .unwrap();
        (dir, s, run)
    }

    fn at(t0: &chrono::DateTime<chrono::Utc>, secs: i64) -> String {
        crate::clock::stamp(*t0 + chrono::Duration::seconds(secs))
    }

    fn write(s: &mut Store, run: &Run, wake: &Wake) -> Option<String> {
        s.immediate(|tx| outbox::write(tx, &run.run_id, wake))
            .unwrap()
    }

    fn rows(s: &Store, run: &Run, event: &str) -> i64 {
        s.conn()
            .query_row(
                "SELECT count(*) FROM outbox WHERE run_id = ?1 AND event = ?2",
                [&run.run_id, event],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn worker(s: &mut Store, run: &Run, status: &str) -> String {
        let id = s
            .immediate(|tx| {
                workers::insert(
                    tx,
                    &NewWorker {
                        run_id: run.run_id.clone(),
                        persona: "dev".into(),
                        ..Default::default()
                    },
                )
            })
            .unwrap();
        s.conn()
            .execute(
                "UPDATE workers SET status = ?2 WHERE worker_id = ?1",
                [&id, status],
            )
            .unwrap();
        id
    }

    #[test]
    fn trf_22_a_task_startable_for_2_min_gets_1_rewake_per_board_state() {
        let (_dir, mut s, run) = world();
        let t0 = crate::clock::now();
        let mut seen = None;
        let rewake = |s: &Store, secs, seen: &mut Option<Sighting>| {
            startable_rewake(s.conn(), &run, &at(&t0, secs), seen).unwrap()
        };

        assert_eq!(rewake(&s, 0, &mut seen), None);
        assert_eq!(rewake(&s, 90, &mut seen), None, "under 2 min");
        let first = rewake(&s, 120, &mut seen).expect("2 min with no worker on its way");
        assert_eq!(first.event, OutboxEvent::Startable);
        assert!(first.text.contains("Spawn: dev x1."), "{}", first.text);
        assert!(
            outbox::key(&run.run_id, &first)
                .starts_with(&format!("{}:startable:rewake:", run.run_id)),
            "{first:?}"
        );
        assert!(write(&mut s, &run, &first).is_some());

        for secs in [150, 180, 600] {
            let again = rewake(&s, secs, &mut seen).expect("still startable");
            assert_eq!(
                outbox::key(&run.run_id, &again),
                outbox::key(&run.run_id, &first)
            );
            assert_eq!(
                write(&mut s, &run, &again),
                None,
                "1 re-wake per board state"
            );
        }
        assert_eq!(rows(&s, &run, "startable"), 1);

        s.immediate(|tx| {
            tasks::add(
                tx,
                &run,
                &NewTask::work("b", "b.md", "dev"),
                Actor::Orchestrator,
            )
        })
        .unwrap();
        assert_eq!(rewake(&s, 610, &mut seen), None, "the board changed");
        let second = rewake(&s, 730, &mut seen).expect("2 min in the new board state");
        assert!(second.text.contains("Spawn: dev x2."), "{}", second.text);
        assert!(write(&mut s, &run, &second).is_some());
        assert_eq!(rows(&s, &run, "startable"), 2);
    }

    #[test]
    fn trf_22_a_starting_or_claiming_worker_holds_the_rewake() {
        for status in ["spawning", "starting", "claiming"] {
            let (_dir, mut s, run) = world();
            let t0 = crate::clock::now();
            let mut seen = None;
            worker(&mut s, &run, status);
            for secs in [0, 120, 300] {
                assert_eq!(
                    startable_rewake(s.conn(), &run, &at(&t0, secs), &mut seen).unwrap(),
                    None,
                    "a {status} worker is on its way"
                );
            }
            assert_eq!(seen, None);
        }
    }

    #[test]
    fn trf_22_the_first_startable_wake_dates_the_board_state_after_a_restart() {
        let (_dir, mut s, run) = world();
        let t0 = crate::clock::now();
        let ready = tasks::startable(s.conn(), &run.run_id).unwrap()[0].short();
        let board = outbox::board(s.conn(), &run.run_id, true).unwrap();
        let first = Wake::new(OutboxEvent::Startable, ready, "first").on_board(board);
        write(&mut s, &run, &first).unwrap();
        s.conn()
            .execute("UPDATE outbox SET created_at = ?1", [at(&t0, -100)])
            .unwrap();

        let mut seen = None;
        assert_eq!(
            startable_rewake(s.conn(), &run, &at(&t0, 0), &mut seen).unwrap(),
            None,
            "100 s"
        );
        assert!(startable_rewake(s.conn(), &run, &at(&t0, 20), &mut seen)
            .unwrap()
            .is_some());
    }

    fn spawned(
        s: &mut Store,
        run: &Run,
        t0: &chrono::DateTime<chrono::Utc>,
        checkin: i64,
    ) -> String {
        let id = worker(s, run, "claiming");
        s.conn()
            .execute(
                "UPDATE workers SET spawned_at = ?2, last_checkin_at = ?3 WHERE worker_id = ?1",
                [&id, &at(t0, 0), &at(t0, checkin)],
            )
            .unwrap();
        id
    }

    #[test]
    fn trf_23_a_spawned_worker_with_no_claim_after_3_min_is_flagged_once() {
        let (_dir, mut s, run) = world();
        let t0 = crate::clock::now() - chrono::Duration::seconds(1_000);
        let id = spawned(&mut s, &run, &t0, 170);

        assert!(unclaimed_spawns(s.conn(), &run, &at(&t0, 175))
            .unwrap()
            .is_empty());
        let late = unclaimed_spawns(s.conn(), &run, &at(&t0, 180)).unwrap();
        assert_eq!(late.len(), 1, "{late:?}");
        assert_eq!(late[0].worker.worker_id, id);
        assert_eq!(late[0].waited_s, 180);

        let wake = s
            .immediate(|tx| flag_unclaimed(tx, &run, &late[0], Some("idle")))
            .unwrap()
            .expect("the first flag");
        assert_eq!(wake.event, OutboxEvent::UnclaimedSpawn);
        assert_eq!(
            outbox::key(&run.run_id, &wake),
            format!("{}:unclaimed-spawn:asked:{id}", run.run_id)
        );
        assert!(
            wake.text.contains("has not claimed a task in 180s"),
            "{}",
            wake.text
        );
        assert!(wake.text.contains("herdr status idle"), "{}", wake.text);
        assert!(write(&mut s, &run, &wake).is_some());

        assert!(unclaimed_spawns(s.conn(), &run, &at(&t0, 200))
            .unwrap()
            .is_empty());
        assert_eq!(
            s.immediate(|tx| flag_unclaimed(tx, &run, &late[0], None))
                .unwrap(),
            None
        );
        s.conn()
            .execute(
                "UPDATE workers SET last_checkin_at = ?2 WHERE worker_id = ?1",
                [&id, &util::now()],
            )
            .unwrap();
        let swept = workers::sweep_preclaim(s.conn(), &run, &|_| true).unwrap();
        assert!(swept.is_empty(), "{swept:?}");
        assert_eq!(rows(&s, &run, "unclaimed-spawn"), 1);
    }

    #[test]
    fn trf_23_a_stale_or_claimed_worker_is_not_flagged() {
        let (_dir, mut s, run) = world();
        let t0 = crate::clock::now();
        spawned(&mut s, &run, &t0, 10);
        assert!(unclaimed_spawns(s.conn(), &run, &at(&t0, 300))
            .unwrap()
            .is_empty());
        let id = spawned(&mut s, &run, &t0, 290);
        s.conn()
            .execute(
                "UPDATE workers SET status = 'working' WHERE worker_id = ?1",
                [&id],
            )
            .unwrap();
        assert!(unclaimed_spawns(s.conn(), &run, &at(&t0, 300))
            .unwrap()
            .is_empty());
    }
}
