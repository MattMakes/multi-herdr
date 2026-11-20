//! The wrapper runs an exec attempt (fleetd-p7 ticket 17; FR-6, RR-11,
//! FM-18): it starts the CLI with `START.md`, writes each stream line to
//! `transcript.jsonl`, renders a short text of each event into the pane,
//! turns each activity event into a heartbeat, ends the CLI on a usage
//! limit with the outcome hint `quota`, and writes `exit.json` at once.
//!
//! Each test runs the fake agents of `crates/horch-e2e` in a temporary
//! fleet root, never a real agent CLI. The test builds the fakes 1 time,
//! as the harness of `horch-e2e` does. No test reads the clock of this
//! machine or waits for a time: each waits for the end of the fake.

#[path = "support/fakes.rs"]
mod fakes;

use std::path::PathBuf;

use horch_core::execution::lifecycle::{
    run_exec, write_exec_exit, ExecAttempt, ExecCli, ExecEnd, TRANSCRIPT_JSONL,
};
use horch_core::fleetd::task_execution::adapters::result_reader::AttemptRef;
use horch_core::fleetd::task_execution::domain::repair::{
    needs_repair, repair_pass, RepairFacts, RepairPort, Repaired, REPAIR_LINE,
};
use horch_core::harness::antigravity::{self, ExecLaunch};
use horch_core::roster::Teammate;
use horch_core::taskledger::supervisor::{
    AgentFacts, AttemptFiles, ExitFile, HeartbeatFile, OutcomeHint, EXIT_JSON, HEARTBEAT_JSON,
};
use horch_platform::start::{self, Command};
use serde_json::{json, Value};

/// The fakes of the exec CLIs.
const FAKES: [&str; 3] = ["fake-claude", "fake-codex", "fake-antigravity"];

fn fake(name: &str) -> PathBuf {
    fakes::fake(&FAKES, name)
}

/// 1 attempt folder with its `inbox/START.md`, a fake log, and the wrapper
/// files of the attempt.
struct Attempt {
    tmp: tempfile::TempDir,
    files: AttemptFiles,
    facts: std::sync::Arc<AgentFacts>,
    pane: Vec<u8>,
}

impl Attempt {
    fn new() -> Attempt {
        let tmp = tempfile::tempdir().unwrap();
        let fleet_root = tmp.path().join("fleet");
        let files = AttemptFiles::new(
            &fleet_root,
            AttemptRef {
                task_id: "T-1".into(),
                attempt_id: "T-1/A1".into(),
            },
        );
        std::fs::create_dir_all(files.dir().join("inbox")).unwrap();
        std::fs::create_dir_all(files.dir().join("outbox")).unwrap();
        std::fs::write(files.dir().join("inbox/START.md"), "# T-1\nDo the task.\n").unwrap();
        Attempt {
            tmp,
            files,
            facts: AgentFacts::new(Some("w1:p1".into())),
            pane: Vec::new(),
        }
    }

    fn start_md(&self) -> String {
        self.files
            .dir()
            .join("inbox/START.md")
            .to_string_lossy()
            .into_owned()
    }

    fn log(&self) -> PathBuf {
        self.tmp.path().join("fake.log")
    }

    /// The command of the fake of `cli` with the exec argv of its adapter
    /// and the scenario `scenario`.
    fn command(&self, cli: ExecCli, scenario: &str) -> Command {
        let start_md = self.start_md();
        let mut cmd = match cli {
            ExecCli::Claude => {
                let mut c = start::command(fake("fake-claude"));
                c.args([
                    "-p",
                    "--output-format",
                    "stream-json",
                    "--verbose",
                    "--include-hook-events",
                    "--session-id",
                    "s-1",
                    &start_md,
                ]);
                c
            }
            ExecCli::Codex => {
                let mut c = start::command(fake("fake-codex"));
                c.args(["exec", "--json", &start_md]);
                c
            }
            ExecCli::Antigravity => antigravity::exec_command(&ExecLaunch {
                bin: &fake("fake-antigravity"),
                model: "gemini-3-pro",
                effort: None,
                home: self.tmp.path(),
                workdir: self.tmp.path(),
                prompt: &start_md,
            }),
        };
        cmd.env("HORCH_FAKE_LOG", self.log())
            .env("HORCH_FAKE_SCENARIO", scenario);
        cmd
    }

