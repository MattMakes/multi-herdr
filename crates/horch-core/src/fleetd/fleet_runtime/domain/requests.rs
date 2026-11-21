use std::path::Path;

pub const REQUEST_ID_MAX_BYTES: usize = 64;

const ECHO_MAX_CHARS: usize = 64;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    LedgerImport,
    LedgerGc,
    TaskRequeue,
    TaskCancel,
    TaskFallback,
    LedgerWaive,
    LedgerReopen,
    LedgerDismiss,
    WorkerStop,
    SpawnGateNote,
    WorkerLaunched,
    WorkerLaunchFailed,
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
    SessionRecords,
}

impl Kind {
    pub const ALL: [Kind; 35] = [
        Kind::LedgerImport,
        Kind::LedgerGc,
        Kind::TaskRequeue,
        Kind::TaskCancel,
        Kind::TaskFallback,
        Kind::LedgerWaive,
        Kind::LedgerReopen,
        Kind::LedgerDismiss,
        Kind::WorkerStop,
        Kind::SpawnGateNote,
        Kind::WorkerLaunched,
        Kind::WorkerLaunchFailed,
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
        Kind::SessionRecords,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::LedgerImport => "ledger_import",
            Kind::LedgerGc => "ledger_gc",
            Kind::TaskRequeue => "task_requeue",
            Kind::TaskCancel => "task_cancel",
            Kind::TaskFallback => "task_fallback",
            Kind::LedgerWaive => "ledger_waive",
            Kind::LedgerReopen => "ledger_reopen",
            Kind::LedgerDismiss => "ledger_dismiss",
            Kind::WorkerStop => "worker_stop",
            Kind::SpawnGateNote => "spawn_gate_note",
            Kind::WorkerLaunched => "worker_launched",
            Kind::WorkerLaunchFailed => "worker_launch_failed",
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
            Kind::SessionRecords => "session_records",
        }
    }

    pub fn class(self) -> Class {
        match self {
            Kind::Progress | Kind::Result | Kind::Ask | Kind::IdeaNote => Class::Worker,
            _ => Class::Operator,
        }
    }

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Retired {
    pub kind: &'static str,
    pub command: &'static str,
    pub instead: &'static str,
}

impl Retired {
    pub fn line(&self) -> String {
        format!("{} is retired. {}", self.command, self.instead)
    }
}

const DIRECT: &str =
    "Add a ticket that you wrote: horch idea add --direct <ticket file> [--to <feature>]";

const RECORDS: &str = "fleetd records each attempt at its launch";

pub const RETIRED: [Retired; 15] = [
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
    Retired {
        kind: "spawn",
        command: "horch spawn",
        instead: "fleetd launches each attempt. To pair with a persona, plan its task with an interactive persona",
    },
    Retired {
        kind: "worker_new",
        command: "horch ledger worker new",
        instead: RECORDS,
    },
    Retired {
        kind: "worker_pid",
        command: "horch ledger worker pid",
        instead: RECORDS,
    },
    Retired {
        kind: "spawn_adopt",
        command: "the adoption of a restart row by horch spawn",
        instead: RECORDS,
    },
    Retired {
        kind: "worker_reap",
        command: "horch ledger reap",
        instead: r#"fleetd stops an orphan worker. To stop 1 worker: horch ledger stop <worker> "<reason>""#,
    },
    Retired {
        kind: "task_unblock",
        command: "horch ledger unblock",
        instead: "Queue the task again: horch task requeue <task>",
    },
    Retired {
        kind: "orchestrator_claim",
        command: "the orchestrator claim",
        instead: "horch fleet start starts fleetd and herdr with no orchestrator, and fleetd starts 1 sub-orchestrator for each feature",
    },
    Retired {
        kind: "orchestrator_stop",
        command: "horch fleet stop",
        instead: r#"No orchestrator runs, so there is none to stop. fleetd and the herdr server stay under the service manager of the OS. To stop 1 worker: horch ledger stop <worker> "<reason>""#,
    },
];

pub fn retired(kind: &str) -> Option<&'static Retired> {
    RETIRED.iter().find(|r| r.kind == kind)
}

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

pub fn check_size(bytes: usize, max_bytes: u64) -> Result<(), RequestError> {
    if u64::try_from(bytes).map_or(true, |b| b > max_bytes) {
        return Err(RequestError::TooLarge { bytes, max_bytes });
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdeaAdd {
    pub idea: IdeaInput,
    pub target: AddTarget,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AddTarget {
    New {
        project: String,
        primary_branch: String,
    },
    Join(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdeaInput {
    Text(String),
    Direct(DirectTicket),
}

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

fn spawn_record_fields(kind: Kind) -> Option<(&'static [&'static str], &'static [&'static str])> {
    match kind {
        Kind::SpawnGateNote => Some((&["text"], &["run_id", "worker_id"])),
        Kind::WorkerLaunched => Some((
            &["run_id", "worker_id", "pane", "role", "harness", "model"],
            &["effort"],
        )),
        Kind::WorkerLaunchFailed => Some((&["run_id", "worker_id", "reason"], &[])),
        _ => None,
    }
}

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RequestError {
    TooLarge { bytes: usize, max_bytes: u64 },
    Malformed(String),
    UnknownKind(String),
    Retired(&'static str),
    BadRequestId(&'static str),
}

impl RequestError {
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

    const MAP_TABLE: [(&str, Class); 35] = [
        ("ledger_import", Class::Operator),
        ("ledger_gc", Class::Operator),
        ("task_requeue", Class::Operator),
        ("task_cancel", Class::Operator),
        ("task_fallback", Class::Operator),
        ("ledger_waive", Class::Operator),
        ("ledger_reopen", Class::Operator),
        ("ledger_dismiss", Class::Operator),
        ("worker_stop", Class::Operator),
        ("spawn_gate_note", Class::Operator),
        ("worker_launched", Class::Operator),
        ("worker_launch_failed", Class::Operator),
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
        ("session_records", Class::Operator),
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
        assert_eq!(
            retired("spawn_adopt").map(|r| r.instead),
            Some(RECORDS),
            "no client sends spawn_adopt after T-895"
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
        assert_eq!(
            check_spawn_record(Kind::WorkerStop, &[("anything", "")]),
            Ok(())
        );
        for kind in [
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
