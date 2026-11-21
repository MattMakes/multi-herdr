use crate::fleetd::fleet_runtime::domain::ledger::Generation;

pub fn next(found: Option<Generation>) -> Generation {
    Generation(found.map_or(1, |g| g.0 + 1))
}

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
