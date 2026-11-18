//! The lease of a held task: its renewal, its expiry and the lease part of
//! I1 (`docs/specs/fleetd/design.md`, "Ledger", rule 2 "Leases, not
//! claims", and "Supervision, liveness and reconciliation").
//!
//! Each time is a value on the fleet clock, in seconds, so a sleep of the
//! machine moves no deadline (PL-4). No I/O.
//!
//! - Renew ([`judge`]): only a pass renews a lease, and only on 3 outside
//!   signals: the process is alive, the wrapper heartbeat is at most
//!   `heartbeat_max_age_s` old, and the last activity is at most
//!   `stall_after_s` old. The model's own words are not a signal.
//! - Expire ([`judge`]): the fleet clock past `expires_at` moves a
//!   `running` task to `stalled`, and ends the resolver or fix attempt of a
//!   `done` task ([`Holder::Repair`]) with no move of the task.
//! - Pause: a task in `waiting_quota` keeps its lease, and the lease does
//!   not expire.
//! - I1 ([`i1_repairs`]): each live attempt of a held task has exactly 1
//!   lease, and each lease belongs to a live attempt of a held task. A
//!   `done` task whose branch is `conflicted` or `gate_failed` is held while
//!   its resolver or fix attempt is live. An attempt whose worker row ended
//!   (a handoff, a reap) is ending: the pass reaps it, and I1 does not
//!   count it as a second live attempt.

use super::task::Seen;

/// The `renew_basis` of the lease that a claim writes.
pub const CLAIM_BASIS: &str = "claim";

/// The `renew_basis` of a renewal: the 3 signals that earned it.
pub const EARNED_BASIS: &str = "process,heartbeat,activity";

/// The `renew_basis` of a lease that the I1 check writes.
pub const I1_BASIS: &str = "i1_repair";

/// A task state that holds a lease.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Holder {
    /// fleetd chose a worker that has not started (Phase 4). The start
    /// timeout, not the lease, limits this state.
    Leased,
    Running,
    /// The lease expired. Rungs 3 and 6 of the stall ladder act on it.
    Stalled,
    /// The lease is paused until the pool resets.
    WaitingQuota,
    /// A `done` task whose branch is `conflicted` or `gate_failed`: its
    /// resolver or fix attempt holds the lease while it is live (I1). The
    /// lease expires like the lease of a `running` task, and its expiry
    /// ends the attempt, not the task ("Leases, not claims", rule 2).
    Repair,
}

impl Holder {
    /// Every task state that holds a lease by its state alone. A `done`
    /// task holds one only with a branch to repair ([`Holder::of_task`]).
    pub const ALL: [Holder; 4] = [
        Holder::Leased,
        Holder::Running,
        Holder::Stalled,
        Holder::WaitingQuota,
    ];

    /// The `tasks.status` word.
    pub fn as_str(self) -> &'static str {
        match self {
            Holder::Leased => "leased",
            Holder::Running => "running",
            Holder::Stalled => "stalled",
            Holder::WaitingQuota => "waiting_quota",
            Holder::Repair => "done",
        }
    }

    /// The holder of a `tasks.status` word; `None` for a state that holds
    /// no lease by its state alone.
    pub fn of(word: &str) -> Option<Holder> {
        Self::ALL.into_iter().find(|h| h.as_str() == word)
    }

    /// The holder of a task in the state `word`. `repair` says that the
    /// task is `done` and its branch is `conflicted` or `gate_failed`.
    pub fn of_task(word: &str, repair: bool) -> Option<Holder> {
        Holder::of(word)
            .or_else(|| (repair && word == Holder::Repair.as_str()).then_some(Holder::Repair))
    }
}

/// The numbers of 1 attempt, in seconds: `lease_s` and `stall_after_s` of
/// the profile of its worker, `heartbeat_max_age_s` of `fleet.toml`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub lease_s: i64,
    pub stall_after_s: i64,
    pub heartbeat_max_age_s: i64,
}

/// The outside signals of 1 live attempt in 1 pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Signals {
    pub process: Seen,
    /// The age of the last wrapper heartbeat; `None` with no heartbeat.
    pub heartbeat_age_s: Option<i64>,
    /// The fleet clock of the last activity (`attempts.active_at`).
    pub active_at: Option<i64>,
}

/// The deadline of a lease on the fleet clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lease {
    pub expires_at: i64,
}

impl Lease {
    /// The lease that a claim at fleet clock `clock` writes.
    pub fn claimed(clock: i64, lease_s: i64) -> Lease {
        Lease {
            expires_at: clock + lease_s,
        }
    }
}

