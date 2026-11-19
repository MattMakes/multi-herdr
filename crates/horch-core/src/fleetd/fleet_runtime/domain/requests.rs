//! The request contract: the kind catalog with the class of each kind, the
//! `request_id` rule, the size rule of a line, and the reasons of a refusal.
//! The JSON form of a request and of a reply, and the payload hash, are in
//! `adapters/request_wire.rs`, because a domain module names only `std`.
//!
//! Requirements: RR-8, SR-8, FM-15, FR-10. Ticket 08 of fleetd-p5, and
//! tickets 06 and 47 of fleetd-p6.

use std::path::Path;

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

/// Each request kind: the table "Request kinds" of
/// `docs/tickets/fleetd-p5/MAP.md`, 1 kind for each command that changes
/// state, with the 4 kinds of the records of 1 spawn after `spawn` (ticket
/// 23 of fleetd-p5), the 4 kinds of the last direct writers of the CLI
/// after `rescue_drop` (T-885), then the idea kinds of Phase 6 (design,
/// "Each request kind has a class"). A kind of [`RETIRED`] is not a kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
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
    SpawnAdopt,
    SpawnGateNote,
    WorkerLaunched,
    WorkerLaunchFailed,
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
    WorkerProgress,
    WorkerDone,
    RunAdopt,
    QuestionAnswer,
    Progress,
    Result,
    Ask,
    IdeaAdd,
    IdeaAmend,
    IdeaApprove,
    IdeaRevise,
    IdeaPark,
    IdeaCancel,
    IdeaNote,
}

impl Kind {
    /// Each kind, in the order of the MAP table, then the idea kinds.
    pub const ALL: [Kind; 42] = [
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
        Kind::SpawnAdopt,
        Kind::SpawnGateNote,
        Kind::WorkerLaunched,
        Kind::WorkerLaunchFailed,
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
        Kind::WorkerProgress,
        Kind::WorkerDone,
        Kind::RunAdopt,
        Kind::QuestionAnswer,
        Kind::Progress,
        Kind::Result,
        Kind::Ask,
        Kind::IdeaAdd,
        Kind::IdeaAmend,
        Kind::IdeaApprove,
        Kind::IdeaRevise,
        Kind::IdeaPark,
        Kind::IdeaCancel,
        Kind::IdeaNote,
    ];

    /// The name of the kind on the wire and in the `requests` row.
    pub fn as_str(self) -> &'static str {
        match self {
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
            Kind::SpawnAdopt => "spawn_adopt",
            Kind::SpawnGateNote => "spawn_gate_note",
            Kind::WorkerLaunched => "worker_launched",
            Kind::WorkerLaunchFailed => "worker_launch_failed",
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
            Kind::WorkerProgress => "worker_progress",
            Kind::WorkerDone => "worker_done",
            Kind::RunAdopt => "run_adopt",
            Kind::QuestionAnswer => "question_answer",
            Kind::Progress => "progress",
            Kind::Result => "result",
            Kind::Ask => "ask",
            Kind::IdeaAdd => "idea_add",
            Kind::IdeaAmend => "idea_amend",
            Kind::IdeaApprove => "idea_approve",
            Kind::IdeaRevise => "idea_revise",
            Kind::IdeaPark => "idea_park",
            Kind::IdeaCancel => "idea_cancel",
            Kind::IdeaNote => "idea_note",
        }
    }

    /// `worker` for the 3 kinds that are files of an attempt and for
    /// `idea_note`, a file of a sub-orchestrator in its idea folder, else
    /// `operator`.
    pub fn class(self) -> Class {
        match self {
            Kind::Progress | Kind::Result | Kind::Ask | Kind::IdeaNote => Class::Worker,
            _ => Class::Operator,
        }
    }

    /// The kind of the wire name `name`. A retired name is refused with
    /// its new command. An unknown name is refused, and the reason repeats
    /// the name.
    pub fn parse(name: &str) -> Result<Kind, RequestError> {
        if let Some(r) = retired(name) {
            return Err(RequestError::Retired(r.instead));
        }
        Kind::ALL
            .into_iter()
            .find(|k| k.as_str() == name)
            .ok_or_else(|| RequestError::UnknownKind(echo(name, ECHO_MAX_CHARS)))
    }
}

/// A request kind that Phase 6 retires, the command that sent it, and what
/// you do in its place: the table "Old flow and new" of
/// `docs/tickets/fleetd-p6/MAP.md`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Retired {
    /// The wire name of the kind.
    pub kind: &'static str,
    /// The old command.
    pub command: &'static str,
    /// What you do in its place.
    pub instead: &'static str,
}

