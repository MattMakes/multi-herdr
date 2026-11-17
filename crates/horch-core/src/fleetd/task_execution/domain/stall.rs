//! The stall ladder of a live attempt (`docs/specs/fleetd/design.md`,
//! "Supervision, liveness and reconciliation": "Liveness comes from
//! outside the model" and "Stall ladder", rungs 1 to 5).
//!
//! Facts in, 1 decision out. No I/O. Each timer is a time on the fleet
//! clock, so a sleep of the machine fires no rung. The facts come from
//! outside the model: the process, the activity and the herdr status of
//! the pane. The heartbeat is a fact of the lease. A usage limit is never
//! a stall.

use crate::fleetd::task_execution::domain::task::Seen;

/// The 1 line that fleetd types into an idle pane at rungs 1 and 2. It
/// holds no task data (FR-7).
pub const DOORBELL: &str =
    "[horch] NOTE: Your task has no result yet. Continue it, or finish it with its finish command.";

/// The time from 1 doorbell to the next rung, in seconds of the fleet
/// clock.
pub const DOORBELL_GAP_S: u64 = 120;

/// The most bytes of the screen excerpt of a needs-you item of rung 4.
pub const EXCERPT_MAX_BYTES: usize = 2048;

/// The herdr status of a pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneStatus {
    Working,
    /// The agent waits on a dialog: a login, trust or error prompt.
    Blocked,
    /// The agent waits at its prompt.
    Idle,
}

impl PaneStatus {
    /// The status of a herdr status word. Another word is no status.
    pub fn parse(word: &str) -> Option<PaneStatus> {
        match word {
            "working" => Some(PaneStatus::Working),
            "blocked" => Some(PaneStatus::Blocked),
            "idle" => Some(PaneStatus::Idle),
            _ => None,
        }
    }
}

/// The pane of an attempt, as part 1 of a pass saw it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pane {
    pub id: String,
    /// `None` when herdr gave no status, or a word that is not a status.
    pub status: Option<PaneStatus>,
    /// The visible text of the pane.
    pub screen: Option<String>,
}

/// The facts of 1 live attempt. Each time is a time on the fleet clock,
/// in seconds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Facts {
    /// The fleet clock of this pass.
    pub clock: i64,
    /// The agent process of the attempt.
    pub process: Seen,
    /// The last activity (`attempts.active_at`). `None`: no activity yet.
    pub active_at: Option<i64>,
    /// The claim of the task by the attempt.
    pub claimed_at: i64,
    /// The pane of the attempt, when it has one.
    pub pane: Option<Pane>,
    /// The pool of the persona of the attempt is limited.
    pub pool_limited: bool,
    /// The worker has an open `questions` row.
    pub open_question: bool,
    /// The end of a declared wait of the worker (`worker_waits.due_at`).
    pub waiting_until: Option<i64>,
    /// The profile runs the agent in an interactive pane.
    pub interactive: bool,
    /// The task of the attempt is `running`.
    pub task_running: bool,
    /// `stall_after_s` of the profile.
    pub stall_after_s: u64,
    /// The time of each doorbell of the attempt, oldest first.
    pub doorbells: Vec<i64>,
}

/// The agent state of 1 attempt, with the words of the design.
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
    /// The word of the design.
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

/// The outcome of an attempt that the ladder ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Rungs 3 and 4. It burns 1 attempt (rung 6).
    Stalled,
    /// The worker asked a question. It burns no attempt.
    Asked,
    /// A usage limit. It burns no attempt.
    Quota,
    /// The process exited with no result. It burns 1 attempt (rung 6).
    NoResult,
}

impl Outcome {
    /// The word of `attempts.outcome`.
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Stalled => "stalled",
            Outcome::Asked => "asked",
            Outcome::Quota => "quota",
            Outcome::NoResult => "no_result",
        }
    }
}

/// What the ladder does to 1 attempt in this pass.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    /// Nothing.
    None,
    /// Rung 1 or 2: type [`DOORBELL`] into the pane `pane`.
    Doorbell { pane: String, rung: u8 },
    /// End the attempt with `outcome`. `stop`: stop its process group.
    End { outcome: Outcome, stop: bool },
    /// Rung 4: end the attempt `stalled`, stop its process group, and open
    /// 1 needs-you item with `excerpt`.
    NeedsYou { excerpt: String },
}

/// Whether the fleet clock `clock` is at least `after_s` past `since`.
fn reached(clock: i64, since: i64, after_s: u64) -> bool {
    clock >= since.saturating_add_unsigned(after_s)
}

