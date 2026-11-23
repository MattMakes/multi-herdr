use super::idea_folder::ASSUMPTIONS_MD;

pub const APPROVAL_JSON: &str = "approval.json";

pub const BY_OPERATOR: &str = "operator";

pub const RELEASE: &str = "release";

pub const AWAITING_APPROVAL: &str = "awaiting_approval";

pub const APPROVED_TO: &str = "specifying";

pub const REVISED_TO: &str = "clarifying";

pub const PAST_APPROVAL: [&str; 4] = ["specifying", "ticketing", "in_progress", "integrating"];

pub const I7_KIND: &str = "approval_changed";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Approval {
    pub idea: String,
    pub file: String,
    pub sha256: String,
    pub approved_at: String,
    pub by: String,
}

impl Approval {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    NotAwaitingApproval {
        idea: String,
        state: String,
    },
    AssumptionsChanged {
        idea: String,
        seen: String,
        now: String,
    },
    NoAssumptions {
        idea: String,
        reason: String,
    },
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

pub fn approved_target<'a>(
    approved_sha256: Option<&'a str>,
    approval: Option<&'a Approval>,
) -> Option<(&'a str, &'a str)> {
    match approved_sha256? {
        RELEASE => approval.map(|a| (a.file.as_str(), a.sha256.as_str())),
        sha => Some((approval.map_or(ASSUMPTIONS_MD, |a| a.file.as_str()), sha)),
    }
}

pub fn i7_approval_holds(state: &str, approved: Option<&str>, current: Option<&str>) -> bool {
    if !PAST_APPROVAL.contains(&state) {
        return true;
    }
    matches!((approved, current), (Some(a), Some(c)) if a == c)
}

pub fn i7_key(idea: &str) -> String {
    format!("{I7_KIND}:idea:{idea}")
}

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
