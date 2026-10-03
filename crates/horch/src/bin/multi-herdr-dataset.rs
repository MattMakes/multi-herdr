//! `multi-herdr-dataset` - competitive dataset runs (OD2). Thin: the
//! commands live in `horch::dataset`.

use std::collections::BTreeMap;
use std::process::ExitCode;

use clap::Parser;
use horch::bootstrap;
use horch::dataset::{self, cli::Cli, exit};

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = bootstrap::context().and_then(|mut ctx| {
        let env = snapshot_candidates();
        dataset::dispatch(&mut ctx, &env, cli.command)
    });
    match result {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("multi-herdr-dataset: {e:#}");
            ExitCode::from(exit::FAILURE)
        }
    }
}

/// The plain variables the environment snapshot may keep (SEC-02):
/// locale, terminal, shell and time zone, read by name. The `HORCH_*`
/// settings come from the context. No other variable is read.
fn snapshot_candidates() -> BTreeMap<String, String> {
    ["LANG", "LC_ALL", "TERM", "SHELL", "TZ"]
        .into_iter()
        .filter_map(|key| Some((key.to_string(), std::env::var(key).ok()?)))
        .collect()
}