impl Retired {
    /// The line that the old command prints: `<command> is retired.
    /// <instead>`.
    pub fn line(&self) -> String {
        format!("{} is retired. {}", self.command, self.instead)
    }
}

/// What you do in place of `horch ledger add` and `horch ledger task`.
const DIRECT: &str =
    "Add a ticket that you wrote: horch idea add --direct <ticket file> [--to <feature>]";

/// Each retired kind (ticket 47 of fleetd-p6). fleetd refuses a request of
/// one with the reason `retired kind: <instead>`: a request file of an old
/// client, written before the update.
pub const RETIRED: [Retired; 7] = [
    Retired {
        kind: "run_new",
        command: "horch ledger run new",
        instead: r#"Add an idea: horch idea add "<idea>""#,
    },
    Retired {
        kind: "run_close",
        command: "horch ledger run close",
        instead: "Merge the pull request of the feature: fleetd sees the merge. To drop a feature: horch feature abandon <feature>",
    },
    Retired {
        kind: "run_gate",
        command: "horch ledger run gate",
        instead: "fleetd runs the full gate of a feature when its task branches are landed",
    },
    Retired {
        kind: "task_add",
        command: "horch ledger add",
        instead: DIRECT,
    },
    Retired {
        kind: "map_add",
        command: "horch ledger add-map",
        instead: r#"Add an idea: horch idea add "<idea>". For 1 ticket that you wrote: horch idea add --direct <ticket file>. To check a map: horch ledger add-map --dry-run <file>"#,
    },
    Retired {
        kind: "task_solo",
        command: "horch ledger task",
        instead: DIRECT,
    },
    Retired {
        kind: "plan_workers",
        command: "horch ledger plan-workers",
        instead: "fleetd dispatches each ready task: horch ideas and horch ledger status show each hold",
    },
];

/// The retired kind of the wire name `kind`, or `None`.
pub fn retired(kind: &str) -> Option<&'static Retired> {
    RETIRED.iter().find(|r| r.kind == kind)
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

/// The payload of `idea_add`: the idea, and the feature that it joins with
/// `--to`, else the project of its new feature. Each field is 1 text:
/// `text`, or `direct` and `sha256`; then `to`, or `project` and
/// `primary_branch`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdeaAdd {
    pub idea: IdeaInput,
    pub target: AddTarget,
}

/// The feature of a new idea.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AddTarget {
    /// No `--to`: a new feature of the project at the absolute root
    /// `project`, from its primary branch `primary_branch`. The command
    /// reads both from the repository of the caller.
    New {
        project: String,
        primary_branch: String,
    },
    /// `--to <feature>`: the id or the name of a feature.
    Join(String),
}

/// What `horch idea add` captures.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdeaInput {
    /// The text of the idea.
    Text(String),
    /// `--direct`: the ticket file that you wrote.
    Direct(DirectTicket),
}

/// A `--direct` ticket file: its absolute path and the lower-case hex
/// sha256 of its bytes. A request line stays small, so it carries the path,
/// and fleetd refuses the request when the file has another sha256 (Q15 of
/// `docs/tickets/fleetd-p5/MAP.md`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectTicket {
    path: String,
    sha256: String,
}

