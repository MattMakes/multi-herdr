//! The entity states and the rules of `transition()` (OR-2).
//!
//! `transition()` moves 1 row of 1 entity to a new state. [`EXITS`] is the
//! table of allowed moves. A move that is not in it is refused
//! (`docs/specs/fleetd/design.md`, "Ledger", rule 1 "One door").

/// The kinds of row that `transition()` moves: the entities of Phase 1
/// (`docs/tickets/fleetd-p1/MAP.md`, Q5) and the worktree of Phase 3
/// (`docs/tickets/fleetd-p3/MAP.md`, Q3). Runs keep their writes until
/// Phase 4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Entity {
    /// `tasks.status`.
    Task,
    /// `workers.status`.
    Worker,
    /// `attempts.state`.
    Attempt,
    /// `jobs.state`.
    Job,
    /// `needs_you.state`.
    NeedsYou,
    /// `worktrees.state`. `worktrees.status` keeps the words and the writes
    /// of today (`docs/tickets/fleetd-p3/MAP.md`, Q3).
    Worktree,
}

impl Entity {
    /// Every entity.
    pub const ALL: [Entity; 6] = [
        Entity::Task,
        Entity::Worker,
        Entity::Attempt,
        Entity::Job,
        Entity::NeedsYou,
        Entity::Worktree,
    ];

    /// The word of `events.entity`. Each event and each message that gives
    /// a state also gives this word.
    pub fn as_str(self) -> &'static str {
        match self {
            Entity::Task => "task",
            Entity::Worker => "worker",
            Entity::Attempt => "attempt",
            Entity::Job => "job",
            Entity::NeedsYou => "needs_you",
            Entity::Worktree => "worktree",
        }
    }
}

impl std::fmt::Display for Entity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The generation of the `fleetd` row: the number of the fleetd that holds
/// the lock. Ticket 14 gives its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Generation(pub i64);

/// What `transition()` does with a move from `from` to `to`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// The row is already in `to`: no write and no event.
    Unchanged,
    /// The table allows the move.
    Allowed,
    /// The table does not allow the move.
    Refused,
}

/// The decision for a move of a row of `entity` from `from` to `to`.
pub fn decide(entity: Entity, from: &str, to: &str) -> Decision {
    if from == to {
        Decision::Unchanged
    } else if EXITS.contains(&(entity, from, to)) {
        Decision::Allowed
    } else {
        Decision::Refused
    }
}

/// The cause of the event of a refused move.
pub fn refused_cause(cause: &str) -> String {
    format!("illegal_transition: {cause}")
}

/// The kind of the needs-you item of a refused move.
pub const REFUSED_KIND: &str = "illegal_transition";

/// The key of the needs-you item of a refused move of 1 row.
pub fn refused_key(entity: Entity, id: &str) -> String {
    format!("{REFUSED_KIND}:{entity}:{id}")
}

/// The task exits that the code of today writes and the 12-state table of
/// the design does not have. Each stays until its phase removes its writer.
pub const TASK_EXITS_OF_TODAY: [(Entity, &str, &str); 3] = [
    // taskledger/finish/mod.rs mark_done: `horch ledger done` of a running
    // task. Phase 4 moves `done` to the verifier, from `verifying` only.
    (Entity::Task, "running", "done"),
    // taskledger/tasks.rs requeue (the hold at `max_attempts`) and
    // finish/mod.rs mark_done (`done --unlanded`). Phase 2 rung 6
    // (ticket 10) goes through `failed`; Phase 4 moves the unlanded park
    // to the verifier.
    (Entity::Task, "running", "escalated"),
    (Entity::Task, "verifying", "escalated"),
];

