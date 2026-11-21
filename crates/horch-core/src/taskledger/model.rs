use anyhow::{bail, Result};

macro_rules! ledger_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident = $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            pub fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $text),+
                }
            }

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
    Origin { Native = "native", Import = "import" }
);

ledger_enum!(
    RunStatus { Active = "active", Done = "done", Abandoned = "abandoned" }
);

ledger_enum!(
    TaskStatus {
        Queued = "queued",
        Running = "running",
        Landing = "landing",
        Done = "done",
        Cancelled = "cancelled",
    }
);

ledger_enum!(
    TaskKind { Work = "work", Review = "review" }
);

ledger_enum!(
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
    ReviewMode { NoReview = "none", Light = "light", Full = "full" }
);

ledger_enum!(
    HeldReason { Unlanded = "unlanded" }
);

ledger_enum!(
    EndReason { HandedOff = "handed_off" }
);

ledger_enum!(
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
        Question = "question",
    }
);

ledger_enum!(
    WorktreeMode { Shared = "shared", Worktree = "worktree" }
);

ledger_enum!(
    FileMode { Own = "own", DoNotTouch = "do_not_touch" }
);

ledger_enum!(
    WorkerKind { Worker = "worker", Orchestrator = "orchestrator" }
);

ledger_enum!(
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
    ProgressSource {
        Agent = "agent",
        Transcript = "transcript",
        HerdrState = "herdr_state",
        ScreenTimer = "screen_timer",
    }
);

ledger_enum!(
    Verdict {
        Progress = "progress",
        ProxyCheckin = "proxy_checkin",
        Suspect = "suspect",
        NoProgress = "no_progress",
        Dead = "dead",
    }
);

ledger_enum!(
    StopRequester {
        Orchestrator = "orchestrator",
        Watchdog = "watchdog",
        SelfExit = "self",
        Operator = "operator",
    }
);

ledger_enum!(
    StopTarget { Agent = "agent", Supervisor = "supervisor", Pane = "pane" }
);

ledger_enum!(
    StopSignal { Term = "TERM", Kill = "KILL" }
);

ledger_enum!(
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
    WorktreeStatus {
        Open = "open",
        Landing = "landing",
        Landed = "landed",
        CleanupFailed = "cleanup_failed",
        Cleaned = "cleaned",
        Unlanded = "unlanded",
        Abandoned = "abandoned",
        Archived = "archived",
        Trashed = "trashed",
    }
);

ledger_enum!(
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
    StepStatus {
        Pending = "pending",
        Running = "running",
        Passed = "passed",
        Failed = "failed",
        NotApplicable = "not_applicable",
    }
);

impl StepStatus {
    pub fn is_through(self) -> bool {
        matches!(self, StepStatus::Passed | StepStatus::NotApplicable)
    }
}

ledger_enum!(
    StepName {
        Check = "check",
        CommentSweep = "comment-sweep",
        CodeSweep = "code-sweep",
        ComplexitySweep = "complexity-sweep",
        Recheck = "recheck",
        Land = "land",
        Pr = "pr",
        Cleanup = "cleanup",
    }
);

ledger_enum!(
    SourceKind {
        SessionLedger = "session_ledger",
        WorktreeClaims = "worktree_claims",
        PendingMail = "pending_mail",
    }
);

ledger_enum!(
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
    FindingStatus { Open = "open", Requeued = "requeued", Dismissed = "dismissed" }
);

pub fn default_finish_steps() -> Vec<StepName> {
    StepName::ALL.to_vec()
}

ledger_enum!(
    VerifyProfile {
        Full = "full",
        Light = "light",
        Minimal = "minimal",
    }
);

pub const VERIFY_ENV: &str = "HORCH_VERIFY";

impl VerifyProfile {
    pub const DEFAULT: VerifyProfile = VerifyProfile::Light;

    pub fn steps(self) -> Vec<StepName> {
        use StepName::*;
        match self {
            VerifyProfile::Full => default_finish_steps(),
            VerifyProfile::Light => {
                vec![Check, CommentSweep, ComplexitySweep, Land, Pr, Cleanup]
            }
            VerifyProfile::Minimal => vec![Check, CommentSweep, Land, Pr, Cleanup],
        }
    }

    pub fn summary(self) -> &'static str {
        match self {
            VerifyProfile::Full => {
                "check, comment-sweep, code-sweep (AI reviewer workers), complexity-sweep, recheck"
            }
            VerifyProfile::Light => {
                "check, comment-sweep and complexity-sweep; no AI reviewer workers"
            }
            VerifyProfile::Minimal => "check and comment-sweep",
        }
    }

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
