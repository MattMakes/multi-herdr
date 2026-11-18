//! The rules of a push dispatch: which ready task binds to which persona
//! now, and why each other ready task waits (ticket 08).
//!
//! `docs/specs/fleetd/design.md`, "The task state machine" (`ready`),
//! FR-5 and "Holds on dispatch are visible"; rows L2, L3 and L6 of
//! `preservation-04-behavior.md`. Facts in, choices out. No I/O.
//!
//! - L2: the order is the target task first, then priority, then seq.
//! - L3: a retry goes first: rung 6 lowers its priority.
//! - L6: no 2 tasks of 1 run that run together own overlapping paths, an
//!   exclusive task runs alone, shared-checkout tasks run 1 at a time, and
//!   a review task never waits on these rules.
//! - Each hold has a cause: a global hold, an unhealthy profile, a hold of
//!   the persona, a hold of the project, `not_before`, an open attempt, or
//!   a tree that waits for its rescue.
//! - An unhealthy profile has no fallback: its task waits (fleetd-p7
//!   MAP.md, Q15).

use std::collections::{BTreeMap, BTreeSet};

use super::retry::claimable_at;

/// The facts of 1 `ready` task that the choice reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadyTask {
    pub task_id: String,
    /// `T-<seq>`, for the text of a hold.
    pub short: String,
    pub run_id: String,
    pub project: String,
    /// The persona of its next attempt: `claimable_by`, else its primary.
    pub persona: String,
    pub priority: i64,
    pub seq: i64,
    /// The caller names this task first (a spawn for a target task, or a
    /// replacement of the worker that held it).
    pub target: bool,
    /// The end of its retry backoff on the fleet clock.
    pub not_before: Option<i64>,
    pub parallel: bool,
    pub shared_checkout: bool,
    pub review: bool,
    /// The paths that it owns.
    pub own: Vec<String>,
    /// An attempt of the task that is not ended (a restart on a fallback).
    pub open_attempt: Option<String>,
    /// Its tree is `orphaned`: its rescue comes first.
    pub tree_orphaned: bool,
}

/// The facts of 1 task that holds its paths now: `leased`, `running`,
/// `stalled`, `verifying`, `blocked_on_user` or `waiting_quota`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunningTask {
    pub short: String,
    pub run_id: String,
    pub parallel: bool,
    pub shared_checkout: bool,
    pub review: bool,
    pub own: Vec<String>,
}

/// The holds on dispatch now, each with its cause.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Holds {
    /// The fleet clock, in seconds.
    pub clock: i64,
    /// The global holds: the health floors and the worktree disk budget.
    pub global: Vec<String>,
    /// The holds of 1 persona, by persona: pacing of its pool, or no check
    /// sandbox.
    pub personas: BTreeMap<String, String>,
    /// The holds of 1 project, by project id: a missing fast gate.
    pub projects: BTreeMap<String, String>,
    /// The unhealthy profiles: a failed conformance run or a dialog marked
    /// each one (fleetd-p7 ticket 26).
    pub unhealthy: BTreeSet<String>,
}

/// Why a ready task is not bound now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hold {
    /// A global hold.
    Global(String),
    /// The profile of the task is unhealthy.
    UnhealthyProfile { profile: String },
    /// A hold of the persona of the task.
    Persona { persona: String, cause: String },
    /// A hold of the project of the task.
    Project { project: String, cause: String },
    /// The retry backoff ends in `seconds` on the fleet clock.
    NotBefore { seconds: i64 },
    /// An attempt of the task is not ended.
    OpenAttempt { attempt: String },
    /// The tree of the task is `orphaned` and waits for its rescue.
    TreeRescue,
    /// The idea of the task is `parked`: no new task of it is dispatched
    /// (`docs/tickets/fleetd-p6/MAP.md`, Q10).
    IdeaParked { idea: String },
    /// An exclusive task runs, or this exclusive task waits for others.
    Exclusive { running: String },
    /// Another shared-checkout task works in the 1 shared checkout.
    SharedCheckout { running: String },
    /// A task that runs owns a path that overlaps a path of this task.
    PathHeld { path: String, by: String },
}

