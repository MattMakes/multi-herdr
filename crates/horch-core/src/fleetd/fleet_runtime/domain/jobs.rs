use super::clock::FleetSeconds;
use super::pass::{stop_step, StopStep};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Pure,
    Effect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    TaskCheck,
    FastGate,
    FullGate,
    TrialMerge,
    BundleBuild,
    Rebase,
    Rescue,
    Fetch,
    Push,
    WorktreeAdd,
    WorktreeRemove,
    WorktreeList,
    DocsSync,
    Conformance,
    TriageApply,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KindRow {
    pub kind: Kind,
    pub word: &'static str,
    pub class: Class,
    pub subject: &'static str,
    pub input_key: &'static str,
    pub executor_phase: u8,
}

pub const KINDS: [KindRow; 15] = [
    KindRow {
        kind: Kind::TaskCheck,
        word: "task_check",
        class: Class::Pure,
        subject: "the task",
        input_key: "the tip of the task branch and the hash of the check",
        executor_phase: 1,
    },
    KindRow {
        kind: Kind::FastGate,
        word: "fast_gate",
        class: Class::Pure,
        subject: "the bundle",
        input_key: "the candidate tip and the base tip",
        executor_phase: 4,
    },
    KindRow {
        kind: Kind::FullGate,
        word: "full_gate",
        class: Class::Pure,
        subject: "the feature",
        input_key: "the feature tip and the fetched tip of the primary branch",
        executor_phase: 4,
    },
    KindRow {
        kind: Kind::TrialMerge,
        word: "trial_merge",
        class: Class::Pure,
        subject: "the 2 features, or a feature and the primary branch",
        input_key: "the 2 tips",
        executor_phase: 6,
    },
    KindRow {
        kind: Kind::BundleBuild,
        word: "bundle_build",
        class: Class::Effect,
        subject: "the bundle",
        input_key: "the base tip and the tip of each member, in queue order",
        executor_phase: 4,
    },
    KindRow {
        kind: Kind::Rebase,
        word: "rebase",
        class: Class::Effect,
        subject: "the branch",
        input_key: "the tip of the branch and the tip that it moves onto",
        executor_phase: 6,
    },
    KindRow {
        kind: Kind::Rescue,
        word: "rescue",
        class: Class::Effect,
        subject: "the worktree",
        input_key: "the path and its HEAD",
        executor_phase: 3,
    },
    KindRow {
        kind: Kind::Fetch,
        word: "fetch",
        class: Class::Effect,
        subject: "the repository",
        input_key: "the repository and its remote",
        executor_phase: 4,
    },
    KindRow {
        kind: Kind::Push,
        word: "push",
        class: Class::Effect,
        subject: "the feature",
        input_key: "the feature tip and seen_remote_tip",
        executor_phase: 4,
    },
    KindRow {
        kind: Kind::WorktreeAdd,
        word: "worktree_add",
        class: Class::Effect,
        subject: "the worktree",
        input_key: "the path, the branch and the base tip",
        executor_phase: 3,
    },
    KindRow {
        kind: Kind::WorktreeRemove,
        word: "worktree_remove",
        class: Class::Effect,
        subject: "the worktree",
        input_key: "the path",
        executor_phase: 3,
    },
    KindRow {
        kind: Kind::WorktreeList,
        word: "worktree_list",
        class: Class::Effect,
        subject: "the repository",
        input_key: "the repository",
        executor_phase: 3,
    },
    KindRow {
        kind: Kind::DocsSync,
        word: "docs_sync",
        class: Class::Effect,
        subject: "the CLI and its version",
        input_key: "the CLI and its version",
        executor_phase: 7,
    },
    KindRow {
        kind: Kind::Conformance,
        word: "conformance",
        class: Class::Effect,
        subject: "the CLI and its version",
        input_key: "the CLI, its version and the hash of its adapter",
        executor_phase: 7,
    },
    KindRow {
        kind: Kind::TriageApply,
        word: "triage_apply",
        class: Class::Effect,
        subject: "the main checkout of the project",
        input_key: "the ARCHIVE, DELETE, LAND and PR decisions of 1 triage reply",
        executor_phase: 5,
    },
];

impl Kind {
    pub fn row(self) -> &'static KindRow {
        let i = match self {
            Kind::TaskCheck => 0,
            Kind::FastGate => 1,
            Kind::FullGate => 2,
            Kind::TrialMerge => 3,
            Kind::BundleBuild => 4,
            Kind::Rebase => 5,
            Kind::Rescue => 6,
            Kind::Fetch => 7,
            Kind::Push => 8,
            Kind::WorktreeAdd => 9,
            Kind::WorktreeRemove => 10,
            Kind::WorktreeList => 11,
            Kind::DocsSync => 12,
            Kind::Conformance => 13,
            Kind::TriageApply => 14,
        };
        &KINDS[i]
    }

    pub fn as_str(self) -> &'static str {
        self.row().word
    }

    pub fn parse(word: &str) -> Option<Kind> {
        KINDS.iter().find(|r| r.word == word).map(|r| r.kind)
    }

    pub fn class(self) -> Class {
        self.row().class
    }
}

