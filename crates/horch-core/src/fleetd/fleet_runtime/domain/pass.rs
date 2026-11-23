use std::collections::BTreeMap;

use crate::fleetd::fleet_runtime::domain::clock::FleetSeconds;

pub const START_TIMEOUT_S: i64 = 90;

pub const TIMED_OUT: &str = "timed_out";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Proc {
    Absent,
    Alive,
    Dead,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Launch {
    pub at: FleetSeconds,
    pub process: Proc,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchStep {
    Launch,
    Wait,
    Live,
    TimedOut,
}

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopStep {
    Polite,
    Wait,
    Kill,
}

pub fn stop_step(polite_at: Option<FleetSeconds>, clock: FleetSeconds, grace_s: u64) -> StopStep {
    let grace = i64::try_from(grace_s).unwrap_or(i64::MAX);
    match polite_at {
        None => StopStep::Polite,
        Some(at) if clock.0 >= at.0.saturating_add(grace) => StopStep::Kill,
        Some(_) => StopStep::Wait,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EffectKind {
    Stop,
    Launch,
    StartJob,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Holding<'a> {
    pub attempt: &'a str,
    pub worker: &'a str,
    pub task: Option<&'a str>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Breach {
    WorkerAttempts {
        worker: String,
        attempts: Vec<String>,
    },
    TaskAttempts {
        task: String,
        attempts: Vec<String>,
    },
}

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
