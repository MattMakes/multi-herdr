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
//!    showing `working` goes to rule 5; else `no_progress`.
//! 5. Nothing else moved, but herdr shows `working` (#203): a proxy
//!    checkin, at most `2 * stall_rounds` checks in a row (`stall_rounds`
//!    counts as at least 1). The next one is `suspect` (1 wake with the
//!    pane), and the one after it is `dead`.
//! 6. Nothing moved: `no_progress`, and 1 more stall round. At the run's
//!    `stall_rounds` the worker is `dead`.
//!
//! A note names the signals the watchdog could not read ("no timer format
//! for harness pi; no transcript"), so `ledger show` explains a reap.
//!
//! This module decides and records; the caller reaps and wakes.

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::Serialize;

use super::model::{EventKind, ProgressSource, Verdict};
use super::runs::Run;
use super::store::{optional, Store};
use super::timer;
use super::util;
use super::workers::{self, Worker};
use crate::exeid::ExeIdentity;

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
    /// The worker's herdr pane, for a `suspect` wake.
    pub pane_id: Option<String>,
}

/// The note prefix of a check that only herdr's `working` status kept alive.
/// [`herdr_only_streak`] counts the streak by this prefix, so every note of
/// [`herdr_fallback`] starts with it, and no other note may.
const HERDR_ONLY: &str = "herdr shows working";

/// The signals the watchdog could not read for `w`, for the check note.
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

/// How many checks in a row, up to the last, only herdr's `working` status
/// kept alive: the checks whose note starts with [`HERDR_ONLY`].
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

