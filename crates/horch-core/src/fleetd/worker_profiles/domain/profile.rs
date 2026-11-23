#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Exec,
    Interactive,
}

impl Mode {
    pub fn default_for(exec_adapter: bool) -> Mode {
        if exec_adapter {
            Mode::Exec
        } else {
            Mode::Interactive
        }
    }

    pub fn of_launch(exec_stream: bool) -> Mode {
        if exec_stream {
            Mode::Exec
        } else {
            Mode::Interactive
        }
    }

    pub fn has_background_wait(self) -> bool {
        self == Mode::Interactive
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Liveness {
    pub start_timeout_s: u64,
    pub stall_after_s: u64,
    pub lease_s: u64,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Retry {
    pub max_attempts: Option<u32>,
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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Profile {
    pub mode: Mode,
    pub liveness: Liveness,
    pub retry: Retry,
    pub fallback: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnUsageLimit<'a> {
    RestartOn(&'a str),
    WaitQuota,
}

impl Profile {
    pub fn fallback(&self) -> Option<&str> {
        self.fallback.as_deref()
    }

    pub fn on_usage_limit(&self) -> OnUsageLimit<'_> {
        match self.fallback() {
            Some(persona) => OnUsageLimit::RestartOn(persona),
            None => OnUsageLimit::WaitQuota,
        }
    }

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
        assert_eq!(p.mode, Mode::Exec);
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
    fn mode_absent_is_exec_in_phase_7() {
        assert_eq!(Mode::default(), Mode::Exec);
        assert_eq!(Mode::default_for(true), Mode::Exec);
        assert_eq!(Mode::default_for(false), Mode::Interactive);
    }

    #[test]
    fn an_attempt_is_exec_only_after_its_exec_stream_opens() {
        assert_eq!(Mode::of_launch(true), Mode::Exec);
        assert_eq!(Mode::of_launch(false), Mode::Interactive);
    }

    #[test]
    fn only_an_interactive_attempt_has_a_background_wait() {
        assert!(Mode::Interactive.has_background_wait());
        assert!(!Mode::Exec.has_background_wait());
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