impl std::fmt::Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Queued,
    Running,
    Passed,
    Failed,
    Abandoned,
}

impl State {
    pub const ALL: [State; 5] = [
        State::Queued,
        State::Running,
        State::Passed,
        State::Failed,
        State::Abandoned,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            State::Queued => "queued",
            State::Running => "running",
            State::Passed => "passed",
            State::Failed => "failed",
            State::Abandoned => "abandoned",
        }
    }

    pub fn parse(word: &str) -> Option<State> {
        State::ALL.into_iter().find(|s| s.as_str() == word)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub passed: bool,
    pub verdict: bool,
    pub text: String,
}

impl Outcome {
    pub fn state(&self) -> State {
        if self.passed {
            State::Passed
        } else {
            State::Failed
        }
    }
}

pub fn reusable(kind: Kind, state: State, verdict: bool) -> bool {
    kind.class() == Class::Pure && (state == State::Passed || (state == State::Failed && verdict))
}

pub fn backoff_s(job_backoff_s: &[u64], abandoned_runs: u64) -> u64 {
    let n = usize::try_from(abandoned_runs.max(1)).unwrap_or(usize::MAX);
    job_backoff_s
        .get(n - 1)
        .or(job_backoff_s.last())
        .copied()
        .unwrap_or(0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfterAbandoned {
    Again {
        abandoned_runs: u64,
        not_before: FleetSeconds,
    },
    Held {
        abandoned_runs: u64,
    },
}

pub fn after_abandoned(
    prior_abandoned_runs: u64,
    abandoned_run_limit: u64,
    job_backoff_s: &[u64],
    clock: FleetSeconds,
) -> AfterAbandoned {
    let abandoned_runs = prior_abandoned_runs.saturating_add(1);
    if held(abandoned_runs, abandoned_run_limit) {
        return AfterAbandoned::Held { abandoned_runs };
    }
    AfterAbandoned::Again {
        abandoned_runs,
        not_before: deadline(clock, backoff_s(job_backoff_s, abandoned_runs)),
    }
}

pub fn held(abandoned_runs: u64, abandoned_run_limit: u64) -> bool {
    abandoned_runs >= abandoned_run_limit
}

pub const NEEDS_YOU_KIND: &str = "job_abandoned";

pub fn needs_you_key(kind: Kind, subject: &str, input_key: &str) -> String {
    format!("{NEEDS_YOU_KIND}:{kind}:{subject}:{input_key}")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Process {
    Alive,
    Gone,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seen {
    Ends(State),
    Stop { kill: bool },
    Runs,
}

pub fn observe(
    process: Process,
    result: Option<State>,
    deadline_at: FleetSeconds,
    term_at: Option<FleetSeconds>,
    stop_grace_s: u64,
    clock: FleetSeconds,
) -> Seen {
    if let Some(state) = result {
        return Seen::Ends(state);
    }
    match process {
        Process::Gone => Seen::Ends(State::Abandoned),
        Process::Alive if clock >= deadline_at => match stop_step(term_at, clock, stop_grace_s) {
            StopStep::Polite => Seen::Stop { kill: false },
            StopStep::Wait => Seen::Runs,
            StopStep::Kill => Seen::Stop { kill: true },
        },
        Process::Alive | Process::Unknown => Seen::Runs,
    }
}

pub fn deadline(start: FleetSeconds, seconds: u64) -> FleetSeconds {
    FleetSeconds(
        start
            .0
            .saturating_add(i64::try_from(seconds).unwrap_or(i64::MAX)),
    )
}

pub fn no_executor(kind: Kind) -> Outcome {
    Outcome {
        passed: false,
        verdict: false,
        text: format!(
            "no executor before phase {} for {kind}",
            kind.row().executor_phase
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_of_the_design_has_its_class_and_input_key() {
        let words = [
            "task_check",
            "fast_gate",
            "full_gate",
            "trial_merge",
            "bundle_build",
            "rebase",
            "rescue",
            "fetch",
            "push",
            "worktree_add",
            "worktree_remove",
            "worktree_list",
            "docs_sync",
            "conformance",
            "triage_apply",
        ];
        assert_eq!(KINDS.map(|r| r.word), words);
        for row in &KINDS {
            assert_eq!(row.kind.row(), row, "the row of each kind is its own");
            assert_eq!(Kind::parse(row.word), Some(row.kind));
            assert_eq!(row.kind.as_str(), row.word);
            assert!(!row.subject.is_empty() && !row.input_key.is_empty());
            assert!((1..=7).contains(&row.executor_phase));
        }
        let pure: Vec<&str> = KINDS
            .iter()
            .filter(|r| r.class == Class::Pure)
            .map(|r| r.word)
            .collect();
        assert_eq!(
            pure,
            ["task_check", "fast_gate", "full_gate", "trial_merge"]
        );
        assert_eq!(
            Kind::TaskCheck.row().input_key,
            "the tip of the task branch and the hash of the check"
        );
        assert_eq!(Kind::TrialMerge.row().input_key, "the 2 tips");
        assert_eq!(Kind::WorktreeList.row().input_key, "the repository");
        assert_eq!(Kind::parse("lint"), None);
    }

    #[test]
    fn only_a_pure_verdict_is_used_again() {
        assert!(reusable(Kind::TaskCheck, State::Passed, true));
        assert!(reusable(Kind::TaskCheck, State::Failed, true));
        assert!(!reusable(Kind::TaskCheck, State::Failed, false));
        assert!(!reusable(Kind::TaskCheck, State::Abandoned, false));
        assert!(!reusable(Kind::Push, State::Passed, true));
        assert!(!reusable(Kind::Fetch, State::Failed, true));
    }

    #[test]
    fn backoff_follows_the_count_and_holds_at_the_limit() {
        let b = [30, 120, 480];
        assert_eq!(backoff_s(&b, 1), 30);
        assert_eq!(backoff_s(&b, 2), 120);
        assert_eq!(backoff_s(&b, 3), 480);
        assert_eq!(backoff_s(&b, 9), 480);
        let t = FleetSeconds(100);
        assert_eq!(
            after_abandoned(0, 3, &b, t),
            AfterAbandoned::Again {
                abandoned_runs: 1,
                not_before: FleetSeconds(130)
            }
        );
        assert_eq!(
            after_abandoned(1, 3, &b, t),
            AfterAbandoned::Again {
                abandoned_runs: 2,
                not_before: FleetSeconds(220)
            }
        );
        assert_eq!(
            after_abandoned(2, 3, &b, t),
            AfterAbandoned::Held { abandoned_runs: 3 }
        );
        assert!(held(3, 3) && !held(2, 3));
    }

    #[test]
    fn a_running_run_ends_by_its_result_its_lost_process_or_a_stop() {
        let d = FleetSeconds(50);
        let before = FleetSeconds(49);
        let at = FleetSeconds(50);
        let in_grace = FleetSeconds(69);
        let past_grace = FleetSeconds(70);
        let ok = Some(State::Passed);
        assert_eq!(
            observe(Process::Gone, ok, d, None, 20, before),
            Seen::Ends(State::Passed)
        );
        assert_eq!(
            observe(Process::Alive, ok, d, Some(at), 20, past_grace),
            Seen::Ends(State::Passed)
        );
        assert_eq!(
            observe(Process::Gone, None, d, None, 20, before),
            Seen::Ends(State::Abandoned)
        );
        assert_eq!(
            observe(Process::Alive, None, d, None, 20, before),
            Seen::Runs
        );
        assert_eq!(
            observe(Process::Alive, None, d, None, 20, at),
            Seen::Stop { kill: false }
        );
        assert_eq!(
            observe(Process::Alive, None, d, Some(at), 20, in_grace),
            Seen::Runs
        );
        assert_eq!(
            observe(Process::Alive, None, d, Some(at), 20, past_grace),
            Seen::Stop { kill: true }
        );
        assert_eq!(
            observe(Process::Unknown, None, d, Some(at), 20, past_grace),
            Seen::Runs
        );
    }

    #[test]
    fn a_run_first_seen_past_its_deadline_and_grace_gets_the_term_before_the_kill() {
        let d = FleetSeconds(50);
        let late = FleetSeconds(500);
        assert_eq!(
            observe(Process::Alive, None, d, None, 20, late),
            Seen::Stop { kill: false }
        );
        assert_eq!(
            observe(Process::Alive, None, d, Some(late), 20, FleetSeconds(519)),
            Seen::Runs
        );
        assert_eq!(
            observe(Process::Alive, None, d, Some(late), 20, FleetSeconds(520)),
            Seen::Stop { kill: true }
        );
    }

    #[test]
    fn a_held_key_has_1_needs_you_key() {
        assert_eq!(
            needs_you_key(Kind::TaskCheck, "T-1", "abc"),
            "job_abandoned:task_check:T-1:abc"
        );
        assert_eq!(
            no_executor(Kind::Push).text,
            "no executor before phase 4 for push"
        );
    }
}
