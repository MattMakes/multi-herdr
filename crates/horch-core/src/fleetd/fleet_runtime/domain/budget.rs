//! The work budget of a pass, the fact scan budgets and the time budget.
//!
//! `docs/specs/fleetd/design.md`, "Supervision, liveness and
//! reconciliation": a pass handles at most `pass_budget` rows of each kind,
//! each fact scan stops at `scan_budget_ms`, and the whole pass stops at
//! `pass_time_budget_s`. A [`Cursor`] for each kind (the table
//! `pass_cursors`) says where the next pass continues, so each item gets its
//! turn in a fair order, also after a restart.

use std::time::{Duration, Instant};

/// The count of rows of each kind that 1 pass handles (`pass_budget`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PassBudget(usize);

impl PassBudget {
    /// The budget of `pass_budget` rows of each kind.
    pub fn new(pass_budget: u64) -> Self {
        Self(usize::try_from(pass_budget).unwrap_or(usize::MAX))
    }

    /// The count of rows of each kind.
    pub fn rows(self) -> usize {
        self.0
    }
}

/// The time at which a pass, or a scan, stops. fleetd looks at it between
/// 2 items.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Deadline(Instant);

impl Deadline {
    /// The deadline of a pass that started at `start`
    /// (`pass_time_budget_s`).
    pub fn pass(start: Instant, pass_time_budget_s: u64) -> Self {
        Self(start + Duration::from_secs(pass_time_budget_s))
    }

    /// The deadline of a scan that starts at `start` in this pass
    /// (`scan_budget_ms`). A scan never runs past the deadline of its pass.
    pub fn scan(self, start: Instant, scan_budget_ms: u64) -> Self {
        Self(self.0.min(start + Duration::from_millis(scan_budget_ms)))
    }

    /// Whether the deadline is reached at `now`.
    pub fn reached(self, now: Instant) -> bool {
        now >= self.0
    }
}

/// The 4 fact scans of part 1 of a pass, each with its row in
/// `pass_cursors` (`docs/tickets/fleetd-p1/MAP.md`, Q4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanKind {
    /// The PIDs that rows of `workers`, `attempts` and `jobs` name.
    Processes,
    /// The outbox folder of each attempt that is not `ended`.
    Outboxes,
    /// The folder of each worktree that is not cleaned.
    WorktreeFolders,
    /// The folder `<fleet root>/run/requests/`.
    Requests,
}

impl ScanKind {
    /// Each scan, in the order of part 1.
    pub const ALL: [ScanKind; 4] = [
        ScanKind::Processes,
        ScanKind::Outboxes,
        ScanKind::WorktreeFolders,
        ScanKind::Requests,
    ];

    /// The `kind` of its row in `pass_cursors`.
    pub fn as_str(self) -> &'static str {
        match self {
            ScanKind::Processes => "processes",
            ScanKind::Outboxes => "outboxes",
            ScanKind::WorktreeFolders => "worktree_folders",
            ScanKind::Requests => "requests",
        }
    }
}

/// The rows of `pass_cursors` that the idea duties of Phase 6 own in the
/// pass (ticket 51 of fleetd-p6). Each takes at most `pass_budget` rows in
/// 1 pass. The dispatcher, the question flow, the sub-orchestrators and the
/// waits keep their cursors in their use cases.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdeaCursor {
    /// The folders of `<fleet root>/ideas/`: the notes of each idea. The
    /// scan has the time budget of the fact scans.
    IdeaFolders,
    /// The `requests` rows whose effects the pass wrote, by row number.
    RequestEffects,
    /// The tasks that wait for their replan.
    Replans,
    /// The files of `<fleet root>/needs-you/`.
    NeedsYou,
}

impl IdeaCursor {
    /// The `kind` of the row in `pass_cursors`.
    pub fn as_str(self) -> &'static str {
        match self {
            IdeaCursor::IdeaFolders => "idea_folders",
            IdeaCursor::RequestEffects => "idea_effects",
            IdeaCursor::Replans => "replans",
            IdeaCursor::NeedsYou => "needs_you",
        }
    }
}

/// The 3 row kinds of integration that part 2 of a pass walks within
/// `pass_budget`, each with its row in `pass_cursors`
/// (`docs/tickets/fleetd-p4/47-pass-wiring-integration.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    /// The task branches that are not terminal: each follows its task.
    Branches,
    /// The bundles that are not ended, and the ended bundles whose
    /// candidate ref is not verified as deleted.
    Bundles,
    /// The `open` and `ready` features.
    Features,
}

impl RowKind {
    /// The `kind` of its row in `pass_cursors`.
    pub const fn as_str(self) -> &'static str {
        match self {
            RowKind::Branches => "branches",
            RowKind::Bundles => "bundles",
            RowKind::Features => "features",
        }
    }
}

/// The cursor key of the row id `id`: a fixed width, so that the order of
/// the keys is the order of the ids.
pub fn id_key(id: i64) -> String {
    format!("{id:020}")
}

/// 1 row of `pass_cursors`: where the next pass continues for 1 kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cursor {
    /// The last key handled. The next pass starts after it, and wraps at
    /// the end. `None` before the first item.
    pub after_key: Option<String>,
    /// When the cursor last wrapped (or was made).
    pub lap_started_at: String,
    /// The lap before: each item that waits was handled after this time.
    pub prev_lap_started_at: Option<String>,
}

impl Cursor {
    /// A cursor before its first item, made at `now`.
    pub fn new(now: &str) -> Self {
        Self {
            after_key: None,
            lap_started_at: now.to_string(),
            prev_lap_started_at: None,
        }
    }

