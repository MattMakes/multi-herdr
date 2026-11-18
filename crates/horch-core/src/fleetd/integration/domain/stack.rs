//! A stack: a conflicting trial merge puts the younger feature on the older
//! one, 1 time, with 1 resolver task.
//!
//! `docs/specs/fleetd/design.md`, "Between features", "Stack on evidence"
//! and "Starting values"; `ddd/08-code/integration/design.md`,
//! `StackPlanner`. The younger feature is the one with the later
//! `created_at`. A feature has at most 1 lower feature (`stacked_on`), so a
//! conflict of a feature that is on another feature already waits for the
//! end of that edge (ticket 38). A resolver task covers at most
//! `max_lines` changed lines and `max_files` files, else each file is 1
//! resolver task. The rules here have no I/O.

/// A feature of a conflicting trial merge, as the ledger has it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StackSide<'a> {
    pub id: &'a str,
    pub created_at: &'a str,
    pub stacked_on: Option<&'a str>,
}

/// What the planner decides for 1 conflict of 2 features.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Put `upper`, the younger feature, on `lower`, the older one, and
    /// open its resolver tasks.
    Stack { lower: String, upper: String },
    /// `upper` is on `lower` already: its resolver tasks exist.
    Stacked { lower: String, upper: String },
    /// `upper` is on the feature `on`, which is not `lower`. Its conflict
    /// with `lower` waits for the end of that edge.
    Held { upper: String, on: String },
}

/// The conflict of 1 file, in `zdiff3` form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub file: String,
    pub zdiff3: String,
}

/// The first marker of a conflict hunk.
const OPEN: &str = "<<<<<<<";
/// The last marker of a conflict hunk.
const CLOSE: &str = ">>>>>>>";
/// The markers between the sides of a `zdiff3` hunk.
const BETWEEN: [&str; 2] = ["|||||||", "======="];

impl Conflict {
    /// The changed lines of the conflict: each line of each side of each
    /// hunk, with no marker line.
    pub fn lines(&self) -> u64 {
        let mut inside = false;
        let mut count = 0;
        for line in self.zdiff3.lines() {
            if line.starts_with(OPEN) {
                inside = true;
            } else if line.starts_with(CLOSE) {
                inside = false;
            } else if inside && !BETWEEN.iter().any(|m| line.starts_with(m)) {
                count += 1;
            }
        }
        count
    }
}

/// The domain service `StackPlanner`, with the size limits of a resolver
/// task (`resolver_max_lines` and `resolver_max_files` of `fleet.toml`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StackPlanner {
    pub max_lines: u64,
    pub max_files: u64,
}

impl StackPlanner {
    /// The decision for a conflict of `a` and `b`. The younger is the one
    /// with the later `created_at`; on the same time, the larger id.
    pub fn decide(&self, a: &StackSide<'_>, b: &StackSide<'_>) -> Decision {
        let (lower, upper) = if (a.created_at, a.id) <= (b.created_at, b.id) {
            (a, b)
        } else {
            (b, a)
        };
        let (lower_id, upper_id) = (lower.id.to_string(), upper.id.to_string());
        match upper.stacked_on {
            None => Decision::Stack {
                lower: lower_id,
                upper: upper_id,
            },
            Some(on) if on == lower.id => Decision::Stacked {
                lower: lower_id,
                upper: upper_id,
            },
            Some(on) => Decision::Held {
                upper: upper_id,
                on: on.to_string(),
            },
        }
    }

    /// The resolver tasks of `conflicts`: 1 task with each file when the
    /// conflict is in the limits, else 1 task for each file.
    pub fn split(&self, conflicts: Vec<Conflict>) -> Vec<Vec<Conflict>> {
        let lines: u64 = conflicts.iter().map(Conflict::lines).sum();
        let files = conflicts.len() as u64;
        if lines <= self.max_lines && files <= self.max_files {
            vec![conflicts]
        } else {
            conflicts.into_iter().map(|c| vec![c]).collect()
        }
    }
}

/// 1 requirement of a feature: its idea, its id and its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Requirement {
    pub idea: String,
    pub id: String,
    pub text: String,
}

/// 1 feature of a stack edge, as its resolver task shows it.
#[derive(Debug, Clone, Copy)]
pub struct ResolverSide<'a> {
    pub feature: &'a str,
    pub branch: &'a str,
    pub tip: &'a str,
    pub requirements: &'a [Requirement],
    /// The unit test commands of its tasks.
    pub tests: &'a [String],
}

/// The title of the resolver task of `files`, from `upper` on `lower`.
pub fn resolver_title(lower: &str, upper: &str, files: &[Conflict]) -> String {
    let names: Vec<&str> = files.iter().map(|c| c.file.as_str()).collect();
    format!(
        "Resolve the conflict of {upper} with {lower}: {}",
        names.join(", ")
    )
}

