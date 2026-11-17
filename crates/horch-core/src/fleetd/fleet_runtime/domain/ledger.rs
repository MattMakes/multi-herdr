//! The entity states and the rules of `transition()` (OR-2).
//!
//! `transition()` moves 1 row of 1 entity to a new state. [`EXITS`] is the
//! table of allowed moves. A move that is not in it is refused
//! (`docs/specs/fleetd/design.md`, "Ledger", rule 1 "One door").

/// The kinds of row that `transition()` moves in Phase 1
/// (`docs/tickets/fleetd-p1/MAP.md`, Q5). Runs and worktrees keep their
/// writes until Phases 4 and 3.
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
}

impl Entity {
    /// Every entity.
    pub const ALL: [Entity; 5] = [
        Entity::Task,
        Entity::Worker,
        Entity::Attempt,
        Entity::Job,
        Entity::NeedsYou,
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

/// The allowed moves: entity, from, to. The moves of task and worker are
/// the moves that the code of today makes; each comment names its site.
/// The moves of attempt, job and needs-you item are the exits of their
/// state tables in the design and in ticket 10.
pub const EXITS: &[(Entity, &str, &str)] = &[
    // taskledger/tasks.rs take_task: the claim.
    (Entity::Task, "queued", "running"),
    // taskledger/finish/mod.rs: the finish of a running task starts to land.
    (Entity::Task, "running", "landing"),
    // taskledger/finish/mod.rs mark_done: the finish ends the task.
    (Entity::Task, "running", "done"),
    (Entity::Task, "landing", "done"),
    // taskledger/tasks.rs requeue (the worker ended), tasks.rs handoff
    // (running only), finish/mod.rs mark_done (the task parks unlanded).
    (Entity::Task, "running", "queued"),
    (Entity::Task, "landing", "queued"),
    // taskledger/tasks.rs cancel: each state that is not done or cancelled.
    (Entity::Task, "queued", "cancelled"),
    (Entity::Task, "running", "cancelled"),
    (Entity::Task, "landing", "cancelled"),
    // taskledger/tasks.rs requeue_by_operator: `horch ledger requeue`.
    (Entity::Task, "cancelled", "queued"),
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

    #[test]
    fn allowed_transitions_of_task_equal_the_exits_of_today() {
        let today = vec![
            ("queued", "running"),
            ("queued", "cancelled"),
            ("running", "landing"),
            ("running", "done"),
            ("running", "queued"),
            ("running", "cancelled"),
            ("landing", "done"),
            ("landing", "queued"),
            ("landing", "cancelled"),
            ("cancelled", "queued"),
        ];
        assert_eq!(exits_of(Entity::Task), sorted(today));
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
                };
                assert!(known, "{entity} {word}");
            }
        }
    }

    #[test]
    fn a_move_is_allowed_refused_or_unchanged() {
        assert_eq!(decide(Entity::Task, "queued", "running"), Decision::Allowed);
        assert_eq!(decide(Entity::Task, "done", "running"), Decision::Refused);
        assert_eq!(
            decide(Entity::Worker, "queued", "running"),
            Decision::Refused
        );
        assert_eq!(
            decide(Entity::Task, "queued", "queued"),
            Decision::Unchanged
        );
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