impl DirectTicket {
    pub fn new(path: &str, sha256: &str) -> Result<DirectTicket, RequestError> {
        if !Path::new(path).is_absolute() {
            return Err(RequestError::malformed(
                "the --direct ticket file is not an absolute path",
            ));
        }
        let hex = sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if sha256.len() != 64 || !hex {
            return Err(RequestError::malformed(
                "the sha256 of the --direct ticket file is not 64 lower-case hex characters",
            ));
        }
        Ok(DirectTicket {
            path: path.to_string(),
            sha256: sha256.to_string(),
        })
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

impl IdeaAdd {
    /// The payload `fields`, each a key and its text. It refuses an unknown
    /// key, both `text` and `direct` or neither, `direct` without `sha256`
    /// or `sha256` without `direct`, an empty `to`, `to` with `project` or
    /// `primary_branch`, and with no `to` a missing or relative `project` or
    /// a missing or empty `primary_branch`.
    pub fn from_fields(fields: &[(&str, &str)]) -> Result<IdeaAdd, RequestError> {
        const KEYS: [&str; 6] = [
            "text",
            "direct",
            "sha256",
            "to",
            "project",
            "primary_branch",
        ];
        if let Some((key, _)) = fields.iter().find(|(k, _)| !KEYS.contains(k)) {
            return Err(RequestError::malformed(&format!(
                "idea_add has the unknown payload field `{key}`"
            )));
        }
        let field = |key: &str| fields.iter().find(|(k, _)| *k == key).map(|(_, v)| *v);
        let idea = match (field("text"), field("direct"), field("sha256")) {
            (Some(text), None, None) => IdeaInput::Text(text.to_string()),
            (None, Some(path), Some(sha256)) => IdeaInput::Direct(DirectTicket::new(path, sha256)?),
            (None, Some(_), None) => {
                return Err(RequestError::malformed(
                    "idea_add --direct has no sha256 of its ticket file",
                ))
            }
            _ => {
                return Err(RequestError::malformed(
                    "idea_add needs a text or a --direct ticket file with its sha256, not both",
                ))
            }
        };
        let target = match (field("to"), field("project"), field("primary_branch")) {
            (Some(""), _, _) => {
                return Err(RequestError::malformed("idea_add --to names no feature"))
            }
            (Some(to), None, None) => AddTarget::Join(to.to_string()),
            (Some(_), _, _) => {
                return Err(RequestError::malformed(
                    "idea_add --to joins a feature: it takes no project and no primary_branch",
                ))
            }
            (None, Some(project), Some(branch)) => {
                if !Path::new(project).is_absolute() {
                    return Err(RequestError::malformed(
                        "the project of idea_add is not an absolute path",
                    ));
                }
                if branch.is_empty() {
                    return Err(RequestError::malformed(
                        "idea_add names no primary_branch of its project",
                    ));
                }
                AddTarget::New {
                    project: project.to_string(),
                    primary_branch: branch.to_string(),
                }
            }
            (None, _, _) => return Err(RequestError::malformed(
                "idea_add with no --to needs the project and the primary_branch of its new feature",
            )),
        };
        Ok(IdeaAdd { idea, target })
    }

    /// The payload fields of this request, each a key and its text.
    pub fn fields(&self) -> Vec<(&'static str, &str)> {
        let mut fields = match &self.idea {
            IdeaInput::Text(text) => vec![("text", text.as_str())],
            IdeaInput::Direct(ticket) => {
                vec![("direct", ticket.path()), ("sha256", ticket.sha256())]
            }
        };
        match &self.target {
            AddTarget::New {
                project,
                primary_branch,
            } => {
                fields.push(("project", project.as_str()));
                fields.push(("primary_branch", primary_branch.as_str()));
            }
            AddTarget::Join(to) => fields.push(("to", to.as_str())),
        }
        fields
    }
}

/// The required and the optional payload fields of each kind of the
/// records of 1 spawn (ticket 23 of fleetd-p5). `None` for each other kind.
fn spawn_record_fields(kind: Kind) -> Option<(&'static [&'static str], &'static [&'static str])> {
    match kind {
        Kind::SpawnAdopt => Some((
            &[
                "run_id",
                "worker_id",
                "persona",
                "target_task_id",
                "spawn_pid",
            ],
            &[],
        )),
        Kind::SpawnGateNote => Some((&["text"], &["run_id", "worker_id"])),
        Kind::WorkerLaunched => Some((
            &["run_id", "worker_id", "pane", "role", "harness", "model"],
            &["effort"],
        )),
        Kind::WorkerLaunchFailed => Some((&["run_id", "worker_id", "reason"], &[])),
        _ => None,
    }
}

