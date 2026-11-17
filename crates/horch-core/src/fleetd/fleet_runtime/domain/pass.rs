//! The 3-part reconcile pass: intended rows and attempts in `launching` (RR-4, RR-7, FM-11).
//!
//! `docs/specs/fleetd/design.md`, "The reconcile pass": part 1 reads the
//! facts, part 2 writes each effect as a row in an intended state in 1
//! transaction, and part 3 does the effects after the commit. These are the
//! rules of part 2 and part 3, with no I/O:
//!
//! - [`observe_launch`]: the next step of an attempt in `launching`.
//! - [`stop_step`]: the signal of a stop, with its grace on the fleet clock.
//! - [`EffectKind`]: stops come before launches.
//! - [`breaches`]: the breaches of I1 and I4 that no step repairs.

use std::collections::BTreeMap;

use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;

/// The start timeout of an attempt, in seconds on the fleet clock: an
/// attempt in `launching` with no process this long after its launch ends
/// `timed_out`.
pub const START_TIMEOUT_S: i64 = 90;

/// The outcome of an attempt that ends at its start timeout.
pub const TIMED_OUT: &str = "timed_out";

/// What part 1 of a pass saw of 1 process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Proc {
    /// No process is recorded.
    Absent,
    Alive,
    Dead,
    /// The scan did not reach it, or could not tell.
    Unknown,
}

/// The launch of an attempt: when it was done, on the fleet clock, and
/// what part 1 saw of its process. The process is [`Proc::Absent`] when
/// the launch did not record one: it failed, or fleetd died within it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Launch {
    pub at: FleetSeconds,
    pub process: Proc,
}

/// The next step of an attempt in `launching`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchStep {
    /// No launch yet: launch it in part 3.
    Launch,
    /// Wait for a later pass.
    Wait,
    /// A live process has the attempt id: the attempt is `live`.
    Live,
    /// No process past the start timeout: the attempt ends `timed_out`.
    TimedOut,
}

/// The next step of an attempt in `launching` at `clock`. `row` is the
/// process that its row names, and `launch` its launch. An attempt is
/// launched at most once, so its id never names a second process. A fact
/// that the pass could not read makes it wait.
pub fn observe_launch(row: Proc, launch: Option<Launch>, clock: FleetSeconds) -> LaunchStep {
    let launched = launch.map_or(Proc::Absent, |l| l.process);
    if row == Proc::Alive || launched == Proc::Alive {
        return LaunchStep::Live;
    }
    if row == Proc::Unknown || launched == Proc::Unknown {
        return LaunchStep::Wait;
    }
    match launch {
        None => LaunchStep::Launch,
        Some(l) if clock.0 >= l.at.0.saturating_add(START_TIMEOUT_S) => LaunchStep::TimedOut,
        Some(_) => LaunchStep::Wait,
    }
}

/// The signal that a stop sends in this pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopStep {
    /// The polite stop that the process can catch.
    Polite,
    /// The polite stop was sent, and its grace has not ended.
    Wait,
    /// The grace ended: the kill.
    Kill,
}

/// The signal of a stop at `clock`, where the polite stop was sent at
/// `polite_at` (`None`: not yet). The grace is `grace_s` on the fleet
/// clock, so a pass never waits for it.
pub fn stop_step(polite_at: Option<FleetSeconds>, clock: FleetSeconds, grace_s: u64) -> StopStep {
    let grace = i64::try_from(grace_s).unwrap_or(i64::MAX);
    match polite_at {
        None => StopStep::Polite,
        Some(at) if clock.0 >= at.0.saturating_add(grace) => StopStep::Kill,
        Some(_) => StopStep::Wait,
    }
}

/// The kinds of effect of part 3, in the order of part 3: each stop runs
/// before each launch, and a job starts after the attempts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EffectKind {
    /// Stop a process or close a pane.
    Stop,
    /// Launch an attempt.
    Launch,
    /// Start the process of a job.
    StartJob,
}

/// 1 attempt that holds its worker and its task.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Holding<'a> {
    pub attempt: &'a str,
    pub worker: &'a str,
    pub task: Option<&'a str>,
}

/// A breach of an invariant that no step of the pass repairs.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Breach {
    /// I4: the worker holds more than 1 attempt.
    WorkerAttempts {
        worker: String,
        attempts: Vec<String>,
    },
    /// I1: the task has more than 1 attempt that holds it.
    TaskAttempts { task: String, attempts: Vec<String> },
}

