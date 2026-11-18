//! The state machine of a feature (ticket 36).
//!
//! `docs/specs/fleetd/design.md`, "The feature state machine", "Between
//! features" (The full gate; Merged), I8; `docs/tickets/fleetd-p4/MAP.md`,
//! Q20: in Phases 4 and 5 a feature is 1 run, and "each idea is
//! `integrating`, `cancelled` or `parked`" is "each task of the run is
//! `done` or `cancelled`".
//!
//! [`next`] gives the move of a feature from the facts that fleetd read
//! for it. `merged` and `abandoned` are terminal: fleetd decides `merged`
//! 1 time and reads no fact of a terminal feature again (I8).

/// The state of a feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureState {
    Open,
    Ready,
    Merged,
    Abandoned,
}

impl FeatureState {
    /// The word of the state in the ledger.
    pub fn as_str(self) -> &'static str {
        match self {
            FeatureState::Open => "open",
            FeatureState::Ready => "ready",
            FeatureState::Merged => "merged",
            FeatureState::Abandoned => "abandoned",
        }
    }

    /// The state of the word `word`.
    pub fn parse(word: &str) -> Option<FeatureState> {
        [
            FeatureState::Open,
            FeatureState::Ready,
            FeatureState::Merged,
            FeatureState::Abandoned,
        ]
        .into_iter()
        .find(|s| s.as_str() == word)
    }

    /// Whether the state is terminal: fleetd reads no fact for it.
    pub fn is_terminal(self) -> bool {
        matches!(self, FeatureState::Merged | FeatureState::Abandoned)
    }
}

/// The verdict of the full gate on the current feature tip over the
/// current primary tip. A verdict on a tip that moved is not this verdict:
/// it is stale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullGate {
    /// No verdict on these tips yet.
    NotJudged,
    Passed,
    Failed,
}

/// The facts of 1 feature that is not terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Facts {
    /// Each task of the run is `done` or `cancelled` (Q20).
    pub tasks_settled: bool,
    /// Each task branch of the feature is `landed` or `abandoned`.
    pub branches_settled: bool,
    /// At least 1 task branch of the feature is `landed`. A feature with no
    /// landed branch has no changes of its own: it is not ready, and its
    /// tip in the primary branch is not a merge.
    pub has_landed_work: bool,
    pub full_gate: FullGate,
    /// git shows the changes of the feature tip in the primary tip by
    /// content (I8).
    pub changes_in_primary: bool,
}

impl Facts {
    /// The feature waits only for its full gate.
    fn settled(&self) -> bool {
        self.tasks_settled && self.branches_settled && self.has_landed_work
    }
}

/// What fleetd does with a feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    Stay,
    Enter(FeatureState),
    /// Queue the full gate on the current tips. The feature stays `open`.
    RunFullGate,
}

/// The move of a feature in `state` with the facts `facts`.
///
/// - `open` or `ready`, and its changes are in the primary tip → `merged`.
/// - `open`: settled and the full gate passed → `ready`; settled and no
///   verdict → run the full gate.
/// - `ready`: no longer settled, or no pass on the current tips (a tip
///   moved) → `open`.
pub fn next(state: FeatureState, facts: &Facts) -> Next {
    if state.is_terminal() {
        return Next::Stay;
    }
    if facts.has_landed_work && facts.changes_in_primary {
        return Next::Enter(FeatureState::Merged);
    }
    let settled = facts.settled();
    match state {
        FeatureState::Open if settled => match facts.full_gate {
            FullGate::Passed => Next::Enter(FeatureState::Ready),
            FullGate::NotJudged => Next::RunFullGate,
            FullGate::Failed => Next::Stay,
        },
        FeatureState::Ready if !settled || facts.full_gate != FullGate::Passed => {
            Next::Enter(FeatureState::Open)
        }
        _ => Next::Stay,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: FeatureState = FeatureState::Open;

    fn facts() -> Facts {
        Facts {
            tasks_settled: true,
            branches_settled: true,
            has_landed_work: true,
            full_gate: FullGate::Passed,
            changes_in_primary: false,
        }
    }

    #[test]
    fn words_round_trip() {
        for w in ["open", "ready", "merged", "abandoned"] {
            assert_eq!(FeatureState::parse(w).unwrap().as_str(), w);
        }
        assert_eq!(FeatureState::parse("landed"), None);
    }

    #[test]
    fn open_is_ready_only_when_settled_and_passed() {
        assert_eq!(next(S, &facts()), Next::Enter(FeatureState::Ready));
        let gate = |g| Facts {
            full_gate: g,
            ..facts()
        };
        assert_eq!(next(S, &gate(FullGate::NotJudged)), Next::RunFullGate);
        assert_eq!(next(S, &gate(FullGate::Failed)), Next::Stay);
        for f in [
            Facts {
                tasks_settled: false,
                ..facts()
            },
            Facts {
                branches_settled: false,
                ..facts()
            },
            Facts {
                has_landed_work: false,
                ..facts()
            },
        ] {
            assert_eq!(next(S, &f), Next::Stay);
            assert_eq!(
                next(FeatureState::Ready, &f),
                Next::Enter(FeatureState::Open)
            );
        }
    }

    #[test]
    fn ready_goes_back_to_open_without_a_pass_on_its_tips() {
        let r = FeatureState::Ready;
        assert_eq!(next(r, &facts()), Next::Stay);
        for g in [FullGate::NotJudged, FullGate::Failed] {
            let f = Facts {
                full_gate: g,
                ..facts()
            };
            assert_eq!(next(r, &f), Next::Enter(FeatureState::Open));
        }
    }

    #[test]
    fn merged_needs_landed_work_and_is_terminal() {
        let merged = Facts {
            changes_in_primary: true,
            tasks_settled: false,
            full_gate: FullGate::NotJudged,
            ..facts()
        };
        for s in [FeatureState::Open, FeatureState::Ready] {
            assert_eq!(next(s, &merged), Next::Enter(FeatureState::Merged));
        }
        let empty = Facts {
            has_landed_work: false,
            ..merged
        };
        assert_eq!(next(S, &empty), Next::Stay);
        for s in [FeatureState::Merged, FeatureState::Abandoned] {
            assert!(s.is_terminal());
            assert_eq!(next(s, &facts()), Next::Stay);
            assert_eq!(next(s, &empty), Next::Stay);
        }
    }
}
