//! The adapter conformance suite: the 10 checks, the verdict of 1 check and
//! the form of `conformance.json` (`docs/specs/fleetd/design.md`, "Docs sync
//! and adapter conformance"). fleetd-p7 tickets 23 and 24 fill it.
//!
//! Each check is a real launch of the CLI in a throwaway task folder with a
//! tiny fixed prompt. The launch gives 1 [`Observation`]: what reached the
//! wrapper, the files in the task folder and the report of the agent.
//! [`judge`] turns it into the verdict of the check.

use std::collections::BTreeSet;
use std::time::Duration;

/// 1 check of the design, in the order of its table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Check {
    LaunchHeadless,
    PrivateHome,
    ExactMcpSet,
    SharedWrite,
    SandboxBoundary,
    OperatorBoundary,
    ContextLimit,
    HooksFire,
    NoTrustPrompt,
    StopGate,
}

/// The 10 checks of the design.
pub const CHECKS: [Check; 10] = [
    Check::LaunchHeadless,
    Check::PrivateHome,
    Check::ExactMcpSet,
    Check::SharedWrite,
    Check::SandboxBoundary,
    Check::OperatorBoundary,
    Check::ContextLimit,
    Check::HooksFire,
    Check::NoTrustPrompt,
    Check::StopGate,
];

impl Check {
    /// The name of the check in the design and in `conformance.json`.
    pub fn name(self) -> &'static str {
        match self {
            Check::LaunchHeadless => "Launch headless",
            Check::PrivateHome => "Private home honoured",
            Check::ExactMcpSet => "Exact MCP set",
            Check::SharedWrite => "Shared write",
            Check::SandboxBoundary => "Sandbox boundary",
            Check::OperatorBoundary => "Operator boundary",
            Check::ContextLimit => "Context limit",
            Check::HooksFire => "Hooks fire",
            Check::NoTrustPrompt => "No trust prompt",
            Check::StopGate => "Stop gate",
        }
    }

    /// The check of `name`.
    pub fn of_name(name: &str) -> Option<Check> {
        CHECKS.into_iter().find(|c| c.name() == name)
    }

    /// Whether the suite of 1 exec CLI launches this check. "Operator
    /// boundary" is fleetd-p7 ticket 24.
    pub fn in_exec_suite(self) -> bool {
        self != Check::OperatorBoundary
    }

    /// The tiny fixed prompt of the launch of this check.
    pub fn prompt(self) -> &'static str {
        match self {
            Check::StopGate => STOP_GATE_PROMPT,
            _ => PROBE_PROMPT,
        }
    }
}

/// The time limit for the first stream event of a launch ("Launch
/// headless"). The runner takes the limit as a value.
pub const FIRST_EVENT_LIMIT: Duration = Duration::from_secs(60);

/// The effective compaction setting that "Context limit" reads back (FR-9).
pub const CONTEXT_LIMIT: u64 = 256_000;

/// The name of the marker skill that the runner puts only in the private
/// config home of a launch ("Private home honoured").
pub const MARKER_SKILL: &str = "horch-conformance-marker";

/// The name of the test MCP server that the runner adds to the profile of
/// a launch ("Exact MCP set").
pub const TEST_MCP_SERVER: &str = "horch-conformance";

/// The prompt of each check but "Stop gate". It names the keys of the
/// report that the agent gives as its last message.
pub const PROBE_PROMPT: &str = "\
This is a conformance check of horch. Do each step, then give 1 JSON object as your last message, with no other text.
1. List the names of the skills and of the rules that you have: the keys `skills` and `rules`.
2. List the names of your MCP servers: the key `mcp_servers`.
3. Read the auto-compact setting of your CLI (the env var CLAUDE_CODE_AUTO_COMPACT_WINDOW, or the config key model_auto_compact_token_limit). Give its value as text, or null: the key `compaction`.
4. Write the text {} into the file outbox/result.json of the folder above the folder of this file. Give `allowed`, or `refused: <the error>`: the key `result_write`.
5. Try to write the file horch-conformance-outside in the folder above your working folder. Give `allowed`, or `refused: <the error>`: the key `write_outside` of the object `operator_boundary`.
";

/// The prompt of "Stop gate": the agent ends with no result.
pub const STOP_GATE_PROMPT: &str = "\
This is a conformance check of horch. End now. Do not write a file, and do not run `horch result`.
";

/// The verdict of 1 check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Fail,
}

impl Verdict {
    /// The word of the verdict in `conformance.json`.
    pub fn word(self) -> &'static str {
        match self {
            Verdict::Pass => "pass",
            Verdict::Fail => "fail",
        }
    }

    /// The verdict of `word`.
    pub fn of_word(word: &str) -> Option<Verdict> {
        [Verdict::Pass, Verdict::Fail]
            .into_iter()
            .find(|v| v.word() == word)
    }
}

