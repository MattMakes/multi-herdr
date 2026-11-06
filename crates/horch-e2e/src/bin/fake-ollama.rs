//! A recording stand-in for the `ollama` CLI: `ollama list` only.
//!
//! Scenarios: `ok` (the default) lists `qwen3.8:latest`; `model_missing`
//! lists another model; `crash` exits 1.
//!
//! `$HORCH_FAKE_OLLAMA_DELAY_MS` makes every call wait that long before it
//! starts: a slow start, as under a first-exec scan.

use horch_e2e::{say, scenario, Call};

fn main() {
    if let Some(ms) = std::env::var("HORCH_FAKE_OLLAMA_DELAY_MS")
        .ok()
        .and_then(|ms| ms.parse().ok())
    {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }
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
