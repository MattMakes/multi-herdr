//! The real launch of each conformance check through the launcher.
//! fleetd-p7 tickets 23 and 24 fill it.
//!
//! 1 launch is the exec launch of the adapter (fleetd-p7 tickets 13 to 15)
//! with the preparation of a worker launch, run by the wrapper of ticket 17
//! ([`run_exec`]), so that each event that reaches a worker's wrapper
//! reaches the check too. The process starts through the platform start
//! function, which removes each forbidden name (SR-7, MAP.md Q1).
//!
//! Each launch has its own throwaway folder under the scratch folder of
//! the runner: a task folder with `inbox/START.md` (the fixed prompt of the
//! check) and `outbox/`, and a working folder. The runner removes the
//! folder after the launch, and the marker skill when it is elsewhere.
//!
//! The runner ends a CLI that shows no stream event within its limit, or
//! that prints a line that is not a stream event: an exec CLI prints only
//! its stream on stdout, so another line is a dialog.
//!
//! Windows gap: [`horch_platform::group::kill_group_id`] is not supported
//! on Windows, so there a CLI that hangs is not ended.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{Context, Result};
use horch_platform::start::Command;
use serde_json::{json, Value};

use crate::execution::lifecycle::{run_exec, ExecAttempt, ExecCli, ExecEnd, TRANSCRIPT_JSONL};
use crate::execution::SessionMode;
use crate::fleetd::task_execution::adapters::result_reader::AttemptRef;
use crate::fleetd::task_execution::domain::repair::{
    repair_pass, RepairFacts, RepairPort, Repaired,
};
use crate::fleetd::worker_profiles::application::conformance::{
    read_probe_report, ConformanceLaunch,
};
use crate::fleetd::worker_profiles::domain::conformance::{
    Check, Hook, LaunchEnd, Observation, PrivateHome, MARKER_SKILL, TEST_MCP_SERVER,
};
use crate::harness::launch::{self, LaunchRequest};
use crate::roster::Teammate;
use crate::runtime::RuntimeContext;
use crate::taskledger::supervisor::{AgentFacts, AttemptFiles};

/// The file that the prompt of a check names for the outside write, in the
/// throwaway folder: outside the task folder and the working folder.
const OUTSIDE_FILE: &str = "horch-conformance-outside";

/// The role of a conformance launch. It names the private codex home and
/// the launch marker of the launch.
const ROLE: &str = "conformance";

/// The [`ConformanceLaunch`] of 1 CLI.
pub struct ConformanceRunner<'a> {
    /// The context of the process that runs the suite.
    pub ctx: &'a RuntimeContext,
    /// The reference profile of the CLI, in exec mode. Each launch adds
    /// [`TEST_MCP_SERVER`] to it.
    pub teammate: &'a Teammate,
    /// The fleet window of the reference profile and its source, as a
    /// worker launch gets it (`Roster::fleet_window`).
    pub fleet_window: Option<(u64, String)>,
    /// The variables that the CLI inherits on top of the environment of
    /// this process.
    pub env_rule: Vec<(String, String)>,
    /// The folder of the throwaway folders.
    pub scratch: PathBuf,
    /// The limit for the first stream event ("Launch headless").
    pub first_event_limit: Duration,
}

impl ConformanceLaunch for ConformanceRunner<'_> {
    fn launch(&mut self, check: Check) -> Result<Observation, String> {
        let cli = ExecCli::of(self.teammate).ok_or_else(|| {
            format!(
                "the profile {} has no exec launch: its mode is interactive, or its CLI has no exec adapter",
                self.teammate.name
            )
        })?;
        let dir = self
            .scratch
            .join(format!("conformance-{}", uuid::Uuid::new_v4().simple()));
        let observed = self.launch_in(&dir, cli, check);
        let _ = std::fs::remove_dir_all(&dir);
        observed.map_err(|e| format!("{e:#}"))
    }
}