/// The verdict of 1 check with 1 line of evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckResult {
    pub check: Check,
    pub verdict: Verdict,
    pub evidence: String,
}

/// The most characters of the evidence of 1 check.
const EVIDENCE_MAX: usize = 240;

impl CheckResult {
    pub fn pass(check: Check, evidence: impl AsRef<str>) -> CheckResult {
        CheckResult::new(check, Verdict::Pass, evidence.as_ref())
    }

    pub fn fail(check: Check, evidence: impl AsRef<str>) -> CheckResult {
        CheckResult::new(check, Verdict::Fail, evidence.as_ref())
    }

    /// The evidence is 1 line: each line break becomes a space, and a long
    /// text is cut.
    fn new(check: Check, verdict: Verdict, evidence: &str) -> CheckResult {
        let line: String = evidence
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(EVIDENCE_MAX)
            .collect();
        CheckResult {
            check,
            verdict,
            evidence: line,
        }
    }
}

/// The text of `conformance.json`: the CLI, its version text, the hash of
/// its adapter and the verdict of each check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConformanceReport {
    pub cli: String,
    pub version: String,
    pub adapter_hash: String,
    pub checks: Vec<CheckResult>,
}

impl ConformanceReport {
    /// The checks that failed, in the order of the run.
    pub fn failed(&self) -> Vec<Check> {
        self.checks
            .iter()
            .filter(|r| r.verdict == Verdict::Fail)
            .map(|r| r.check)
            .collect()
    }

    /// The contract `adapter-conformance-failed` (Slice 7): a run with at
    /// least 1 failed check. `None` when each check passed.
    pub fn failure(&self) -> Option<AdapterConformanceFailed> {
        let failed = self.failed();
        (!failed.is_empty()).then(|| AdapterConformanceFailed {
            cli: self.cli.clone(),
            version: self.version.clone(),
            failed,
        })
    }
}

/// The result `adapter-conformance-failed`: the CLI, its version and each
/// failed check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterConformanceFailed {
    pub cli: String,
    pub version: String,
    pub failed: Vec<Check>,
}

impl AdapterConformanceFailed {
    pub const NAME: &'static str = "adapter-conformance-failed";
}

impl std::fmt::Display for AdapterConformanceFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.failed.iter().map(|c| c.name()).collect();
        write!(
            f,
            "{}: {} {}: {}",
            Self::NAME,
            self.cli,
            self.version,
            names.join(", ")
        )
    }
}

/// A tool start, a tool end or a stop that reached the wrapper ("Hooks
/// fire"): a hook event of the stream, or the stream line of the same act.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Hook {
    ToolStart,
    ToolEnd,
    Stop,
}

/// How the launch ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchEnd {
    /// The CLI exited by itself with this code. `None`: a signal ended it.
    Exited(Option<i32>),
    /// The runner ended the CLI: no stream event came within the limit.
    NoEventInTime,
    /// The runner ended the CLI: it showed this dialog line on its stream.
    Dialog(String),
}

/// The private config home of a launch, as the runner found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrivateHome {
    /// The runner put [`MARKER_SKILL`] in this private config home.
    Marked(String),
    /// The launch has no private config home, for this reason.
    Missing(String),
}

/// What the agent reports for [`PROBE_PROMPT`]. A key the agent leaves out
/// is empty.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProbeReport {
    pub skills: Vec<String>,
    pub rules: Vec<String>,
    pub mcp_servers: Vec<String>,
    pub compaction: Option<String>,
    pub result_write: Option<String>,
    /// The result of the outside write: `allowed` or `refused: <why>`.
    pub write_outside: Option<String>,
}

/// What 1 launch of a check gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    pub end: LaunchEnd,
    /// The first stream event reached the wrapper within the limit.
    pub first_event: bool,
    /// The tool starts, tool ends and stops that reached the wrapper.
    pub hooks: BTreeSet<Hook>,
    /// The report of the agent in its last message, when it gave one.
    pub report: Option<ProbeReport>,
    /// `outbox/result.json` of the task folder exists after the launch.
    pub result_json: bool,
    /// `outbox/questions/` of the task folder holds a question file.
    pub question_file: bool,
    /// The file of the outside write exists after the launch.
    pub outside_written: bool,
    pub private_home: PrivateHome,
    /// The MCP servers that the profile of the launch names, with
    /// [`TEST_MCP_SERVER`].
    pub profile_mcp: BTreeSet<String>,
    /// The CLI has a compaction setting ("where the CLI supports it").
    pub compaction_setting: bool,
    /// A Stop hook event of the CLI reached the wrapper (Claude Code).
    pub stop_hook: bool,
    /// The wrapper's repair pass started after the end of the launch.
    pub repair_started: bool,
}

