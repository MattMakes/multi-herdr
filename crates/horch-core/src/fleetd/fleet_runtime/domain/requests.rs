//! The request contract: the kind catalog with the class of each kind, the
//! `request_id` rule, the size rule of a line, and the reasons of a refusal.
//! The JSON form of a request and of a reply, and the payload hash, are in
//! `adapters/request_wire.rs`, because a domain module names only `std`.
//!
//! Requirements: RR-8, SR-8, FM-15. Ticket 08 of fleetd-p5.

/// The most bytes of a `request_id`.
pub const REQUEST_ID_MAX_BYTES: usize = 64;

/// The most characters of an untrusted text that a refusal reason repeats.
const ECHO_MAX_CHARS: usize = 64;

/// Who may send a kind. An `operator` kind comes on the channel or in a
/// request file. A `worker` kind is a file of its own attempt only, and
/// fleetd refuses it on the channel and in a request file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Operator,
    Worker,
}

impl Class {
    pub fn as_str(self) -> &'static str {
        match self {
            Class::Operator => "operator",
            Class::Worker => "worker",
        }
    }
}

/// Each request kind of Phase 5: the table "Request kinds" of
/// `docs/tickets/fleetd-p5/MAP.md`, 1 kind for each command that changes
/// state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    RunNew,
    RunClose,
    RunGate,
    TaskAdd,
    MapAdd,
    TaskSolo,
    PlanWorkers,
    LedgerImport,
    LedgerGc,
    TaskRequeue,
    TaskCancel,
    TaskUnblock,
    TaskFallback,
    LedgerWaive,
    LedgerReopen,
    LedgerDismiss,
    WorkerStop,
    WorkerReap,
    WorkerNew,
    WorkerPid,
    Spawn,
    OrchestratorClaim,
    OrchestratorStop,
    JobRequeue,
    WorktreeScratch,
    WorktreeDrop,
    WorktreeRestore,
    WorktreeTriage,
    FeatureAbandon,
    BranchAbandon,
    RescueDrop,
    Progress,
    Result,
    Ask,
}

impl Kind {
    /// Each kind, in the order of the MAP table.
    pub const ALL: [Kind; 34] = [
        Kind::RunNew,
        Kind::RunClose,
        Kind::RunGate,
        Kind::TaskAdd,
        Kind::MapAdd,
        Kind::TaskSolo,
        Kind::PlanWorkers,
        Kind::LedgerImport,
        Kind::LedgerGc,
        Kind::TaskRequeue,
        Kind::TaskCancel,
        Kind::TaskUnblock,
        Kind::TaskFallback,
        Kind::LedgerWaive,
        Kind::LedgerReopen,
        Kind::LedgerDismiss,
        Kind::WorkerStop,
        Kind::WorkerReap,
        Kind::WorkerNew,
        Kind::WorkerPid,
        Kind::Spawn,
        Kind::OrchestratorClaim,
        Kind::OrchestratorStop,
        Kind::JobRequeue,
        Kind::WorktreeScratch,
        Kind::WorktreeDrop,
        Kind::WorktreeRestore,
        Kind::WorktreeTriage,
        Kind::FeatureAbandon,
        Kind::BranchAbandon,
        Kind::RescueDrop,
        Kind::Progress,
        Kind::Result,
        Kind::Ask,
    ];

