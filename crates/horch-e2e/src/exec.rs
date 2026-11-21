//! Exec mode of the fake agents (fleetd-p7 ticket 16).
//!
//! A fake that gets the exec argv of its adapter (tickets 13 to 15) reads
//! the path on the last line of the prompt, `inbox/START.md`, and prints
//! the stream lines of
//! `crates/horch-core/tests/fixtures/streams/<cli>-exec.jsonl`, without the
//! doc keys of the fixture. With no key it prints the progress lines, then
//! 1 result line, and exits 0.
//!
//! Scenario keys:
//! - `usage_limit`: the progress lines, then the first usage-limit line of
//!   the fixture, and exit 1.
//! - `long_context`: the progress lines, then 1 token count past 256000,
//!   and no exit. Codex prints its token count only in `turn.completed`.
//! - `exit_without_result`: the progress lines, and exit 0.
//! - `trust_prompt`: a dialog that is not a stream line, and no exit.
//! - `conformance`, and with any key the prompt of a conformance check: the
//!   probes of [`crate::probe`], and the write of
//!   `outbox/result.json` of the task folder (`result_write`). The report
//!   goes into the result line and into the log as `conformance`. For the
//!   prompt of "Stop gate" the fake obeys the prompt: the progress lines,
//!   and exit 0 with no result.
//! - `ignore_stop_gate`: with `conformance`, the fake does not obey the
//!   prompt of "Stop gate": it writes its result as for each other check.

use serde_json::{json, Value};

use horch_core::fleetd::worker_profiles::domain::conformance::STOP_GATE_PROMPT;

use crate::{hang, probe, say, scenario_has, Call};

/// The first sentence of the prompt of each conformance check
/// (`domain::conformance`, `PROBE_PROMPT` and the others).
const CHECK_PROMPT_HEAD: &str = "This is a conformance check of horch.";

/// The CLIs with an exec adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cli {
    Claude,
    Codex,
    Agy,
}

/// The token count of a `long_context` run: past the window of 256000.
pub const LONG_CONTEXT_TOKENS: u64 = 300_000;

/// What the dialog of `trust_prompt` prints.
pub const TRUST_DIALOG: &str = "Do you trust the files in this folder? 1. Yes, proceed 2. No, exit";

const CLAUDE_STREAM: &str =
    include_str!("../../horch-core/tests/fixtures/streams/claude-exec.jsonl");
const CODEX_STREAM: &str = include_str!("../../horch-core/tests/fixtures/streams/codex-exec.jsonl");
const AGY_STREAM: &str = include_str!("../../horch-core/tests/fixtures/streams/agy-exec.jsonl");

/// What 1 fixture line is for the run of a fake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A line of a run that makes progress. A token count is 1 of them.
    Progress,
    Tokens,
    UsageLimit,
    Result,
    /// An error or a warning: not in a run of a fake.
    Other,
}

impl Cli {
    /// The CLI of an exec argv, and its prompt path; `None` for any other
    /// argv. `fake` is the name of the fake that got `argv`.
    pub fn of_argv<'a>(fake: &str, argv: &'a [String]) -> Option<(Cli, &'a str)> {
        let has = |flag: &str| argv.iter().any(|a| a == flag);
        let stream_json = argv
            .windows(2)
            .any(|w| w[0] == "--output-format" && w[1] == "stream-json");
        match fake {
            // The quota probe also has `--input-format stream-json`.
            "claude" if has("-p") && stream_json && !has("--input-format") => {
                Some((Cli::Claude, argv.last()?))
            }
            "codex" if argv.first()? == "exec" && has("--json") => Some((Cli::Codex, argv.last()?)),
            "antigravity" if argv.first()? == "-p" && stream_json => Some((Cli::Agy, argv.get(1)?)),
            _ => None,
        }
    }

    /// The fixture lines of this CLI, each with its kind and without its
    /// doc keys.
    fn stream(self) -> Vec<(Kind, Value)> {
        let text = match self {
            Cli::Claude => CLAUDE_STREAM,
            Cli::Codex => CODEX_STREAM,
            Cli::Agy => AGY_STREAM,
        };
        text.lines()
            .map(|line| serde_json::from_str(line).expect("a fixture line is JSON"))
            .map(|entry| self.entry(entry))
            .collect()
    }

    fn entry(self, mut entry: Value) -> (Kind, Value) {
        match self {
            Cli::Claude => {
                let map = entry.as_object_mut().expect("a fixture line is an object");
                map.remove("_doc");
                let expect = map.remove("_expect").unwrap_or_default();
                let kind = match expect.as_str().unwrap_or_default() {
                    "activity" => Kind::Progress,
                    "result" => Kind::Result,
                    "usage_limited" => Kind::UsageLimit,
                    e if e.starts_with("context_tokens") => Kind::Tokens,
                    _ => Kind::Other,
                };
                (kind, entry)
            }
            Cli::Codex => {
                let kind = match entry["event"].as_str().unwrap_or_default() {
                    "activity" => Kind::Progress,
                    "result" => Kind::Result,
                    "usage_limited" => Kind::UsageLimit,
                    _ => Kind::Other,
                };
                (kind, entry["line"].take())
            }
            Cli::Agy => {
                entry
                    .as_object_mut()
                    .expect("a fixture line is an object")
                    .remove("_doc");
                (agy_kind(&entry), entry)
            }
        }
    }
}