    fn run(&mut self, cli: ExecCli, cmd: Command) -> ExecEnd {
        let mut attempt = ExecAttempt {
            cli,
            files: &self.files,
            facts: &self.facts,
            pane: &mut self.pane,
        };
        run_exec(cmd, &mut attempt).unwrap()
    }

    fn transcript(&self) -> Vec<String> {
        std::fs::read_to_string(self.files.dir().join(TRANSCRIPT_JSONL))
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn pane_text(&self) -> String {
        String::from_utf8(self.pane.clone()).unwrap()
    }

    fn exit(&self) -> ExitFile {
        let text = std::fs::read_to_string(self.files.dir().join(EXIT_JSON)).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    /// Each call of the fakes, from the log.
    fn calls(&self) -> Vec<Value> {
        std::fs::read_to_string(self.log())
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    fn assert_no_violation(&self) {
        for call in self.calls() {
            let v = &call["violations"];
            assert!(
                v.is_null() || v.as_array().is_some_and(Vec::is_empty),
                "the fake saw a violation: {call}"
            );
        }
    }
}

const CLIS: [ExecCli; 3] = [ExecCli::Claude, ExecCli::Codex, ExecCli::Antigravity];

#[test]
fn exec_attempt_runs_to_completion_and_writes_exit_json() {
    for cli in CLIS {
        let mut a = Attempt::new();
        let cmd = a.command(cli, "default");
        let end = a.run(cli, cmd);
        assert_eq!(end, ExecEnd::Exited(Some(0)), "{cli:?}");
        assert_eq!(end.outcome_hint(), None);
        write_exec_exit(&a.files, end, false).unwrap();

        let exit = a.exit();
        assert_eq!(exit.code, Some(0), "{cli:?}");
        assert_eq!(exit.outcome_hint, None, "{cli:?}");
        assert!(!exit.repaired);
        // The CLI read START.md of the attempt.
        let calls = a.calls();
        assert_eq!(calls.len(), 1, "{cli:?}: {calls:?}");
        assert_eq!(calls[0]["exec_prompt"], "# T-1\nDo the task.\n", "{cli:?}");
        a.assert_no_violation();
        // Each stream line is in the transcript, the last 1 is the result.
        let lines = a.transcript();
        assert!(lines.len() >= 2, "{cli:?}: {lines:?}");
        assert_eq!(
            cli.read(lines.last().unwrap()),
            horch_core::execution::lifecycle::StreamEvent::Result,
            "{cli:?}"
        );
    }
}

#[test]
fn exec_stream_event_writes_heartbeat_activity() {
    for cli in CLIS {
        let mut a = Attempt::new();
        let cmd = a.command(cli, "default");
        a.run(cli, cmd);
        let activity = a
            .transcript()
            .iter()
            .filter(|l| cli.read(l).is_activity())
            .count();
        assert!(activity > 0, "{cli:?}");

        let text = std::fs::read_to_string(a.files.dir().join(HEARTBEAT_JSON)).unwrap();
        let beat: HeartbeatFile = serde_json::from_str(&text).unwrap();
        assert_eq!(beat.task, "T-1");
        assert_eq!(beat.attempt, "A1");
        assert!(beat.pid.is_some(), "{cli:?}: {beat:?}");
        assert!(beat.last_output_at.is_some(), "{cli:?}: {beat:?}");
        // 1 heartbeat for each activity event.
        assert_eq!(beat.seq, activity as u64, "{cli:?}: {beat:?}");
    }
}

#[test]
fn exec_usage_limit_writes_quota() {
    for cli in CLIS {
        let mut a = Attempt::new();
        let cmd = a.command(cli, "usage_limit");
        let end = a.run(cli, cmd);
        assert!(matches!(end, ExecEnd::UsageLimited(_)), "{cli:?}: {end:?}");
        assert_eq!(end.outcome_hint(), Some(OutcomeHint::Quota));
        assert!(!end.repair_applies(), "a quota end gets no repair");
        write_exec_exit(&a.files, end, false).unwrap();
        assert_eq!(a.exit().outcome_hint, Some(OutcomeHint::Quota), "{cli:?}");
    }
}

/// A CLI that shows the usage limit and goes on running is ended by the
/// wrapper: the run returns, with no wait for the CLI.
#[cfg(unix)]
#[test]
fn exec_usage_limit_ends_a_cli_that_goes_on() {
    let mut a = Attempt::new();
    let line = r#"{"type":"rate_limit_event","rate_limit_info":{"status":"rejected","resetsAt":1795000000},"session_id":"s-1"}"#;
    let mut cmd = start::command("/bin/sh");
    cmd.arg("-c")
        .arg(format!("printf '%s\\n' '{line}'; exec sleep 600"));
    let end = a.run(ExecCli::Claude, cmd);
    assert_eq!(end, ExecEnd::UsageLimited(None), "a signal ended the CLI");
}

/// The outcome hint is a hint from a process that the agent can reach,
/// never a proof of success: `exit.json` holds only the closed word
/// `quota`, so no hint can say that the task is done, and an exit with a
/// hint and no `result.json` still has no result.
#[test]
fn exit_hint_is_never_proof_of_success() {
    for word in ["completed", "done", "passed", "verified", ""] {
        let forged = json!({
            "task": "T-1", "attempt": "A1", "code": 0, "repaired": false,
            "at": "2026-11-18T00:00:00Z", "outcome_hint": word,
        });
        assert!(
            serde_json::from_value::<ExitFile>(forged).is_err(),
            "the hint {word:?} is refused"
        );
    }
    let mut a = Attempt::new();
    let cmd = a.command(ExecCli::Claude, "usage_limit");
    let end = a.run(ExecCli::Claude, cmd);
    write_exec_exit(&a.files, end, false).unwrap();
    assert_eq!(a.exit().outcome_hint, Some(OutcomeHint::Quota));
    assert!(!a.files.dir().join("outbox/result.json").exists());
}

/// The repair port of 1 exec attempt: its facts come from the outbox, and
/// the resume runs the CLI again through the wrapper.
struct ExecRepair<'a> {
    attempt: &'a mut Attempt,
    recorded: bool,
}

impl RepairPort for ExecRepair<'_> {
    type Error = anyhow::Error;

    fn facts(&mut self) -> anyhow::Result<RepairFacts> {
        let outbox = self.attempt.files.dir().join("outbox");
        Ok(RepairFacts {
            exited: true,
            commits_since_base: true,
            result_json: outbox.join("result.json").is_file(),
            question_file: outbox.join("questions").is_dir(),
            repaired: self.recorded,
        })
    }

    fn record(&mut self) -> anyhow::Result<()> {
        self.recorded = true;
        Ok(())
    }

    fn resume(&mut self, line: &str) -> anyhow::Result<Option<i32>> {
        let mut cmd = start::command(fake("fake-claude"));
        cmd.args([
            "-p",
            "--output-format",
            "stream-json",
            "--verbose",
            "--resume",
            "s-1",
            line,
        ])
        .env("HORCH_FAKE_LOG", self.attempt.log());
        Ok(self.attempt.run(ExecCli::Claude, cmd).code())
    }
}

#[test]
fn exec_exit_without_result_runs_the_repair_pass() {
    let mut a = Attempt::new();
    let cmd = a.command(ExecCli::Claude, "exit_without_result");
    let end = a.run(ExecCli::Claude, cmd);
    assert_eq!(end, ExecEnd::Exited(Some(0)));
    assert!(end.repair_applies());
    let first_run = a.transcript().len();
    assert!(a
        .transcript()
        .iter()
        .all(|l| ExecCli::Claude.read(l) != horch_core::execution::lifecycle::StreamEvent::Result));

    let mut port = ExecRepair {
        attempt: &mut a,
        recorded: false,
    };
    assert!(needs_repair(&port.facts().unwrap()));
    let repaired = repair_pass(&mut port).unwrap();
    assert!(matches!(repaired, Repaired::Resumed(_)), "{repaired:?}");
    let recorded = port.recorded;
    assert!(recorded);
    write_exec_exit(&a.files, end, recorded).unwrap();

    // The resume ran through the wrapper as an exec run of the same
    // session, with the repair line as its prompt. The fake reads each
    // prompt as a path, so it prints no stream for the line.
    assert!(a.transcript().len() >= first_run);
    let calls = a.calls();
    assert_eq!(calls.len(), 2, "{calls:?}");
    let argv: Vec<&str> = calls[1]["argv"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(argv.contains(&"--resume"), "{argv:?}");
    assert_eq!(argv.last(), Some(&REPAIR_LINE), "{argv:?}");
    let exit = a.exit();
    assert!(exit.repaired);
    assert_eq!(exit.code, Some(0), "the code of the first exit");
    assert_eq!(exit.outcome_hint, None);
}

#[test]
fn exec_stream_is_rendered_into_the_pane_text() {
    for cli in CLIS {
        let mut a = Attempt::new();
        let cmd = a.command(cli, "default");
        a.run(cli, cmd);
        let pane = a.pane_text();
        let lines: Vec<&str> = pane.lines().collect();
        // 1 short line for each stream line.
        assert_eq!(lines.len(), a.transcript().len(), "{cli:?}:\n{pane}");
        assert!(lines.iter().all(|l| l.chars().count() <= 120), "{pane}");
        assert!(lines.iter().any(|l| l.starts_with("[activity]")), "{pane}");
        assert!(lines.last().unwrap().starts_with("[result]"), "{pane}");
    }
    let mut a = Attempt::new();
    let cmd = a.command(ExecCli::Claude, "usage_limit");
    a.run(ExecCli::Claude, cmd);
    assert!(
        a.pane_text()
            .lines()
            .last()
            .unwrap()
            .starts_with("[usage limit]"),
        "{}",
        a.pane_text()
    );
}

fn teammate(agent: &str, mode: Option<&str>) -> Teammate {
    let mut t = json!({"name": "w", "agent": agent, "brief_description": "A worker."});
    if let Some(mode) = mode {
        t["mode"] = json!(mode);
    }
    serde_json::from_value(t).unwrap()
}

#[test]
fn interactive_attempt_is_unchanged() {
    // The mode of the profile decides the launch (FR-6).
    for agent in ["claude", "codex", "antigravity"] {
        assert_eq!(
            ExecCli::of(&teammate(agent, Some("interactive"))),
            None,
            "{agent}"
        );
    }
    assert_eq!(
        ExecCli::of(&teammate("claude", None)),
        Some(ExecCli::Claude)
    );
    assert_eq!(ExecCli::of(&teammate("codex", None)), Some(ExecCli::Codex));
    assert_eq!(
        ExecCli::of(&teammate("antigravity", None)),
        Some(ExecCli::Antigravity)
    );
    for agent in ["opencode", "pi", "prime", "none"] {
        assert_eq!(ExecCli::of(&teammate(agent, None)), None, "{agent}");
    }
    assert_eq!(
        ExecCli::of(&teammate("claude", Some("exec"))),
        Some(ExecCli::Claude)
    );
    assert_eq!(
        ExecCli::of(&teammate("codex", Some("exec"))),
        Some(ExecCli::Codex)
    );
    assert_eq!(
        ExecCli::of(&teammate("antigravity", Some("exec"))),
        Some(ExecCli::Antigravity)
    );
    // A CLI with no exec adapter stays interactive.
    for agent in ["opencode", "pi", "prime", "none"] {
        assert_eq!(ExecCli::of(&teammate(agent, Some("exec"))), None, "{agent}");
    }
}
