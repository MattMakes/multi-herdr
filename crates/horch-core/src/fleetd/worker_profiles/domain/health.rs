//! The health of a worker profile, read from its last `profile_unhealthy` or
//! `profile_healthy` event (fleetd-p7 MAP.md Q10). No event means healthy.
//!
//! `docs/specs/fleetd/design.md`, "Worker profiles and adapters" (The
//! health of a profile), rung 4 of the stall ladder, and step 4 of "Docs
//! sync and adapter conformance": a failed conformance run or a dialog
//! marks a profile unhealthy, a passed run marks it healthy again, and a
//! mark that does not change the health writes no event. Facts in,
//! choices out. No I/O.

use super::conformance::ConformanceReport;

/// The `entity` word of a health event: the event is about a profile, which
/// is not a row of the ledger.
pub const PROFILE: &str = "profile";

/// The health of 1 profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Health {
    Healthy,
    /// The cause of its last `profile_unhealthy` event.
    Unhealthy {
        reason: String,
    },
}

/// The contract `profile-marked-unhealthy` (Slice 7). Its fields are the
/// `data_json` of the `profile_unhealthy` event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileMarkedUnhealthy {
    pub profile_id: String,
    pub reason: String,
}

impl ProfileMarkedUnhealthy {
    pub const NAME: &'static str = "profile-marked-unhealthy";
}

/// The contract `profile-marked-healthy` (Slice 7). Its field is the
/// `data_json` of the `profile_healthy` event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileMarkedHealthy {
    pub profile_id: String,
}

impl ProfileMarkedHealthy {
    pub const NAME: &'static str = "profile-marked-healthy";
}

/// 1 mark that changes the health of 1 profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mark {
    Unhealthy(ProfileMarkedUnhealthy),
    Healthy(ProfileMarkedHealthy),
}

/// The mark `unhealthy` of `profile` with `reason`, or `None` when the
/// profile is unhealthy already.
pub fn mark_unhealthy(now: &Health, profile: &str, reason: &str) -> Option<Mark> {
    (*now == Health::Healthy).then(|| {
        Mark::Unhealthy(ProfileMarkedUnhealthy {
            profile_id: profile.to_string(),
            reason: reason.to_string(),
        })
    })
}

/// The mark `healthy` of `profile`, or `None` when the profile is healthy
/// already.
pub fn mark_healthy(now: &Health, profile: &str) -> Option<Mark> {
    (*now != Health::Healthy).then(|| {
        Mark::Healthy(ProfileMarkedHealthy {
            profile_id: profile.to_string(),
        })
    })
}

/// The marks after the conformance run `report` of 1 CLI. `profiles` holds
/// each profile whose `agent` is that CLI, with its health now. A failed
/// run marks each healthy one unhealthy, with the failed checks as the
/// cause. A passed run marks each unhealthy one healthy again.
pub fn after_conformance(report: &ConformanceReport, profiles: &[(String, Health)]) -> Vec<Mark> {
    let failure = report.failure().map(|f| f.to_string());
    profiles
        .iter()
        .filter_map(|(profile, now)| match &failure {
            Some(reason) => mark_unhealthy(now, profile, reason),
            None => mark_healthy(now, profile),
        })
        .collect()
}

/// The cause of a mark after a dialog at rung 4 of the stall ladder.
pub fn dialog_reason(attempt: &str) -> String {
    format!("dialog: attempt {attempt} waited on a dialog (rung 4)")
}

#[cfg(test)]
mod tests {
    use super::super::conformance::{Check, CheckResult};
    use super::*;

    fn unhealthy() -> Health {
        Health::Unhealthy {
            reason: "old".into(),
        }
    }

    fn report(fail: bool) -> ConformanceReport {
        let check = if fail {
            CheckResult::fail(Check::StopGate, "no repair")
        } else {
            CheckResult::pass(Check::StopGate, "repaired")
        };
        ConformanceReport {
            cli: "claude".into(),
            version: "1.0".into(),
            adapter_hash: "sha256:0".into(),
            checks: vec![check],
        }
    }

    #[test]
    fn same_health_gives_no_mark() {
        assert_eq!(mark_unhealthy(&unhealthy(), "p", "new"), None);
        assert_eq!(mark_healthy(&Health::Healthy, "p"), None);
    }

    #[test]
    fn a_change_gives_1_mark() {
        assert!(matches!(
            mark_unhealthy(&Health::Healthy, "p", "why"),
            Some(Mark::Unhealthy(m)) if m.profile_id == "p" && m.reason == "why"
        ));
        assert!(matches!(
            mark_healthy(&unhealthy(), "p"),
            Some(Mark::Healthy(m)) if m.profile_id == "p"
        ));
    }

    #[test]
    fn a_failed_run_names_its_failed_checks() {
        let profiles = vec![
            ("a".to_string(), Health::Healthy),
            ("b".into(), unhealthy()),
        ];
        let marks = after_conformance(&report(true), &profiles);
        assert_eq!(marks.len(), 1, "{marks:?}");
        let Mark::Unhealthy(m) = &marks[0] else {
            panic!("{marks:?}");
        };
        assert_eq!(m.profile_id, "a");
        assert!(m.reason.contains(Check::StopGate.name()), "{}", m.reason);
    }

    #[test]
    fn a_passed_run_marks_only_the_unhealthy_profiles() {
        let profiles = vec![
            ("a".to_string(), Health::Healthy),
            ("b".into(), unhealthy()),
        ];
        let marks = after_conformance(&report(false), &profiles);
        assert_eq!(
            marks,
            vec![Mark::Healthy(ProfileMarkedHealthy {
                profile_id: "b".into()
            })]
        );
    }
}
