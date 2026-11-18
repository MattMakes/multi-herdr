//! The orchestrator seat: `is_orchestrator_seat`, the 1 function that knows
//! a seat, from the ledger and `fleet.toml` only (Q2 of the map).
//!
//! Requirements: SR-8, FM-32. Ticket 09 of fleetd-p5 fills it.

/// The facts of 1 live attempt, as the ledger records them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttemptFacts {
    /// The id of the attempt.
    pub attempt: String,
    /// The task of the attempt. `None` when the ledger row has a null task.
    pub task: Option<String>,
    /// The persona of the attempt, as the ledger records it.
    pub persona: String,
}

/// Is `attempt` an orchestrator seat: its task is null and its persona is
/// in `seats`, the `orchestrator_seats` of `fleet.toml`.
///
/// A seat is exempt from the peer process check of the channel, so until
/// Phase 6 the orchestrator agent has operator rights (Q2 of
/// `docs/tickets/fleetd-p5/MAP.md`). This function is the only place of
/// that exemption. It reads no environment variable: a worker cannot make
/// itself a seat with `HORCH_ROLE` or with the name of a persona.
///
/// Phase 6 removes this exemption.
pub fn is_orchestrator_seat(attempt: &AttemptFacts, seats: &[String]) -> bool {
    attempt.task.is_none() && seats.contains(&attempt.persona)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seats() -> Vec<String> {
        vec!["orchestrator".to_string(), "orchestrator-codex".to_string()]
    }

    fn attempt(task: Option<&str>, persona: &str) -> AttemptFacts {
        AttemptFacts {
            attempt: "A-7".to_string(),
            task: task.map(str::to_string),
            persona: persona.to_string(),
        }
    }

    #[test]
    fn attempt_with_a_task_is_never_a_seat() {
        assert!(!is_orchestrator_seat(
            &attempt(Some("T-9"), "orchestrator"),
            &seats()
        ));
    }

    #[test]
    fn persona_not_in_the_seats_of_fleet_toml_is_not_a_seat() {
        assert!(!is_orchestrator_seat(
            &attempt(None, "opus-hardening"),
            &seats()
        ));
        assert!(!is_orchestrator_seat(&attempt(None, "orchestrator"), &[]));
        assert!(is_orchestrator_seat(
            &attempt(None, "orchestrator"),
            &seats()
        ));
    }

    #[test]
    fn worker_that_sets_horch_role_or_the_persona_name_in_its_environment_is_not_a_seat() {
        const VARS: [&str; 2] = ["HORCH_ROLE", "HORCH_TEAMMATE"];
        let before = VARS.map(std::env::var_os);
        // The variables hold the forged values only for the 2 calls.
        for name in VARS {
            std::env::set_var(name, "orchestrator");
        }
        let with_task = is_orchestrator_seat(&attempt(Some("T-9"), "opus-hardening"), &seats());
        let null_task = is_orchestrator_seat(&attempt(None, "opus-hardening"), &seats());
        for (name, value) in VARS.into_iter().zip(before) {
            match value {
                Some(v) => std::env::set_var(name, v),
                None => std::env::remove_var(name),
            }
        }
        assert!(!with_task);
        assert!(!null_task);
    }
}
