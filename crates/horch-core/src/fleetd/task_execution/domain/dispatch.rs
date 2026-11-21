use std::collections::{BTreeMap, BTreeSet};

use super::retry::claimable_at;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadyTask {
    pub task_id: String,
    pub short: String,
    pub run_id: String,
    pub project: String,
    pub persona: String,
    pub priority: i64,
    pub seq: i64,
    pub target: bool,
    pub not_before: Option<i64>,
    pub parallel: bool,
    pub shared_checkout: bool,
    pub review: bool,
    pub own: Vec<String>,
    pub open_attempt: Option<String>,
    pub tree_orphaned: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunningTask {
    pub short: String,
    pub run_id: String,
    pub parallel: bool,
    pub shared_checkout: bool,
    pub review: bool,
    pub own: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Holds {
    pub clock: i64,
    pub global: Vec<String>,
    pub personas: BTreeMap<String, String>,
    pub projects: BTreeMap<String, String>,
    pub unhealthy: BTreeSet<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hold {
    Global(String),
    UnhealthyProfile { profile: String },
    Persona { persona: String, cause: String },
    Project { project: String, cause: String },
    NotBefore { seconds: i64 },
    OpenAttempt { attempt: String },
    TreeRescue,
    IdeaParked { idea: String },
    Exclusive { running: String },
    SharedCheckout { running: String },
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Choice {
    Bind { task_id: String, persona: String },
    Held { task_id: String, hold: Hold },
}

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

fn exclusive(t: &ReadyTask, work: &[&RunningTask]) -> Option<Hold> {
    let blocker = work
        .iter()
        .find(|a| !a.parallel)
        .or_else(|| (!t.parallel).then(|| work.first()).flatten());
    blocker.map(|a| Hold::Exclusive {
        running: a.short.clone(),
    })
}

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