/// The allowed moves: entity, from, to. The task moves are the 12-state
/// task table of the design ("The task state machine", the column "Leaves
/// when") and [`TASK_EXITS_OF_TODAY`]. The moves of worker are the moves
/// that the code of today makes; each comment names its site. The moves of
/// attempt, job and needs-you item are the exits of their state tables in
/// the design and in ticket 10. The moves of worktree are the column
/// "Leaves when" of the worktree table of the design, with the choices of
/// `docs/tickets/fleetd-p3/MAP.md`, Q10.
pub const EXITS: &[(Entity, &str, &str)] = &[
    // The task table of the design. `done` and `cancelled` are terminal;
    // each other state has an exit to `cancelled` (A cancel).
    (Entity::Task, "pending", "ready"),
    (Entity::Task, "pending", "cancelled"),
    // `ready -> running` is the claim of Phase 2 (MAP.md, Q5).
    (Entity::Task, "ready", "leased"),
    (Entity::Task, "ready", "running"),
    (Entity::Task, "ready", "cancelled"),
    // `ready` and `failed` are rung 6 after `timed_out` or `crashed`.
    (Entity::Task, "leased", "running"),
    (Entity::Task, "leased", "ready"),
    (Entity::Task, "leased", "failed"),
    (Entity::Task, "leased", "cancelled"),
    // `ready` is rung 6 and the restart on a fallback; `failed` is rung 6
    // with no attempt left.
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
    // `ready` is the outcome `rejected`, rung 6 and a fix round.
    (Entity::Task, "verifying", "done"),
    (Entity::Task, "verifying", "ready"),
    (Entity::Task, "verifying", "failed"),
    (Entity::Task, "verifying", "cancelled"),
    (Entity::Task, "blocked_on_user", "ready"),
    (Entity::Task, "blocked_on_user", "cancelled"),
    (Entity::Task, "waiting_quota", "ready"),
    (Entity::Task, "waiting_quota", "cancelled"),
    // `failed -> ready` is the 1 replan (MAP.md, Q12).
    (Entity::Task, "failed", "ready"),
    (Entity::Task, "failed", "escalated"),
    (Entity::Task, "failed", "cancelled"),
    // `horch ledger requeue` of an escalated task.
    (Entity::Task, "escalated", "ready"),
    (Entity::Task, "escalated", "cancelled"),
    // The task exits of today that the design does not have.
    TASK_EXITS_OF_TODAY[0],
    TASK_EXITS_OF_TODAY[1],
    TASK_EXITS_OF_TODAY[2],
    // taskledger/workers.rs supervisor_started: `horch worker` runs.
    (Entity::Worker, "spawning", "starting"),
    // taskledger/workers.rs agent_started: the agent child runs.
    (Entity::Worker, "starting", "claiming"),
    // taskledger/tasks.rs take_task: check_claimer takes each live state.
    // spawning also from orchestrator.rs (the orchestrator row and its
    // register) and cmd/taskledger.rs (the orchestrator row).
    (Entity::Worker, "spawning", "working"),
    (Entity::Worker, "starting", "working"),
    (Entity::Worker, "claiming", "working"),
    // taskledger/finish/mod.rs mark_done (working), runs.rs close (the
    // orchestrator), cmd/messaging.rs end_taskless_row (each live state).
    (Entity::Worker, "spawning", "done"),
    (Entity::Worker, "starting", "done"),
    (Entity::Worker, "claiming", "done"),
    (Entity::Worker, "working", "done"),
    // taskledger/stop.rs end_row (`stopped`) and tasks.rs cancel: each
    // live state.
    (Entity::Worker, "spawning", "stopped"),
    (Entity::Worker, "starting", "stopped"),
    (Entity::Worker, "claiming", "stopped"),
    (Entity::Worker, "working", "stopped"),
    // taskledger/stop.rs reap and workers.rs hand_off (each live state),
    // supervisor.rs exit_rule (working), orchestrator.rs
    // end_without_respawn.
    (Entity::Worker, "spawning", "incomplete"),
    (Entity::Worker, "starting", "incomplete"),
    (Entity::Worker, "claiming", "incomplete"),
    (Entity::Worker, "working", "incomplete"),
    // taskledger/workers.rs launch_failed and sweep_preclaim,
    // supervisor.rs exit_rule, orchestrator.rs end_without_respawn
    // (spawning).
    (Entity::Worker, "spawning", "launch_failed"),
    (Entity::Worker, "starting", "launch_failed"),
    // taskledger/tasks.rs no_task (check_claimer takes each live state;
    // a working worker gets its own task back), supervisor.rs exit_rule
    // and workers.rs sweep_preclaim (claiming).
    (Entity::Worker, "spawning", "no_task"),
    (Entity::Worker, "starting", "no_task"),
    (Entity::Worker, "claiming", "no_task"),
    // The launch is observed; the launch fails; the process ends.
    (Entity::Attempt, "launching", "live"),
    (Entity::Attempt, "launching", "ended"),
    (Entity::Attempt, "live", "ended"),
    // A job starts and ends. A new run is a new row.
    (Entity::Job, "queued", "running"),
    (Entity::Job, "running", "passed"),
    (Entity::Job, "running", "failed"),
    (Entity::Job, "running", "abandoned"),
    // fleetd/ideas: an item ends, and a new write of its key opens it again.
    (Entity::NeedsYou, "open", "resolved"),
    (Entity::NeedsYou, "open", "superseded"),
    (Entity::NeedsYou, "resolved", "open"),
    (Entity::NeedsYou, "superseded", "open"),
    // The worktree table of the design. A row starts in `reserved` (a tree
    // that fleetd makes) or `orphaned` (a folder that the reconciler
    // adopts). The drop of a `reserved` row is not a move: the door deletes
    // the row (MAP.md of fleetd-p3, Q18). `removed` is terminal.
    (Entity::Worktree, "reserved", "created"),
    (Entity::Worktree, "created", "active"),
    // `created` is `done --unlanded`; `orphaned` is a dead owner.
    (Entity::Worktree, "active", "created"),
    (Entity::Worktree, "active", "landing"),
    (Entity::Worktree, "active", "orphaned"),
    // `rescued` is the rescue of an abandoned branch (Phase 4).
    (Entity::Worktree, "landing", "landed"),
    (Entity::Worktree, "landing", "rescued"),
    (Entity::Worktree, "landed", "removed"),
    (Entity::Worktree, "orphaned", "rescued"),
    // `active` is the next claim.
    (Entity::Worktree, "rescued", "active"),
    (Entity::Worktree, "rescued", "removed"),
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

    /// The 12 states of the design table.
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

    /// The column "Leaves when" of the 12-state task table of the design.
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
        // RR-2: no path leaves a task held with no exit.
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

    /// The 8 states of the worktree table of the design, as the CHECK of
    /// `worktrees.state` lists them.
    const WORKTREE_STATES: [&str; 8] = [
        "reserved", "created", "active", "landing", "landed", "removed", "orphaned", "rescued",
    ];

    #[test]
    fn allowed_transitions_of_worktree_equal_the_design_table() {
        // The column "Leaves when", with the choices of MAP.md of
        // fleetd-p3, Q10. The drop of a `reserved` row is not a move.
        let design = vec![
            ("reserved", "created"),
            ("created", "active"),
            ("active", "created"),
            ("active", "landing"),
            ("active", "orphaned"),
            ("landing", "landed"),
            ("landing", "rescued"),
            ("landed", "removed"),
            ("orphaned", "rescued"),
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
        assert_eq!(refused_cause("claim"), "illegal_transition: claim");
    }
}
