//! Greedy diversity over the eligible set (B3, CMP-06).
//!
//! A round learns most when its candidates differ. Each pick takes the
//! eligible entry that adds the most values not yet seen in any of the 3
//! dimensions: harness, model and effort. Ties go to the first entry by
//! teammate name, so the pick is stable.

use std::collections::BTreeSet;

use crate::routing::eligible::EligibleEntry;

/// The values a round already covers, per dimension.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Coverage {
    harnesses: BTreeSet<String>,
    models: BTreeSet<String>,
    efforts: BTreeSet<String>,
}

/// An entry's (harness, model, effort), with `-` for no model or effort.
fn key(e: &EligibleEntry) -> (String, String, String) {
    (
        e.harness.as_str().to_string(),
        e.model
            .as_ref()
            .map_or_else(|| "-".to_string(), |m| m.as_str().to_string()),
        e.effort.clone().unwrap_or_else(|| "-".to_string()),
    )
}

impl Coverage {
    /// Count one more candidate's values as covered.
    pub(crate) fn add(&mut self, harness: &str, model: &str, effort: Option<&str>) {
        self.harnesses.insert(harness.to_string());
        self.models.insert(model.to_string());
        self.efforts.insert(effort.unwrap_or("-").to_string());
    }

    pub(crate) fn add_entry(&mut self, e: &EligibleEntry) {
        let (h, m, f) = key(e);
        self.harnesses.insert(h);
        self.models.insert(m);
        self.efforts.insert(f);
    }

    /// How many new values `e` adds: 0 to 3.
    pub(crate) fn gain(&self, e: &EligibleEntry) -> u32 {
        let (h, m, f) = key(e);
        u32::from(!self.harnesses.contains(&h))
            + u32::from(!self.models.contains(&m))
            + u32::from(!self.efforts.contains(&f))
    }
}

/// Pick up to `count` entries from `pool`, greedily, each with the largest
/// [`Coverage::gain`] over what `covered` and the earlier picks hold.
/// Ineligible entries are skipped. `covered` ends with every pick added.
pub(crate) fn pick_diverse(
    pool: &[EligibleEntry],
    covered: &mut Coverage,
    count: usize,
) -> Vec<EligibleEntry> {
    let mut left: Vec<&EligibleEntry> = pool.iter().filter(|e| e.is_eligible()).collect();
    left.sort_by(|a, b| a.teammate.as_str().cmp(b.teammate.as_str()));
    let mut picks = Vec::new();
    while picks.len() < count && !left.is_empty() {
        let mut best = 0;
        for (i, e) in left.iter().enumerate() {
            // Strictly greater: the first by name wins a tie.
            if covered.gain(e) > covered.gain(left[best]) {
                best = i;
            }
        }
        let e = left.remove(best);
        covered.add_entry(e);
        picks.push(e.clone());
    }
    picks
}
