use std::path::PathBuf;

use horch_e2e::{say, scenario, scenario_has, write_skills_report, Call};
use serde_json::json;

fn main() {
    let call = Call::start("pi");
    let code = if scenario() == "crash" {
        eprintln!("pi: Node.js 22.19 or newer is required");
        1
    } else if call.argv == ["update", "--self"] {
        update(&call)
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

fn update(call: &Call) -> ! {
    call.flush();
    if horch_e2e::scenario_has("update_fail:pi") {
        eprintln!("\u{1b}[31merror: pi update failed: network unreachable\u{1b}[0m");
        std::process::exit(1);
    }
    if horch_e2e::scenario_has("update_hang:pi") {
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
    horch_e2e::say("pi 0.85.1 is up to date");
    std::process::exit(0);
}