/// The verdict of `check` from the observation of its launch.
pub fn judge(check: Check, o: &Observation) -> CheckResult {
    match check {
        Check::LaunchHeadless => launch_headless(o),
        Check::PrivateHome => private_home(o),
        Check::ExactMcpSet => exact_mcp_set(o),
        Check::SharedWrite => shared_write(o),
        Check::SandboxBoundary => sandbox_boundary(o),
        Check::OperatorBoundary => CheckResult::fail(check, "the suite has no launch for it yet"),
        Check::ContextLimit => context_limit(o),
        Check::HooksFire => hooks_fire(o),
        Check::NoTrustPrompt => no_trust_prompt(o),
        Check::StopGate => stop_gate(o),
    }
}

fn end_text(end: &LaunchEnd) -> String {
    match end {
        LaunchEnd::Exited(Some(code)) => format!("exit {code}"),
        LaunchEnd::Exited(None) => "a signal ended the CLI".into(),
        LaunchEnd::NoEventInTime => "no stream event within the limit".into(),
        LaunchEnd::Dialog(line) => format!("a dialog: {line}"),
    }
}

fn launch_headless(o: &Observation) -> CheckResult {
    let c = Check::LaunchHeadless;
    let end = end_text(&o.end);
    match (&o.end, o.first_event) {
        (LaunchEnd::Exited(Some(0)), true) => CheckResult::pass(c, format!("first event, {end}")),
        (_, false) => CheckResult::fail(c, format!("no stream event; {end}")),
        _ => CheckResult::fail(c, format!("first event, then {end}")),
    }
}

/// The report of `o`, or the failed result of `check` without it.
fn report_of(check: Check, o: &Observation) -> Result<&ProbeReport, CheckResult> {
    o.report.as_ref().ok_or_else(|| {
        CheckResult::fail(
            check,
            format!("the agent gave no report; {}", end_text(&o.end)),
        )
    })
}

fn private_home(o: &Observation) -> CheckResult {
    let c = Check::PrivateHome;
    let home = match &o.private_home {
        PrivateHome::Missing(why) => return CheckResult::fail(c, why),
        PrivateHome::Marked(home) => home,
    };
    let r = match report_of(c, o) {
        Ok(r) => r,
        Err(fail) => return fail,
    };
    let seen = r.skills.iter().chain(&r.rules).any(|n| n == MARKER_SKILL);
    if seen {
        CheckResult::pass(c, format!("{MARKER_SKILL} from {home} is visible"))
    } else {
        CheckResult::fail(
            c,
            format!(
                "{MARKER_SKILL} from {home} is not visible; skills {:?}, rules {:?}",
                r.skills, r.rules
            ),
        )
    }
}

fn exact_mcp_set(o: &Observation) -> CheckResult {
    let c = Check::ExactMcpSet;
    let r = match report_of(c, o) {
        Ok(r) => r,
        Err(fail) => return fail,
    };
    let listed: BTreeSet<&str> = r.mcp_servers.iter().map(String::as_str).collect();
    let extra: Vec<&str> = listed
        .iter()
        .copied()
        .filter(|n| !o.profile_mcp.contains(*n))
        .collect();
    if !listed.contains(TEST_MCP_SERVER) {
        CheckResult::fail(
            c,
            format!("{TEST_MCP_SERVER} of the profile is not listed; listed {listed:?}"),
        )
    } else if !extra.is_empty() {
        CheckResult::fail(
            c,
            format!("servers outside the profile are listed: {extra:?}"),
        )
    } else {
        CheckResult::pass(c, format!("listed {listed:?}, each of the profile"))
    }
}

fn shared_write(o: &Observation) -> CheckResult {
    let c = Check::SharedWrite;
    if o.result_json {
        CheckResult::pass(c, "outbox/result.json is in the task folder")
    } else {
        let said = o
            .report
            .as_ref()
            .and_then(|r| r.result_write.as_deref())
            .unwrap_or("no report of the write");
        CheckResult::fail(
            c,
            format!(
                "outbox/result.json is not in the task folder; {said}; {}",
                end_text(&o.end)
            ),
        )
    }
}

