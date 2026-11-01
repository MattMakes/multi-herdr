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
    fn check_lists_every_word() {
        assert_eq!(
            Origin::check("origin"),
            "CHECK (origin IN ('native', 'import'))"
        );
    }
}
