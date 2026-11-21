#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    pub used: f64,
    pub reset_at: Option<i64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    pub pool: String,
    pub blocks: bool,
    pub windows: Vec<Window>,
}

fn full(w: &Window, full_use: f64, now: i64) -> bool {
    w.used >= full_use && w.reset_at.is_none_or(|t| now < t)
}

pub fn limited(r: &Reading, full_use: f64, now: i64) -> bool {
    r.blocks || r.windows.iter().any(|w| full(w, full_use, now))
}

pub fn reset_at(r: &Reading, full_use: f64) -> Option<i64> {
    let mut at = None;
    for w in r.windows.iter().filter(|w| w.used >= full_use) {
        at = Some(at.unwrap_or(i64::MIN).max(w.reset_at?));
    }
    at
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Exit {
    Restart { persona: String },
    Wait,
}

pub fn exit(own: &str, fallback: Option<(&str, &Reading)>, full_use: f64, now: i64) -> Exit {
    match fallback {
        Some((persona, r)) if r.pool != own && !limited(r, full_use, now) => Exit::Restart {
            persona: persona.to_string(),
        },
        _ => Exit::Wait,
    }
}

pub fn released(r: &Reading, full_use: f64, now: i64) -> bool {
    match reset_at(r, full_use) {
        Some(t) => now >= t,
        None => !limited(r, full_use, now),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: f64 = 0.98;
    const NOW: i64 = 1_800_000_000;

    fn reading(pool: &str, blocks: bool, windows: &[(f64, Option<i64>)]) -> Reading {
        Reading {
            pool: pool.to_string(),
            blocks,
            windows: windows
                .iter()
                .map(|&(used, reset_at)| Window { used, reset_at })
                .collect(),
        }
    }

    fn ok(pool: &str) -> Reading {
        reading(pool, false, &[(0.2, Some(NOW + 3600))])
    }

    fn full_until(pool: &str, reset_at: Option<i64>) -> Reading {
        reading(pool, false, &[(0.3, Some(NOW + 60)), (1.0, reset_at)])
    }

    #[test]
    fn pool_at_full_use_is_limited() {
        assert!(limited(&full_until("claude", Some(NOW + 600)), FULL, NOW));
        assert!(limited(
            &reading("claude", false, &[(FULL, None)]),
            FULL,
            NOW
        ));
        assert!(!limited(
            &reading("claude", false, &[(0.97, None)]),
            FULL,
            NOW
        ));
        assert!(!limited(&ok("claude"), FULL, NOW));
        assert!(!limited(&full_until("claude", Some(NOW)), FULL, NOW));
    }

    #[test]
    fn pool_state_that_blocks_is_limited() {
        assert!(limited(&reading("codex", true, &[]), FULL, NOW));
        assert!(limited(&reading("codex", true, &[(0.1, None)]), FULL, NOW));
        assert!(!limited(&reading("codex", false, &[]), FULL, NOW));
    }

    #[test]
    fn fallback_on_another_pool_restarts() {
        let fb = ok("codex");
        assert_eq!(
            exit("claude", Some(("codex-sol", &fb)), FULL, NOW),
            Exit::Restart {
                persona: "codex-sol".into()
            }
        );
    }

    #[test]
    fn fallback_on_the_same_pool_waits() {
        let fb = ok("claude");
        assert_eq!(exit("claude", Some(("opus", &fb)), FULL, NOW), Exit::Wait);
        assert_eq!(exit("claude", None, FULL, NOW), Exit::Wait);
    }

    #[test]
    fn both_pools_limited_waits() {
        for fb in [
            full_until("codex", Some(NOW + 600)),
            reading("codex", true, &[]),
        ] {
            assert_eq!(
                exit("claude", Some(("codex-sol", &fb)), FULL, NOW),
                Exit::Wait
            );
        }
    }

    #[test]
    fn usage_limit_without_reset_at_waits_for_pool_probe_ok() {
        let limited_no_reset = full_until("claude", None);
        assert_eq!(reset_at(&limited_no_reset, FULL), None);
        for later in [NOW, NOW + 86_400, NOW + 30 * 86_400] {
            assert!(!released(&limited_no_reset, FULL, later));
        }
        assert!(!released(&reading("claude", true, &[]), FULL, NOW + 86_400));
        assert!(released(&ok("claude"), FULL, NOW));
        assert!(released(&reading("claude", false, &[]), FULL, NOW));
    }

    #[test]
    fn reset_at_in_the_past_releases_the_task() {
        let r = reading(
            "claude",
            true,
            &[(1.0, Some(NOW - 1)), (0.99, Some(NOW - 10))],
        );
        assert_eq!(reset_at(&r, FULL), Some(NOW - 1));
        assert!(released(&r, FULL, NOW));
        let r = reading(
            "claude",
            false,
            &[(1.0, Some(NOW + 5)), (0.99, Some(NOW - 10))],
        );
        assert_eq!(reset_at(&r, FULL), Some(NOW + 5));
        assert!(!released(&r, FULL, NOW));
        assert!(released(&r, FULL, NOW + 5));
    }
}
