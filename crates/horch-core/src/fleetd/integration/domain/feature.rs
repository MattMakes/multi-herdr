#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureState {
    Open,
    Ready,
    Merged,
    Abandoned,
}

impl FeatureState {
    pub fn as_str(self) -> &'static str {
        match self {
            FeatureState::Open => "open",
            FeatureState::Ready => "ready",
            FeatureState::Merged => "merged",
            FeatureState::Abandoned => "abandoned",
        }
    }

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

    pub fn is_terminal(self) -> bool {
        matches!(self, FeatureState::Merged | FeatureState::Abandoned)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullGate {
    NotJudged,
    Passed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Facts {
    pub ideas_settled: bool,
    pub has_landed_work: bool,
    pub full_gate: FullGate,
    pub changes_in_primary: bool,
}

impl Facts {
    fn settled(&self) -> bool {
        self.ideas_settled && self.has_landed_work
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    Stay,
    Enter(FeatureState),
    RunFullGate,
}

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
            ideas_settled: true,
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
                ideas_settled: false,
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
            ideas_settled: false,
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