/// The breaches of I4 and of I1 in `holding`, each attempt that is not
/// `ended`: a worker or a task with more than 1 of them.
pub fn breaches(holding: &[Holding<'_>]) -> Vec<Breach> {
    let mut by_worker: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    let mut by_task: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for h in holding {
        by_worker
            .entry(h.worker)
            .or_default()
            .push(h.attempt.to_string());
        if let Some(task) = h.task {
            by_task.entry(task).or_default().push(h.attempt.to_string());
        }
    }
    let workers = by_worker
        .into_iter()
        .filter(|(_, a)| a.len() > 1)
        .map(|(w, attempts)| Breach::WorkerAttempts {
            worker: w.to_string(),
            attempts,
        });
    let tasks = by_task
        .into_iter()
        .filter(|(_, a)| a.len() > 1)
        .map(|(t, attempts)| Breach::TaskAttempts {
            task: t.to_string(),
            attempts,
        });
    workers.chain(tasks).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: FleetSeconds = FleetSeconds(100);

    fn launched(at: i64, process: Proc) -> Option<Launch> {
        Some(Launch {
            at: FleetSeconds(at),
            process,
        })
    }

    #[test]
    fn an_attempt_with_no_launch_is_launched() {
        assert_eq!(observe_launch(Proc::Absent, None, T0), LaunchStep::Launch);
        assert_eq!(observe_launch(Proc::Dead, None, T0), LaunchStep::Launch);
    }

    #[test]
    fn a_live_process_of_the_attempt_makes_it_live() {
        assert_eq!(observe_launch(Proc::Alive, None, T0), LaunchStep::Live);
        let l = launched(90, Proc::Alive);
        assert_eq!(observe_launch(Proc::Absent, l, T0), LaunchStep::Live);
    }

    #[test]
    fn a_launch_with_no_process_ends_at_the_start_timeout() {
        for process in [Proc::Dead, Proc::Absent] {
            let l = launched(10, process);
            assert_eq!(
                observe_launch(Proc::Absent, l, FleetSeconds(99)),
                LaunchStep::Wait
            );
            assert_eq!(
                observe_launch(Proc::Absent, l, FleetSeconds(100)),
                LaunchStep::TimedOut
            );
        }
    }

    #[test]
    fn a_fact_that_the_pass_could_not_read_waits() {
        assert_eq!(observe_launch(Proc::Unknown, None, T0), LaunchStep::Wait);
        let l = launched(0, Proc::Unknown);
        assert_eq!(observe_launch(Proc::Absent, l, T0), LaunchStep::Wait);
    }

    #[test]
    fn a_stop_is_polite_first_and_kills_after_its_grace() {
        assert_eq!(stop_step(None, T0, 20), StopStep::Polite);
        assert_eq!(stop_step(Some(T0), FleetSeconds(119), 20), StopStep::Wait);
        assert_eq!(stop_step(Some(T0), FleetSeconds(120), 20), StopStep::Kill);
    }

    #[test]
    fn stops_come_before_launches() {
        let mut kinds = vec![EffectKind::StartJob, EffectKind::Launch, EffectKind::Stop];
        kinds.sort();
        assert_eq!(
            kinds,
            vec![EffectKind::Stop, EffectKind::Launch, EffectKind::StartJob]
        );
    }

    #[test]
    fn a_second_holding_attempt_of_a_worker_or_a_task_is_a_breach() {
        let h = |attempt, worker, task| Holding {
            attempt,
            worker,
            task,
        };
        assert_eq!(
            breaches(&[h("a1", "w1", Some("T-1")), h("a2", "w2", Some("T-2"))]),
            Vec::new()
        );
        assert_eq!(
            breaches(&[h("a1", "w1", Some("T-1")), h("a2", "w1", None)]),
            vec![Breach::WorkerAttempts {
                worker: "w1".into(),
                attempts: vec!["a1".into(), "a2".into()],
            }]
        );
        assert_eq!(
            breaches(&[h("a1", "w1", Some("T-1")), h("a2", "w2", Some("T-1"))]),
            vec![Breach::TaskAttempts {
                task: "T-1".into(),
                attempts: vec!["a1".into(), "a2".into()],
            }]
        );
    }
}
