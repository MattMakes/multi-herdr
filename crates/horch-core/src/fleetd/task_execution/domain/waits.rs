#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Child {
    Alive,
    Gone,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Proof {
    pub child: Child,
    pub step_running: bool,
    pub review_live: bool,
}

impl Proof {
    fn holds(self) -> bool {
        self.child == Child::Alive || self.step_running || self.review_live
    }

    fn child_gone_alone(self) -> bool {
        self.child == Child::Gone && !self.step_running && !self.review_live
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Behind {
    Question { open: bool },
    Reviews { working: bool, ended: bool },
    HeldForFinish,
    Background(Proof),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wait {
    pub behind: Behind,
    pub due_at: i64,
    pub deadline_passed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Facts {
    pub clock: i64,
    pub max_wait_s: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    MaxWaitReached,
    QuestionClosed,
    ReviewsDone,
    DeadlinePassed,
    ChildGone,
}

impl End {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Holds { due_at: i64 },
    Ends(End),
}

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
