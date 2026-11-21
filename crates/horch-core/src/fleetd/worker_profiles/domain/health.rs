use super::conformance::ConformanceReport;

pub const PROFILE: &str = "profile";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Health {
    Healthy,
    Unhealthy { reason: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileMarkedUnhealthy {
    pub profile_id: String,
    pub reason: String,
}

impl ProfileMarkedUnhealthy {
    pub const NAME: &'static str = "profile-marked-unhealthy";
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileMarkedHealthy {
    pub profile_id: String,
}

impl ProfileMarkedHealthy {
    pub const NAME: &'static str = "profile-marked-healthy";
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mark {
    Unhealthy(ProfileMarkedUnhealthy),
    Healthy(ProfileMarkedHealthy),
}

pub fn mark_unhealthy(now: &Health, profile: &str, reason: &str) -> Option<Mark> {
    (*now == Health::Healthy).then(|| {
        Mark::Unhealthy(ProfileMarkedUnhealthy {
            profile_id: profile.to_string(),
            reason: reason.to_string(),
        })
    })
}

pub fn mark_healthy(now: &Health, profile: &str) -> Option<Mark> {
    (*now != Health::Healthy).then(|| {
        Mark::Healthy(ProfileMarkedHealthy {
            profile_id: profile.to_string(),
        })
    })
}

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