impl std::fmt::Display for Hold {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Hold::Global(cause) => write!(f, "{cause}"),
            Hold::UnhealthyProfile { profile } => write!(f, "unhealthy profile {profile}"),
            Hold::Persona { persona, cause } => write!(f, "persona {persona}: {cause}"),
            Hold::Project { project, cause } => write!(f, "project {project}: {cause}"),
            Hold::NotBefore { seconds } => write!(f, "retry after {seconds} s"),
            Hold::OpenAttempt { attempt } => write!(f, "attempt {attempt} is not ended"),
            Hold::TreeRescue => write!(f, "its worktree is orphaned and waits for its rescue"),
            Hold::IdeaParked { idea } => write!(f, "its idea {idea} is parked"),
            Hold::Exclusive { running } => write!(f, "exclusive: waits for {running}"),
            Hold::SharedCheckout { running } => {
                write!(f, "exclusive: shared checkout, waits for {running}")
            }
            Hold::PathHeld { path, by } => write!(f, "{path} is held by {by}"),
        }
    }
}

/// The choice for 1 ready task.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Choice {
    /// Bind the task to a new attempt of `persona`.
    Bind { task_id: String, persona: String },
    /// The task stays `ready`, for this cause.
    Held { task_id: String, hold: Hold },
}

/// The choice for each task of `ready`, in dispatch order. The tasks are
/// chosen greedily: each one is checked against `running` and the tasks
/// bound before it.
pub fn choose(ready: &[ReadyTask], running: &[RunningTask], holds: &Holds) -> Vec<Choice> {
    let mut order: Vec<&ReadyTask> = ready.iter().collect();
    order.sort_by_key(|t| (!t.target, t.priority, t.seq));
    let mut active: Vec<RunningTask> = running.to_vec();
    let mut out = Vec::new();
    for t in order {
        match hold(t, &active, holds) {
            Some(hold) => out.push(Choice::Held {
                task_id: t.task_id.clone(),
                hold,
            }),
            None => {
                active.push(RunningTask {
                    short: t.short.clone(),
                    run_id: t.run_id.clone(),
                    parallel: t.parallel,
                    shared_checkout: t.shared_checkout,
                    review: t.review,
                    own: t.own.clone(),
                });
                out.push(Choice::Bind {
                    task_id: t.task_id.clone(),
                    persona: t.persona.clone(),
                });
            }
        }
    }
    out
}

/// The first hold of `t` beside the `active` tasks, or `None`.
fn hold(t: &ReadyTask, active: &[RunningTask], holds: &Holds) -> Option<Hold> {
    if let Some(cause) = holds.global.first() {
        return Some(Hold::Global(cause.clone()));
    }
    if holds.unhealthy.contains(&t.persona) {
        return Some(Hold::UnhealthyProfile {
            profile: t.persona.clone(),
        });
    }
    if let Some(cause) = holds.personas.get(&t.persona) {
        return Some(Hold::Persona {
            persona: t.persona.clone(),
            cause: cause.clone(),
        });
    }
    if let Some(cause) = holds.projects.get(&t.project) {
        return Some(Hold::Project {
            project: t.project.clone(),
            cause: cause.clone(),
        });
    }
    if !claimable_at(t.not_before, holds.clock) {
        let seconds = t.not_before.unwrap_or_default() - holds.clock;
        return Some(Hold::NotBefore { seconds });
    }
    if let Some(attempt) = &t.open_attempt {
        return Some(Hold::OpenAttempt {
            attempt: attempt.clone(),
        });
    }
    if t.tree_orphaned {
        return Some(Hold::TreeRescue);
    }
    if t.review {
        return None;
    }
    let work: Vec<&RunningTask> = active
        .iter()
        .filter(|a| a.run_id == t.run_id && !a.review)
        .collect();
    exclusive(t, &work)
        .or_else(|| shared_checkout(t, &work))
        .or_else(|| path_held(t, &work))
}

/// An exclusive task runs, or `t` is exclusive and others run.
fn exclusive(t: &ReadyTask, work: &[&RunningTask]) -> Option<Hold> {
    let blocker = work
        .iter()
        .find(|a| !a.parallel)
        .or_else(|| (!t.parallel).then(|| work.first()).flatten());
    blocker.map(|a| Hold::Exclusive {
        running: a.short.clone(),
    })
}

