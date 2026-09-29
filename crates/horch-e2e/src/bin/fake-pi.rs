//! A recording stand-in for the `pi` CLI. Only `--version` is answered: the
//! local-pool health check is the one thing horch asks of pi outside a pane.
//!
//! Scenarios: `ok` (the default), `crash` (exit 1), `model_missing` (as `ok`;
//! the missing model is fake-ollama's half of that scenario).

use horch_e2e::{say, scenario, Call};

fn main() {
    let call = Call::start("pi");
    let code = if scenario() == "crash" {
        eprintln!("pi: Node.js 22.19 or newer is required");
        1
    } else if call.argv.iter().any(|a| a == "--version") {
        say("0.85.1");
        0
    } else {
        0
    };
    call.flush();
    std::process::exit(code);
}