/// What 1 pass does to 1 lease.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// The signals earned a renewal: the new deadline.
    Renew { expires_at: i64 },
    /// No change.
    Keep,
    /// The fleet clock passed the deadline: the task moves to `stalled`.
    Expire,
}

/// Whether `s` earns a renewal at fleet clock `clock`.
fn earned(clock: i64, s: &Signals, l: &Limits) -> bool {
    s.process == Seen::Alive
        && s.heartbeat_age_s
            .is_some_and(|age| age <= l.heartbeat_max_age_s)
        && s.active_at.is_some_and(|at| clock - at <= l.stall_after_s)
}

/// The verdict of 1 pass at fleet clock `clock` on the lease of a task in
/// state `holder`. An expiry wins over the signals of the same pass: the
/// lease is a deadline, and the signals of the passes before did not earn
/// it. Only a `running` task and a repair attempt of a `done` task expire:
/// `leased` has the start timeout, `stalled` expired already, and
/// `waiting_quota` is paused.
pub fn judge(holder: Holder, lease: Lease, clock: i64, s: &Signals, l: &Limits) -> Verdict {
    if !matches!(holder, Holder::Running | Holder::Repair) {
        return Verdict::Keep;
    }
    if clock > lease.expires_at {
        return Verdict::Expire;
    }
    if earned(clock, s, l) {
        return Verdict::Renew {
            expires_at: clock + l.lease_s,
        };
    }
    Verdict::Keep
}

/// The lease facts of 1 task: a task that holds a lease, or that has a
/// lease row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskLeases {
    pub task_id: String,
    /// `None` when the state of the task holds no lease.
    pub holder: Option<Holder>,
    /// The attempt of its lease row, when it has one.
    pub lease_attempt: Option<String>,
    /// Its attempts that are not ended and whose worker row is live,
    /// oldest first.
    pub live_attempts: Vec<String>,
    /// Its attempts that are not ended and whose worker row ended.
    pub ending_attempts: Vec<String>,
}

/// 1 breach of the lease part of I1, and its repair.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Repair {
    /// The live attempt has no lease, or the lease names another attempt:
    /// write its lease with a full time.
    Write { task_id: String, attempt_id: String },
    /// The lease belongs to an attempt that is not live, or to a task that
    /// is not held: delete it.
    Delete { task_id: String, attempt_id: String },
    /// 2 live attempts of 1 task: no repair is safe, so the operator gets
    /// 1 needs-you item.
    TwoLive {
        task_id: String,
        attempts: Vec<String>,
    },
}