/// The ticket of the resolver task of `conflicts`: `upper` is on `lower`.
/// It holds the conflict in `zdiff3` form, the requirements and the unit
/// tests of both features, and the rules of the task.
pub fn resolver_ticket(
    lower: &ResolverSide<'_>,
    upper: &ResolverSide<'_>,
    conflicts: &[Conflict],
) -> String {
    let mut out = format!(
        "# {}\n\n**Kind:** feature\n\n\
         The trial merge of {} at `{}` and {} at `{}` conflicts. \
         fleetd put the younger feature {} (branch `{}`) on the older feature {} (branch `{}`).\n\n\
         ## Rules\n\n\
         - Resolve the conflict of each file below, so that {} holds the work of {} \
         and each requirement below holds.\n\
         - Make 1 commit for each requirement.\n\
         - Run the unit tests of both features.\n\
         - This task lands on the branch of {}.\n",
        resolver_title(lower.feature, upper.feature, conflicts),
        lower.feature,
        lower.tip,
        upper.feature,
        upper.tip,
        upper.feature,
        upper.branch,
        lower.feature,
        lower.branch,
        upper.feature,
        lower.feature,
        upper.feature,
    );
    out.push_str("\n## Conflict (zdiff3)\n");
    for c in conflicts {
        let fence = fence(&c.zdiff3);
        out.push_str(&format!("\n### `{}`\n\n{fence}text\n{}", c.file, c.zdiff3));
        if !c.zdiff3.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&format!("{fence}\n"));
    }
    for side in [lower, upper] {
        out.push_str(&format!("\n## Requirements of {}\n\n", side.feature));
        if side.requirements.is_empty() {
            out.push_str("- None in the ledger.\n");
        }
        for r in side.requirements {
            out.push_str(&format!("- {} of idea {}: {}\n", r.id, r.idea, r.text));
        }
    }
    out.push_str("\n## Unit tests\n\n");
    for side in [lower, upper] {
        if side.tests.is_empty() {
            out.push_str(&format!("- {}: none in the ledger.\n", side.feature));
        }
        for t in side.tests {
            out.push_str(&format!("- {}: `{t}`\n", side.feature));
        }
    }
    out
}

/// A code fence that is longer than each run of backticks in `text`.
fn fence(text: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for ch in text.chars() {
        run = if ch == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    "`".repeat(longest.max(2) + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLANNER: StackPlanner = StackPlanner {
        max_lines: 100,
        max_files: 2,
    };

    fn side<'a>(id: &'a str, at: &'a str, on: Option<&'a str>) -> StackSide<'a> {
        StackSide {
            id,
            created_at: at,
            stacked_on: on,
        }
    }

    fn conflict(file: &str, lines: usize) -> Conflict {
        let ours: String = (0..lines).map(|i| format!("ours {i}\n")).collect();
        Conflict {
            file: file.into(),
            zdiff3: format!(
                "a\n<<<<<<< ours\n{ours}||||||| base\nb\n=======\nc\n>>>>>>> theirs\nd\n"
            ),
        }
    }

    #[test]
    fn the_younger_feature_goes_on_the_older_one_in_either_order() {
        let old = side("F-0002", "2026-11-01T00:00:00Z", None);
        let young = side("F-0001", "2026-11-02T00:00:00Z", None);
        let stack = Decision::Stack {
            lower: "F-0002".into(),
            upper: "F-0001".into(),
        };
        assert_eq!(PLANNER.decide(&old, &young), stack);
        assert_eq!(PLANNER.decide(&young, &old), stack);
    }

    #[test]
    fn the_same_time_orders_by_id() {
        let a = side("F-0001", "2026-11-01T00:00:00Z", None);
        let b = side("F-0002", "2026-11-01T00:00:00Z", None);
        assert!(
            matches!(PLANNER.decide(&b, &a), Decision::Stack { upper, .. } if upper == "F-0002")
        );
    }

    #[test]
    fn a_feature_is_stacked_1_time() {
        let old = side("F-1", "1", None);
        let young = side("F-2", "2", Some("F-1"));
        assert!(matches!(
            PLANNER.decide(&old, &young),
            Decision::Stacked { .. }
        ));
        let other = side("F-2", "2", Some("F-0"));
        assert_eq!(
            PLANNER.decide(&old, &other),
            Decision::Held {
                upper: "F-2".into(),
                on: "F-0".into()
            }
        );
    }

    #[test]
    fn changed_lines_are_the_lines_of_each_side_with_no_marker() {
        assert_eq!(conflict("a", 3).lines(), 5);
        let clean = Conflict {
            file: "a".into(),
            zdiff3: "a\nb\n".into(),
        };
        assert_eq!(clean.lines(), 0);
    }

    #[test]
    fn a_conflict_in_the_limits_is_1_task_else_1_task_for_each_file() {
        let files = |groups: Vec<Vec<Conflict>>| -> Vec<Vec<String>> {
            groups
                .into_iter()
                .map(|g| g.into_iter().map(|c| c.file).collect())
                .collect()
        };
        let small = vec![conflict("a", 1), conflict("b", 1)];
        assert_eq!(files(PLANNER.split(small)), [vec!["a", "b"]]);
        let three = vec![conflict("a", 1), conflict("b", 1), conflict("c", 1)];
        assert_eq!(files(PLANNER.split(three)), [["a"], ["b"], ["c"]]);
        let long = vec![conflict("a", 99), conflict("b", 1)];
        assert_eq!(files(PLANNER.split(long)), [["a"], ["b"]]);
        let one_long = vec![conflict("a", 500)];
        assert_eq!(files(PLANNER.split(one_long)), [["a"]]);
    }

    #[test]
    fn the_fence_is_longer_than_each_backtick_run() {
        assert_eq!(fence("a"), "```");
        assert_eq!(fence("a ```` b"), "`````");
    }
}