impl ConformanceRunner<'_> {
    /// The reference profile with [`TEST_MCP_SERVER`]: a stdio server of
    /// the `horch` binary.
    fn profile(&self) -> Teammate {
        let mut t = self.teammate.clone();
        let server = json!({
            "type": "stdio",
            "command": self.ctx.bins.horch_exe,
            "args": ["--version"],
        });
        t.mcp_servers
            .get_or_insert_with(BTreeMap::new)
            .insert(TEST_MCP_SERVER.to_owned(), server);
        t
    }

    fn launch_in(&self, dir: &Path, cli: ExecCli, check: Check) -> Result<Observation> {
        let work = dir.join("work");
        let files = AttemptFiles::new(
            &dir.join("fleet"),
            AttemptRef {
                task_id: "conformance".into(),
                attempt_id: "conformance/A1".into(),
            },
        );
        for d in [
            &work,
            &files.dir().join("inbox"),
            &files.dir().join("outbox"),
        ] {
            std::fs::create_dir_all(d).with_context(|| format!("creating {}", d.display()))?;
        }
        let start_md = files.dir().join("inbox/START.md");
        std::fs::write(&start_md, check.prompt())
            .with_context(|| format!("writing {}", start_md.display()))?;
        let prompt = start_md
            .to_str()
            .context("the throwaway folder is not UTF-8")?
            .to_owned();

        let mut ctx = self.ctx.clone();
        ctx.paths.cwd = Some(work.clone());
        let teammate = self.profile();
        let session = SessionMode::Fresh(None);
        let req = LaunchRequest {
            role: ROLE,
            teammate: &teammate,
            session: &session,
            prompt: &prompt,
            model_override: None,
            exec_rules: &[],
            child_env: self.env_rule.clone(),
            record: None,
            fleet_window: self.fleet_window.clone(),
            briefing_skill: None,
        };
        let facts = AgentFacts::new(None);
        let mut home = PrivateHome::Missing("the launch did not start".into());
        let mut marker = None;
        let watched = launch::run_exec_flow(&ctx, req, files.dir(), |cmd| {
            (home, marker) = mark_private_home(cli, &cmd, &ctx)?;
            run_watched(cmd, cli, &files, &facts, self.first_event_limit)
        });
        if let Some(marker) = marker {
            let _ = std::fs::remove_dir_all(marker);
        }
        let watched = watched?;
        Ok(observe(watched, &files, dir, home, &teammate, cli))
    }
}

/// The private config home of the launch `cmd`, with [`MARKER_SKILL`] put
/// in it, and the folder of the marker. Claude Code: the `--plugin-dir` of
/// the launch. Codex: its `CODEX_HOME`. Antigravity: its `HOME`. A home that
/// is the home of this process is not private: the runner puts nothing
/// there.
fn mark_private_home(
    cli: ExecCli,
    cmd: &Command,
    ctx: &RuntimeContext,
) -> Result<(PrivateHome, Option<PathBuf>)> {
    let env = |key: &str| {
        cmd.get_envs()
            .find(|(k, _)| *k == key)
            .and_then(|(_, v)| v)
            .map(PathBuf::from)
    };
    let (home, skills) = match cli {
        ExecCli::Claude => {
            let args: Vec<_> = cmd.get_args().collect();
            let dir = args
                .windows(2)
                .rev()
                .find(|w| w[0] == "--plugin-dir")
                .map(|w| PathBuf::from(w[1]));
            (dir.clone(), dir.map(|d| d.join("skills")))
        }
        ExecCli::Codex => {
            let dir = env("CODEX_HOME");
            (dir.clone(), dir.map(|d| d.join("skills")))
        }
        ExecCli::Antigravity => {
            let dir = env("HOME");
            let skills = dir
                .as_ref()
                .map(|d| d.join(".gemini/antigravity-cli/skills"));
            (dir, skills)
        }
    };
    let (Some(home), Some(skills)) = (home, skills) else {
        let why = "the launch names no private config home";
        return Ok((PrivateHome::Missing(why.into()), None));
    };
    let operator = [
        Some(ctx.paths.home.as_os_str()),
        ctx.inherited.home_var.as_deref(),
    ];
    if operator.into_iter().flatten().any(|h| Path::new(h) == home) {
        let why = format!(
            "the launch uses the home of the operator, {}",
            home.display()
        );
        return Ok((PrivateHome::Missing(why), None));
    }
    let marker = skills.join(MARKER_SKILL);
    std::fs::create_dir_all(&marker).with_context(|| format!("creating {}", marker.display()))?;
    let text = format!(
        "---\nname: {MARKER_SKILL}\ndescription: The marker of a horch conformance check.\n---\n\nThe marker of a horch conformance check.\n"
    );
    std::fs::write(marker.join("SKILL.md"), text)
        .with_context(|| format!("writing the marker in {}", marker.display()))?;
    Ok((
        PrivateHome::Marked(home.display().to_string()),
        Some(marker),
    ))
}