fn sandbox_boundary(o: &Observation) -> CheckResult {
    let c = Check::SandboxBoundary;
    if o.outside_written {
        return CheckResult::fail(c, "the file of the outside write exists");
    }
    let r = match report_of(c, o) {
        Ok(r) => r,
        Err(fail) => return fail,
    };
    match r.write_outside.as_deref() {
        Some(said) if said.starts_with("refused") => {
            CheckResult::pass(c, format!("the outside write: {said}"))
        }
        Some(said) => CheckResult::fail(c, format!("the outside write: {said}")),
        None => CheckResult::fail(c, "the agent gave no result of the outside write"),
    }
}

fn context_limit(o: &Observation) -> CheckResult {
    let c = Check::ContextLimit;
    if !o.compaction_setting {
        return CheckResult::pass(
            c,
            format!("the CLI has no compaction setting; the wrapper ends the attempt at {CONTEXT_LIMIT}"),
        );
    }
    let r = match report_of(c, o) {
        Ok(r) => r,
        Err(fail) => return fail,
    };
    let want = CONTEXT_LIMIT.to_string();
    match r.compaction.as_deref() {
        Some(v) if v.trim() == want => CheckResult::pass(c, format!("reads back {want}")),
        Some(v) => CheckResult::fail(c, format!("reads back {v}, not {want}")),
        None => CheckResult::fail(c, "the compaction setting reads back as unset"),
    }
}

fn hooks_fire(o: &Observation) -> CheckResult {
    let c = Check::HooksFire;
    let missing: Vec<&str> = [
        (Hook::ToolStart, "tool start"),
        (Hook::ToolEnd, "tool end"),
        (Hook::Stop, "stop"),
    ]
    .into_iter()
    .filter(|(h, _)| !o.hooks.contains(h))
    .map(|(_, name)| name)
    .collect();
    if missing.is_empty() {
        CheckResult::pass(c, "tool start, tool end and stop reached the wrapper")
    } else {
        CheckResult::fail(
            c,
            format!(
                "no {} reached the wrapper; {}",
                missing.join(", no "),
                end_text(&o.end)
            ),
        )
    }
}

fn no_trust_prompt(o: &Observation) -> CheckResult {
    let c = Check::NoTrustPrompt;
    match &o.end {
        LaunchEnd::Dialog(line) => CheckResult::fail(c, format!("a dialog: {line}")),
        // A guard whose fact is unknown does not hold.
        LaunchEnd::NoEventInTime => CheckResult::fail(
            c,
            "no stream event within the limit, so a hidden dialog is possible",
        ),
        LaunchEnd::Exited(_) => CheckResult::pass(c, "no dialog on the stream"),
    }
}

fn stop_gate(o: &Observation) -> CheckResult {
    let c = Check::StopGate;
    if o.stop_hook {
        return CheckResult::pass(c, "the stop hook fired");
    }
    if o.repair_started {
        return CheckResult::pass(
            c,
            format!(
                "{} with no result; the repair pass started",
                end_text(&o.end)
            ),
        );
    }
    let why = if o.result_json || o.question_file {
        "the agent left a result or a question"
    } else {
        "the repair pass did not start"
    };
    CheckResult::fail(c, format!("{why}; {}", end_text(&o.end)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_10_names_of_the_design() {
        let names: Vec<&str> = CHECKS.iter().map(|c| c.name()).collect();
        assert_eq!(
            names,
            [
                "Launch headless",
                "Private home honoured",
                "Exact MCP set",
                "Shared write",
                "Sandbox boundary",
                "Operator boundary",
                "Context limit",
                "Hooks fire",
                "No trust prompt",
                "Stop gate",
            ]
        );
        for c in CHECKS {
            assert_eq!(Check::of_name(c.name()), Some(c));
        }
        assert_eq!(CHECKS.iter().filter(|c| c.in_exec_suite()).count(), 9);
    }

    #[test]
    fn evidence_is_1_line() {
        let r = CheckResult::fail(Check::StopGate, "a\nb\n\n  c");
        assert_eq!(r.evidence, "a b c");
        let long = "x".repeat(1000);
        assert_eq!(
            CheckResult::pass(Check::StopGate, long).evidence.len(),
            EVIDENCE_MAX
        );
    }

    #[test]
    fn failure_names_the_cli_the_version_and_each_failed_check() {
        let report = ConformanceReport {
            cli: "codex".into(),
            version: "0.130.0".into(),
            adapter_hash: "h".into(),
            checks: vec![
                CheckResult::pass(Check::LaunchHeadless, "ok"),
                CheckResult::fail(Check::HooksFire, "no stop"),
                CheckResult::fail(Check::StopGate, "no repair"),
            ],
        };
        let failure = report.failure().unwrap();
        assert_eq!(failure.failed, [Check::HooksFire, Check::StopGate]);
        assert_eq!(
            failure.to_string(),
            "adapter-conformance-failed: codex 0.130.0: Hooks fire, Stop gate"
        );
    }
}
