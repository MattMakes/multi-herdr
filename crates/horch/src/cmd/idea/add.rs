//! `horch idea add <text> | -f <file> | --direct <ticket file>`: capture an
//! idea (`docs/specs/fleetd/design.md`, "Ideas", "Features"). It sends
//! `idea_add` to fleetd and prints the id of the idea. No AI runs.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
use clap::{ArgGroup, Args};
use horch_core::digest::sha256_bytes;
use horch_core::fleetd::fleet_runtime::domain::requests::{
    AddTarget, DirectTicket, IdeaAdd, IdeaInput, Kind,
};
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger::git::Git;
use horch_core::taskledger::triage::default_branch;
use serde_json::{json, Map, Value};

const COMMAND: &str = "horch idea add";

#[derive(Args)]
#[command(group(
    ArgGroup::new("source")
        .required(true)
        .args(["text", "file", "direct"])
))]
pub struct AddArgs {
    /// The idea, in your words.
    pub text: Option<String>,
    /// Read the idea from this file.
    #[arg(short = 'f', long)]
    pub file: Option<PathBuf>,
    /// A ready ticket file: the idea skips the interview and the spec.
    #[arg(long, value_name = "TICKET_FILE")]
    pub direct: Option<PathBuf>,
    /// Add the idea to this open feature, for example `F-3`.
    #[arg(long, value_name = "FEATURE")]
    pub to: Option<String>,
    /// The priority of the idea. A higher number goes first.
    #[arg(long, value_name = "N")]
    pub priority: Option<i64>,
    /// Print the result as JSON.
    #[arg(long)]
    pub json: bool,
}

/// `horch idea add`.
pub fn run(ctx: &RuntimeContext, args: AddArgs) -> Result<ExitCode> {
    if args.direct.is_some() {
        if let Some(refused) = super::refuse_inside_fleet_attempt(ctx, "horch idea add --direct") {
            return Ok(refused);
        }
    }
    if args.priority.is_some() {
        eprintln!("{COMMAND}: --priority is refused: the idea_add request of fleetd has no priority field yet");
        return Ok(ExitCode::from(1));
    }
    let add = IdeaAdd {
        idea: input(&args)?,
        target: target(ctx, args.to)?,
    };
    let payload: Map<String, Value> = add
        .fields()
        .into_iter()
        .map(|(key, text)| (key.to_string(), json!(text)))
        .collect();
    let json = args.json;
    super::send(ctx, COMMAND, Kind::IdeaAdd, payload, |result| {
        if json {
            println!("{result}");
        } else {
            println!("{}", super::text(result, "idea"));
        }
    })
}

/// The idea: the text, the text of `-f`, or the `--direct` ticket file with
/// the sha256 of its bytes now.
fn input(args: &AddArgs) -> Result<IdeaInput> {
    if let Some(path) = &args.direct {
        let path = std::fs::canonicalize(path)
            .with_context(|| format!("reading the ticket file {}", path.display()))?;
        let bytes = std::fs::read(&path)
            .with_context(|| format!("reading the ticket file {}", path.display()))?;
        let path = path
            .to_str()
            .with_context(|| format!("the ticket file {} is not UTF-8", path.display()))?;
        return Ok(IdeaInput::Direct(DirectTicket::new(
            path,
            &sha256_bytes(&bytes).hex(),
        )?));
    }
    let text = match (&args.text, &args.file) {
        (Some(text), _) => text.clone(),
        (None, Some(file)) => std::fs::read_to_string(file)
            .with_context(|| format!("reading the idea file {}", file.display()))?,
        (None, None) => bail!("{COMMAND}: give the idea, -f <file> or --direct <ticket file>"),
    };
    if text.trim().is_empty() {
        bail!("{COMMAND}: the idea is empty");
    }
    Ok(IdeaInput::Text(text))
}

/// The feature of the idea: `--to`, else a new feature of the repository
/// of this command, from its primary branch.
fn target(ctx: &RuntimeContext, to: Option<String>) -> Result<AddTarget> {
    if let Some(feature) = to {
        return Ok(AddTarget::Join(feature));
    }
    let dir = ctx.paths.project()?;
    let git = Git::new(&ctx.bins.harness.git);
    let root = git.main_root(&dir).with_context(|| {
        format!(
            "{COMMAND}: {} is not in a git repository. A new feature needs the repository of its project; give --to <feature> to join a feature.",
            dir.display()
        )
    })?;
    let primary_branch = default_branch(&git, &root)
        .with_context(|| format!("{COMMAND}: {} has no primary branch", root.display()))?;
    let project = root
        .to_str()
        .with_context(|| format!("the path {} is not UTF-8", root.display()))?
        .to_string();
    Ok(AddTarget::New {
        project,
        primary_branch,
    })
}
