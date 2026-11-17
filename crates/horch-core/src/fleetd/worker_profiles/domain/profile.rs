//! The profile of a worker persona: its mode, its liveness and retry
//! numbers, and its fallback (`docs/specs/fleetd/design.md`, "Worker
//! profiles and adapters" and "The numbers of the design").
//!
//! This module does no I/O and parses no text: the roster reads the
//! frontmatter of a teammate file and builds a [`Profile`].

/// How fleetd runs a worker of the persona.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// A run-to-completion process.
    Exec,
    /// An agent in a herdr pane. The default until Phase 7 makes exec the
    /// default (`docs/tickets/fleetd-p2/MAP.md`, Q10).
    #[default]
    Interactive,
}

/// The liveness numbers of a persona, in seconds of the fleet clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Liveness {
    /// No first signal in this time: the attempt failed. Starting value 90.
    pub start_timeout_s: u64,
    /// No activity in this time: the lease is not renewed. Starting value 300.
    pub stall_after_s: u64,
    /// The lease of an attempt, below `stall_after_s`. Starting value 120.
    pub lease_s: u64,
    /// The cap on a declared wait of an interactive persona (FM-3).
    /// Default 3600.
    pub max_wait_s: u64,
}

impl Default for Liveness {
    fn default() -> Self {
        Liveness {
            start_timeout_s: 90,
            stall_after_s: 300,
            lease_s: 120,
            max_wait_s: 3600,
        }
    }
}

/// The retry numbers of a persona (RR-6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Retry {
    /// The attempts of a task. `None`: the `max_attempts` of the run.
    pub max_attempts: Option<u32>,
    /// The backoff before each retry, in seconds of the fleet clock. The
    /// last value repeats. Default 30, 120, 480.
    pub backoff_s: Vec<u64>,
}

impl Default for Retry {
    fn default() -> Self {
        Retry {
            max_attempts: None,
            backoff_s: vec![30, 120, 480],
        }
    }
}

/// The profile of 1 persona, as fleetd reads it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Profile {
    pub mode: Mode,
    pub liveness: Liveness,
    pub retry: Retry,
    /// The persona whose launch a task borrows on a usage limit or a
    /// provider outage (RR-11). `None` for a persona that is exempt or
    /// whose file gives a reason for no fallback.
    pub fallback: Option<String>,
}

/// What fleetd does with a task of the persona on a usage limit or a
/// provider outage (RR-11). No attempt is burned on either path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnUsageLimit<'a> {
    /// Restart the task at once on this persona.
    RestartOn(&'a str),
    /// Wait in `waiting_quota` with the lease paused. The task never moves
    /// to another harness.
    WaitQuota,
}

impl Profile {
    /// The fallback persona, if the profile names one.
    pub fn fallback(&self) -> Option<&str> {
        self.fallback.as_deref()
    }

    /// The path of a task of this persona on a usage limit or an outage.
    pub fn on_usage_limit(&self) -> OnUsageLimit<'_> {
        match self.fallback() {
            Some(persona) => OnUsageLimit::RestartOn(persona),
            None => OnUsageLimit::WaitQuota,
        }
    }

    /// Each rule that the numbers break, as 1 line that names the field.
    /// Empty: the numbers are valid.
    pub fn problems(&self) -> Vec<String> {
        let l = &self.liveness;
        let mut out: Vec<String> = [
            ("liveness.start_timeout_s", l.start_timeout_s),
            ("liveness.stall_after_s", l.stall_after_s),
            ("liveness.lease_s", l.lease_s),
            ("liveness.max_wait_s", l.max_wait_s),
        ]
        .into_iter()
        .filter(|(_, v)| *v == 0)
        .map(|(field, _)| format!("{field} must be above 0"))
        .collect();
        if l.lease_s >= l.stall_after_s {
            out.push(format!(
                "liveness.lease_s ({}) must be below liveness.stall_after_s ({})",
                l.lease_s, l.stall_after_s
            ));
        }
        if self.retry.max_attempts == Some(0) {
            out.push("retry.max_attempts must be above 0".to_string());
        }
        if self.retry.backoff_s.is_empty() {
            out.push("retry.backoff_s must not be empty".to_string());
        } else if self.retry.backoff_s.contains(&0) {
            out.push(format!(
                "retry.backoff_s must hold values above 0, got {:?}",
                self.retry.backoff_s
            ));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_profile_has_the_design_numbers_and_no_problem() {
        let p = Profile::default();
        assert_eq!(p.mode, Mode::Interactive);
        assert_eq!(
            p.liveness,
            Liveness {
                start_timeout_s: 90,
                stall_after_s: 300,
                lease_s: 120,
                max_wait_s: 3600,
            }
        );
        assert_eq!(p.retry.max_attempts, None);
        assert_eq!(p.retry.backoff_s, [30, 120, 480]);
        assert!(p.problems().is_empty(), "{:?}", p.problems());
    }

    #[test]
    fn lease_not_below_stall_names_both_fields() {
        let mut p = Profile::default();
        p.liveness.lease_s = 300;
        assert_eq!(
            p.problems(),
            ["liveness.lease_s (300) must be below liveness.stall_after_s (300)"]
        );
    }

    #[test]
    fn a_zero_number_or_an_empty_backoff_names_its_field() {
        let mut p = Profile::default();
        p.liveness.max_wait_s = 0;
        p.retry.max_attempts = Some(0);
        p.retry.backoff_s.clear();
        assert_eq!(
            p.problems(),
            [
                "liveness.max_wait_s must be above 0",
                "retry.max_attempts must be above 0",
                "retry.backoff_s must not be empty",
            ]
        );
        p.retry.backoff_s = vec![30, 0];
        assert!(p.problems()[2].starts_with("retry.backoff_s must hold values above 0"));
    }

    #[test]
    fn fallback_reads_the_named_persona() {
        let p = Profile {
            fallback: Some("codex-sol".to_string()),
            ..Profile::default()
        };
        assert_eq!(p.fallback(), Some("codex-sol"));
        assert_eq!(Profile::default().fallback(), None);
    }

    #[test]
    fn a_persona_with_no_fallback_waits_in_waiting_quota() {
        assert_eq!(Profile::default().on_usage_limit(), OnUsageLimit::WaitQuota);
        let p = Profile {
            fallback: Some("opus".to_string()),
            ..Profile::default()
        };
        assert_eq!(p.on_usage_limit(), OnUsageLimit::RestartOn("opus"));
    }
}