/// 1 checkout holds the edits of each shared task: they go 1 by 1.
fn shared_checkout(t: &ReadyTask, work: &[&RunningTask]) -> Option<Hold> {
    if !t.shared_checkout {
        return None;
    }
    work.iter()
        .find(|a| a.shared_checkout)
        .map(|a| Hold::SharedCheckout {
            running: a.short.clone(),
        })
}

/// The first path of `t` that overlaps a path of a task of `work`.
fn path_held(t: &ReadyTask, work: &[&RunningTask]) -> Option<Hold> {
    work.iter().find_map(|a| {
        a.own.iter().find_map(|theirs| {
            t.own
                .iter()
                .find(|mine| overlap(mine, theirs))
                .map(|p| Hold::PathHeld {
                    path: p.clone(),
                    by: a.short.clone(),
                })
        })
    })
}

/// 2 paths overlap when they are equal or 1 is a folder of the other.
fn overlap(a: &str, b: &str) -> bool {
    let a = a.trim_end_matches('/');
    let b = b.trim_end_matches('/');
    a == b || b.starts_with(&format!("{a}/")) || a.starts_with(&format!("{b}/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready(seq: i64, own: &[&str]) -> ReadyTask {
        ReadyTask {
            task_id: format!("t{seq}"),
            short: format!("T-{seq}"),
            run_id: "r1".into(),
            project: "p1".into(),
            persona: "opus".into(),
            priority: 100,
            seq,
            target: false,
            not_before: None,
            parallel: true,
            shared_checkout: false,
            review: false,
            own: own.iter().map(|s| s.to_string()).collect(),
            open_attempt: None,
            tree_orphaned: false,
        }
    }

    fn bound(choices: &[Choice]) -> Vec<&str> {
        choices
            .iter()
            .filter_map(|c| match c {
                Choice::Bind { task_id, .. } => Some(task_id.as_str()),
                Choice::Held { .. } => None,
            })
            .collect()
    }

    #[test]
    fn order_is_target_then_priority_then_seq() {
        let mut retry = ready(3, &[]);
        retry.priority = -900;
        let mut target = ready(4, &[]);
        target.target = true;
        let c = choose(
            &[ready(1, &[]), ready(2, &[]), retry, target],
            &[],
            &Holds::default(),
        );
        assert_eq!(bound(&c), vec!["t4", "t3", "t1", "t2"]);
    }

    #[test]
    fn overlapping_paths_of_another_run_do_not_hold() {
        let mut other = ready(2, &["src/a"]);
        other.run_id = "r2".into();
        let c = choose(&[ready(1, &["src/a"]), other], &[], &Holds::default());
        assert_eq!(bound(&c), vec!["t1", "t2"]);
    }

    #[test]
    fn overlap_is_a_path_or_its_folder() {
        assert!(overlap("src/a", "src/a/b.rs"));
        assert!(overlap("src/a/", "src/a"));
        assert!(!overlap("src/a", "src/ab.rs"));
    }

    #[test]
    fn global_hold_wins_over_each_other_cause() {
        let holds = Holds {
            global: vec!["free disk 1 GB is below 10 GB".into()],
            ..Holds::default()
        };
        let mut t = ready(1, &[]);
        t.not_before = Some(50);
        assert_eq!(
            choose(&[t], &[], &holds),
            vec![Choice::Held {
                task_id: "t1".into(),
                hold: Hold::Global("free disk 1 GB is below 10 GB".into()),
            }]
        );
    }

    #[test]
    fn unhealthy_profile_holds_its_task_before_a_persona_hold() {
        let holds = Holds {
            unhealthy: ["opus".to_string()].into(),
            personas: [("opus".to_string(), "pacing".to_string())].into(),
            ..Holds::default()
        };
        let mut other = ready(2, &[]);
        other.persona = "sonnet".into();
        let c = choose(&[ready(1, &[]), other], &[], &holds);
        assert_eq!(bound(&c), vec!["t2"]);
        let hold = Hold::UnhealthyProfile {
            profile: "opus".into(),
        };
        assert_eq!(hold.to_string(), "unhealthy profile opus");
        assert!(c.contains(&Choice::Held {
            task_id: "t1".into(),
            hold,
        }));
    }
}
