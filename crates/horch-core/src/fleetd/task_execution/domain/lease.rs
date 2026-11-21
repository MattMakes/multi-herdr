use super::task::Seen;

pub const CLAIM_BASIS: &str = "claim";

pub const EARNED_BASIS: &str = "process,heartbeat,activity";

pub const I1_BASIS: &str = "i1_repair";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Holder {
    Leased,
    Running,
    Stalled,
    WaitingQuota,
    Repair,
}

impl Holder {
    pub const ALL: [Holder; 4] = [
        Holder::Leased,
        Holder::Running,
        Holder::Stalled,
        Holder::WaitingQuota,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Holder::Leased => "leased",
            Holder::Running => "running",
            Holder::Stalled => "stalled",
            Holder::WaitingQuota => "waiting_quota",
            Holder::Repair => "done",
        }
    }

    pub fn of(word: &str) -> Option<Holder> {
        Self::ALL.into_iter().find(|h| h.as_str() == word)
    }

    pub fn of_task(word: &str, repair: bool) -> Option<Holder> {
        Holder::of(word)
            .or_else(|| (repair && word == Holder::Repair.as_str()).then_some(Holder::Repair))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub lease_s: i64,
    pub stall_after_s: i64,
    pub heartbeat_max_age_s: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Signals {
    pub process: Seen,
    pub heartbeat_age_s: Option<i64>,
    pub active_at: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lease {
    pub expires_at: i64,
}

impl Lease {
    pub fn claimed(clock: i64, lease_s: i64) -> Lease {
        Lease {
            expires_at: clock + lease_s,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Renew { expires_at: i64 },
    Keep,
    Expire,
}

fn earned(clock: i64, s: &Signals, l: &Limits) -> bool {
    s.process == Seen::Alive
        && s.heartbeat_age_s
            .is_some_and(|age| age <= l.heartbeat_max_age_s)
        && s.active_at.is_some_and(|at| clock - at <= l.stall_after_s)
}

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskLeases {
    pub task_id: String,
    pub holder: Option<Holder>,
    pub lease_attempt: Option<String>,
    pub live_attempts: Vec<String>,
    pub ending_attempts: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Repair {
    Write {
        task_id: String,
        attempt_id: String,
    },
    Delete {
        task_id: String,
        attempt_id: String,
    },
    TwoLive {
        task_id: String,
        attempts: Vec<String>,
    },
}

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
        assert_eq!(
            judge(Holder::Running, lease, 121, &earning(121), &LIMITS),
            Verdict::Expire
        );
    }

    #[test]
    fn sleep_does_not_expire_a_lease() {
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

    struct Walk(u64);

    const LCG_MULTIPLIER: u64 = 6_364_136_223_846_793_005;
    const LCG_INCREMENT: u64 = 1_442_695_040_888_963_407;

    impl Walk {
        fn pick(&mut self, n: u64) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(LCG_MULTIPLIER)
                .wrapping_add(LCG_INCREMENT);
            (self.0 >> 33) % n
        }
    }

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
                clock += 5 * walk.pick(3) as i64;
                let process = [Seen::Alive, Seen::Dead, Seen::Unknown][walk.pick(3) as usize];
                let heartbeat_age_s = [Some(0), Some(11), None][walk.pick(3) as usize];
                if walk.pick(2) == 0 {
                    active_at = Some(clock);
                }
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
        let claimed = TaskLeases {
            task_id: "t".into(),
            holder: Some(Holder::Running),
            lease_attempt: Some("t/A2".into()),
            live_attempts: vec!["t/A2".into()],
            ending_attempts: vec!["t/A1".into()],
        };
        assert_eq!(i1_repairs(&[claimed]), vec![]);
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

    #[test]
    fn done_task_holds_a_lease_only_with_a_branch_to_repair() {
        assert_eq!(Holder::of_task("done", true), Some(Holder::Repair));
        assert_eq!(Holder::of_task("done", false), None);
        assert_eq!(Holder::of_task("running", true), Some(Holder::Running));
        assert_eq!(Holder::of_task("cancelled", true), None);
    }

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