    /// 1 lap of `items` in turn order: the items with a key after
    /// `after_key`, then the others, each in order of its key. A pass walks
    /// at most this lap, so it handles each item at most once.
    pub fn turns<'a, T>(&self, items: &'a [T], key: impl Fn(&T) -> &str) -> Vec<&'a T> {
        let mut sorted: Vec<&T> = items.iter().collect();
        sorted.sort_by(|a, b| key(a).cmp(key(b)));
        let split = match &self.after_key {
            Some(after) => sorted.partition_point(|t| key(t) <= after.as_str()),
            None => 0,
        };
        sorted.rotate_left(split);
        sorted
    }

    /// Record that the item `key` was handled at `now`, in turn order. A
    /// key at or before `after_key` means the walk wrapped: a new lap
    /// starts at `now`.
    pub fn step(&mut self, key: &str, now: &str) {
        if self.after_key.as_deref().is_some_and(|after| key <= after) {
            self.prev_lap_started_at =
                Some(std::mem::replace(&mut self.lap_started_at, now.to_string()));
        }
        self.after_key = Some(key.to_string());
    }

    /// The rows of part 2 that this pass handles: at most `budget` of
    /// `keys`, in turn order. The cursor steps past each of them; the rest
    /// wait for the next pass.
    pub fn take<'a>(&mut self, keys: &'a [String], budget: PassBudget, now: &str) -> Vec<&'a str> {
        let chosen: Vec<&'a str> = self
            .turns(keys, |k| k.as_str())
            .into_iter()
            .take(budget.rows())
            .map(String::as_str)
            .collect();
        for key in &chosen {
            self.step(key, now);
        }
        chosen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("k{i:04}")).collect()
    }

    #[test]
    fn id_keys_sort_as_the_ids() {
        let keys: Vec<String> = [2, 10, 9].into_iter().map(id_key).collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(sorted, [id_key(2), id_key(9), id_key(10)]);
        assert_eq!(RowKind::Branches.as_str(), "branches");
    }

    #[test]
    fn the_cursor_wraps_and_each_item_gets_its_turn() {
        let all = keys(5);
        let mut cursor = Cursor::new("t0");
        let mut seen = Vec::new();
        for (pass, now) in ["t1", "t2", "t3"].iter().enumerate() {
            let taken = cursor.take(&all, PassBudget::new(2), now);
            assert_eq!(taken.len(), 2, "pass {pass}");
            seen.extend(taken.iter().map(|k| k.to_string()));
        }
        // 3 passes of 2: k0 k1 | k2 k3 | k4, then the wrap to k0.
        assert_eq!(seen, ["k0000", "k0001", "k0002", "k0003", "k0004", "k0000"]);
        assert_eq!(cursor.after_key.as_deref(), Some("k0000"));
        assert_eq!(cursor.lap_started_at, "t3");
        assert_eq!(cursor.prev_lap_started_at.as_deref(), Some("t0"));
    }

    #[test]
    fn rows_past_the_pass_budget_wait_for_the_next_pass() {
        let all = keys(450);
        let mut cursor = Cursor::new("t0");
        let first = cursor.take(&all, PassBudget::new(200), "t1");
        assert_eq!(
            first,
            all[..200].iter().map(String::as_str).collect::<Vec<_>>()
        );
        let second = cursor.take(&all, PassBudget::new(200), "t2");
        assert_eq!(
            second,
            all[200..400].iter().map(String::as_str).collect::<Vec<_>>()
        );
        let third = cursor.take(&all, PassBudget::new(200), "t3");
        let want: Vec<&str> = all[400..]
            .iter()
            .chain(&all[..150])
            .map(String::as_str)
            .collect();
        assert_eq!(third, want);
        assert_eq!(cursor.lap_started_at, "t3");
    }

    #[test]
    fn a_pass_takes_each_row_at_most_once() {
        let all = keys(3);
        let mut cursor = Cursor::new("t0");
        cursor.step("k0001", "t0");
        let taken = cursor.take(&all, PassBudget::new(10), "t1");
        assert_eq!(taken, ["k0002", "k0000", "k0001"]);
    }

    #[test]
    fn turns_start_after_a_key_that_is_gone() {
        let all = vec!["a".to_string(), "c".to_string(), "e".to_string()];
        let mut cursor = Cursor::new("t0");
        cursor.step("b", "t0");
        let order: Vec<&String> = cursor.turns(&all, |k| k.as_str());
        assert_eq!(order, ["c", "e", "a"]);
    }

    #[test]
    fn a_lone_item_starts_a_new_lap_on_each_pass() {
        let all = keys(1);
        let mut cursor = Cursor::new("t0");
        cursor.take(&all, PassBudget::new(5), "t1");
        assert_eq!(cursor.lap_started_at, "t0");
        cursor.take(&all, PassBudget::new(5), "t2");
        assert_eq!(cursor.lap_started_at, "t2");
        assert_eq!(cursor.prev_lap_started_at.as_deref(), Some("t0"));
    }

    #[test]
    fn a_scan_deadline_never_passes_the_pass_deadline() {
        let start = Instant::now();
        let pass = Deadline::pass(start, 1);
        let late = start + Duration::from_millis(900);
        assert_eq!(pass.scan(late, 2000), pass);
        let scan = pass.scan(start, 100);
        assert!(!scan.reached(start + Duration::from_millis(99)));
        assert!(scan.reached(start + Duration::from_millis(100)));
    }

    #[test]
    fn scan_kinds_have_the_names_of_their_cursor_rows() {
        let names: Vec<&str> = ScanKind::ALL.iter().map(|k| k.as_str()).collect();
        assert_eq!(
            names,
            ["processes", "outboxes", "worktree_folders", "requests"]
        );
    }
}
