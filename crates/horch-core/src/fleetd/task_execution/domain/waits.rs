//! The capped waits of a task (FM-3; `docs/tickets/fleetd-p2/MAP.md`, Q23).
//!
//! Facts in, 1 verdict out. No I/O. A declared wait of an interactive
//! persona counts as activity only while something live is behind it, and
//! at most `max_wait_s` on the fleet clock after the last read that saw a
//! proof of work:
//! - a question wait: its `questions` row is `open`;
//! - a reviewers wait: 1 of its review tasks is live;
//! - a background wait: a child of the agent, a running finish step, or a
//!   live review task of the task.
//!
//! Only an interactive attempt declares a background wait. An exec
//! attempt has none: its process runs until it exits, and the wrapper sees
//! the exit (`docs/tickets/fleetd-p7/MAP.md`, Q3; `Mode::has_background_wait`).
//!
//! A read that sees a proof moves `due_at` to the fleet clock plus
//! `max_wait_s`. With no proof, the wait ends at `due_at`. A background
//! wait whose agent child is gone, with no other proof, ends at once
//! (MAP.md, Q23). A question wait
//! and a held reviewers wait have no proof: each ends at its first
//! `due_at`.

/// The child process fact of a background wait: a member of the process
/// group of the agent, other than the agent itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Child {
    Alive,
    Gone,
    /// The platform cannot tell (Windows has no process groups). An
    /// unknown fact is no proof, and ends no wait before its cap.
    Unknown,
}

/// The proof of work behind a background wait, as the read saw it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Proof {
    pub child: Child,
    /// A finish step of the task of the worker is `running`.
    pub step_running: bool,
    /// A review task of the task of the worker is `ready`, `leased` or
    /// `running`.
    pub review_live: bool,
}

impl Proof {
    fn holds(self) -> bool {
        self.child == Child::Alive || self.step_running || self.review_live
    }

    /// The child of the agent is gone, and no other proof holds the wait:
    /// the wait ends at once. An unknown child keeps the wait to its cap.
    fn child_gone_alone(self) -> bool {
        self.child == Child::Gone && !self.step_running && !self.review_live
    }
}

/// What is behind 1 wait.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Behind {
    /// A question wait. `open`: its `questions` row is `open`.
    Question { open: bool },
    /// A reviewers wait with review task ids. `working`: 1 of them is
    /// `ready`, `leased` or `running`, a proof of work. `ended`: each of
    /// them is `done` or `cancelled`. A review in another state (`failed`,
    /// `waiting_quota`) holds the wait only to its cap.
    Reviews { working: bool, ended: bool },
    /// A reviewers wait with no task ids: the reviews are in, and the
    /// author has yet to run `horch ledger finish` (#177).
    HeldForFinish,
    /// A background wait: a long command or a detached finish.
    Background(Proof),
}

/// 1 wait, as the read saw it. Each time is on the fleet clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wait {
    pub behind: Behind,
    /// `worker_waits.due_at`.
    pub due_at: i64,
    /// The calendar `deadline` of the wait has passed.
    pub deadline_passed: bool,
}

/// The facts of the read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Facts {
    /// The fleet clock of the read.
    pub clock: i64,
    /// `liveness.max_wait_s` of the profile of the worker.
    pub max_wait_s: i64,
}

/// Why a wait ends: the cause of its `wait_ended` event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    /// The fleet clock reached `due_at`.
    MaxWaitReached,
    /// The `questions` row is no longer `open`.
    QuestionClosed,
    /// Each review task of the wait has ended.
    ReviewsDone,
    /// The calendar `deadline` of the wait passed first.
    DeadlinePassed,
    /// The child process of the agent behind a background wait is gone,
    /// and no other proof of work holds it (MAP.md, Q23).
    ChildGone,
}

impl End {
    /// The cause word.
    pub fn as_str(self) -> &'static str {
        match self {
            End::MaxWaitReached => "max_wait_reached",
            End::QuestionClosed => "question_closed",
            End::ReviewsDone => "reviews_done",
            End::DeadlinePassed => "deadline_passed",
            End::ChildGone => "child_gone",
        }
    }
}

/// The verdict on 1 wait.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// The wait holds until `due_at`, which a proof can move later.
    Holds {
        due_at: i64,
    },
    Ends(End),
}

/// The verdict on `wait` at the read `facts`.
pub fn judge(wait: &Wait, facts: &Facts) -> Verdict {
    if wait.deadline_passed {
        return Verdict::Ends(End::DeadlinePassed);
    }
    let proof = match wait.behind {
        Behind::Question { open: false } => return Verdict::Ends(End::QuestionClosed),
        Behind::Reviews { ended: true, .. } => return Verdict::Ends(End::ReviewsDone),
        Behind::Question { open: true } | Behind::HeldForFinish => false,
        Behind::Reviews { working, .. } => working,
        Behind::Background(p) if p.child_gone_alone() => return Verdict::Ends(End::ChildGone),
        Behind::Background(p) => p.holds(),
    };
    let due_at = if proof {
        wait.due_at
            .max(facts.clock.saturating_add(facts.max_wait_s))
    } else {
        wait.due_at
    };
    if facts.clock >= due_at {
        return Verdict::Ends(End::MaxWaitReached);
    }
    Verdict::Holds { due_at }
}