/// Check the payload `fields` of a kind of the records of 1 spawn: each key
/// is a field of the kind, each text is not empty, and each required field
/// is there. `spawn_gate_note` has both `run_id` and `worker_id` or
/// neither. Each other kind passes.
pub fn check_spawn_record(kind: Kind, fields: &[(&str, &str)]) -> Result<(), RequestError> {
    let Some((required, optional)) = spawn_record_fields(kind) else {
        return Ok(());
    };
    let name = kind.as_str();
    for (key, text) in fields {
        if !required.contains(key) && !optional.contains(key) {
            return Err(RequestError::malformed(&format!(
                "{name} has the unknown payload field `{key}`"
            )));
        }
        if text.trim().is_empty() {
            return Err(RequestError::malformed(&format!(
                "{name} has the empty payload field `{key}`"
            )));
        }
    }
    let has = |key: &str| fields.iter().any(|(k, _)| *k == key);
    if let Some(key) = required.iter().find(|k| !has(k)) {
        return Err(RequestError::malformed(&format!(
            "{name} has no payload field `{key}`"
        )));
    }
    if kind == Kind::SpawnGateNote && has("run_id") != has("worker_id") {
        return Err(RequestError::malformed(
            "spawn_gate_note has both run_id and worker_id, or neither",
        ));
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
    /// The kind is retired. The text is what you do in its place.
    Retired(&'static str),
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
            RequestError::Retired(instead) => write!(f, "retired kind: {instead}"),
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

    /// The table "Request kinds" of `docs/tickets/fleetd-p5/MAP.md` with no
    /// retired kind, then the idea kinds of Phase 6.
    const MAP_TABLE: [(&str, Class); 42] = [
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
        ("spawn_adopt", Class::Operator),
        ("spawn_gate_note", Class::Operator),
        ("worker_launched", Class::Operator),
        ("worker_launch_failed", Class::Operator),
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
        ("worker_progress", Class::Operator),
        ("worker_done", Class::Operator),
        ("run_adopt", Class::Operator),
        ("question_answer", Class::Operator),
        ("progress", Class::Worker),
        ("result", Class::Worker),
        ("ask", Class::Worker),
        ("idea_add", Class::Operator),
        ("idea_amend", Class::Operator),
        ("idea_approve", Class::Operator),
        ("idea_revise", Class::Operator),
        ("idea_park", Class::Operator),
        ("idea_cancel", Class::Operator),
        ("idea_note", Class::Worker),
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
        let err = Kind::parse("idea_show").unwrap_err();
        assert_eq!(err, RequestError::UnknownKind("idea_show".into()));
        assert!(err.to_string().contains("idea_show"), "{err}");
        assert!(Kind::parse("").is_err());
        assert!(Kind::parse("Run_New").is_err(), "names are exact");
        let err = Kind::parse("x\u{1b}[2J\n").unwrap_err().to_string();
        assert!(!err.chars().any(char::is_control), "{err:?}");
        let long = "k".repeat(10_000);
        assert!(Kind::parse(&long).unwrap_err().to_string().len() < 200);
    }

    #[test]
    fn retired_kind_is_refused_with_its_new_command() {
        for r in RETIRED {
            let err = Kind::parse(r.kind).unwrap_err();
            assert_eq!(err, RequestError::Retired(r.instead));
            assert_eq!(err.to_string(), format!("retired kind: {}", r.instead));
            assert_eq!(r.line(), format!("{} is retired. {}", r.command, r.instead));
            assert!(
                !Kind::ALL.iter().any(|k| k.as_str() == r.kind),
                "{}",
                r.kind
            );
        }
        assert_eq!(
            retired("run_new").map(|r| r.command),
            Some("horch ledger run new")
        );
        assert_eq!(retired("idea_add"), None);
    }

    #[test]
    fn worker_kind_is_class_worker() {
        for name in ["progress", "result", "ask", "idea_note"] {
            assert_eq!(Kind::parse(name).unwrap().class(), Class::Worker, "{name}");
        }
        let workers = Kind::ALL.iter().filter(|k| k.class() == Class::Worker);
        assert_eq!(workers.count(), 4);
        assert_eq!(Class::Worker.as_str(), "worker");
        assert_eq!(Class::Operator.as_str(), "operator");
    }

    #[test]
    fn spawn_record_payload_check_refuses_an_unknown_an_empty_and_a_missing_field() {
        let launched = [
            ("run_id", "R1"),
            ("worker_id", "W1"),
            ("pane", "p1"),
            ("role", "opus-1"),
            ("harness", "claude"),
            ("model", "m"),
        ];
        assert_eq!(check_spawn_record(Kind::WorkerLaunched, &launched), Ok(()));
        let mut extra = launched.to_vec();
        extra.push(("task_id", "T1"));
        let err = check_spawn_record(Kind::WorkerLaunched, &extra).unwrap_err();
        assert!(
            err.to_string().contains("unknown payload field `task_id`"),
            "{err}"
        );
        let err = check_spawn_record(Kind::WorkerLaunched, &launched[1..]).unwrap_err();
        assert!(
            err.to_string().contains("no payload field `run_id`"),
            "{err}"
        );
        let err = check_spawn_record(
            Kind::WorkerLaunchFailed,
            &[("run_id", "R1"), ("worker_id", " "), ("reason", "x")],
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("empty payload field `worker_id`"),
            "{err}"
        );
        assert_eq!(
            check_spawn_record(Kind::SpawnGateNote, &[("text", "spawn gate: x")]),
            Ok(())
        );
        assert!(
            check_spawn_record(Kind::SpawnGateNote, &[("text", "x"), ("run_id", "R1")]).is_err()
        );
        assert_eq!(check_spawn_record(Kind::Spawn, &[("anything", "")]), Ok(()));
        for kind in [
            Kind::SpawnAdopt,
            Kind::SpawnGateNote,
            Kind::WorkerLaunched,
            Kind::WorkerLaunchFailed,
        ] {
            assert_eq!(kind.class(), Class::Operator, "{}", kind.as_str());
        }
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
