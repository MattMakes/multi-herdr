pub const ABORT_EXIT_CODE: i32 = 86;

use std::collections::BTreeSet;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Faults(BTreeSet<String>);

impl Faults {
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

    pub fn has(&self, point: &str) -> bool {
        self.0.contains(point)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn points(&self) -> &BTreeSet<String> {
        &self.0
    }

    pub fn indexed(&self, prefix: &str) -> Option<String> {
        self.0.iter().find_map(|p| {
            p.strip_prefix(prefix)
                .and_then(|rest| rest.strip_prefix(':'))
                .map(str::to_owned)
        })
    }

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
        assert_eq!(many.points().len(), 2);

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
        f.abort_if("abort-after-execution-insert");
    }
}
