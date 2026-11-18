//! The approval of an idea: approval.json and the hash of the assumptions
//! that you saw.
//!
//! Requirements: FR-10 (only your approval moves an idea past
//! `awaiting_approval`), FM-32 (fleetd ignores an approval.json that it did
//! not write), I7 (the approval part: an idea past `awaiting_approval` whose
//! approved file changed is held). An approval names the file that it
//! approves and its sha256: `assumptions.md`, the copied ticket of a
//! `--direct` idea, or the `MAP.md` of a run in flight
//! (`docs/tickets/fleetd-p6/MAP.md`, Q6 and Q9).

use super::idea_folder::ASSUMPTIONS_MD;

/// The file of an approval in the idea folder.
pub const APPROVAL_JSON: &str = "approval.json";

/// The `by` of each approval: only the operator approves.
pub const BY_OPERATOR: &str = "operator";

/// The `approved_sha256` of an idea that a release backfilled: its
/// approval.json names the file and its sha256 (MAP.md, Q6).
pub const RELEASE: &str = "release";

/// The state of an idea that waits for your approval.
pub const AWAITING_APPROVAL: &str = "awaiting_approval";

/// The state of an idea after an approval.
pub const APPROVED_TO: &str = "specifying";

/// The state of an idea after a revise.
pub const REVISED_TO: &str = "clarifying";

/// The states past `awaiting_approval` where an idea plans or runs work, so
/// I7 checks its approval.
pub const PAST_APPROVAL: [&str; 4] = ["specifying", "ticketing", "in_progress", "integrating"];

/// The kind of the needs-you item of an idea that I7 holds.
pub const I7_KIND: &str = "approval_changed";

/// approval.json: what you approved, and when.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Approval {
    pub idea: String,
    /// The approved file: a path in the idea folder, or an absolute path.
    pub file: String,
    /// The sha256 of `file` that you approved.
    pub sha256: String,
    /// RFC 3339.
    pub approved_at: String,
    pub by: String,
}

impl Approval {
    /// Your approval of the assumptions.md of `idea` with the hash `sha256`.
    pub fn of_assumptions(idea: &str, sha256: &str, approved_at: &str) -> Approval {
        Approval {
            idea: idea.to_string(),
            file: ASSUMPTIONS_MD.to_string(),
            sha256: sha256.to_string(),
            approved_at: approved_at.to_string(),
            by: BY_OPERATOR.to_string(),
        }
    }
}

/// Why fleetd refuses an approve or a revise. The text is what you read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The idea is not in `awaiting_approval`.
    NotAwaitingApproval { idea: String, state: String },
    /// assumptions.md is not the file that you saw.
    AssumptionsChanged {
        idea: String,
        seen: String,
        now: String,
    },
    /// The idea has no assumptions.md that fleetd can read.
    NoAssumptions { idea: String, reason: String },
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::NotAwaitingApproval { idea, state } => write!(
                f,
                "idea {idea} is {state}, not {AWAITING_APPROVAL}: nothing to approve or revise"
            ),
            Refusal::AssumptionsChanged { idea, seen, now } => write!(
                f,
                "the {ASSUMPTIONS_MD} of idea {idea} changed after you read it \
                 (you saw {seen}, it is now {now}): run horch idea show {idea} again"
            ),
            Refusal::NoAssumptions { idea, reason } => {
                write!(
                    f,
                    "idea {idea} has no {ASSUMPTIONS_MD} to approve: {reason}"
                )
            }
        }
    }
}

/// An idea in `state` may be revised.
pub fn check_revise(idea: &str, state: &str) -> Result<(), Refusal> {
    if state == AWAITING_APPROVAL {
        Ok(())
    } else {
        Err(Refusal::NotAwaitingApproval {
            idea: idea.to_string(),
            state: state.to_string(),
        })
    }
}

/// An idea in `state` may be approved for the hash `seen` when its
/// assumptions.md has the hash `now`.
pub fn check_approve(idea: &str, state: &str, seen: &str, now: &str) -> Result<(), Refusal> {
    check_revise(idea, state)?;
    if seen == now {
        Ok(())
    } else {
        Err(Refusal::AssumptionsChanged {
            idea: idea.to_string(),
            seen: seen.to_string(),
            now: now.to_string(),
        })
    }
}

/// The file that I7 compares and its approved sha256, from the
/// `approved_sha256` of the idea and its approval.json that fleetd wrote.
/// The marker [`RELEASE`] takes both from approval.json; another hash is
/// the approved hash of the file that approval.json names, else of
/// assumptions.md. `None` when nothing is approved.
pub fn approved_target<'a>(
    approved_sha256: Option<&'a str>,
    approval: Option<&'a Approval>,
) -> Option<(&'a str, &'a str)> {
    match approved_sha256? {
        RELEASE => approval.map(|a| (a.file.as_str(), a.sha256.as_str())),
        sha => Some((approval.map_or(ASSUMPTIONS_MD, |a| a.file.as_str()), sha)),
    }
}

/// I7, the approval part: an idea in `state` past `awaiting_approval`
/// holds only when the approved hash `approved` is the hash `current` of
/// the approved file now. An unknown hash does not hold.
pub fn i7_approval_holds(state: &str, approved: Option<&str>, current: Option<&str>) -> bool {
    if !PAST_APPROVAL.contains(&state) {
        return true;
    }
    matches!((approved, current), (Some(a), Some(c)) if a == c)
}

/// The key of the needs-you item of an idea that I7 holds.
pub fn i7_key(idea: &str) -> String {
    format!("{I7_KIND}:idea:{idea}")
}

/// The text of the needs-you item of an idea that I7 holds on `file`.
pub fn i7_text(idea: &str, file: &str) -> String {
    format!(
        "idea {idea} is held: {file} is not the file that you approved. \
         No plan commit and no new task of the idea starts until it matches again."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approval(file: &str, sha: &str) -> Approval {
        Approval {
            idea: "I-0001".into(),
            file: file.into(),
            sha256: sha.into(),
            approved_at: "2026-11-18T00:00:00Z".into(),
            by: BY_OPERATOR.into(),
        }
    }

    #[test]
    fn a_release_marker_compares_the_file_of_approval_json() {
        let a = approval("/repo/docs/tickets/x/MAP.md", "m1");
        assert_eq!(
            approved_target(Some(RELEASE), Some(&a)),
            Some(("/repo/docs/tickets/x/MAP.md", "m1"))
        );
        assert_eq!(approved_target(Some(RELEASE), None), None);
    }

    #[test]
    fn a_direct_approval_compares_its_ticket_with_approved_sha256() {
        let a = approval("tickets/01-x.md", "t1");
        assert_eq!(
            approved_target(Some("t1"), Some(&a)),
            Some(("tickets/01-x.md", "t1"))
        );
        assert_eq!(
            approved_target(Some("t1"), None),
            Some((ASSUMPTIONS_MD, "t1"))
        );
        assert_eq!(approved_target(None, Some(&a)), None);
    }

    #[test]
    fn i7_checks_only_the_states_past_approval() {
        assert!(i7_approval_holds("clarifying", None, None));
        assert!(i7_approval_holds("awaiting_approval", None, Some("x")));
        assert!(i7_approval_holds("ticketing", Some("x"), Some("x")));
        assert!(!i7_approval_holds("ticketing", Some("x"), Some("y")));
        assert!(!i7_approval_holds("in_progress", None, Some("y")));
        assert!(!i7_approval_holds("specifying", Some("x"), None));
    }
}