/// Nothing but herdr's `working` status moved: a proxy checkin up to the
/// run's cap of `2 * stall_rounds` checks, then `suspect` once, then `dead`.
/// `detail` follows [`HERDR_ONLY`] in the note of a proxy checkin.
fn herdr_fallback(
    conn: &Connection,
    w: &Worker,
    run: &Run,
    prev_verdict: Option<&str>,
    detail: &str,
    missing: &str,
) -> Result<(Verdict, String, Option<ProgressSource>)> {
    // A run made with 0 or fewer stall rounds still gets 1 round, so the
    // cap and the query limit stay positive.
    let cap = 2 * run.stall_rounds.max(1);
    let streak = herdr_only_streak(conn, &w.worker_id, cap)?;
    // The last check was this fallback's own `suspect`: its note starts
    // with HERDR_ONLY, so the streak counts it. A `suspect` of the timer
    // rule (rule 4) has another note, and ends the streak at 0.
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

/// Check every due working worker of `run`. `force` checks every one now,
/// due or not.
pub fn check_run(
    store: &mut Store,
    run: &Run,
    observer: &dyn Observer,
    force: bool,
) -> Result<Vec<Checked>> {
    check_run_at(store, run, observer, force, util::now())
}

/// [`check_run`] at the ledger stamp `now`.
fn check_run_at(
    store: &mut Store,
    run: &Run,
    observer: &dyn Observer,
    force: bool,
    now: String,
) -> Result<Vec<Checked>> {
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
        let waits = workers::waiting(store.conn(), &w.worker_id)?;
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
        let missing = missing_signals(&w, &o, timer_s);
        let (mut verdict, mut note, source) = if !o.agent_alive {
            (Verdict::Dead, "the agent process is gone".to_string(), None)
        } else if !waits.is_empty() {
            (
                Verdict::Progress,
                format!(
                    "waiting: {}",
                    waits
                        .iter()
                        .map(|w| w.reason.as_str())
                        .collect::<Vec<_>>()
                        .join("; ")
                ),
                None,
            )
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
            // A frozen timer under a `working` spinner gets the same cap.
            herdr_fallback(
                store.conn(),
                &w,
                run,
                p.as_ref().map(|p| p.verdict.as_str()),
                &format!(
                    "but the timer moved {}s in {elapsed}s",
                    timer_delta.unwrap_or(0)
                ),
                &missing,
            )?
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
        } else if o.herdr_status.as_deref() == Some("working") {
            herdr_fallback(
                store.conn(),
                &w,
                run,
                p.as_ref().map(|p| p.verdict.as_str()),
                &format!("and nothing else moved in {elapsed}s"),
                &missing,
            )?
        } else {
            (
                Verdict::NoProgress,
                format!(
                    "no ledger call, no transcript growth, no state change, no running timer in {elapsed}s{missing}"
                ),
                None,
            )
        };
        let mut round = match verdict {
            Verdict::NoProgress => workers::get(store.conn(), &w.worker_id)?.stall_rounds + 1,
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
            pane_id: w.pane_id.clone(),
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

/// The file in a watchdog's lock dir that names the binary it runs (#216).
const EXE_FILE: &str = "exe";

/// How long a new watchdog has to record its binary after it takes the
/// lock. Until then a missing record is not a sign of an old watchdog.
const EXE_RECORD_GRACE_S: i64 = 10;

/// Record in the held lock the binary this watchdog runs. The write is
/// atomic, so a reader sees the whole record or none.
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

/// The live process that holds a run's watchdog lock.
#[derive(Debug, Clone, PartialEq)]
pub struct LockHolder {
    pub pid: u32,
    /// Its start time; `None` on a platform without start times.
    pub started: Option<u64>,
    /// The binary it runs; `None` for a watchdog older than this record, or
    /// for a new one that has not written it yet.
    pub exe: Option<ExeIdentity>,
    /// It took the lock less than [`EXE_RECORD_GRACE_S`] ago, so a missing
    /// binary record can still be on its way.
    pub recent: bool,
}

impl LockHolder {
    /// Whether it recorded no binary and is not new: a watchdog older than
    /// the binary record.
    fn unrecorded(&self) -> bool {
        self.exe.is_none() && !self.recent
    }

    /// Whether it runs another binary than `current` (`horch install`). A
    /// holder older than the binary record is not current either. With
    /// `current` unknown nothing can be told: `false`.
    pub fn runs_other_binary(&self, current: Option<&ExeIdentity>) -> bool {
        current.is_some_and(|want| match &self.exe {
            Some(exe) => exe != want,
            None => self.unrecorded(),
        })
    }

    /// Whether the file it started from changed since (a self-update or a
    /// rebuild replaced it), or it is older than the binary record (#216).
    /// A holder that runs another binary at another path, unchanged, is
    /// left alone: 2 binaries on 1 ledger do not replace each other's
    /// watchdog on every command.
    pub fn binary_replaced(&self) -> bool {
        match &self.exe {
            Some(exe) => {
                crate::exeid::exe_identity(std::path::Path::new(&exe.path)).as_ref() != Some(exe)
            }
            None => self.unrecorded(),
        }
    }

    /// Stop it with SIGTERM, then wait up to `wait` for it to end. The
    /// signal goes only to the PID that still has the recorded start time,
    /// never to a process found by name.
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

    /// No signals here: a watchdog of another binary is reported, not stopped.
    #[cfg(not(unix))]
    pub fn stop(&self, _wait: std::time::Duration) -> Result<()> {
        anyhow::bail!(
            "watchdog PID {} runs another binary; stop it by hand on this platform",
            self.pid
        )
    }
}

/// The live holder of the watchdog lock of `run_id` in `dir`, or `None` when
/// no live process holds it.
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
    // The wall clock, not the ledger clock: HORCH_NOW does not move a lock.
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

/// Record the watchdog's own heartbeat on the run.
pub fn checkin(conn: &Connection, run_id: &str, pid: u32, started: Option<u64>) -> Result<()> {
    conn.execute(
        "UPDATE runs SET watchdog_pid = ?2, watchdog_started = ?3, watchdog_checkin_at = ?4
         WHERE run_id = ?1",
        params![run_id, pid as i64, started.map(|s| s as i64), util::now()],
    )?;
    Ok(())
}

/// Record on the run the watchdog process that a starter just started, but
/// no checkin. Until it takes its lock and checks in, the next command
/// sees it alive in [`needs_restart`] and does not start a second one.
pub fn record_start(conn: &Connection, run_id: &str, pid: u32, started: Option<u64>) -> Result<()> {
    conn.execute(
        "UPDATE runs SET watchdog_pid = ?2, watchdog_started = ?3 WHERE run_id = ?1",
        params![run_id, pid as i64, started.map(|s| s as i64)],
    )?;
    Ok(())
}

/// Every active run of the ledger, of any project: `horch install` checks
/// the watchdog of each (#216).
pub fn active_runs(conn: &Connection) -> Result<Vec<Run>> {
    let mut stmt = conn.prepare("SELECT run_id FROM runs WHERE status = 'active'")?;
    let ids = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ids.iter().map(|id| super::runs::get(conn, id)).collect()
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

    /// Back-date the worker's last check (or claim) by `secs`, as if that
    /// much time had passed.
    fn pass_time(s: &Store, w: &str, secs: i64) {
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
    }

    #[test]
    fn a_timer_that_ran_the_whole_interval_is_a_proxy_checkin() {
        let (_d, mut s, run, w) = setup();
        pass_time(&s, &w, 600);
        let obs = Script(RefCell::new(vec![
            live(screen(10), "working"),
            live(screen(20), "working"),
        ]));
        let r = check_run(&mut s, &run, &obs, false).unwrap();
        assert_eq!(r[0].verdict, "proxy_checkin", "{r:?}");
        pass_time(&s, &w, 600);
        let r = check_run(&mut s, &run, &obs, false).unwrap();
        assert_eq!(r[0].verdict, "proxy_checkin", "{r:?}");
        assert_eq!(workers::get(s.conn(), &w).unwrap().stall_rounds, 0);
    }

    #[test]
    fn a_timer_that_ran_less_than_the_clock_and_an_idle_pane_is_no_progress() {
        let (_d, mut s, run, w) = setup();
        pass_time(&s, &w, 600);
        let obs = Script(RefCell::new(vec![
            live(screen(10), "working"),
            live(screen(15), "idle"),
        ]));
        // Both checks at one stamp, so the second sees exactly 600 s pass
        // even when the wall clock ticks a second between them.
        let now = util::now();
        check_run_at(&mut s, &run, &obs, false, now.clone()).unwrap();
        pass_time(&s, &w, 600);
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
        pass_time(&s, &w, 600);
        let obs = Script(RefCell::new(vec![Observation::default()]));
        let r = check_run(&mut s, &run, &obs, false).unwrap();
        assert_eq!(r[0].verdict, "dead");
    }

    #[test]
    fn three_stalled_checks_are_dead_and_progress_resets_the_count() {
        let (_d, mut s, run, w) = setup();
        let idle = || live(None, "idle");
        let obs = Script(RefCell::new(vec![idle(), idle(), idle(), idle(), idle()]));
        for want in ["no_progress", "no_progress"] {
            pass_time(&s, &w, 600);
            assert_eq!(
                check_run(&mut s, &run, &obs, false).unwrap()[0].verdict,
                want
            );
        }
        // The agent records progress: the count resets.
        s.immediate(|tx| {
            workers::progress(tx, &w, ProgressSource::Agent, EventKind::Progress, "x")
        })
        .unwrap();
        pass_time(&s, &w, 600);
        s.conn().execute("UPDATE workers SET last_progress_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now', '-10 seconds') WHERE worker_id = ?1", [&w]).unwrap();
        let obs2 = Script(RefCell::new(vec![idle()]));
        assert_eq!(
            check_run(&mut s, &run, &obs2, false).unwrap()[0].verdict,
            "progress"
        );
        for want in ["no_progress", "no_progress", "dead"] {
            pass_time(&s, &w, 600);
            assert_eq!(
                check_run(&mut s, &run, &obs, false).unwrap()[0].verdict,
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
            pass_time(&s, &w, 600);
            let r = check_run(&mut s, &run, &obs, false).unwrap();
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
        // The run keeps the default 3 stall rounds: the cap is 6 checks.
        assert_eq!(run.stall_rounds, 3);
        let busy = || live(None, "working");
        let obs = Script(RefCell::new((0..8).map(|_| busy()).collect()));
        let mut got = Vec::new();
        for _ in 0..8 {
            pass_time(&s, &w, 600);
            got.push(check_run(&mut s, &run, &obs, false).unwrap().remove(0));
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
        // The timer shows 10m at every check: it froze after the first.
        let frozen = || live(screen(10), "working");
        let obs = Script(RefCell::new((0..9).map(|_| frozen()).collect()));
        let mut got = Vec::new();
        for _ in 0..9 {
            pass_time(&s, &w, 600);
            got.push(check_run(&mut s, &run, &obs, false).unwrap().remove(0));
        }
        let verdicts: Vec<&str> = got.iter().map(|c| c.verdict.as_str()).collect();
        // Check 1: the timer ran the whole interval. Checks 2 to 7: only
        // herdr's spinner moved, up to the cap of 6. Then 1 suspect, then
        // dead.
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
            pass_time(&s, &w, 600);
            verdicts.push(
                check_run(&mut s, &run, &obs, false)
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
        // Check 6: the transcript appears and grows at check 7.
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
            pass_time(&s, &w, 600);
            verdicts.push(
                check_run(&mut s, &run, &obs, false)
                    .unwrap()
                    .remove(0)
                    .verdict,
            );
        }
        // 13 checks, none past the cap of 6 in a row: the growth at check 7
        // starts a new streak.
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
        pass_time(&s, &w, 600);
        let obs = Script(RefCell::new(vec![live(None, "idle")]));
        let r = check_run(&mut s, &run, &obs, false).unwrap();
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
        // A new holder can still be writing its binary record: it is left
        // alone.
        assert!(new.recent);
        assert!(!new.runs_other_binary(Some(&current)));
        assert!(!new.binary_replaced());
        // The same holder 1 minute later recorded no binary: it is older
        // than the record.
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
        // A holder of another binary at another path: `install` replaces
        // it, but only a change of that file itself counts for `ensure`.
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

    /// A watchdog that its starter recorded, but that has not checked in
    /// yet, is not restarted while it runs; once it is gone, it is.
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
            return; // No start times on this platform: a pid alone is alive.
        };
        // The same PID with another start time: the started process ended.
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
            return; // No start times on this platform.
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
        pass_time(s, w, 600);
        let obs = Script(RefCell::new(vec![o]));
        check_run(s, run, &obs, false).unwrap().remove(0)
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
        pass_time(&s, &w, 600);
        s.immediate(|tx| {
            workers::progress(tx, &w, ProgressSource::Agent, EventKind::Progress, "x")
        })
        .unwrap();
        let obs = Script(RefCell::new(vec![live(None, "idle")]));
        let c = check_run(&mut s, &run, &obs, false).unwrap().remove(0);
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
        // The same size again is no growth.
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
        // The same mtime as the last check is not new.
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
