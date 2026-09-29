//! A recording stand-in for the `ollama` CLI: `ollama list` only.
//!
//! Scenarios: `ok` (the default) lists `qwen3.8:latest`; `model_missing`
//! lists another model; `crash` exits 1.

use horch_e2e::{say, scenario, Call};

fn main() {
    let call = Call::start("ollama");
    let code = match (scenario().as_str(), call.argv.first().map(String::as_str)) {
        ("crash", _) => {
            eprintln!("Error: could not connect to ollama app, is it running?");
            1
        }
        (s, Some("list")) => {
            say("NAME              ID              SIZE      MODIFIED");
            if s == "model_missing" {
                say("llama9:latest     0123456789ab    4.7 GB    2 days ago");
            } else {
                say("qwen3.8:latest    0123456789ab    5.2 GB    2 days ago");
            }
            0
        }
        _ => 0,
    };
    call.flush();
    std::process::exit(code);
}
