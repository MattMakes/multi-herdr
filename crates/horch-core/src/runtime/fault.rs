//! Fault injection points for the end-to-end tests: `HORCH_FAULT`.
//!
//! The variable holds one point name or a comma-separated list, for example
//! `abort-after-append`. A point that is not armed costs nothing.
//!
//! An `abort-*` point ends the process where it stands, through
//! [`Faults::abort_if`]. A `fail-*` point makes its step return an error; the
//! step checks [`Faults::has`] itself.

/// The exit code of a process that [`Faults::abort_if`] stopped. A design
/// choice: no spec names one.
pub const ABORT_EXIT_CODE: i32 = 86;

use std::collections::BTreeSet;

/// The armed fault points.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Faults(BTreeSet<String>);

impl Faults {
    /// Parse a `HORCH_FAULT` value. Blank entries are ignored.
    pub fn parse(raw: Option<&str>) -> Faults {
        Faults(
            raw.unwrap_or_default()
                .split(',')
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(str::to_owned)
                .collect(),
        )
    }

    /// Is `point` armed? An exact match.
    pub fn has(&self, point: &str) -> bool {
        self.0.contains(point)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The index of an armed `<prefix>:<n>` point, for example `Some("2")`
    /// for `abort-after-worktree:2` and the prefix `abort-after-worktree`.
    pub fn indexed(&self, prefix: &str) -> Option<String> {
        self.0.iter().find_map(|p| {
            p.strip_prefix(prefix)
                .and_then(|rest| rest.strip_prefix(':'))
                .map(str::to_owned)
        })
    }

    /// Stop the process here, with [`ABORT_EXIT_CODE`] and a line on stderr,
    /// when `point` is armed. This simulates a crash: nothing is cleaned up.
    pub fn abort_if(&self, point: &str) {
        if self.has(point) {
            eprintln!("horch: HORCH_FAULT {point}: aborting");
            std::process::exit(ABORT_EXIT_CODE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faults_parse_one_or_many_points() {
        let one = Faults::parse(Some("after-append"));
        assert!(one.has("after-append"));
        assert!(!one.has("abort-after-append"));

        let many = Faults::parse(Some(" abort-after-append, ,x "));
        assert!(many.has("abort-after-append"));
        assert!(many.has("x"));
        assert!(!many.has(""));

        assert!(Faults::parse(None).is_empty());
        assert!(Faults::parse(Some("")).is_empty());
    }

    #[test]
    fn faults_indexed_points_carry_their_index() {
        let f = Faults::parse(Some("abort-after-worktree:2,abort-after-freeze:B"));
        assert_eq!(f.indexed("abort-after-worktree").as_deref(), Some("2"));
        assert_eq!(f.indexed("abort-after-freeze").as_deref(), Some("B"));
        assert_eq!(f.indexed("abort-after-worktre"), None);
        assert_eq!(f.indexed("abort-after-validation"), None);
        // An unarmed point never aborts.
        f.abort_if("abort-after-execution-insert");
    }
}