/// The repairs of the lease part of I1. A task in `waiting_quota` keeps its
/// lease with no live attempt: the usage limit ended the attempt, and the
/// lease is paused. The lease of an ending attempt stays until the pass
/// reaps the attempt, so the lease can still expire.
pub fn i1_repairs(tasks: &[TaskLeases]) -> Vec<Repair> {
    let mut out = Vec::new();
    for t in tasks {
        let task_id = t.task_id.clone();
        match (
            t.holder,
            t.live_attempts.as_slice(),
            t.lease_attempt.as_deref(),
        ) {
            (Some(_), many @ [_, _, ..], _) => out.push(Repair::TwoLive {
                task_id,
                attempts: many.to_vec(),
            }),
            (Some(_), [live], lease) if lease != Some(live.as_str()) => out.push(Repair::Write {
                task_id,
                attempt_id: live.clone(),
            }),
            (None, _, Some(stale)) => out.push(Repair::Delete {
                task_id,
                attempt_id: stale.to_string(),
            }),
            (
                Some(Holder::Leased | Holder::Running | Holder::Stalled | Holder::Repair),
                [],
                Some(stale),
            ) if !t.ending_attempts.iter().any(|a| a == stale) => out.push(Repair::Delete {
                task_id,
                attempt_id: stale.to_string(),
            }),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fleetd::task_execution::domain::task::Seen;

    const LIMITS: Limits = Limits {
        lease_s: 120,
        stall_after_s: 300,
        heartbeat_max_age_s: 30,
    };

    /// Signals that earn a renewal at fleet clock `clock`.
    fn earning(clock: i64) -> Signals {
        Signals {
            process: Seen::Alive,
            heartbeat_age_s: Some(10),
            active_at: Some(clock),
        }
    }

    #[test]
    fn renews_only_with_process_heartbeat_and_activity() {
        let clock = 1_000;
        let lease = Lease::claimed(clock - 60, LIMITS.lease_s);
        assert_eq!(
            judge(Holder::Running, lease, clock, &earning(clock), &LIMITS),
            Verdict::Renew {
                expires_at: clock + 120
            }
        );
        let mut no_process = earning(clock);
        no_process.process = Seen::Dead;
        let mut unknown_process = earning(clock);
        unknown_process.process = Seen::Unknown;
        let mut old_heartbeat = earning(clock);
        old_heartbeat.heartbeat_age_s = Some(31);
        let mut no_heartbeat = earning(clock);
        no_heartbeat.heartbeat_age_s = None;
        let mut idle = earning(clock);
        idle.active_at = Some(clock - 301);
        let mut never_active = earning(clock);
        never_active.active_at = None;
        for s in [
            no_process,
            unknown_process,
            old_heartbeat,
            no_heartbeat,
            idle,
            never_active,
        ] {
            assert_eq!(
                judge(Holder::Running, lease, clock, &s, &LIMITS),
                Verdict::Keep,
                "{s:?}"
            );
        }
        let mut at_the_limits = earning(clock);
        at_the_limits.heartbeat_age_s = Some(30);
        at_the_limits.active_at = Some(clock - 300);
        assert!(matches!(
            judge(Holder::Running, lease, clock, &at_the_limits, &LIMITS),
            Verdict::Renew { .. }
        ));
    }

    #[test]
    fn expires_on_the_fleet_clock_not_the_wall_clock() {
        let lease = Lease::claimed(0, LIMITS.lease_s);
        let dead = Signals {
            process: Seen::Dead,
            heartbeat_age_s: None,
            active_at: Some(0),
        };
        assert_eq!(
            judge(Holder::Running, lease, 120, &dead, &LIMITS),
            Verdict::Keep,
            "the lease holds up to its expiry"
        );
        assert_eq!(
            judge(Holder::Running, lease, 121, &dead, &LIMITS),
            Verdict::Expire
        );
        // An expired lease is not renewed by signals of the same pass.
        assert_eq!(
            judge(Holder::Running, lease, 121, &earning(121), &LIMITS),
            Verdict::Expire
        );
    }

    #[test]
    fn sleep_does_not_expire_a_lease() {
        // A sleep of 2 h: the fleet clock does not move, the heartbeat is
        // 7,200 s old on the wall clock and no activity is seen.
        let clock = 500;
        let lease = Lease::claimed(clock - 10, LIMITS.lease_s);
        let after_sleep = Signals {
            process: Seen::Alive,
            heartbeat_age_s: Some(7_200),
            active_at: Some(clock - 10),
        };
        assert_eq!(
            judge(Holder::Running, lease, clock, &after_sleep, &LIMITS),
            Verdict::Keep
        );
    }

    #[test]
    fn waiting_quota_pauses_the_lease() {
        let lease = Lease::claimed(0, LIMITS.lease_s);
        let gone = Signals {
            process: Seen::Dead,
            heartbeat_age_s: None,
            active_at: None,
        };
        for clock in [0, 121, 100_000] {
            assert_eq!(
                judge(Holder::WaitingQuota, lease, clock, &gone, &LIMITS),
                Verdict::Keep
            );
            assert_eq!(
                judge(Holder::WaitingQuota, lease, clock, &earning(clock), &LIMITS),
                Verdict::Keep
            );
        }
        // An I1 check keeps the lease of a waiting task with no live attempt.
        let t = TaskLeases {
            task_id: "t".into(),
            holder: Some(Holder::WaitingQuota),
            lease_attempt: Some("t/A1".into()),
            live_attempts: vec![],
            ending_attempts: vec![],
        };
        assert_eq!(i1_repairs(&[t]), vec![]);
    }

    #[test]
    fn idle_worker_lease_expires_within_lease_s_of_the_stall_threshold() {
        const PASS_S: i64 = 5;
        let mut lease = Lease::claimed(0, LIMITS.lease_s);
        // Alive, with a fresh heartbeat, and its last activity at the claim.
        let idle = Signals {
            process: Seen::Alive,
            heartbeat_age_s: Some(5),
            active_at: Some(0),
        };
        let mut clock = 0;
        let stalled_at = loop {
            clock += PASS_S;
            match judge(Holder::Running, lease, clock, &idle, &LIMITS) {
                Verdict::Renew { expires_at } => lease.expires_at = expires_at,
                Verdict::Keep => {}
                Verdict::Expire => break clock,
            }
            assert!(clock < 10_000, "the idle lease never expired");
        };
        assert!(
            lease.expires_at <= LIMITS.stall_after_s + LIMITS.lease_s,
            "the last renewal is at the stall threshold: {lease:?}"
        );
        assert!(
            stalled_at <= LIMITS.stall_after_s + LIMITS.lease_s + PASS_S,
            "stalled at {stalled_at}"
        );
    }

    /// A pseudo-random walk with a fixed seed (no `proptest`: NFR-11): a
    /// 64-bit linear congruential generator.
    struct Walk(u64);

    /// The multiplier and the increment of the generator: the constants of
    /// Knuth's MMIX. Any full-period pair works; the walk needs only a
    /// fixed sequence.
    const LCG_MULTIPLIER: u64 = 6_364_136_223_846_793_005;
    const LCG_INCREMENT: u64 = 1_442_695_040_888_963_407;

    impl Walk {
        /// The next value in `0..n`, from the high bits of the state.
        fn pick(&mut self, n: u64) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(LCG_MULTIPLIER)
                .wrapping_add(LCG_INCREMENT);
            (self.0 >> 33) % n
        }
    }

    /// The task states of the walk. `Ready` holds no lease.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum State {
        Ready,
        Held(Holder),
    }

    #[test]
    fn rr2_no_sequence_of_facts_leaves_a_task_held_with_an_expired_lease() {
        let limits = Limits {
            lease_s: 20,
            stall_after_s: 30,
            heartbeat_max_age_s: 10,
        };
        let mut walk = Walk(0x5eed);
        let mut expiries = 0;
        for _ in 0..2_000 {
            let mut clock = 0_i64;
            let mut state = State::Held(Holder::Running);
            let mut lease = Some(Lease::claimed(clock, limits.lease_s));
            let mut active_at = Some(clock);
            for _ in 0..60 {
                // The fleet clock moves by 0 to 2 pass periods of 5 s.
                clock += 5 * walk.pick(3) as i64;
                let process = [Seen::Alive, Seen::Dead, Seen::Unknown][walk.pick(3) as usize];
                let heartbeat_age_s = [Some(0), Some(11), None][walk.pick(3) as usize];
                if walk.pick(2) == 0 {
                    active_at = Some(clock);
                }
                // The other rules of the state machine, each now and then.
                match (state, walk.pick(8)) {
                    (State::Held(Holder::Running), 0) => state = State::Held(Holder::WaitingQuota),
                    (State::Held(Holder::WaitingQuota), 0) | (State::Held(Holder::Stalled), 0) => {
                        state = State::Ready;
                        lease = None;
                    }
                    (State::Ready, _) => {
                        state = State::Held(Holder::Running);
                        lease = Some(Lease::claimed(clock, limits.lease_s));
                        active_at = Some(clock);
                    }
                    _ => {}
                }
                let signals = Signals {
                    process,
                    heartbeat_age_s,
                    active_at,
                };
                if let (State::Held(h), Some(l)) = (state, lease.as_mut()) {
                    match judge(h, *l, clock, &signals, &limits) {
                        Verdict::Renew { expires_at } => l.expires_at = expires_at,
                        Verdict::Keep => {}
                        Verdict::Expire => {
                            expiries += 1;
                            state = State::Held(Holder::Stalled);
                        }
                    }
                }
                if state == State::Held(Holder::Running) {
                    let l = lease.expect("a running task has a lease");
                    assert!(
                        clock <= l.expires_at,
                        "a running task holds an expired lease at {clock}: {l:?}"
                    );
                }
            }
        }
        assert!(expiries > 100, "the walk expired only {expiries} leases");
    }

    #[test]
    fn i1_repairs_each_breach_of_the_lease_part() {
        let held = |lease: Option<&str>, live: &[&str]| TaskLeases {
            task_id: "t".into(),
            holder: Some(Holder::Running),
            lease_attempt: lease.map(String::from),
            live_attempts: live.iter().map(|a| a.to_string()).collect(),
            ending_attempts: vec![],
        };
        assert_eq!(i1_repairs(&[held(Some("t/A1"), &["t/A1"])]), vec![]);
        assert_eq!(
            i1_repairs(&[held(None, &["t/A1"])]),
            vec![Repair::Write {
                task_id: "t".into(),
                attempt_id: "t/A1".into()
            }]
        );
        assert_eq!(
            i1_repairs(&[held(Some("t/A1"), &["t/A2"])]),
            vec![Repair::Write {
                task_id: "t".into(),
                attempt_id: "t/A2".into()
            }]
        );
        assert_eq!(
            i1_repairs(&[held(Some("t/A1"), &[])]),
            vec![Repair::Delete {
                task_id: "t".into(),
                attempt_id: "t/A1".into()
            }]
        );
        assert_eq!(
            i1_repairs(&[held(Some("t/A1"), &["t/A1", "t/A2"])]),
            vec![Repair::TwoLive {
                task_id: "t".into(),
                attempts: vec!["t/A1".into(), "t/A2".into()]
            }]
        );
        let not_held = TaskLeases {
            holder: None,
            ..held(Some("t/A1"), &["t/A1"])
        };
        assert_eq!(
            i1_repairs(&[not_held]),
            vec![Repair::Delete {
                task_id: "t".into(),
                attempt_id: "t/A1".into()
            }]
        );
    }

    #[test]
    fn i1_does_not_count_an_ending_attempt() {
        // A handoff ended the worker of t/A1, and a new worker claimed t/A2.
        let claimed = TaskLeases {
            task_id: "t".into(),
            holder: Some(Holder::Running),
            lease_attempt: Some("t/A2".into()),
            live_attempts: vec!["t/A2".into()],
            ending_attempts: vec!["t/A1".into()],
        };
        assert_eq!(i1_repairs(&[claimed]), vec![]);
        // The worker of the lease ended, and the pass did not reap the
        // attempt yet: the lease stays, so it can still expire.
        let reaping = TaskLeases {
            task_id: "t".into(),
            holder: Some(Holder::Running),
            lease_attempt: Some("t/A1".into()),
            live_attempts: vec![],
            ending_attempts: vec!["t/A1".into()],
        };
        assert_eq!(i1_repairs(&[reaping]), vec![]);
    }

    #[test]
    fn holder_reads_the_4_held_words() {
        for (word, h) in [
            ("leased", Some(Holder::Leased)),
            ("running", Some(Holder::Running)),
            ("stalled", Some(Holder::Stalled)),
            ("waiting_quota", Some(Holder::WaitingQuota)),
            ("verifying", None),
            ("ready", None),
            ("done", None),
        ] {
            assert_eq!(Holder::of(word), h, "{word}");
        }
    }

    /// Ticket 33: a `done` task holds a lease only with a branch to repair.
    #[test]
    fn done_task_holds_a_lease_only_with_a_branch_to_repair() {
        assert_eq!(Holder::of_task("done", true), Some(Holder::Repair));
        assert_eq!(Holder::of_task("done", false), None);
        assert_eq!(Holder::of_task("running", true), Some(Holder::Running));
        assert_eq!(Holder::of_task("cancelled", true), None);
    }

    /// Ticket 33: the lease of a repair attempt expires like the lease of a
    /// `running` task.
    #[test]
    fn repair_lease_expires_like_a_running_lease() {
        let lease = Lease::claimed(0, LIMITS.lease_s);
        let dead = Signals {
            process: Seen::Dead,
            heartbeat_age_s: None,
            active_at: None,
        };
        assert_eq!(
            judge(Holder::Repair, lease, 120, &dead, &LIMITS),
            Verdict::Keep
        );
        assert_eq!(
            judge(Holder::Repair, lease, 121, &dead, &LIMITS),
            Verdict::Expire
        );
        assert_eq!(
            judge(Holder::Repair, lease, 100, &earning(100), &LIMITS),
            Verdict::Renew { expires_at: 220 }
        );
    }

    /// Ticket 33: a live repair attempt of a `done` task gets its lease, and
    /// a `done` task with no live attempt keeps none.
    #[test]
    fn i1_writes_and_deletes_the_lease_of_a_repair_attempt() {
        let missing = TaskLeases {
            task_id: "t".into(),
            holder: Some(Holder::Repair),
            lease_attempt: None,
            live_attempts: vec!["t/A2".into()],
            ending_attempts: vec![],
        };
        assert_eq!(
            i1_repairs(&[missing]),
            vec![Repair::Write {
                task_id: "t".into(),
                attempt_id: "t/A2".into()
            }]
        );
        let ended = TaskLeases {
            task_id: "t".into(),
            holder: Some(Holder::Repair),
            lease_attempt: Some("t/A2".into()),
            live_attempts: vec![],
            ending_attempts: vec![],
        };
        assert_eq!(
            i1_repairs(&[ended]),
            vec![Repair::Delete {
                task_id: "t".into(),
                attempt_id: "t/A2".into()
            }]
        );
    }
}
