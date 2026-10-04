//! `horch doctor` - the port of the justfile's `require-herdr` recipe.
//!
//! Fails fast with a clear message when herdr is missing or its server is not
//! reachable. The old `jq` check is gone: nothing shells out to jq any more.

use anyhow::{bail, Result};
use horch_core::roster::operator_effort_warnings;
use horch_core::runtime::{process, RuntimeContext};
use horch_core::workspace::herdr::Herdr;

pub fn doctor(ctx: &RuntimeContext) -> Result<()> {
    check(ctx)?;
    println!("herdr is installed and its server is reachable.");
    let roster = super::load_roster(ctx, None)?;
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
        eprintln!(
            "roster has {} problem(s) (see horch teammates --check):",
            problems.len()
        );
        for p in &problems {
            eprintln!("  {p}");
        }
    }
    // Settings that quietly override the effort in every teammate file.
    let home = ctx.inherited.home_var.as_deref().map(std::path::Path::new);
    let codex_home = horch_core::harness::codex::codex_home(
        &ctx.paths.home,
        ctx.inherited.codex_home.as_deref(),
    );
    for w in operator_effort_warnings(
        home,
        &codex_home,
        ctx.inherited.claude_code_effort_level.as_deref(),
    ) {
        eprintln!("warning: {w}");
    }
    Ok(())
}

/// The precondition every recipe shares. Deliberately does NOT validate the
/// roster: a roster warning must not stop a fleet from launching.
pub fn check(ctx: &RuntimeContext) -> Result<()> {
    let herdr_bin = ctx.bins.harness.herdr.clone();
    let found = if herdr_bin.components().count() > 1 {
        herdr_bin.is_file()
    } else {
        process::which(
            ctx.inherited.path.as_deref(),
            ctx.inherited.pathext.as_deref(),
            &herdr_bin.to_string_lossy(),
        )
        .is_some()
    };
    if !found {
        bail!(
            "herdr CLI not found on PATH.\n\
             Install it with `herdr-install`, or see https://herdr.dev/docs/install/"
        );
    }
    if !Herdr::with_bin(&ctx.bins.harness.herdr).server_reachable() {
        bail!(
            "herdr server is not reachable (herdr workspace list failed). Checks:\n\
             \x20 - is a herdr session running? (launch the herdr app, or `herdr server` headless)\n\
             \x20 - `herdr status` shows the expected socket path; export HERDR_SESSION /\n\
             \x20   HERDR_SOCKET_PATH if you run a non-default session"
        );
    }
    Ok(())
}
