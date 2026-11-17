//! The job kinds, the run number, `abandoned_run_limit` and `job_backoff_s`
//! (`docs/specs/fleetd/design.md`, "Supervision, liveness and
//! reconciliation": "Long work is a job. A pass never waits for it" and
//! "The kinds of job").
//!
//! Each piece of work that can take more than 1 s is a job: a row in
//! `jobs`, then its own process outside the pass. A run is keyed by kind,
//! subject, input key and run number. These are the rules of a run, with
//! no I/O.

use super::clock::FleetSeconds;

/// The class of a job kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Its result depends only on its input key. A `passed` result, and a
    /// `failed` result with a verdict, is used again.
    Pure,
    /// It changes or reads the world outside its input key. Its result is
    /// never used again: each need of it is a new run.
    Effect,
}

/// A kind of job: the `kind` column of `jobs`.
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
}

/// 1 row of the table "The kinds of job" of the design.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KindRow {
    pub kind: Kind,
    /// The word of the `kind` column.
    pub word: &'static str,
    pub class: Class,
    /// What the `subject` column names.
    pub subject: &'static str,
    /// What the `input_key` column holds.
    pub input_key: &'static str,
    /// The phase of the migration plan that gives the kind its executor.
    pub executor_phase: u8,
}

/// The table "The kinds of job", in the order of the design. The phase of
/// each executor comes from the "Migration plan".
pub const KINDS: [KindRow; 14] = [
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
];

impl Kind {
    /// The row of the kind in [`KINDS`].
    pub fn row(self) -> &'static KindRow {
        KINDS
            .iter()
            .find(|r| r.kind == self)
            .expect("each kind has 1 row in KINDS")
    }

    /// The word of the `kind` column.
    pub fn as_str(self) -> &'static str {
        self.row().word
    }

    /// The kind of a `kind` column word.
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

/// The state of a run: the `state` column of `jobs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Queued,
    Running,
    Passed,
    Failed,
    /// Stopped at its time limit, or its process was lost. It has no
    /// verdict.
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

    /// The word of the `state` column.
    pub fn as_str(self) -> &'static str {
        match self {
            State::Queued => "queued",
            State::Running => "running",
            State::Passed => "passed",
            State::Failed => "failed",
            State::Abandoned => "abandoned",
        }
    }

    /// The state of a `state` column word.
    pub fn parse(word: &str) -> Option<State> {
        State::ALL.into_iter().find(|s| s.as_str() == word)
    }
}

/// How a run that ended by itself ended: what its process writes in
/// `result_json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// `passed` or `failed`.
    pub passed: bool,
    /// Whether the run went to its end and the check, the gate or the trial
    /// merge gave the verdict. A `passed` run always has one. A `failed`
    /// run with no verdict (no executor, no check command) is not used
    /// again.
    pub verdict: bool,
    /// What the run did, for a person.
    pub text: String,
}

impl Outcome {
    /// The end state of the run.
    pub fn state(&self) -> State {
        if self.passed {
            State::Passed
        } else {
            State::Failed
        }
    }
}

/// Whether the result of a run of `kind` that ended in `state` is used
/// again for its input key.
pub fn reusable(kind: Kind, state: State, verdict: bool) -> bool {
    kind.class() == Class::Pure && (state == State::Passed || (state == State::Failed && verdict))
}

/// The wait before the next run of a key with `abandoned_runs` abandoned
/// runs in a row (at least 1): the value of `job_backoff_s` for that
/// count, and its last value past the end of the list.
pub fn backoff_s(job_backoff_s: &[u64], abandoned_runs: u64) -> u64 {
    let n = usize::try_from(abandoned_runs.max(1)).unwrap_or(usize::MAX);
    job_backoff_s
        .get(n - 1)
        .or(job_backoff_s.last())
        .copied()
        .unwrap_or(0)
}

/// What follows a run that ended `abandoned`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfterAbandoned {
    /// The next run, not before the end of its backoff.
    Again {
        abandoned_runs: u64,
        not_before: FleetSeconds,
    },
    /// The key is at `abandoned_run_limit`: no new run, and 1 needs-you
    /// item, until `requeue`.
    Held { abandoned_runs: u64 },
}

/// What follows the abandoned end of a run of a key that had
/// `prior_abandoned_runs` abandoned runs in a row before it, at `clock`.
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

/// Whether a key whose last run ended `abandoned` with `abandoned_runs`
/// is held for `requeue`.
pub fn held(abandoned_runs: u64, abandoned_run_limit: u64) -> bool {
    abandoned_runs >= abandoned_run_limit
}

/// The `kind` column of the needs-you item of a held key.
pub const NEEDS_YOU_KIND: &str = "job_abandoned";

/// The key of the needs-you item of a held key.
pub fn needs_you_key(kind: Kind, subject: &str, input_key: &str) -> String {
    format!("{NEEDS_YOU_KIND}:{kind}:{subject}:{input_key}")
}

/// What a pass knows of the process of a `running` run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Process {
    Alive,
    /// No process has the pid and start time.
    Gone,
    /// The scan cannot tell, or did not reach it in this pass.
    Unknown,
}

/// What a pass does with a `running` run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seen {
    /// It ends in this state.
    Ends(State),
    /// It is past its deadline and its process runs: stop it after the
    /// commit. `kill` is true past the deadline plus `stop_grace_s`.
    Stop { kill: bool },
    /// It goes on.
    Runs,
}

/// What a pass does at `clock` with a `running` run whose process is
/// `process`, whose stored result ends it in `result` (when it wrote one),
/// with its `deadline`. A written result wins over each other fact.
pub fn observe(
    process: Process,
    result: Option<State>,
    deadline_at: FleetSeconds,
    stop_grace_s: u64,
    clock: FleetSeconds,
) -> Seen {
    if let Some(state) = result {
        return Seen::Ends(state);
    }
    match process {
        Process::Gone => Seen::Ends(State::Abandoned),
        Process::Alive if clock >= deadline_at => Seen::Stop {
            kill: clock >= deadline(deadline_at, stop_grace_s),
        },
        Process::Alive | Process::Unknown => Seen::Runs,
    }
}

/// The value `seconds` after `start` on the fleet clock: the deadline of a
/// run, or the end of a backoff.
pub fn deadline(start: FleetSeconds, seconds: u64) -> FleetSeconds {
    FleetSeconds(
        start
            .0
            .saturating_add(i64::try_from(seconds).unwrap_or(i64::MAX)),
    )
}

/// The result of a run of a kind with no executor in this phase.
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
        // The kind CHECK of `jobs` (schema v8), in its order.
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
        ];
        assert_eq!(KINDS.map(|r| r.word), words);
        for row in &KINDS {
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
        let past_grace = FleetSeconds(70);
        let ok = Some(State::Passed);
        assert_eq!(
            observe(Process::Gone, ok, d, 20, before),
            Seen::Ends(State::Passed)
        );
        assert_eq!(
            observe(Process::Alive, ok, d, 20, past_grace),
            Seen::Ends(State::Passed)
        );
        assert_eq!(
            observe(Process::Gone, None, d, 20, before),
            Seen::Ends(State::Abandoned)
        );
        assert_eq!(observe(Process::Alive, None, d, 20, before), Seen::Runs);
        assert_eq!(
            observe(Process::Alive, None, d, 20, at),
            Seen::Stop { kill: false }
        );
        assert_eq!(
            observe(Process::Alive, None, d, 20, past_grace),
            Seen::Stop { kill: true }
        );
        assert_eq!(
            observe(Process::Unknown, None, d, 20, past_grace),
            Seen::Runs
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
