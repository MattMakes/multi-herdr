//! `horch doctor` - the port of the justfile's `require-herdr` recipe.
//!
//! Fails fast with a clear message when herdr is missing or its server is not
//! reachable. The old `jq` check is gone: nothing shells out to jq any more.

use anyhow::{bail, Result};
use horch_core::agent;
use horch_core::herdr::Herdr;
use horch_core::teammates::Roster;

pub fn doctor() -> Result<()> {
    check()?;
    println!("herdr is installed and its server is reachable.");
    let roster = Roster::load()?;
    let problems = roster.check();
    if problems.is_empty() {
        println!(
            "roster: {} teammates, {} offered to the orchestrator.",
            roster.names().len(),
            roster.offered().len()
        );
    } else {
        // Not fatal: a fleet still launches, but at least one teammate would
        // misbehave in a pane nobody is watching.
        eprintln!("roster has {} problem(s) (see horch teammates --check):", problems.len());
        for p in &problems {
            eprintln!("  {p}");
        }
    }
    Ok(())
}

/// The precondition every recipe shares. Deliberately does NOT validate the
/// roster: a roster warning must not stop a fleet from launching.
pub fn check() -> Result<()> {
    if agent::which("herdr").is_none() {
        bail!(
            "herdr CLI not found on PATH.\n\
             Install it with `herdr-install`, or see https://herdr.dev/docs/install/"
        );
    }
    if !Herdr::new().server_reachable() {
        bail!(
            "herdr server is not reachable (herdr workspace list failed). Checks:\n\
             \x20 - is a herdr session running? (launch the herdr app, or `herdr server` headless)\n\
             \x20 - `herdr status` shows the expected socket path; export HERDR_SESSION /\n\
             \x20   HERDR_SOCKET_PATH if you run a non-default session"
        );
    }
    Ok(())
}
