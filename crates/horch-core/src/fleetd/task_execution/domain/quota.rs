//! The usage limit of a task: `waiting_quota` and the fallback
//! (`docs/specs/fleetd/design.md`, RR-11, FM-5, "Stall ladder", and "The
//! task state machine", the rows `running` and `waiting_quota`).
//!
//! A usage limit is not a stall and burns no attempt. The task restarts at
//! once on its fallback persona when the fallback draws on another pool
//! that is not limited. Else it waits in `waiting_quota` until its pool
//! resets: the calendar time passes `reset_at`, or, with no `reset_at`,
//! the pool reading is not limited. When both pools are limited, the task
//! waits; there is no third persona.
//!
//! Facts in, 1 decision out. No I/O. Each time is a calendar time in Unix
//! seconds: a usage window of a provider runs in calendar time.

/// 1 usage window of a pool.
#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    /// The used share, 0 to 1. A reading above 1 is over the limit.
    pub used: f64,
    /// The reset of the window. `None`: the reading gives no reset.
    pub reset_at: Option<i64>,
}

/// The reading of the pool of 1 persona.
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    /// The pool: `pool_for` of the agent and the model of the persona.
    pub pool: String,
    /// The state of the pool blocks a spawn (`State::blocks`).
    pub blocks: bool,
    /// The windows that limit the model of the persona.
    pub windows: Vec<Window>,
}

/// A window at or above `full_use` that did not reset at `now`.
fn full(w: &Window, full_use: f64, now: i64) -> bool {
    w.used >= full_use && w.reset_at.is_none_or(|t| now < t)
}

/// The pool of `r` is limited at `now`: its state blocks, or a window is
/// at or above `full_use` and did not reset.
pub fn limited(r: &Reading, full_use: f64, now: i64) -> bool {
    r.blocks || r.windows.iter().any(|w| full(w, full_use, now))
}

/// The reset of the limit of `r`: the latest `reset_at` of its windows at
/// or above `full_use`. `None` when no window is at full use, or when 1 of
/// them has no `reset_at`: then only a reading that is not limited ends
/// the wait.
pub fn reset_at(r: &Reading, full_use: f64) -> Option<i64> {
    let mut at = None;
    for w in r.windows.iter().filter(|w| w.used >= full_use) {
        at = Some(at.unwrap_or(i64::MIN).max(w.reset_at?));
    }
    at
}

/// Where a usage limit moves a `running` task.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Exit {
    /// `ready` at once, claimable by the fallback `persona`, and 1 new
    /// worker of that persona.
    Restart { persona: String },
    /// `waiting_quota` until the pool resets.
    Wait,
}

/// The exit of a task whose attempt on the pool `own` ended with a usage
/// limit. `fallback` is the fallback persona and the reading of its pool,
/// when the task has one.
pub fn exit(own: &str, fallback: Option<(&str, &Reading)>, full_use: f64, now: i64) -> Exit {
    match fallback {
        Some((persona, r)) if r.pool != own && !limited(r, full_use, now) => Exit::Restart {
            persona: persona.to_string(),
        },
        _ => Exit::Wait,
    }
}

/// A task in `waiting_quota` on the pool of `r` is released at `now`: the
/// calendar time passed `reset_at`, or, with no `reset_at`, the pool is not
/// limited.
pub fn released(r: &Reading, full_use: f64, now: i64) -> bool {
    match reset_at(r, full_use) {
        Some(t) => now >= t,
        None => !limited(r, full_use, now),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The design default of `pool_full_use`.
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
        // A full window whose reset passed limits nothing.
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
        // The fallback names its own fallback; there is no third persona.
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
        // Calendar time alone never releases it.
        for later in [NOW, NOW + 86_400, NOW + 30 * 86_400] {
            assert!(!released(&limited_no_reset, FULL, later));
        }
        // A blocked state with no window waits too.
        assert!(!released(&reading("claude", true, &[]), FULL, NOW + 86_400));
        // The probe reports ok: the task is released.
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
        // The state of a reading that is not probed again still blocks: the
        // calendar time past `reset_at` releases the task.
        assert!(released(&r, FULL, NOW));
        // Before the reset of the last full window, the task waits.
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
