#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Entity {
    Task,
    Worker,
    Attempt,
    Job,
    NeedsYou,
    Worktree,
    TaskBranch,
    Bundle,
    Feature,
    Idea,
}

impl Entity {
    pub const ALL: [Entity; 10] = [
        Entity::Task,
        Entity::Worker,
        Entity::Attempt,
        Entity::Job,
        Entity::NeedsYou,
        Entity::Worktree,
        Entity::TaskBranch,
        Entity::Bundle,
        Entity::Feature,
        Entity::Idea,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Entity::Task => "task",
            Entity::Worker => "worker",
            Entity::Attempt => "attempt",
            Entity::Job => "job",
            Entity::NeedsYou => "needs_you",
            Entity::Worktree => "worktree",
            Entity::TaskBranch => "task_branch",
            Entity::Bundle => "bundle",
            Entity::Feature => "feature",
            Entity::Idea => "idea",
        }
    }
}

impl std::fmt::Display for Entity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Entity::TaskBranch => f.write_str("task branch"),
            other => f.write_str(other.as_str()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Generation(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Unchanged,
    Allowed,
    Refused,
}

pub fn decide(entity: Entity, from: &str, to: &str) -> Decision {
    if from == to {
        Decision::Unchanged
    } else if EXITS.contains(&(entity, from, to)) {
        Decision::Allowed
    } else {
        Decision::Refused
    }
}

pub fn is_resume(entity: Entity, from: &str, to: &str) -> bool {
    entity == Entity::Idea && from == "parked" && to != "cancelled"
}

pub fn refused_cause(cause: &str) -> String {
    format!("illegal_transition: {cause}")
}

pub const REFUSED_KIND: &str = "illegal_transition";

pub fn refused_key(entity: Entity, id: &str) -> String {
    format!("{REFUSED_KIND}:{}:{id}", entity.as_str())
}

pub const TASK_EXITS_OF_TODAY: [(Entity, &str, &str); 3] = [
    (Entity::Task, "running", "done"),
    (Entity::Task, "running", "escalated"),
    (Entity::Task, "verifying", "escalated"),
];

pub const EXITS: &[(Entity, &str, &str)] = &[
    (Entity::Task, "pending", "ready"),
    (Entity::Task, "pending", "cancelled"),
    (Entity::Task, "ready", "leased"),
    (Entity::Task, "ready", "running"),
    (Entity::Task, "ready", "cancelled"),
    (Entity::Task, "leased", "running"),
    (Entity::Task, "leased", "ready"),
    (Entity::Task, "leased", "failed"),
    (Entity::Task, "leased", "cancelled"),
    (Entity::Task, "running", "verifying"),
    (Entity::Task, "running", "blocked_on_user"),
    (Entity::Task, "running", "ready"),
    (Entity::Task, "running", "waiting_quota"),
    (Entity::Task, "running", "stalled"),
    (Entity::Task, "running", "failed"),
    (Entity::Task, "running", "cancelled"),
    (Entity::Task, "stalled", "ready"),
    (Entity::Task, "stalled", "failed"),
    (Entity::Task, "stalled", "verifying"),
    (Entity::Task, "stalled", "cancelled"),
    (Entity::Task, "verifying", "done"),
    (Entity::Task, "verifying", "ready"),
    (Entity::Task, "verifying", "pending"),
    (Entity::Task, "verifying", "failed"),
    (Entity::Task, "verifying", "cancelled"),
    (Entity::Task, "blocked_on_user", "ready"),
    (Entity::Task, "blocked_on_user", "cancelled"),
    (Entity::Task, "waiting_quota", "ready"),
    (Entity::Task, "waiting_quota", "cancelled"),
    (Entity::Task, "failed", "ready"),
    (Entity::Task, "failed", "escalated"),
    (Entity::Task, "failed", "cancelled"),
    (Entity::Task, "escalated", "ready"),
    (Entity::Task, "escalated", "cancelled"),
    TASK_EXITS_OF_TODAY[0],
    TASK_EXITS_OF_TODAY[1],
    TASK_EXITS_OF_TODAY[2],
    (Entity::Worker, "spawning", "starting"),
    (Entity::Worker, "starting", "claiming"),
    (Entity::Worker, "spawning", "working"),
    (Entity::Worker, "starting", "working"),
    (Entity::Worker, "claiming", "working"),
    (Entity::Worker, "spawning", "done"),
    (Entity::Worker, "starting", "done"),
    (Entity::Worker, "claiming", "done"),
    (Entity::Worker, "working", "done"),
    (Entity::Worker, "spawning", "stopped"),
    (Entity::Worker, "starting", "stopped"),
    (Entity::Worker, "claiming", "stopped"),
    (Entity::Worker, "working", "stopped"),
    (Entity::Worker, "spawning", "incomplete"),
    (Entity::Worker, "starting", "incomplete"),
    (Entity::Worker, "claiming", "incomplete"),
    (Entity::Worker, "working", "incomplete"),
    (Entity::Worker, "spawning", "launch_failed"),
    (Entity::Worker, "starting", "launch_failed"),
    (Entity::Worker, "spawning", "no_task"),
    (Entity::Worker, "starting", "no_task"),
    (Entity::Worker, "claiming", "no_task"),
    (Entity::Attempt, "launching", "live"),
    (Entity::Attempt, "launching", "ended"),
    (Entity::Attempt, "live", "ended"),
    (Entity::Job, "queued", "running"),
    (Entity::Job, "running", "passed"),
    (Entity::Job, "running", "failed"),
    (Entity::Job, "running", "abandoned"),
    (Entity::NeedsYou, "open", "resolved"),
    (Entity::NeedsYou, "open", "superseded"),
    (Entity::NeedsYou, "resolved", "open"),
    (Entity::NeedsYou, "superseded", "open"),
    (Entity::Worktree, "reserved", "created"),
    (Entity::Worktree, "created", "active"),
    (Entity::Worktree, "created", "orphaned"),
    (Entity::Worktree, "active", "created"),
    (Entity::Worktree, "active", "landing"),
    (Entity::Worktree, "active", "orphaned"),
    (Entity::Worktree, "landing", "landed"),
    (Entity::Worktree, "landing", "rescued"),
    (Entity::Worktree, "landed", "removed"),
    (Entity::Worktree, "landed", "orphaned"),
    (Entity::Worktree, "orphaned", "rescued"),
    (Entity::Worktree, "orphaned", "removed"),
    (Entity::Worktree, "rescued", "active"),
    (Entity::Worktree, "rescued", "removed"),
    (Entity::TaskBranch, "building", "verifying"),
    (Entity::TaskBranch, "building", "abandoned"),
    (Entity::TaskBranch, "verifying", "ready"),
    (Entity::TaskBranch, "verifying", "building"),
    (Entity::TaskBranch, "verifying", "abandoned"),
    (Entity::TaskBranch, "ready", "in_bundle"),
    (Entity::TaskBranch, "ready", "abandoned"),
    (Entity::TaskBranch, "in_bundle", "landed"),
    (Entity::TaskBranch, "in_bundle", "ready"),
    (Entity::TaskBranch, "in_bundle", "gate_failed"),
    (Entity::TaskBranch, "in_bundle", "conflicted"),
    (Entity::TaskBranch, "in_bundle", "abandoned"),
    (Entity::TaskBranch, "conflicted", "ready"),
    (Entity::TaskBranch, "conflicted", "abandoned"),
    (Entity::TaskBranch, "gate_failed", "verifying"),
    (Entity::TaskBranch, "gate_failed", "abandoned"),
    (Entity::Bundle, "building", "gating"),
    (Entity::Bundle, "building", "dropped"),
    (Entity::Bundle, "gating", "passed"),
    (Entity::Bundle, "gating", "failed"),
    (Entity::Bundle, "gating", "dropped"),
    (Entity::Bundle, "passed", "landed"),
    (Entity::Bundle, "passed", "dropped"),
    (Entity::Feature, "open", "ready"),
    (Entity::Feature, "open", "merged"),
    (Entity::Feature, "open", "abandoned"),
    (Entity::Feature, "ready", "open"),
    (Entity::Feature, "ready", "merged"),
    (Entity::Feature, "ready", "abandoned"),
    (Entity::Idea, "captured", "clarifying"),
    (Entity::Idea, "captured", "ticketing"),
    (Entity::Idea, "captured", "parked"),
    (Entity::Idea, "captured", "cancelled"),
    (Entity::Idea, "clarifying", "awaiting_approval"),
    (Entity::Idea, "clarifying", "parked"),
    (Entity::Idea, "clarifying", "cancelled"),
    (Entity::Idea, "awaiting_approval", "specifying"),
    (Entity::Idea, "awaiting_approval", "clarifying"),
    (Entity::Idea, "awaiting_approval", "parked"),
    (Entity::Idea, "awaiting_approval", "cancelled"),
    (Entity::Idea, "specifying", "ticketing"),
    (Entity::Idea, "specifying", "awaiting_approval"),
    (Entity::Idea, "specifying", "parked"),
    (Entity::Idea, "specifying", "cancelled"),
    (Entity::Idea, "ticketing", "in_progress"),
    (Entity::Idea, "ticketing", "awaiting_approval"),
    (Entity::Idea, "ticketing", "parked"),
    (Entity::Idea, "ticketing", "cancelled"),
    (Entity::Idea, "in_progress", "integrating"),
    (Entity::Idea, "in_progress", "awaiting_approval"),
    (Entity::Idea, "in_progress", "parked"),
    (Entity::Idea, "in_progress", "cancelled"),
    (Entity::Idea, "integrating", "merged"),
    (Entity::Idea, "integrating", "in_progress"),
    (Entity::Idea, "integrating", "awaiting_approval"),
    (Entity::Idea, "integrating", "parked"),
    (Entity::Idea, "integrating", "cancelled"),
    (Entity::Idea, "parked", "captured"),
    (Entity::Idea, "parked", "clarifying"),
    (Entity::Idea, "parked", "awaiting_approval"),
    (Entity::Idea, "parked", "specifying"),
    (Entity::Idea, "parked", "ticketing"),
    (Entity::Idea, "parked", "in_progress"),
    (Entity::Idea, "parked", "integrating"),
    (Entity::Idea, "parked", "cancelled"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::taskledger::model::{TaskStatus, WorkerStatus};

    fn exits_of(entity: Entity) -> Vec<(&'static str, &'static str)> {
        let mut out: Vec<_> = EXITS
            .iter()
            .filter(|(e, _, _)| *e == entity)
            .map(|(_, from, to)| (*from, *to))
            .collect();
        out.sort_unstable();
        out
    }

    fn sorted(mut moves: Vec<(&'static str, &'static str)>) -> Vec<(&'static str, &'static str)> {
        moves.sort_unstable();
        moves
    }

    const TASK_STATES: [&str; 12] = [
        "pending",
        "ready",
        "leased",
        "running",
        "stalled",
        "verifying",
        "blocked_on_user",
        "waiting_quota",
        "done",
        "failed",
        "escalated",
        "cancelled",
    ];

    fn design_task_exits() -> Vec<(&'static str, &'static str)> {
        vec![
            ("pending", "ready"),
            ("pending", "cancelled"),
            ("ready", "leased"),
            ("ready", "running"),
            ("ready", "cancelled"),
            ("leased", "running"),
            ("leased", "ready"),
            ("leased", "failed"),
            ("leased", "cancelled"),
            ("running", "verifying"),
            ("running", "blocked_on_user"),
            ("running", "ready"),
            ("running", "waiting_quota"),
            ("running", "stalled"),
            ("running", "failed"),
            ("running", "cancelled"),
            ("stalled", "ready"),
            ("stalled", "failed"),
            ("stalled", "verifying"),
            ("stalled", "cancelled"),
            ("verifying", "done"),
            ("verifying", "ready"),
            ("verifying", "pending"),
            ("verifying", "failed"),
            ("verifying", "cancelled"),
            ("blocked_on_user", "ready"),
            ("blocked_on_user", "cancelled"),
            ("waiting_quota", "ready"),
            ("waiting_quota", "cancelled"),
            ("failed", "ready"),
            ("failed", "escalated"),
            ("failed", "cancelled"),
            ("escalated", "ready"),
            ("escalated", "cancelled"),
        ]
    }

    fn task_exits_of(from: &str) -> Vec<&'static str> {
        EXITS
            .iter()
            .filter(|(e, f, _)| *e == Entity::Task && *f == from)
            .map(|(_, _, to)| *to)
            .collect()
    }

    #[test]
    fn allowed_transitions_of_task_equal_the_12_state_table() {
        let mut expected = design_task_exits();
        expected.extend(TASK_EXITS_OF_TODAY.iter().map(|(_, f, t)| (*f, *t)));
        assert_eq!(exits_of(Entity::Task), sorted(expected));
    }

    #[test]
    fn task_exits_of_today_are_exactly_the_3_that_the_design_does_not_have() {
        let today: Vec<_> = TASK_EXITS_OF_TODAY
            .iter()
            .map(|(_, f, t)| (*f, *t))
            .collect();
        assert_eq!(
            sorted(today.clone()),
            sorted(vec![
                ("running", "done"),
                ("running", "escalated"),
                ("verifying", "escalated"),
            ])
        );
        for exit in today {
            assert!(
                !design_task_exits().contains(&exit),
                "{exit:?} is in the design"
            );
        }
    }

    #[test]
    fn each_state_that_is_not_terminal_has_an_exit_to_cancelled() {
        for state in TASK_STATES {
            if state == "done" || state == "cancelled" {
                continue;
            }
            assert_eq!(
                decide(Entity::Task, state, "cancelled"),
                Decision::Allowed,
                "{state} has no exit to cancelled"
            );
        }
    }

    #[test]
    fn done_and_cancelled_have_no_exit() {
        for terminal in ["done", "cancelled"] {
            assert_eq!(task_exits_of(terminal), Vec::<&str>::new(), "{terminal}");
            for to in TASK_STATES {
                assert_ne!(decide(Entity::Task, terminal, to), Decision::Allowed);
            }
        }
    }

    #[test]
    fn every_held_state_has_an_exit_that_frees_the_task() {
        for held in [
            "leased",
            "running",
            "stalled",
            "blocked_on_user",
            "waiting_quota",
        ] {
            let mut seen = vec![held];
            let mut next = vec![held];
            while let Some(state) = next.pop() {
                for to in task_exits_of(state) {
                    if !seen.contains(&to) {
                        seen.push(to);
                        next.push(to);
                    }
                }
            }
            assert!(
                seen.contains(&"ready") && seen.contains(&"cancelled"),
                "{held} reaches only {seen:?}"
            );
        }
    }

    #[test]
    fn the_retry_exits_of_rr6_exist() {
        for (from, to) in [
            ("running", "stalled"),
            ("stalled", "ready"),
            ("stalled", "failed"),
            ("failed", "escalated"),
            ("escalated", "ready"),
        ] {
            assert_eq!(
                decide(Entity::Task, from, to),
                Decision::Allowed,
                "{from} -> {to}"
            );
        }
    }

    #[test]
    fn allowed_transitions_of_worker_equal_the_exits_of_today() {
        let mut today = vec![
            ("spawning", "starting"),
            ("starting", "claiming"),
            ("spawning", "working"),
            ("starting", "working"),
            ("claiming", "working"),
            ("spawning", "launch_failed"),
            ("starting", "launch_failed"),
            ("spawning", "no_task"),
            ("starting", "no_task"),
            ("claiming", "no_task"),
        ];
        for live in ["spawning", "starting", "claiming", "working"] {
            for end in ["done", "stopped", "incomplete"] {
                today.push((live, end));
            }
        }
        assert_eq!(exits_of(Entity::Worker), sorted(today));
    }

    const WORKTREE_STATES: [&str; 8] = [
        "reserved", "created", "active", "landing", "landed", "removed", "orphaned", "rescued",
    ];

    #[test]
    fn allowed_transitions_of_worktree_equal_the_design_table() {
        let design = vec![
            ("reserved", "created"),
            ("created", "active"),
            ("created", "orphaned"),
            ("active", "created"),
            ("active", "landing"),
            ("active", "orphaned"),
            ("landing", "landed"),
            ("landing", "rescued"),
            ("landed", "removed"),
            ("landed", "orphaned"),
            ("orphaned", "rescued"),
            ("orphaned", "removed"),
            ("rescued", "active"),
            ("rescued", "removed"),
        ];
        assert_eq!(exits_of(Entity::Worktree), sorted(design));
        for to in WORKTREE_STATES {
            assert_ne!(
                decide(Entity::Worktree, "removed", to),
                Decision::Allowed,
                "removed -> {to}"
            );
        }
    }

    #[test]
    fn each_state_of_the_table_is_a_word_of_its_column() {
        for (entity, from, to) in EXITS {
            for word in [from, to] {
                let known = match entity {
                    Entity::Task => TaskStatus::parse(word).is_ok(),
                    Entity::Worker => WorkerStatus::parse(word).is_ok(),
                    Entity::Attempt => ["launching", "live", "ended"].contains(word),
                    Entity::Job => {
                        ["queued", "running", "passed", "failed", "abandoned"].contains(word)
                    }
                    Entity::NeedsYou => ["open", "resolved", "superseded"].contains(word),
                    Entity::Worktree => WORKTREE_STATES.contains(word),
                    Entity::TaskBranch => [
                        "building",
                        "verifying",
                        "ready",
                        "in_bundle",
                        "landed",
                        "conflicted",
                        "gate_failed",
                        "abandoned",
                    ]
                    .contains(word),
                    Entity::Bundle => [
                        "building", "gating", "passed", "landed", "failed", "dropped",
                    ]
                    .contains(word),
                    Entity::Feature => ["open", "ready", "merged", "abandoned"].contains(word),
                    Entity::Idea => [
                        "captured",
                        "clarifying",
                        "awaiting_approval",
                        "specifying",
                        "ticketing",
                        "in_progress",
                        "integrating",
                        "merged",
                        "parked",
                        "cancelled",
                    ]
                    .contains(word),
                };
                assert!(known, "{entity} {word}");
            }
        }
    }

    #[test]
    fn a_move_is_allowed_refused_or_unchanged() {
        assert_eq!(decide(Entity::Task, "ready", "running"), Decision::Allowed);
        assert_eq!(decide(Entity::Task, "done", "running"), Decision::Refused);
        assert_eq!(
            decide(Entity::Worker, "queued", "running"),
            Decision::Refused
        );
        assert_eq!(decide(Entity::Task, "ready", "ready"), Decision::Unchanged);
    }

    #[test]
    fn a_refused_move_names_its_entity_in_its_key_and_cause() {
        assert_eq!(
            refused_key(Entity::Task, "T-3"),
            "illegal_transition:task:T-3"
        );
        assert_eq!(
            refused_key(Entity::TaskBranch, "fleet/a"),
            "illegal_transition:task_branch:fleet/a"
        );
        assert_eq!(Entity::TaskBranch.to_string(), "task branch");
        assert_eq!(refused_cause("claim"), "illegal_transition: claim");
    }
}
