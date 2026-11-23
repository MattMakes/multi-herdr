use serde_json::{json, Value};

use horch_core::fleetd::worker_profiles::domain::conformance::STOP_GATE_PROMPT;

use crate::{hang, probe, say, scenario_has, Call};

const CHECK_PROMPT_HEAD: &str = "This is a conformance check of horch.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cli {
    Claude,
    Codex,
    Agy,
}

pub const LONG_CONTEXT_TOKENS: u64 = 300_000;

pub const TRUST_DIALOG: &str = "Do you trust the files in this folder? 1. Yes, proceed 2. No, exit";

const CLAUDE_STREAM: &str =
    include_str!("../../horch-core/tests/fixtures/streams/claude-exec.jsonl");
const CODEX_STREAM: &str = include_str!("../../horch-core/tests/fixtures/streams/codex-exec.jsonl");
const AGY_STREAM: &str = include_str!("../../horch-core/tests/fixtures/streams/agy-exec.jsonl");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Progress,
    Tokens,
    UsageLimit,
    Result,
    Other,
}

impl Cli {
    pub fn of_argv<'a>(fake: &str, argv: &'a [String]) -> Option<(Cli, &'a str)> {
        let has = |flag: &str| argv.iter().any(|a| a == flag);
        let stream_json = argv
            .windows(2)
            .any(|w| w[0] == "--output-format" && w[1] == "stream-json");
        match fake {
            "claude" if has("-p") && stream_json && !has("--input-format") => {
                Some((Cli::Claude, argv.last()?))
            }
            "codex" if argv.first()? == "exec" && has("--json") => Some((Cli::Codex, argv.last()?)),
            "antigravity" if argv.first()? == "-p" && stream_json => Some((Cli::Agy, argv.get(1)?)),
            _ => None,
        }
    }

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

fn hook_events(lines: &[&Value]) -> Vec<String> {
    let mut events: Vec<String> = Vec::new();
    for event in lines.iter().filter_map(|l| l["hook_event"].as_str()) {
        if !events.iter().any(|e| e == event) {
            events.push(event.to_owned());
        }
    }
    events
}