    /// The name of the kind on the wire and in the `requests` row.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::RunNew => "run_new",
            Kind::RunClose => "run_close",
            Kind::RunGate => "run_gate",
            Kind::TaskAdd => "task_add",
            Kind::MapAdd => "map_add",
            Kind::TaskSolo => "task_solo",
            Kind::PlanWorkers => "plan_workers",
            Kind::LedgerImport => "ledger_import",
            Kind::LedgerGc => "ledger_gc",
            Kind::TaskRequeue => "task_requeue",
            Kind::TaskCancel => "task_cancel",
            Kind::TaskUnblock => "task_unblock",
            Kind::TaskFallback => "task_fallback",
            Kind::LedgerWaive => "ledger_waive",
            Kind::LedgerReopen => "ledger_reopen",
            Kind::LedgerDismiss => "ledger_dismiss",
            Kind::WorkerStop => "worker_stop",
            Kind::WorkerReap => "worker_reap",
            Kind::WorkerNew => "worker_new",
            Kind::WorkerPid => "worker_pid",
            Kind::Spawn => "spawn",
            Kind::OrchestratorClaim => "orchestrator_claim",
            Kind::OrchestratorStop => "orchestrator_stop",
            Kind::JobRequeue => "job_requeue",
            Kind::WorktreeScratch => "worktree_scratch",
            Kind::WorktreeDrop => "worktree_drop",
            Kind::WorktreeRestore => "worktree_restore",
            Kind::WorktreeTriage => "worktree_triage",
            Kind::FeatureAbandon => "feature_abandon",
            Kind::BranchAbandon => "branch_abandon",
            Kind::RescueDrop => "rescue_drop",
            Kind::Progress => "progress",
            Kind::Result => "result",
            Kind::Ask => "ask",
        }
    }

    /// `worker` for the 3 kinds that are files of an attempt, else
    /// `operator`.
    pub fn class(self) -> Class {
        match self {
            Kind::Progress | Kind::Result | Kind::Ask => Class::Worker,
            _ => Class::Operator,
        }
    }

    /// The kind of the wire name `name`. An unknown name is refused, and
    /// the reason repeats the name.
    pub fn parse(name: &str) -> Result<Kind, RequestError> {
        Kind::ALL
            .into_iter()
            .find(|k| k.as_str() == name)
            .ok_or_else(|| RequestError::UnknownKind(echo(name, ECHO_MAX_CHARS)))
    }
}

/// The idempotency key of a request: 1 UUID v4 text that the client makes.
/// The rule: 1 to 64 bytes, each of `[A-Za-z0-9-]`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RequestId(String);

impl RequestId {
    pub fn parse(text: &str) -> Result<RequestId, RequestError> {
        if text.is_empty() {
            return Err(RequestError::BadRequestId("it is empty"));
        }
        if text.len() > REQUEST_ID_MAX_BYTES {
            return Err(RequestError::BadRequestId("it is over 64 bytes"));
        }
        if !text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err(RequestError::BadRequestId(
                "it has a character outside [A-Za-z0-9-]",
            ));
        }
        Ok(RequestId(text.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for RequestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Refuses a line of `bytes` bytes over the limit `max_bytes`
/// (`request_max_bytes` of `fleet.toml`). The caller checks it before it
/// parses the line.
pub fn check_size(bytes: usize, max_bytes: u64) -> Result<(), RequestError> {
    if u64::try_from(bytes).map_or(true, |b| b > max_bytes) {
        return Err(RequestError::TooLarge { bytes, max_bytes });
    }
    Ok(())
}

/// The reason that a request or a reply is refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RequestError {
    /// The line has more bytes than the limit.
    TooLarge { bytes: usize, max_bytes: u64 },
    /// The line is not the JSON form of the contract. The text is safe to
    /// print: control characters are escaped and the length is bounded.
    Malformed(String),
    /// The kind is not in the catalog. The text is the escaped name.
    UnknownKind(String),
    /// The `request_id` breaks its rule. The text says which part.
    BadRequestId(&'static str),
}

impl RequestError {
    /// A `Malformed` error from the untrusted text `text`.
    pub fn malformed(text: &str) -> RequestError {
        RequestError::Malformed(echo(text, 4 * ECHO_MAX_CHARS))
    }
}

impl std::fmt::Display for RequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RequestError::TooLarge { bytes, max_bytes } => {
                write!(
                    f,
                    "request of {bytes} bytes is over the limit of {max_bytes} bytes"
                )
            }
            RequestError::Malformed(text) => write!(f, "malformed request: {text}"),
            RequestError::UnknownKind(name) => write!(f, "unknown request kind `{name}`"),
            RequestError::BadRequestId(why) => write!(f, "bad request_id: {why}"),
        }
    }
}

impl std::error::Error for RequestError {}

