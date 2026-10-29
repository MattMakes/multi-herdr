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
        check_run(&mut s, &run, &obs, false).unwrap();
        pass_time(&s, &w, 600);
        let r = check_run(&mut s, &run, &obs, false).unwrap();
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
    fn a_worker_not_due_is_not_checked() {
        let (_d, mut s, run, _w) = setup();
        let obs = Script(RefCell::new(vec![]));
        assert!(check_run(&mut s, &run, &obs, false).unwrap().is_empty());
    }
}
