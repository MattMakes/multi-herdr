//! The words the task ledger stores: every enum column's values.
//!
//! Each enum is the one source of its column's `CHECK` list in the schema
//! ([`super::schema`]), so the code and the database cannot disagree.

use anyhow::{bail, Result};

macro_rules! ledger_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident = $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            /// Every value, in declaration order.
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            /// The stored word.
            pub fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $text),+
                }
            }

            /// The value a stored word names.
            pub fn parse(text: &str) -> Result<$name> {
                match text {
                    $($text => Ok($name::$variant),)+
                    other => bail!(
                        "'{other}' is not a {}; expected one of: {}",
                        stringify!($name),
                        [$($text),+].join(", ")
                    ),
                }
            }

            /// `CHECK (<column> IN ('a', 'b', ...))`.
            pub fn check(column: &str) -> String {
                let words: Vec<String> = [$($text),+].iter().map(|w| format!("'{w}'")).collect();
                format!("CHECK ({column} IN ({}))", words.join(", "))
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

ledger_enum!(
    /// Where a row came from: written by this ledger, or imported (section 11).
    Origin { Native = "native", Import = "import" }
);

ledger_enum!(
    /// `runs.status`.
    RunStatus { Active = "active", Done = "done", Abandoned = "abandoned" }
);

ledger_enum!(
    /// `tasks.status`. "Blocked" is not stored: it is derived from `task_deps`.
    TaskStatus {
        Queued = "queued",
        Running = "running",
        Landing = "landing",
        Done = "done",
        Cancelled = "cancelled",
    }
);

ledger_enum!(
    /// `tasks.kind`: ordinary work, or a code-sweep reviewer task that changes
    /// no code and so runs no finish steps.
    TaskKind { Work = "work", Review = "review" }
);

ledger_enum!(
    /// `tasks.work_kind`: the ticket's `**Kind:**` field (trace-fixes C1).
    /// `tasks.kind` already means work vs review.
    WorkKind {
        Feature = "feature",
        Quality = "quality",
        Refactor = "refactor",
        Security = "security",
        Docs = "docs",
        Report = "report",
    }
);

ledger_enum!(
    /// `tasks.review_mode`: the ticket's `**Review:**` field (trace-fixes C1).
    ReviewMode { NoReview = "none", Light = "light", Full = "full" }
);

ledger_enum!(
    /// `tasks.held_reason` words that horch acts on. The column is free text;
    /// `unlanded` marks a task parked with its worktree (trace-fixes D16).
    HeldReason { Unlanded = "unlanded" }
);

ledger_enum!(
    /// `workers.end_reason` words that horch acts on. The column is free
    /// text; `handed_off` marks a worker that ended through
    /// `horch ledger handoff` (trace-fixes C4).
    EndReason { HandedOff = "handed_off" }
);

ledger_enum!(
    /// `outbox.event`: the wakes the task ledger writes for the orchestrator
    /// (trace-fixes C3).
    OutboxEvent {
        Startable = "startable",
        Settled = "settled",
        Reaped = "reaped",
        Held = "held",
        Cancelled = "cancelled",
        ReviewsIn = "reviews-in",
        NoTask = "no-task",
        UnclaimedSpawn = "unclaimed-spawn",
        OrchestratorRespawned = "orchestrator-respawned",
        // A ruling horch asks the orchestrator for (code-sweep TRF-27).
        Question = "question",
    }
);

ledger_enum!(
    /// `tasks.worktree_mode`.
    WorktreeMode { Shared = "shared", Worktree = "worktree" }
);

ledger_enum!(
    /// `task_files.mode`.
    FileMode { Own = "own", DoNotTouch = "do_not_touch" }
);

ledger_enum!(
    /// `workers.kind`.
    WorkerKind { Worker = "worker", Orchestrator = "orchestrator" }
);

ledger_enum!(
    /// `workers.status`.
    WorkerStatus {
        Spawning = "spawning",
        Starting = "starting",
        Claiming = "claiming",
        Working = "working",
        Done = "done",
        NoTask = "no_task",
        Incomplete = "incomplete",
        Stopped = "stopped",
        LaunchFailed = "launch_failed",
    }
);

impl WorkerStatus {
    /// Whether a worker in this state may still run.
    pub fn is_live(self) -> bool {
        matches!(
            self,
            WorkerStatus::Spawning
                | WorkerStatus::Starting
                | WorkerStatus::Claiming
                | WorkerStatus::Working
        )
    }
}

ledger_enum!(
    /// `workers.last_progress_source`: why `last_progress_at` last moved.
    ProgressSource {
        Agent = "agent",
        Transcript = "transcript",
        HerdrState = "herdr_state",
        ScreenTimer = "screen_timer",
    }
);

ledger_enum!(
    /// `checks.verdict`.
    Verdict {
        Progress = "progress",
        ProxyCheckin = "proxy_checkin",
        Suspect = "suspect",
        NoProgress = "no_progress",
        Dead = "dead",
    }
);

ledger_enum!(
    /// `stops.requested_by`.
    StopRequester {
        Orchestrator = "orchestrator",
        Watchdog = "watchdog",
        SelfExit = "self",
        Operator = "operator",
    }
);

ledger_enum!(
    /// `stops.target`.
    StopTarget { Agent = "agent", Supervisor = "supervisor", Pane = "pane" }
);

ledger_enum!(
    /// `stops.signal`.
    StopSignal { Term = "TERM", Kill = "KILL" }
);

ledger_enum!(
    /// `stops.result`.
    StopResult {
        Sent = "sent",
        AlreadyGone = "already_gone",
        NotSameProcess = "not_same_process",
        RefusedOtherHost = "refused_other_host",
        PaneClosed = "pane_closed",
        Error = "error",
    }
);

ledger_enum!(
    /// `worktrees.status`.
    WorktreeStatus {
        Open = "open",
        Landing = "landing",
        Landed = "landed",
        CleanupFailed = "cleanup_failed",
        Cleaned = "cleaned",
        Unlanded = "unlanded",
        Abandoned = "abandoned",
        // A triage decision freed the folder; a ref holds the work (#308).
        Archived = "archived",
        Trashed = "trashed",
    }
);

ledger_enum!(
    /// `worktrees.decision`: what the operator decided at a worktree triage
    /// (#308).
    TriageDecision {
        Land = "LAND",
        Pr = "PR",
        Finish = "FINISH",
        Archive = "ARCHIVE",
        Keep = "KEEP",
        Delete = "DELETE",
    }
);

ledger_enum!(
    /// `events.actor`.
    Actor {
        Orchestrator = "orchestrator",
        Worker = "worker",
        Supervisor = "supervisor",
        Watchdog = "watchdog",
        Horch = "horch",
        Operator = "operator",
    }
);

ledger_enum!(
    /// `events.kind`.
    EventKind {
        RunCreated = "run_created",
        Added = "added",
        Claimed = "claimed",
        NoTask = "no_task",
        Progress = "progress",
        Note = "note",
        Question = "question",
        Blocked = "blocked",
        Answered = "answered",
        Fallback = "fallback",
        StepStarted = "step_started",
        StepPassed = "step_passed",
        StepFailed = "step_failed",
        LandStarted = "land_started",
        LandRed = "land_red",
        Landed = "landed",
        PrUpdated = "pr_updated",
        Cleaned = "cleaned",
        CleanupFailed = "cleanup_failed",
        Done = "done",
        Unlanded = "unlanded",
        Reaped = "reaped",
        Requeued = "requeued",
        Stopped = "stopped",
        Session = "session",
        Exited = "exited",
        Cancelled = "cancelled",
        Verdict = "verdict",
        Woke = "woke",
        Imported = "imported",
        Reopened = "reopened",
        Dismissed = "dismissed",
    }
);

ledger_enum!(
    /// `task_steps.status`.
    StepStatus {
        Pending = "pending",
        Running = "running",
        Passed = "passed",
        Failed = "failed",
        NotApplicable = "not_applicable",
    }
);

impl StepStatus {
    /// Whether the step lets the pipeline go on.
    pub fn is_through(self) -> bool {
        matches!(self, StepStatus::Passed | StepStatus::NotApplicable)
    }
}

ledger_enum!(
    /// The finish steps (section 5.5), in their default order.
    StepName {
        Check = "check",
        CodeSweep = "code-sweep",
        ComplexitySweep = "complexity-sweep",
        Recheck = "recheck",
        Land = "land",
        Pr = "pr",
        Cleanup = "cleanup",
    }
);

ledger_enum!(
    /// `import_raw.source_kind`.
    SourceKind {
        SessionLedger = "session_ledger",
        WorktreeClaims = "worktree_claims",
        PendingMail = "pending_mail",
    }
);

ledger_enum!(
    /// `import_findings.kind`.
    FindingKind {
        WorktreeUnlanded = "worktree_unlanded",
        WorktreeOrphan = "worktree_orphan",
        DoneNotLanded = "done_not_landed",
        EndedWithoutDone = "ended_without_done",
        DirtyWorktree = "dirty_worktree",
        DiskHeavy = "disk_heavy",
        TaskCancelledOpenWorktree = "task_cancelled_open_worktree",
        ReapedNeverFinished = "reaped_never_finished",
    }
);

ledger_enum!(
    /// `import_findings.status`.
    FindingStatus { Open = "open", Requeued = "requeued", Dismissed = "dismissed" }
);

/// The default finish pipeline of a run.
pub fn default_finish_steps() -> Vec<StepName> {
    StepName::ALL.to_vec()
}

ledger_enum!(
    /// How much a run verifies before a task lands: a named set of finish
    /// steps, picked at `horch fleet` start (`--verify`). Only the steps are
    /// stored; the profile is the name for them.
    VerifyProfile {
        Full = "full",
        Light = "light",
        Minimal = "minimal",
    }
);

/// The variable that carries the profile from `horch fleet` into the
/// orchestrator's pane, where `horch ledger run new` reads it. An operator
/// can also set it in their shell as their own default.
pub const VERIFY_ENV: &str = "HORCH_VERIFY";

impl VerifyProfile {
    /// The profile `horch fleet` and `horch ledger run new` use when none is
    /// named: tests must pass, but no AI reviewer workers are queued.
    pub const DEFAULT: VerifyProfile = VerifyProfile::Light;

    /// The finish steps of the profile, in pipeline order.
    pub fn steps(self) -> Vec<StepName> {
        use StepName::*;
        match self {
            VerifyProfile::Full => default_finish_steps(),
            // No code-sweep, so no recheck: nothing commits between check
            // and land but a fix, and a fix re-runs check from the top.
            VerifyProfile::Light => vec![Check, ComplexitySweep, Land, Pr, Cleanup],
            VerifyProfile::Minimal => vec![Check, Land, Pr, Cleanup],
        }
    }

    /// 1 line for the operator: what the profile runs before a task lands.
    pub fn summary(self) -> &'static str {
        match self {
            VerifyProfile::Full => {
                "check, code-sweep (AI reviewer workers), complexity-sweep, recheck"
            }
            VerifyProfile::Light => "check and complexity-sweep; no AI reviewer workers",
            VerifyProfile::Minimal => "check only",
        }
    }

    /// The profile whose steps are exactly `steps`, if one is.
    pub fn of_steps(steps: &[StepName]) -> Option<VerifyProfile> {
        VerifyProfile::ALL
            .iter()
            .copied()
            .find(|p| p.steps() == steps)
    }
}