/// The untrusted text `text` made safe to print: control characters,
/// quotes and backslashes escaped, and at most `max_chars` characters kept.
fn echo(text: &str, max_chars: usize) -> String {
    let mut out: String = text
        .chars()
        .take(max_chars)
        .flat_map(char::escape_debug)
        .collect();
    if text.chars().nth(max_chars).is_some() {
        out.push_str("...");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The table "Request kinds" of `docs/tickets/fleetd-p5/MAP.md`.
    const MAP_TABLE: [(&str, Class); 34] = [
        ("run_new", Class::Operator),
        ("run_close", Class::Operator),
        ("run_gate", Class::Operator),
        ("task_add", Class::Operator),
        ("map_add", Class::Operator),
        ("task_solo", Class::Operator),
        ("plan_workers", Class::Operator),
        ("ledger_import", Class::Operator),
        ("ledger_gc", Class::Operator),
        ("task_requeue", Class::Operator),
        ("task_cancel", Class::Operator),
        ("task_unblock", Class::Operator),
        ("task_fallback", Class::Operator),
        ("ledger_waive", Class::Operator),
        ("ledger_reopen", Class::Operator),
        ("ledger_dismiss", Class::Operator),
        ("worker_stop", Class::Operator),
        ("worker_reap", Class::Operator),
        ("worker_new", Class::Operator),
        ("worker_pid", Class::Operator),
        ("spawn", Class::Operator),
        ("orchestrator_claim", Class::Operator),
        ("orchestrator_stop", Class::Operator),
        ("job_requeue", Class::Operator),
        ("worktree_scratch", Class::Operator),
        ("worktree_drop", Class::Operator),
        ("worktree_restore", Class::Operator),
        ("worktree_triage", Class::Operator),
        ("feature_abandon", Class::Operator),
        ("branch_abandon", Class::Operator),
        ("rescue_drop", Class::Operator),
        ("progress", Class::Worker),
        ("result", Class::Worker),
        ("ask", Class::Worker),
    ];

    #[test]
    fn each_kind_of_the_map_table_parses_and_has_its_class() {
        for (name, class) in MAP_TABLE {
            let kind = Kind::parse(name).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(kind.as_str(), name);
            assert_eq!(kind.class(), class, "{name}");
        }
        let names: Vec<&str> = Kind::ALL.iter().map(|k| k.as_str()).collect();
        let table: Vec<&str> = MAP_TABLE.iter().map(|(n, _)| *n).collect();
        assert_eq!(names, table, "Kind::ALL is exactly the MAP table");
    }

    #[test]
    fn unknown_kind_is_refused_with_its_name() {
        let err = Kind::parse("idea_add").unwrap_err();
        assert_eq!(err, RequestError::UnknownKind("idea_add".into()));
        assert!(err.to_string().contains("idea_add"), "{err}");
        assert!(Kind::parse("").is_err());
        assert!(Kind::parse("Run_New").is_err(), "names are exact");
        let err = Kind::parse("x\u{1b}[2J\n").unwrap_err().to_string();
        assert!(!err.chars().any(char::is_control), "{err:?}");
        let long = "k".repeat(10_000);
        assert!(Kind::parse(&long).unwrap_err().to_string().len() < 200);
    }

    #[test]
    fn worker_kind_is_class_worker() {
        for name in ["progress", "result", "ask"] {
            assert_eq!(Kind::parse(name).unwrap().class(), Class::Worker, "{name}");
        }
        let workers = Kind::ALL.iter().filter(|k| k.class() == Class::Worker);
        assert_eq!(workers.count(), 3);
        assert_eq!(Class::Worker.as_str(), "worker");
        assert_eq!(Class::Operator.as_str(), "operator");
    }

    #[test]
    fn request_id_with_a_slash_is_refused() {
        assert!(matches!(
            RequestId::parse("../a/b"),
            Err(RequestError::BadRequestId(_))
        ));
        let uuid = "6f1c2a3e-0b9d-4c55-8e7f-1a2b3c4d5e6f";
        assert_eq!(RequestId::parse(uuid).unwrap().as_str(), uuid);
        assert!(RequestId::parse("").is_err(), "empty");
        assert!(RequestId::parse(&"a".repeat(64)).is_ok());
        assert!(RequestId::parse(&"a".repeat(65)).is_err(), "over 64 bytes");
        for bad in ["a b", "a.b", "a\\b", "a\nb", "a\0b", "é", "a_b"] {
            assert!(RequestId::parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn size_over_the_limit_is_refused() {
        assert_eq!(check_size(64, 64), Ok(()));
        assert_eq!(
            check_size(65, 64),
            Err(RequestError::TooLarge {
                bytes: 65,
                max_bytes: 64
            })
        );
    }
}
