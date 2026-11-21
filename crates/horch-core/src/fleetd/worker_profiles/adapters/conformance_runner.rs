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
    Check, Hook, HostFolder, LaunchEnd, Observation, PrivateHome, Unreachable, MARKER_SKILL,
    TEST_MCP_SERVER,
};
use crate::fleetd::worker_profiles::domain::operator_boundary::{Act, OperatorPaths};
use crate::harness::launch::{self, LaunchRequest};
use crate::harness::HarnessKind;
use crate::roster::{validation, Teammate};
use crate::runtime::RuntimeContext;
use crate::taskledger::supervisor::{AgentFacts, AttemptFiles};

const OUTSIDE_FILE: &str = "horch-conformance-outside";

const ROLE: &str = "conformance";

pub fn reference_profile(cli: ExecCli) -> Teammate {
    let (name, agent, model) = match cli {
        ExecCli::Claude => ("conformance-claude", HarnessKind::Claude, "opus"),
        ExecCli::Codex => ("conformance-codex", HarnessKind::Codex, "gpt-5.6-sol"),
        ExecCli::Antigravity => (
            "conformance-antigravity",
            HarnessKind::Antigravity,
            "gemini-3.8-flash",
        ),
    };
    let mut t = Teammate {
        name: name.into(),
        agent,
        model: Some(model.into()),
        effort: Some("medium".into()),
        ..Teammate::default()
    };
    if cli == ExecCli::Claude {
        t.sandbox = Some(serde_json::Map::new());
    }
    let mut v = serde_json::to_value(&t).expect("a teammate serializes");
    v["mode"] = json!("exec");
    v["permission_mode"] = json!("auto");
    if cli != ExecCli::Antigravity {
        v["phase"] = json!("implementation");
    }
    serde_json::from_value(v).expect("a teammate with mode exec deserializes")
}

pub struct ConformanceRunner<'a> {
    pub ctx: &'a RuntimeContext,
    pub teammate: &'a Teammate,
    pub fleet_window: Option<(u64, String)>,
    pub env_rule: Vec<(String, String)>,
    pub scratch: PathBuf,
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

    fn operator_boundary(&mut self) -> Result<Vec<Unreachable>, String> {
        let root = &self.ctx.paths.fleet_root;
        let boundary =
            validation::operator_boundary_with_bins(self.teammate, root, &self.ctx.bins.harness);
        boundary.map_err(|e| e.to_string())?;
        Ok(operator_reach(&OperatorPaths::of(root)))
    }
}

impl ConformanceRunner<'_> {
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
        let setup = self.setup(dir, check)?;
        let ctx = &setup.ctx;
        let files = &setup.files;
        let session = SessionMode::Fresh(None);
        let req = setup.request(self, &session);
        let facts = AgentFacts::new(None);
        let mut home = PrivateHome::Missing("the launch did not start".into());
        let mut marker = None;
        let watched = launch::run_exec_flow(ctx, req, files.dir(), |cmd| {
            (home, marker) = mark_private_home(cli, &cmd, ctx)?;
            run_watched(cmd, cli, files, &facts, self.first_event_limit)
        });
        if let Some(marker) = marker {
            let _ = std::fs::remove_dir_all(marker);
        }
        let watched = watched?;
        let fleet_root = host_folder(&self.ctx.paths.fleet_root);
        Ok(observe(
            watched,
            files,
            dir,
            home,
            &setup.teammate,
            cli,
            fleet_root,
        ))
    }

    fn setup(&self, dir: &Path, check: Check) -> Result<Setup> {
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
        let operator = OperatorPaths::of(&self.ctx.paths.fleet_root);
        let start_md = files.dir().join("inbox/START.md");
        std::fs::write(&start_md, check.prompt(&operator))
            .with_context(|| format!("writing {}", start_md.display()))?;
        let prompt = start_md
            .to_str()
            .context("the throwaway folder is not UTF-8")?
            .to_owned();
        let mut ctx = self.ctx.clone();
        ctx.paths.cwd = Some(work);
        Ok(Setup {
            files,
            ctx,
            prompt,
            teammate: self.profile(),
        })
    }

    pub fn adapter_hash(&self) -> Result<String> {
        let cli = ExecCli::of(self.teammate).with_context(|| {
            format!(
                "the profile {} has no exec launch: its mode is interactive, or its CLI has no exec adapter",
                self.teammate.name
            )
        })?;
        let dir = self
            .scratch
            .join(format!("conformance-{}", uuid::Uuid::new_v4().simple()));
        let rendered = self.render_in(&dir, cli);
        let _ = std::fs::remove_dir_all(&dir);
        Ok(crate::digest::sha256_bytes(rendered?.as_bytes()).hex())
    }

    fn render_in(&self, dir: &Path, cli: ExecCli) -> Result<String> {
        let setup = self.setup(dir, Check::LaunchHeadless)?;
        let session = SessionMode::Fresh(None);
        let req = setup.request(self, &session);
        let bundles = setup.ctx.paths.state_root.join("skill-bundles");
        launch::run_exec_flow(&setup.ctx, req, setup.files.dir(), |cmd| {
            rendered_launch(cli, &cmd, dir, &bundles)
        })
    }
}

struct Setup {
    files: AttemptFiles,
    ctx: RuntimeContext,
    prompt: String,
    teammate: Teammate,
}