/// What the runner saw of the stream of 1 launch.
#[derive(Debug, Default)]
struct Seen {
    first_event: bool,
    no_event_in_time: bool,
    dialog: Option<String>,
    hooks: BTreeSet<Hook>,
    stop_hook: bool,
    /// The last message of the agent.
    last_text: Option<String>,
}

/// The end of 1 watched run.
struct Watched {
    end: ExecEnd,
    seen: Seen,
}

/// A signal to the watch of the first event.
enum Signal {
    FirstEvent,
    Done,
}

/// Run `cmd` through the wrapper, and end the CLI when no stream event
/// comes within `limit` or when it shows a dialog.
fn run_watched(
    cmd: Command,
    cli: ExecCli,
    files: &AttemptFiles,
    facts: &AgentFacts,
    limit: Duration,
) -> Result<Watched> {
    let (tx, rx) = mpsc::channel();
    let mut pane = Watch {
        cli,
        transcript: files.dir().join(TRANSCRIPT_JSONL),
        reader: None,
        line: Vec::new(),
        facts,
        tx: tx.clone(),
        seen: Seen::default(),
    };
    let (end, timed_out) = std::thread::scope(|s| {
        let watch = s.spawn(move || match rx.recv_timeout(limit) {
            Err(mpsc::RecvTimeoutError::Timeout) => {
                end_cli(facts);
                true
            }
            _ => false,
        });
        let end = run_exec(
            cmd,
            &mut ExecAttempt {
                cli,
                files,
                facts,
                pane: &mut pane,
            },
        );
        let _ = tx.send(Signal::Done);
        (end, watch.join().unwrap_or(false))
    });
    let mut seen = pane.seen;
    seen.no_event_in_time = timed_out && !seen.first_event;
    Ok(Watched { end: end?, seen })
}

/// End the CLI of `facts` and its process group.
fn end_cli(facts: &AgentFacts) {
    if let Some(pid) = facts.pid() {
        let _ = horch_platform::group::kill_group_id(pid);
    }
}

/// The pane of a conformance launch. The wrapper writes each stream line
/// to the transcript and then 1 pane line for it, so each pane line names
/// the next line of the transcript.
struct Watch<'a> {
    cli: ExecCli,
    transcript: PathBuf,
    reader: Option<BufReader<std::fs::File>>,
    line: Vec<u8>,
    facts: &'a AgentFacts,
    tx: mpsc::Sender<Signal>,
    seen: Seen,
}

impl Watch<'_> {
    /// The next line of the transcript.
    fn next_stream_line(&mut self) -> Option<String> {
        if self.reader.is_none() {
            self.reader = std::fs::File::open(&self.transcript)
                .ok()
                .map(BufReader::new);
        }
        let mut raw = String::new();
        let read = self.reader.as_mut()?.read_line(&mut raw).ok()?;
        (read > 0).then(|| raw.trim_end_matches(['\n', '\r']).to_owned())
    }

    fn on_stream_line(&mut self, raw: &str) {
        let Ok(value) = serde_json::from_str::<Value>(raw) else {
            if !raw.trim().is_empty() && self.seen.dialog.is_none() {
                self.seen.dialog = Some(raw.trim().to_owned());
                end_cli(self.facts);
            }
            return;
        };
        if !self.seen.first_event {
            self.seen.first_event = true;
            let _ = self.tx.send(Signal::FirstEvent);
        }
        let acts = acts(self.cli, &value);
        self.seen.hooks.extend(acts.hooks);
        self.seen.stop_hook |= acts.stop_hook;
        if let Some(text) = acts.text {
            self.seen.last_text = Some(text);
        }
    }
}

