//! Spawn-time worker placement for pane hosts without geometry (`Caps {
//! geometry: false }`).
//!
//! Pure module (ARC-27): no process, file, or environment access.

use crate::workspace::model::Direction;

/// The workers currently sitting on one tab, ordered by terminal address
/// (`t7` -> `7`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabPanes {
    pub tab_id: String,
    pub workers: Vec<String>,
}

/// Where the next spawned worker pane should be created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placement {
    Split { from: String, direction: Direction },
    NewTab { label: String },
}

/// Creation-order sort key for terminal addresses (`"t7"` -> `(7, "t7")`).
pub fn address_key(address: &str) -> (u64, &str) {
    let n = address
        .strip_prefix('t')
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(u64::MAX);
    (n, address)
}

/// Sort worker terminal addresses in creation order.
pub fn sort_addresses(workers: &mut [String]) {
    workers.sort_by(|a, b| address_key(a).cmp(&address_key(b)));
}

/// Decide where a newly spawned worker belongs given `orchestrator` and the
/// workspace's `tabs` in tab-strip order.
pub fn place(orchestrator: &str, tabs: &[TabPanes]) -> Placement {
    if tabs.is_empty() {
        return Placement::Split {
            from: orchestrator.to_string(),
            direction: Direction::Right,
        };
    }
    for (idx, tab) in tabs.iter().enumerate() {
        let (cap, cols) = if idx == 0 { (4, 2) } else { (6, 3) };
        let mut workers = tab.workers.clone();
        sort_addresses(&mut workers);
        let n = workers.len();
        if idx > 0 && n == 0 {
            continue;
        }
        if n < cap {
            if idx == 0 && n == 0 {
                return Placement::Split {
                    from: orchestrator.to_string(),
                    direction: Direction::Right,
                };
            }
            if n < cols {
                return Placement::Split {
                    from: workers[n - 1].clone(),
                    direction: Direction::Right,
                };
            }
            return Placement::Split {
                from: workers[n - cols].clone(),
                direction: Direction::Down,
            };
        }
    }
    Placement::NewTab {
        label: format!("workers {}", tabs.len() + 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(id: &str, workers: &[&str]) -> TabPanes {
        let mut w: Vec<String> = workers.iter().map(|s| (*s).to_string()).collect();
        sort_addresses(&mut w);
        TabPanes {
            tab_id: id.to_string(),
            workers: w,
        }
    }

    #[test]
    fn hcb_09_first_worker_splits_the_orchestrator_right() {
        assert_eq!(
            place("t1", &[tab("tb1", &[])]),
            Placement::Split {
                from: "t1".into(),
                direction: Direction::Right,
            }
        );
    }

    #[test]
    fn hcb_09_first_tab_fills_two_columns_then_rows() {
        // Worker 2: splits right from w[0] (t2) to open column 2.
        assert_eq!(
            place("t1", &[tab("tb1", &["t2"])]),
            Placement::Split {
                from: "t2".into(),
                direction: Direction::Right,
            }
        );
        // Worker 3: splits down from w[0] (t2) to fill row 2 of column 1.
        // Input deliberately unsorted ("t10", "t2") to verify numeric address sort.
        assert_eq!(
            place("t1", &[tab("tb1", &["t10", "t2"])]),
            Placement::Split {
                from: "t2".into(),
                direction: Direction::Down,
            }
        );
        // Worker 4: splits down from w[1] (t10) to fill row 2 of column 2.
        assert_eq!(
            place("t1", &[tab("tb1", &["t10", "t2", "t11"])]),
            Placement::Split {
                from: "t10".into(),
                direction: Direction::Down,
            }
        );
    }

    #[test]
    fn hcb_09_fifth_worker_opens_a_tab_named_workers_2() {
        assert_eq!(
            place("t1", &[tab("tb1", &["t2", "t3", "t4", "t5"])]),
            Placement::NewTab {
                label: "workers 2".into(),
            }
        );
    }

    #[test]
    fn hcb_09_later_tab_holds_six_in_three_columns() {
        let full_tab1 = tab("tb1", &["t2", "t3", "t4", "t5"]);
        // Tab 2 starts with 1 worker (t6) from tab_new:
        // Worker 6 (n=1 < 3): right of t6 -> column 2.
        assert_eq!(
            place("t1", &[full_tab1.clone(), tab("tb2", &["t6"])]),
            Placement::Split {
                from: "t6".into(),
                direction: Direction::Right,
            }
        );
        // Worker 7 (n=2 < 3): right of t7 -> column 3.
        assert_eq!(
            place("t1", &[full_tab1.clone(), tab("tb2", &["t6", "t7"])]),
            Placement::Split {
                from: "t7".into(),
                direction: Direction::Right,
            }
        );
        // Worker 8 (n=3 >= 3): down of w[0] (t6) -> col 1 row 2.
        assert_eq!(
            place("t1", &[full_tab1.clone(), tab("tb2", &["t6", "t7", "t8"])]),
            Placement::Split {
                from: "t6".into(),
                direction: Direction::Down,
            }
        );
        // Worker 9 (n=4 >= 3): down of w[1] (t7) -> col 2 row 2.
        assert_eq!(
            place(
                "t1",
                &[full_tab1.clone(), tab("tb2", &["t6", "t7", "t8", "t9"])]
            ),
            Placement::Split {
                from: "t7".into(),
                direction: Direction::Down,
            }
        );
        // Worker 10 (n=5 >= 3): down of w[2] (t8) -> col 3 row 2.
        assert_eq!(
            place(
                "t1",
                &[
                    full_tab1.clone(),
                    tab("tb2", &["t6", "t7", "t8", "t9", "t10"])
                ]
            ),
            Placement::Split {
                from: "t8".into(),
                direction: Direction::Down,
            }
        );
        // Worker 11 (n=6 == cap): opens "workers 3".
        assert_eq!(
            place(
                "t1",
                &[
                    full_tab1,
                    tab("tb2", &["t6", "t7", "t8", "t9", "t10", "t11"])
                ]
            ),
            Placement::NewTab {
                label: "workers 3".into(),
            }
        );
    }
}