/// The fleet clock at which `wait` stops counting as activity, or `None`
/// when it is not valid now. The stall ladder reads it.
pub fn waiting_until(wait: &Wait, facts: &Facts) -> Option<i64> {
    match judge(wait, facts) {
        Verdict::Holds { due_at } => Some(due_at),
        Verdict::Ends(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FACTS: Facts = Facts {
        clock: 1_000,
        max_wait_s: 3_600,
    };

    fn background(child: Child, due_at: i64) -> Wait {
        Wait {
            behind: Behind::Background(Proof {
                child,
                step_running: false,
                review_live: false,
            }),
            due_at,
            deadline_passed: false,
        }
    }

    fn at(clock: i64) -> Facts {
        Facts { clock, ..FACTS }
    }

    #[test]
    fn background_wait_ends_when_the_agent_child_is_gone() {
        let wait = background(Child::Gone, 4_600);
        assert_eq!(
            judge(&wait, &at(1_000)),
            Verdict::Ends(End::ChildGone),
            "the wait ends at once, long before its cap"
        );
        assert_eq!(waiting_until(&wait, &at(1_000)), None);
    }

    #[test]
    fn background_wait_with_a_gone_child_and_a_running_step_still_holds() {
        let wait = Wait {
            behind: Behind::Background(Proof {
                child: Child::Gone,
                step_running: true,
                review_live: false,
            }),
            due_at: 4_600,
            deadline_passed: false,
        };
        assert_eq!(
            judge(&wait, &at(1_000)),
            Verdict::Holds { due_at: 4_600 },
            "the running step is a proof: the wait holds to the clock plus max_wait_s"
        );
    }

    #[test]
    fn background_wait_holds_while_a_child_is_alive_up_to_max_wait_s() {
        let wait = background(Child::Alive, 4_600);
        assert_eq!(
            judge(&wait, &at(4_600)),
            Verdict::Holds { due_at: 8_200 },
            "a live child moves due_at to the clock plus max_wait_s"
        );
        assert_eq!(waiting_until(&wait, &at(10_000)), Some(13_600));
    }

    #[test]
    fn unknown_child_fact_keeps_the_wait_to_its_cap() {
        let wait = background(Child::Unknown, 4_600);
        assert_eq!(waiting_until(&wait, &at(4_000)), Some(4_600));
        assert_eq!(waiting_until(&wait, &at(4_600)), None);
    }

    #[test]
    fn a_running_step_or_a_live_review_is_a_proof() {
        for (step_running, review_live) in [(true, false), (false, true)] {
            let wait = Wait {
                behind: Behind::Background(Proof {
                    child: Child::Gone,
                    step_running,
                    review_live,
                }),
                due_at: 4_600,
                deadline_passed: false,
            };
            assert_eq!(waiting_until(&wait, &at(5_000)), Some(8_600));
        }
    }

    #[test]
    fn a_question_wait_ends_when_its_row_closes_or_at_its_cap() {
        let open = Wait {
            behind: Behind::Question { open: true },
            due_at: 4_600,
            deadline_passed: false,
        };
        assert_eq!(waiting_until(&open, &FACTS), Some(4_600));
        assert_eq!(judge(&open, &at(4_600)), Verdict::Ends(End::MaxWaitReached));
        let closed = Wait {
            behind: Behind::Question { open: false },
            ..open
        };
        assert_eq!(judge(&closed, &FACTS), Verdict::Ends(End::QuestionClosed));
    }

    #[test]
    fn a_reviewers_wait_holds_while_a_review_is_live() {
        let live = Wait {
            behind: Behind::Reviews {
                working: true,
                ended: false,
            },
            due_at: 4_600,
            deadline_passed: false,
        };
        assert_eq!(waiting_until(&live, &at(9_000)), Some(12_600));
        let done = Wait {
            behind: Behind::Reviews {
                working: false,
                ended: true,
            },
            ..live
        };
        assert_eq!(judge(&done, &FACTS), Verdict::Ends(End::ReviewsDone));
        let held = Wait {
            behind: Behind::HeldForFinish,
            ..live
        };
        assert_eq!(judge(&held, &at(4_600)), Verdict::Ends(End::MaxWaitReached));
    }

    #[test]
    fn a_review_that_does_not_work_holds_the_wait_only_to_its_cap() {
        let failed = Wait {
            behind: Behind::Reviews {
                working: false,
                ended: false,
            },
            due_at: 4_600,
            deadline_passed: false,
        };
        assert_eq!(waiting_until(&failed, &at(4_000)), Some(4_600));
        assert_eq!(
            judge(&failed, &at(4_600)),
            Verdict::Ends(End::MaxWaitReached)
        );
    }

    #[test]
    fn a_passed_deadline_ends_even_a_wait_with_a_proof() {
        let wait = Wait {
            deadline_passed: true,
            ..background(Child::Alive, 4_600)
        };
        assert_eq!(judge(&wait, &FACTS), Verdict::Ends(End::DeadlinePassed));
    }
}