fn agy_kind(line: &Value) -> Kind {
    let result = &line["result"];
    if line["event"] == "result" {
        return match result["status"].as_str() {
            Some("SUCCESS") => Kind::Result,
            _ if result["error"]
                .as_str()
                .is_some_and(|e| e.starts_with("RESOURCE_EXHAUSTED")) =>
            {
                Kind::UsageLimit
            }
            _ => Kind::Other,
        };
    }
    let step = &line["step_update"];
    if step["step_type"] == "agent_response" && step["usage"].is_object() {
        Kind::Tokens
    } else {
        Kind::Progress
    }
}

/// Run `call` in exec mode for `cli` with `prompt`: the skills briefing,
/// if any, then the path of `START.md` on its last line. The argv of `call`
/// keeps the full prompt. Never
/// returns: the fake exits or hangs as its scenario says. The Phase 4 keys
/// of [`crate::agent`] run first, so `bound` records the state at entry.
pub fn run(mut call: Call, cli: Cli, prompt: &str) -> ! {
    crate::agent::play(&mut call);
    let path = prompt.lines().last().unwrap_or(prompt);
    let (check, stop_gate) = match std::fs::read_to_string(path) {
        Ok(text) => {
            let check = text.starts_with(CHECK_PROMPT_HEAD);
            let stop_gate = text == STOP_GATE_PROMPT;
            call.extra.insert("exec_prompt".into(), Value::String(text));
            (check, stop_gate)
        }
        Err(e) => {
            call.violate(format!(
                "the exec prompt path {path} (START.md) is not a readable file: {e}"
            ));
            call.flush();
            std::process::exit(2);
        }
    };
    if scenario_has("trust_prompt") {
        call.flush();
        say(TRUST_DIALOG);
        hang();
    }
    let stream = cli.stream();
    let progress: Vec<&Value> = stream
        .iter()
        .filter(|(k, _)| matches!(k, Kind::Progress | Kind::Tokens))
        .map(|(_, v)| v)
        .collect();
    let first = |kind: Kind| {
        stream
            .iter()
            .find(|(k, _)| *k == kind)
            .map(|(_, v)| v.clone())
            .expect("the fixture has a line of each kind")
    };
    for line in &progress {
        say(&line.to_string());
    }
    // The prompt of a conformance check wins over the scenario of the
    // fleet: a test world with any scenario gets a passing conformance run,
    // so fleetd keeps its profiles healthy.
    if scenario_has("usage_limit") && !check {
        say(&first(Kind::UsageLimit).to_string());
        call.flush();
        std::process::exit(1);
    }
    if scenario_has("long_context") && !check {
        let kind = if cli == Cli::Codex {
            Kind::Result
        } else {
            Kind::Tokens
        };
        say(&past_the_window(cli, first(kind)).to_string());
        call.flush();
        hang();
    }
    // A real agent obeys the prompt of "Stop gate": it ends with no result.
    // The fake acts on the prompt of a check as a real agent does, with any
    // scenario: the production job runs each check with the scenario of
    // the fleet.
    let conformance = check || scenario_has("conformance");
    let obeys_stop_gate = stop_gate && !scenario_has("ignore_stop_gate");
    let ends_with_no_result =
        scenario_has("exit_without_result") && (!check || scenario_has("conformance"));
    if ends_with_no_result || (obeys_stop_gate && conformance) {
        call.flush();
        std::process::exit(0);
    }
    let mut result = first(Kind::Result);
    if conformance {
        let hooks = hook_events(&progress);
        let mut report = probe::report(cli, &call.argv, hooks);
        report["result_write"] = Value::String(probe::write_result(cli, &call.argv, path));
        let text = report.to_string();
        call.extra.insert("conformance".into(), report);
        match cli {
            Cli::Claude => result["result"] = Value::String(text),
            Cli::Agy => result["result"]["response"] = Value::String(text),
            Cli::Codex => {
                // The probes are commands: Codex ends each with an
                // `item.completed` of its `command_execution` item.
                say(&json!({"type": "item.completed", "item": {"id": "item_1",
                    "type": "command_execution", "command": "bash -lc ls",
                    "exit_code": 0, "status": "completed"}})
                .to_string());
                say(&json!({"type": "item.completed",
                    "item": {"id": "item_probe", "type": "agent_message", "text": text}})
                .to_string());
            }
        }
    }
    say(&result.to_string());
    call.flush();
    std::process::exit(0);
}

/// `line`, a token count of `cli`, with [`LONG_CONTEXT_TOKENS`] in the
/// context.
fn past_the_window(cli: Cli, mut line: Value) -> Value {
    let n = LONG_CONTEXT_TOKENS;
    match cli {
        Cli::Claude => {
            line["message"]["usage"] = json!({"input_tokens": n, "cache_creation_input_tokens": 0,
                "cache_read_input_tokens": 0, "output_tokens": 120});
        }
        Cli::Codex => line["usage"]["input_tokens"] = json!(n),
        Cli::Agy => {
            let usage = &mut line["step_update"]["usage"];
            usage["input_tokens"] = json!(n);
            usage["cache_read_tokens"] = json!(0);
            usage["total_tokens"] = json!(n);
        }
    }
    line
}

/// The hook events of the printed lines, each 1 time, in order.
fn hook_events(lines: &[&Value]) -> Vec<String> {
    let mut events: Vec<String> = Vec::new();
    for event in lines.iter().filter_map(|l| l["hook_event"].as_str()) {
        if !events.iter().any(|e| e == event) {
            events.push(event.to_owned());
        }
    }
    events
}