impl Write for Watch<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        for &b in buf {
            if b == b'\n' {
                self.line.clear();
                if let Some(raw) = self.next_stream_line() {
                    self.on_stream_line(&raw);
                }
            } else {
                self.line.push(b);
            }
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// What 1 stream line shows: hooks, a Stop hook, the text of a message.
#[derive(Default)]
struct Acts {
    hooks: Vec<Hook>,
    stop_hook: bool,
    text: Option<String>,
}

/// The kinds of a Codex item that are not a tool.
const CODEX_NOT_TOOLS: [&str; 2] = ["agent_message", "reasoning"];

/// The acts of 1 stream line of `cli`. A tool end also shows its start.
fn acts(cli: ExecCli, v: &Value) -> Acts {
    let mut a = Acts::default();
    let at = |path: &str| v.pointer(path).and_then(Value::as_str);
    match cli {
        ExecCli::Claude => {
            match at("/hook_event") {
                Some("PreToolUse") => a.hooks.push(Hook::ToolStart),
                Some("PostToolUse" | "PostToolUseFailure") => {
                    a.hooks.extend([Hook::ToolStart, Hook::ToolEnd]);
                }
                Some("Stop") => {
                    a.hooks.push(Hook::Stop);
                    a.stop_hook = true;
                }
                _ => {}
            }
            let has = |kind: &str| {
                v.pointer("/message/content")
                    .and_then(Value::as_array)
                    .is_some_and(|c| c.iter().any(|p| p["type"] == kind))
            };
            match at("/type") {
                Some("assistant") if has("tool_use") => a.hooks.push(Hook::ToolStart),
                Some("user") if has("tool_result") => {
                    a.hooks.extend([Hook::ToolStart, Hook::ToolEnd]);
                }
                Some("result") => {
                    a.hooks.push(Hook::Stop);
                    a.text = at("/result").map(str::to_owned);
                }
                _ => {}
            }
        }
        ExecCli::Codex => {
            let tool = at("/item/type").is_some_and(|t| !CODEX_NOT_TOOLS.contains(&t));
            match at("/type") {
                Some("item.started") if tool => a.hooks.push(Hook::ToolStart),
                Some("item.completed") if tool => a.hooks.extend([Hook::ToolStart, Hook::ToolEnd]),
                Some("item.completed") if at("/item/type") == Some("agent_message") => {
                    a.text = at("/item/text").map(str::to_owned);
                }
                Some("turn.completed") => a.hooks.push(Hook::Stop),
                _ => {}
            }
        }
        ExecCli::Antigravity => match at("/event") {
            Some("step_update") if at("/step_update/step_type") == Some("tool") => {
                a.hooks.push(Hook::ToolStart);
                if at("/step_update/state") == Some("DONE") {
                    a.hooks.push(Hook::ToolEnd);
                }
            }
            Some("result") => {
                a.hooks.push(Hook::Stop);
                a.text = at("/result/response").map(str::to_owned);
            }
            _ => {}
        },
    }
    a
}

/// The repair pass of the wrapper over the facts of a conformance launch.
/// The throwaway task stands for a task with work, so it has commits since
/// its base; the pass resumes nothing.
struct ConformanceRepair {
    facts: RepairFacts,
}

impl RepairPort for ConformanceRepair {
    type Error = std::convert::Infallible;

    fn facts(&mut self) -> Result<RepairFacts, Self::Error> {
        Ok(self.facts)
    }

    fn record(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn resume(&mut self, _line: &str) -> Result<Option<i32>, Self::Error> {
        Ok(None)
    }
}

/// The observation of 1 launch from what the runner saw and the files of
/// the throwaway folder `dir`.
fn observe(
    watched: Watched,
    files: &AttemptFiles,
    dir: &Path,
    private_home: PrivateHome,
    teammate: &Teammate,
    cli: ExecCli,
) -> Observation {
    let Watched { end, seen } = watched;
    let end = match (seen.dialog, seen.no_event_in_time) {
        (Some(line), _) => LaunchEnd::Dialog(line),
        (None, true) => LaunchEnd::NoEventInTime,
        (None, false) => LaunchEnd::Exited(end.code()),
    };
    let outbox = files.dir().join("outbox");
    let result_json = outbox.join("result.json").is_file();
    let question_file = std::fs::read_dir(outbox.join("questions"))
        .is_ok_and(|mut entries| entries.next().is_some());
    let repair_started = matches!(end, LaunchEnd::Exited(_)) && {
        let mut port = ConformanceRepair {
            facts: RepairFacts {
                exited: true,
                commits_since_base: true,
                result_json,
                question_file,
                repaired: false,
            },
        };
        matches!(repair_pass(&mut port), Ok(Repaired::Resumed(_)))
    };
    let mut profile_mcp: BTreeSet<String> = teammate
        .mcp_servers
        .iter()
        .flat_map(|m| m.keys().cloned())
        .collect();
    if teammate.worker_channel {
        profile_mcp.insert("horch".into());
    }
    Observation {
        end,
        first_event: seen.first_event,
        hooks: seen.hooks,
        report: seen.last_text.as_deref().and_then(read_probe_report),
        result_json,
        question_file,
        outside_written: dir.join(OUTSIDE_FILE).exists(),
        private_home,
        profile_mcp,
        compaction_setting: cli != ExecCli::Antigravity,
        stop_hook: seen.stop_hook,
        repair_started,
    }
}
