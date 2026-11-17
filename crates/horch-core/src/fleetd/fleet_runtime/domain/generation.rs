//! The generation of the `fleetd` row: who holds the fleetd lock.
//!
//! Each start of fleetd takes the fleetd lock and then adds 1 to the
//! generation of the `fleetd` row. Each write transaction of fleetd checks
//! that the generation of the row is still its own. So a fleetd of an older
//! generation writes nothing (`docs/specs/fleetd/design.md`, "One fleetd for
//! each ledger"; FM-12, RR-7).

use crate::fleetd::fleet_runtime::domain::ledger::Generation;

/// The generation that a start takes when the row holds `found`: 1 when no
/// fleetd ever started, else 1 more.
pub fn next(found: Option<Generation>) -> Generation {
    Generation(found.map_or(1, |g| g.0 + 1))
}

/// Whether the fleetd of generation `held` still owns the ledger, where the
/// row holds `found`.
pub fn is_current(held: Generation, found: Option<Generation>) -> bool {
    found == Some(held)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_start_takes_generation_1() {
        assert_eq!(next(None), Generation(1));
    }

    #[test]
    fn each_later_start_adds_1() {
        assert_eq!(next(Some(Generation(4))), Generation(5));
    }

    #[test]
    fn only_the_generation_of_the_row_is_current() {
        assert!(is_current(Generation(2), Some(Generation(2))));
        assert!(!is_current(Generation(1), Some(Generation(2))));
        assert!(!is_current(Generation(1), None));
    }
}
