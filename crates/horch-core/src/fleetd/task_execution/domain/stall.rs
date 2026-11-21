use crate::fleetd::task_execution::domain::retry::Outcome;
use crate::fleetd::task_execution::domain::task::Seen;

pub const DOORBELL: &str =
    "[horch] NOTE: Your task has no result yet. Continue it, or finish it with its finish command.";

pub const DOORBELL_GAP_S: u64 = 120;

pub const EXCERPT_MAX_BYTES: usize = 2048;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneStatus {
    Working,
    Blocked,
    Idle,
}

impl PaneStatus {
    pub fn parse(word: &str) -> Option<PaneStatus> {
        match word {
            "working" => Some(PaneStatus::Working),
            "blocked" => Some(PaneStatus::Blocked),
            "idle" => Some(PaneStatus::Idle),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pane {
    pub id: String,
    pub status: Option<PaneStatus>,
    pub screen: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Facts {
    pub clock: i64,
    pub process: Seen,
    pub active_at: Option<i64>,
    pub claimed_at: i64,
    pub pane: Option<Pane>,
    pub pool_limited: bool,
    pub open_question: bool,
    pub waiting_until: Option<i64>,
    pub interactive: bool,
    pub task_running: bool,
    pub stall_after_s: u64,
    pub doorbells: Vec<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentState {
    Starting,
    Working,
    WaitingApproval,
    IdleNoResult,
    Hung,
    UsageLimited,
    Exited,
}

impl AgentState {
    pub fn as_str(self) -> &'static str {
        match self {
            AgentState::Starting => "starting",
            AgentState::Working => "working",
            AgentState::WaitingApproval => "waiting_approval",
            AgentState::IdleNoResult => "idle_no_result",
            AgentState::Hung => "hung",
            AgentState::UsageLimited => "usage_limited",
            AgentState::Exited => "exited",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    None,
    Doorbell { pane: String, rung: u8 },
    End { outcome: Outcome, stop: bool },
    NeedsYou { excerpt: String },
}

fn reached(clock: i64, since: i64, after_s: u64) -> bool {
    clock >= since.saturating_add_unsigned(after_s)
}

pub fn agent_state(f: &Facts) -> AgentState {
    if f.process == Seen::Dead {
        return AgentState::Exited;
    }
    if f.pool_limited {
        return AgentState::UsageLimited;
    }
    let status = f.pane.as_ref().and_then(|p| p.status);
    if status == Some(PaneStatus::Blocked) {
        return AgentState::WaitingApproval;
    }
    let active = f
        .active_at
        .is_some_and(|a| !reached(f.clock, a, f.stall_after_s));
    let waiting = f.waiting_until.is_some_and(|due| f.clock < due);
    if active || waiting {
        return AgentState::Working;
    }
    if f.interactive && status == Some(PaneStatus::Idle) && f.task_running {
        return AgentState::IdleNoResult;
    }
    let quiet_since = f.active_at.unwrap_or(f.claimed_at);
    if f.process == Seen::Alive && reached(f.clock, quiet_since, f.stall_after_s) {
        return AgentState::Hung;
    }
    AgentState::Starting
}

pub fn decide(f: &Facts) -> Decision {
    let stalled = Decision::End {
        outcome: Outcome::Stalled,
        stop: true,
    };
    match agent_state(f) {
        AgentState::Starting | AgentState::Working => Decision::None,
        AgentState::Hung => stalled,
        AgentState::UsageLimited => Decision::End {
            outcome: Outcome::Quota,
            stop: true,
        },
        AgentState::WaitingApproval => Decision::NeedsYou {
            excerpt: excerpt(f.pane.as_ref().and_then(|p| p.screen.as_deref())),
        },
        AgentState::Exited => Decision::End {
            outcome: exit_outcome(f),
            stop: false,
        },
        AgentState::IdleNoResult => {
            let rung = f.doorbells.len();
            let due = f
                .doorbells
                .last()
                .is_none_or(|last| reached(f.clock, *last, DOORBELL_GAP_S));
            match (rung, &f.pane) {
                (_, _) if !due => Decision::None,
                (0 | 1, Some(p)) => Decision::Doorbell {
                    pane: p.id.clone(),
                    rung: if rung == 0 { 1 } else { 2 },
                },
                _ => stalled,
            }
        }
    }
}

fn exit_outcome(f: &Facts) -> Outcome {
    if f.open_question {
        Outcome::Asked
    } else if f.pool_limited {
        Outcome::Quota
    } else {
        Outcome::NoResult
    }
}

fn excerpt(screen: Option<&str>) -> String {
    let screen = screen.unwrap_or_default();
    let mut start = screen.len().saturating_sub(EXCERPT_MAX_BYTES);
    while !screen.is_char_boundary(start) {
        start += 1;
    }
    screen[start..].to_string()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Settle {
    Wait,
    Rung6,
    Stuck,
}

pub fn stop_bound_s(stop_grace_s: u64, pass_period_s: u64) -> u64 {
    stop_grace_s.saturating_add(pass_period_s).saturating_mul(2)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OldProcess {
    pub seen: Seen,
    pub killed_at: Option<i64>,
}

pub fn settle(old: &[OldProcess], clock: i64, bound_at: i64, stop_grace_s: u64) -> Settle {
    let stopped = |p: &OldProcess| match p.seen {
        Seen::Dead => true,
        Seen::Unknown => p.killed_at.is_some_and(|k| reached(clock, k, stop_grace_s)),
        Seen::Alive => false,
    };
    if old.iter().all(stopped) {
        Settle::Rung6
    } else if clock >= bound_at {
        Settle::Stuck
    } else {
        Settle::Wait
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLAIM: i64 = 1_000;

    fn facts() -> Facts {
        Facts {
            clock: CLAIM,
            process: Seen::Alive,
            active_at: Some(CLAIM),
            claimed_at: CLAIM,
            pane: Some(Pane {
                id: "w0:p1".into(),
                status: Some(PaneStatus::Working),
                screen: Some("screen".into()),
            }),
            pool_limited: false,
            open_question: false,
            waiting_until: None,
            interactive: true,
            task_running: true,
            stall_after_s: 300,
            doorbells: Vec::new(),
        }
    }

    fn with_status(mut f: Facts, status: PaneStatus) -> Facts {
        f.pane.as_mut().unwrap().status = Some(status);
        f
    }

    fn idle_at(clock: i64) -> Facts {
        let mut f = with_status(facts(), PaneStatus::Idle);
        f.clock = clock;
        f
    }

    fn stalled() -> Decision {
        Decision::End {
            outcome: Outcome::Stalled,
            stop: true,
        }
    }

    fn doorbell(rung: u8) -> Decision {
        Decision::Doorbell {
            pane: "w0:p1".into(),
            rung,
        }
    }

    fn first_action(f: &Facts, from: i64, limit: i64) -> Option<(i64, Decision)> {
        (from..=from + limit).find_map(|clock| {
            let at = Facts { clock, ..f.clone() };
            let d = decide(&at);
            (d != Decision::None).then_some((clock, d))
        })
    }

    #[test]
    fn each_agent_state_from_its_facts() {
        let exited = Facts {
            process: Seen::Dead,
            ..facts()
        };
        let limited = Facts {
            pool_limited: true,
            ..facts()
        };
        let blocked = with_status(facts(), PaneStatus::Blocked);
        let idle = idle_at(CLAIM + 300);
        let hung = Facts {
            clock: CLAIM + 300,
            ..facts()
        };
        let starting = Facts {
            active_at: None,
            clock: CLAIM + 30,
            ..facts()
        };
        let cases = [
            (exited, AgentState::Exited),
            (limited, AgentState::UsageLimited),
            (blocked, AgentState::WaitingApproval),
            (facts(), AgentState::Working),
            (idle, AgentState::IdleNoResult),
            (hung, AgentState::Hung),
            (starting, AgentState::Starting),
        ];
        for (f, want) in cases {
            assert_eq!(agent_state(&f), want, "{f:?}");
        }
    }

    #[test]
    fn first_match_wins() {
        let mut f = with_status(facts(), PaneStatus::Blocked);
        f.pool_limited = true;
        f.process = Seen::Dead;
        assert_eq!(agent_state(&f), AgentState::Exited);
        f.process = Seen::Alive;
        assert_eq!(agent_state(&f), AgentState::UsageLimited);
        f.pool_limited = false;
        assert_eq!(agent_state(&f), AgentState::WaitingApproval);
        assert_eq!(agent_state(&idle_at(CLAIM + 299)), AgentState::Working);
        let mut f = idle_at(CLAIM + 300);
        f.task_running = false;
        assert_eq!(agent_state(&f), AgentState::Hung);
        let mut f = idle_at(CLAIM + 300);
        f.interactive = false;
        assert_eq!(agent_state(&f), AgentState::Hung);
    }

    #[test]
    fn usage_limited_is_never_hung() {
        for quiet_s in [0, 300, 10_000] {
            let f = Facts {
                pool_limited: true,
                clock: CLAIM + quiet_s,
                ..facts()
            };
            assert_eq!(agent_state(&f), AgentState::UsageLimited);
            assert_eq!(
                decide(&f),
                Decision::End {
                    outcome: Outcome::Quota,
                    stop: true
                }
            );
            let idle = Facts {
                pool_limited: true,
                ..idle_at(CLAIM + quiet_s)
            };
            assert_eq!(agent_state(&idle), AgentState::UsageLimited);
        }
    }

    #[test]
    fn declared_wait_counts_as_working_until_its_due_at() {
        let due = CLAIM + 3_600;
        let at = |clock| Facts {
            clock,
            waiting_until: Some(due),
            ..facts()
        };
        assert_eq!(agent_state(&at(CLAIM + 1_000)), AgentState::Working);
        assert_eq!(agent_state(&at(due - 1)), AgentState::Working);
        assert_eq!(agent_state(&at(due)), AgentState::Hung);
        assert_eq!(decide(&at(due - 1)), Decision::None);
        assert_eq!(decide(&at(due)), stalled());
    }

    #[test]
    fn rung_1_rings_once_and_rung_2_rings_120_s_later() {
        let mut f = idle_at(CLAIM + 300);
        assert_eq!(decide(&f), doorbell(1));
        f.doorbells = vec![CLAIM + 300];
        assert_eq!(
            first_action(&f, CLAIM + 300, 1_000),
            Some((CLAIM + 420, doorbell(2)))
        );
    }

    #[test]
    fn rung_3_ends_the_attempt_120_s_after_rung_2() {
        let mut f = idle_at(CLAIM + 420);
        f.doorbells = vec![CLAIM + 300, CLAIM + 420];
        assert_eq!(
            first_action(&f, CLAIM + 420, 1_000),
            Some((CLAIM + 540, stalled()))
        );
    }

    #[test]
    fn hung_ends_the_attempt_at_once() {
        let f = facts();
        assert_eq!(
            first_action(&f, CLAIM, 1_000),
            Some((CLAIM + 300, stalled()))
        );
        let f = Facts {
            pane: None,
            ..facts()
        };
        assert_eq!(
            first_action(&f, CLAIM, 1_000),
            Some((CLAIM + 300, stalled()))
        );
    }

    #[test]
    fn rr1_stopped_worker_is_hung_after_stall_after_s_with_no_activity() {
        let last = CLAIM + 500;
        let at = |clock: i64| Facts {
            clock,
            active_at: Some(last),
            ..facts()
        };
        assert_eq!(agent_state(&at(last + 299)), AgentState::Working);
        assert_eq!(agent_state(&at(last + 300)), AgentState::Hung);
        assert_eq!(
            first_action(&at(last), last, 1_000),
            Some((last + 300, stalled()))
        );
    }

    #[test]
    fn fm3_idle_claim_is_caught_within_stall_after_plus_lease() {
        let lease_s = 120;
        let f = Facts {
            pane: None,
            ..facts()
        };
        let (at, d) = first_action(&f, CLAIM, 1_000).unwrap();
        assert_eq!(d, stalled());
        assert!(at <= CLAIM + 300 + lease_s, "caught at {at}");
        assert!(at >= CLAIM + 300, "caught before stall_after_s at {at}");
    }

    #[test]
    fn sleep_past_a_rung_fires_it_only_after_the_rest_of_its_fleet_time() {
        let mut f = idle_at(CLAIM + 310);
        f.doorbells = vec![CLAIM + 300];
        assert_eq!(decide(&f), Decision::None);
        f.clock = CLAIM + 419;
        assert_eq!(decide(&f), Decision::None);
        f.clock = CLAIM + 420;
        assert_eq!(decide(&f), doorbell(2));
        let quiet = Facts {
            clock: CLAIM + 299,
            ..facts()
        };
        assert_eq!(decide(&quiet), Decision::None);
    }

    #[test]
    fn exit_with_an_open_question_is_asked_and_burns_no_attempt() {
        let f = Facts {
            process: Seen::Dead,
            open_question: true,
            pool_limited: true,
            ..facts()
        };
        assert_eq!(
            decide(&f),
            Decision::End {
                outcome: Outcome::Asked,
                stop: false
            }
        );
    }

    #[test]
    fn exit_with_a_limited_pool_is_quota() {
        let f = Facts {
            process: Seen::Dead,
            pool_limited: true,
            ..facts()
        };
        assert_eq!(
            decide(&f),
            Decision::End {
                outcome: Outcome::Quota,
                stop: false
            }
        );
    }

    #[test]
    fn exit_with_neither_is_no_result() {
        let f = Facts {
            process: Seen::Dead,
            ..facts()
        };
        assert_eq!(
            decide(&f),
            Decision::End {
                outcome: Outcome::NoResult,
                stop: false
            }
        );
    }

    #[test]
    fn waiting_approval_opens_a_needs_you_item_with_at_most_2_kib_of_the_screen() {
        let mut f = with_status(facts(), PaneStatus::Blocked);
        assert_eq!(
            decide(&f),
            Decision::NeedsYou {
                excerpt: "screen".into()
            }
        );
        let screen = format!("{}é{}", "a".repeat(3_000), "b".repeat(2_047));
        f.pane.as_mut().unwrap().screen = Some(screen);
        let Decision::NeedsYou { excerpt } = decide(&f) else {
            panic!("rung 4");
        };
        assert_eq!(excerpt, "b".repeat(2_047));
    }

    #[test]
    fn starting_and_working_do_nothing() {
        let starting = Facts {
            active_at: None,
            clock: CLAIM + 30,
            ..facts()
        };
        assert_eq!(decide(&starting), Decision::None);
        assert_eq!(decide(&facts()), Decision::None);
    }

    #[test]
    fn doorbell_holds_no_task_data() {
        assert_eq!(DOORBELL.lines().count(), 1);
        assert!(!DOORBELL.contains('{'));
    }

    #[test]
    fn stalled_task_moves_on_only_when_each_old_process_is_stopped() {
        let p = |seen: Seen, killed_at: Option<i64>| OldProcess { seen, killed_at };
        let dead = p(Seen::Dead, None);
        let alive = p(Seen::Alive, Some(10));
        let unknown = p(Seen::Unknown, None);
        let unknown_killed = p(Seen::Unknown, Some(10));
        let at = |old: &[OldProcess], clock: i64| settle(old, clock, 100, 20);
        assert_eq!(at(&[], 10), Settle::Rung6);
        assert_eq!(at(&[dead, dead], 10), Settle::Rung6);
        assert_eq!(at(&[dead, alive], 10), Settle::Wait);
        assert_eq!(at(&[dead, alive], 100), Settle::Stuck);
        assert_eq!(at(&[dead, unknown], 99), Settle::Wait);
        assert_eq!(at(&[dead, unknown], 100), Settle::Stuck);
        assert_eq!(at(&[dead, unknown_killed], 29), Settle::Wait);
        assert_eq!(at(&[dead, unknown_killed], 30), Settle::Rung6);
        assert_eq!(at(&[alive, unknown_killed], 100), Settle::Stuck);
    }

    #[test]
    fn stop_bound_holds_2_graces_and_2_passes() {
        assert_eq!(stop_bound_s(20, 5), 50);
        assert_eq!(stop_bound_s(600, 5), 1_210);
        assert_eq!(stop_bound_s(u64::MAX, 5), u64::MAX);
    }
}