/// The agent state of the attempt of `f`. The first match wins:
/// 1. `exited`: the process is gone.
/// 2. `usage_limited`: the pool of the persona is limited.
/// 3. `waiting_approval`: herdr shows the pane `blocked`.
/// 4. `working`: activity within `stall_after_s`, or a declared wait
///    before its end.
/// 5. `idle_no_result`: an interactive pane that herdr shows `idle`, while
///    the task is `running`.
/// 6. `hung`: the process is alive, with no activity for `stall_after_s`
///    since the last activity, or since the claim when there was none.
/// 7. `starting`: no other state. The attempt has no activity yet within
///    `stall_after_s` of its claim, or part 1 did not read its process.
///    The ladder waits for a fact.
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

/// The decision of the ladder for the attempt of `f`:
/// - `idle_no_result`: rung 1 rings the doorbell. Rung 2 rings it again
///   [`DOORBELL_GAP_S`] after rung 1. Rung 3 ends the attempt `stalled`
///   [`DOORBELL_GAP_S`] after rung 2.
/// - `hung`: rung 3 at once.
/// - `waiting_approval`: rung 4.
/// - `exited`: rung 5. An open question gives `asked`, else a limited
///   pool gives `quota`, else `no_result`.
/// - `usage_limited`: the outcome `quota`, and a stop of the process.
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

/// Rung 5: the outcome of an attempt whose process is gone.
fn exit_outcome(f: &Facts) -> Outcome {
    if f.open_question {
        Outcome::Asked
    } else if f.pool_limited {
        Outcome::Quota
    } else {
        Outcome::NoResult
    }
}

/// The end of `screen`, at most [`EXCERPT_MAX_BYTES`] bytes, cut on a
/// character boundary. The prompt of a dialog is at the end of a screen.
fn excerpt(screen: Option<&str>) -> String {
    let screen = screen.unwrap_or_default();
    let mut start = screen.len().saturating_sub(EXCERPT_MAX_BYTES);
    while !screen.is_char_boundary(start) {
        start += 1;
    }
    screen[start..].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLAIM: i64 = 1_000;

    /// A live attempt in an interactive pane that herdr shows `working`,
    /// claimed at [`CLAIM`] with activity at the claim, at the clock of the
    /// claim.
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

    /// An idle pane with no activity since the claim, at `clock`.
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

    /// The first fleet second at or after `from` at which `f` with that
    /// clock decides something, up to `from + limit`.
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
        // A dead process with a limited pool and a blocked pane is exited.
        let mut f = with_status(facts(), PaneStatus::Blocked);
        f.pool_limited = true;
        f.process = Seen::Dead;
        assert_eq!(agent_state(&f), AgentState::Exited);
        f.process = Seen::Alive;
        assert_eq!(agent_state(&f), AgentState::UsageLimited);
        f.pool_limited = false;
        assert_eq!(agent_state(&f), AgentState::WaitingApproval);
        // An idle pane with recent activity is working.
        assert_eq!(agent_state(&idle_at(CLAIM + 299)), AgentState::Working);
        // An idle pane of a task that is not running is not idle_no_result.
        let mut f = idle_at(CLAIM + 300);
        f.task_running = false;
        assert_eq!(agent_state(&f), AgentState::Hung);
        // An idle pane of an exec profile is not idle_no_result.
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
        // A pane that herdr still shows working, with no activity.
        let f = facts();
        assert_eq!(
            first_action(&f, CLAIM, 1_000),
            Some((CLAIM + 300, stalled()))
        );
        // No pane at all.
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
        // `kill -STOP` at the last activity: the process is alive, its
        // heartbeat ages, herdr keeps its last status, and no activity
        // comes.
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
        // The agent claims and does nothing: no output, no pane status.
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
        // Rung 1 rang at 1300. The machine sleeps 8 h: the fleet clock
        // moves at most 10 s in the pass after the wake. Rung 2 waits for
        // the rest of its 120 s on the fleet clock.
        let mut f = idle_at(CLAIM + 310);
        f.doorbells = vec![CLAIM + 300];
        assert_eq!(decide(&f), Decision::None);
        f.clock = CLAIM + 419;
        assert_eq!(decide(&f), Decision::None);
        f.clock = CLAIM + 420;
        assert_eq!(decide(&f), doorbell(2));
        // The same for the stall timer itself.
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
        // The end of a long screen, cut on a character boundary.
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
}
