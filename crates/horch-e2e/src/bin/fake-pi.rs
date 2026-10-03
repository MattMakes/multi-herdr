//! A recording stand-in for the `pi` CLI. Only `--version` is answered: the
//! local-pool health check is the one thing horch asks of pi outside a pane.
//!
//! Scenarios: `ok` (the default), `crash` (exit 1), `model_missing` (as `ok`;
//! the missing model is fake-ollama's half of that scenario), and
//! `inspect_skills`: a launch also writes its `--skill` dirs (the options
//! before `--`) to `$HORCH_FAKE_LOG.skills.json`
//! (`horch_e2e::write_skills_report`).

use std::path::PathBuf;

use horch_e2e::{say, scenario, scenario_has, write_skills_report, Call};
use serde_json::json;

fn main() {
    let call = Call::start("pi");
    let code = if scenario() == "crash" {
        eprintln!("pi: Node.js 22.19 or newer is required");
        1
    } else if call.argv.iter().any(|a| a == "--version") {
        say("0.85.1");
        0
    } else {
        if scenario_has("inspect_skills") {
            let end = call
                .argv
                .iter()
                .position(|a| a == "--")
                .unwrap_or(call.argv.len());
            let mut dirs = Vec::new();
            let mut flags = Vec::new();
            for pair in call.argv[..end].windows(2).filter(|w| w[0] == "--skill") {
                dirs.push(PathBuf::from(&pair[1]));
                flags.extend(pair.iter().cloned());
            }
            write_skills_report("pi", &dirs, json!({ "flags": flags }));
        }
        0
    };
    call.flush();
    std::process::exit(code);
}