impl Setup {
    fn request<'a>(
        &'a self,
        runner: &ConformanceRunner<'_>,
        session: &'a SessionMode,
    ) -> LaunchRequest<'a> {
        LaunchRequest {
            role: ROLE,
            teammate: &self.teammate,
            session,
            prompt: &self.prompt,
            model_override: None,
            exec_rules: &[],
            child_env: runner.env_rule.clone(),
            record: None,
            fleet_window: runner.fleet_window.clone(),
            briefing_skill: None,
        }
    }
}

fn config_file(cli: ExecCli, cmd: &Command) -> Option<(PathBuf, PathBuf)> {
    let env = |key: &str| {
        cmd.get_envs()
            .find(|(k, _)| *k == key)
            .and_then(|(_, v)| v)
            .map(PathBuf::from)
    };
    match cli {
        ExecCli::Claude => None,
        ExecCli::Codex => env("CODEX_HOME").map(|h| (h.join("config.toml"), h)),
        ExecCli::Antigravity => {
            env("HOME").map(|h| (h.join(".gemini/antigravity-cli/settings.json"), h))
        }
    }
}

fn rendered_launch(cli: ExecCli, cmd: &Command, dir: &Path, bundles: &Path) -> Result<String> {
    let mut text = String::new();
    text.push_str(&cmd.get_program().to_string_lossy());
    for arg in cmd.get_args() {
        text.push('\n');
        text.push_str(&arg.to_string_lossy());
    }
    let mut homes = Vec::new();
    if let Some((file, home)) = config_file(cli, cmd) {
        if let Ok(config) = std::fs::read_to_string(&file) {
            text.push_str("\n--- config\n");
            text.push_str(&config);
        }
        homes.push(home);
    }
    if let Some(bundle) = plugin_dir(cmd) {
        homes.push(bundle);
    }
    if let Ok(entries) = std::fs::read_dir(bundles) {
        homes.extend(entries.filter_map(|e| e.ok()).map(|e| e.path()));
    }
    homes.push(dir.to_path_buf());
    homes.sort_by_key(|p| std::cmp::Reverse(p.as_os_str().len()));
    for (path, word) in homes.iter().map(|p| {
        let word = if p.as_path() == dir {
            "<throwaway>"
        } else {
            "<home>"
        };
        (p.to_string_lossy().into_owned(), word)
    }) {
        text = text.replace(&path, word);
    }
    Ok(text)
}

fn plugin_dir(cmd: &Command) -> Option<PathBuf> {
    let args: Vec<_> = cmd.get_args().collect();
    args.windows(2)
        .rev()
        .find(|w| w[0] == "--plugin-dir")
        .map(|w| PathBuf::from(w[1]))
}

const CHANNEL_CONNECT_LIMIT: Duration = Duration::from_secs(5);

fn operator_reach(operator: &OperatorPaths) -> Vec<Unreachable> {
    let token = std::fs::File::open(&operator.token)
        .and_then(|f| f.metadata())
        .map_err(|e| e.to_string())
        .and_then(|m| match m.len() {
            0 => Err("the token file is empty".to_owned()),
            _ => Ok(()),
        })
        .map_err(|why| (why, false));
    let deadline = std::time::Instant::now() + CHANNEL_CONNECT_LIMIT;
    let channel = horch_platform::channel::Stream::connect(&operator.channel, deadline)
        .map(drop)
        .map_err(|e| {
            let no_server = matches!(
                e.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
            );
            (e.to_string(), no_server)
        });
    let requests = match operator.requests.is_dir() {
        true => probe_write(&operator.requests),
        false => Err(format!("{} is not a folder", operator.requests.display())),
    }
    .map_err(|why| (why, false));
    [
        (Act::ReadToken, token),
        (Act::ConnectChannel, channel),
        (Act::WriteRequests, requests),
    ]
    .into_iter()
    .filter_map(|(act, reach)| {
        reach.err().map(|(why, no_server)| Unreachable {
            act,
            why,
            no_server,
        })
    })
    .collect()
}

fn probe_write(folder: &Path) -> std::result::Result<(), String> {
    let probe = folder.join(format!(
        ".conformance-reach-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .and_then(|f| {
            drop(f);
            std::fs::remove_file(&probe)
        })
        .map_err(|e| e.to_string())
}

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
            let dir = plugin_dir(cmd);
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

#[derive(Debug, Default)]
struct Seen {
    first_event: bool,
    no_event_in_time: bool,
    dialog: Option<String>,
    hooks: BTreeSet<Hook>,
    stop_hook: bool,
    last_text: Option<String>,
}

struct Watched {
    end: ExecEnd,
    seen: Seen,
}

enum Signal {
    FirstEvent,
    Done,
}

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

fn end_cli(facts: &AgentFacts) {
    if let Some(pid) = facts.pid() {
        let _ = horch_platform::group::kill_group_id(pid);
    }
}

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

#[derive(Default)]
struct Acts {
    hooks: Vec<Hook>,
    stop_hook: bool,
    text: Option<String>,
}

const CODEX_NOT_TOOLS: [&str; 2] = ["agent_message", "reasoning"];

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

fn observe(
    watched: Watched,
    files: &AttemptFiles,
    dir: &Path,
    private_home: PrivateHome,
    teammate: &Teammate,
    cli: ExecCli,
    fleet_root: HostFolder,
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
        fleet_root,
    }
}

fn host_folder(folder: &Path) -> HostFolder {
    let id = std::fs::metadata(folder)
        .map_err(|e| e.to_string())
        .and_then(|m| horch_platform::fs::device_and_inode(&m).map_err(|e| e.to_string()))
        .map(|(dev, ino)| format!("{dev}:{ino}"));
    HostFolder {
        path: folder.display().to_string(),
        id,
    }
}