impl std::str::FromStr for VerifyProfile {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        VerifyProfile::parse(s.trim())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_round_trip() {
        for s in TaskStatus::ALL {
            assert_eq!(TaskStatus::parse(s.as_str()).unwrap(), *s);
        }
        for s in StepName::ALL {
            assert_eq!(StepName::parse(s.as_str()).unwrap(), *s);
        }
        assert!(WorkerStatus::parse("dead").is_err());
    }

    #[test]
    fn verify_profiles_keep_pipeline_order_and_always_land() {
        assert_eq!(VerifyProfile::Full.steps(), default_finish_steps());
        for p in VerifyProfile::ALL {
            let steps = p.steps();
            assert_eq!(steps[0], StepName::Check, "{p} gates on the check");
            for tail in [StepName::Land, StepName::Pr, StepName::Cleanup] {
                assert!(steps.contains(&tail), "{p} lacks {tail}");
            }
            let order: Vec<usize> = steps
                .iter()
                .map(|s| StepName::ALL.iter().position(|a| a == s).unwrap())
                .collect();
            assert!(order.windows(2).all(|w| w[0] < w[1]), "{p}: {steps:?}");
            assert_eq!(VerifyProfile::of_steps(&steps), Some(*p));
        }
        assert!(!VerifyProfile::DEFAULT
            .steps()
            .contains(&StepName::CodeSweep));
        assert_eq!(
            " light".parse::<VerifyProfile>().unwrap(),
            VerifyProfile::Light
        );
        assert!("strict".parse::<VerifyProfile>().is_err());
    }

    #[test]
    fn check_lists_every_word() {
        assert_eq!(
            Origin::check("origin"),
            "CHECK (origin IN ('native', 'import'))"
        );
    }
}
