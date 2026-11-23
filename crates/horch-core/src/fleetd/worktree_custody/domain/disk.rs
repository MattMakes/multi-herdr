pub const GB: u64 = 1 << 30;

pub const BUDGET_KEY: &str = "worktree_disk_budget_gb";

pub const NEEDS_YOU_KEY: &str = "worktree_disk";
pub const NEEDS_YOU_KIND: &str = "worktree_disk";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tree<'a> {
    pub id: &'a str,
    pub state: &'a str,
    pub has_task: bool,
    pub idea_state: Option<&'a str>,
    pub created_at: &'a str,
    pub bytes: Option<u64>,
    pub removal_failed: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Decision {
    pub total: u64,
    pub budget: u64,
    pub remove: Vec<String>,
    pub hold: bool,
}

pub fn spare(tree: &Tree<'_>) -> bool {
    if tree.removal_failed {
        return false;
    }
    let finished = matches!(tree.idea_state, Some("merged" | "cancelled"));
    let state = match tree.state {
        "landed" => true,
        "rescued" => tree.has_task,
        _ => false,
    };
    finished && state
}

pub fn decide(trees: &[Tree<'_>], budget_gb: u64) -> Decision {
    let budget = budget_gb.saturating_mul(GB);
    let total = trees
        .iter()
        .filter_map(|t| t.bytes)
        .fold(0u64, u64::saturating_add);
    let mut decision = Decision {
        total,
        budget,
        ..Decision::default()
    };
    if total <= budget {
        return decision;
    }
    let mut spares: Vec<&Tree<'_>> = trees
        .iter()
        .filter(|t| spare(t) && t.bytes.is_some_and(|b| b > 0))
        .collect();
    spares.sort_by_key(|t| (t.created_at, t.id));
    let mut left = total;
    for tree in spares {
        if left <= budget {
            break;
        }
        left = left.saturating_sub(tree.bytes.unwrap_or(0));
        decision.remove.push(tree.id.to_string());
    }
    decision.hold = left > budget;
    decision
}

pub fn hold_cause(d: &Decision) -> String {
    format!(
        "the worktrees use {:.1} GB, above the budget of {} GB, and no landed or rescued tree \
         of a merged or cancelled idea is left to remove ({BUDGET_KEY})",
        d.total as f64 / GB as f64,
        d.budget / GB
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree<'a>(id: &'a str, state: &'a str, idea: &'a str, at: &'a str, gb: u64) -> Tree<'a> {
        Tree {
            id,
            state,
            has_task: true,
            idea_state: Some(idea),
            created_at: at,
            bytes: Some(gb * GB),
            removal_failed: false,
        }
    }

    #[test]
    fn under_or_at_the_budget_removes_nothing_and_holds_nothing() {
        let trees = [tree("a", "landed", "merged", "2026-01-01T00:00:00Z", 20)];
        let d = decide(&trees, 20);
        assert_eq!(d.total, 20 * GB);
        assert!(d.remove.is_empty(), "{d:?}");
        assert!(!d.hold);
    }

    #[test]
    fn over_budget_removes_landed_and_rescued_trees_of_finished_runs_oldest_first() {
        let trees = [
            tree("new", "landed", "merged", "2026-01-03T00:00:00Z", 5),
            tree("live", "active", "in_progress", "2026-01-01T00:00:00Z", 20),
            tree("old", "rescued", "cancelled", "2026-01-01T00:00:00Z", 5),
            tree("mid", "landed", "cancelled", "2026-01-02T00:00:00Z", 5),
        ];
        let d = decide(&trees, 20);
        assert_eq!(d.remove, ["old", "mid", "new"]);
        assert!(!d.hold, "{d:?}");
        assert_eq!(decide(&trees, 30).remove, ["old"]);
    }

    #[test]
    fn over_budget_never_removes_an_active_tree_and_holds_dispatch() {
        let trees = [
            tree("live", "active", "merged", "2026-01-01T00:00:00Z", 30),
            tree("spare", "landed", "merged", "2026-01-02T00:00:00Z", 5),
        ];
        let d = decide(&trees, 20);
        assert_eq!(d.remove, ["spare"]);
        assert!(d.hold, "{d:?}");
        assert_eq!(
            hold_cause(&d),
            "the worktrees use 35.0 GB, above the budget of 20 GB, and no landed or rescued \
             tree of a merged or cancelled idea is left to remove (worktree_disk_budget_gb)"
        );
    }

    #[test]
    fn over_budget_never_removes_a_tree_of_an_active_run() {
        let trees = [
            tree("a", "landed", "in_progress", "2026-01-01T00:00:00Z", 30),
            tree("b", "rescued", "in_progress", "2026-01-02T00:00:00Z", 30),
        ];
        let d = decide(&trees, 20);
        assert!(d.remove.is_empty(), "{d:?}");
        assert!(d.hold);
    }

    #[test]
    fn only_landed_and_rescued_trees_with_a_task_and_a_finished_run_are_spare() {
        let at = "2026-01-01T00:00:00Z";
        for state in ["reserved", "created", "active", "landing", "orphaned"] {
            assert!(!spare(&tree("t", state, "merged", at, 1)), "{state}");
        }
        assert!(spare(&tree("t", "landed", "merged", at, 1)));
        assert!(spare(&tree("t", "rescued", "cancelled", at, 1)));
        let no_task = Tree {
            has_task: false,
            ..tree("t", "rescued", "merged", at, 1)
        };
        assert!(!spare(&no_task), "a rescued tree with no task stays");
        assert!(spare(&Tree {
            has_task: false,
            ..tree("t", "landed", "merged", at, 1)
        }));
        let no_idea = Tree {
            idea_state: None,
            ..tree("t", "landed", "merged", at, 1)
        };
        assert!(!spare(&no_idea), "a tree of no idea stays");
        let failed = Tree {
            removal_failed: true,
            ..tree("t", "landed", "merged", at, 1)
        };
        assert!(!spare(&failed), "a tree whose removal failed stays");
    }

    #[test]
    fn an_unmeasured_tree_counts_0_and_is_not_removed() {
        let unmeasured = Tree {
            bytes: None,
            ..tree("u", "landed", "merged", "2026-01-01T00:00:00Z", 0)
        };
        let trees = [
            unmeasured,
            tree("live", "active", "in_progress", "2026-01-02T00:00:00Z", 25),
        ];
        let d = decide(&trees, 20);
        assert_eq!(d.total, 25 * GB);
        assert!(d.remove.is_empty(), "{d:?}");
        assert!(d.hold);
    }
}
