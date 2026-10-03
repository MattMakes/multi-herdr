//! Fault injection points for the end-to-end tests: `HORCH_FAULT`.
//!
//! The variable holds one point name or a comma-separated list, for example
//! `abort-after-append`. A point that is not armed costs nothing.

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
}
