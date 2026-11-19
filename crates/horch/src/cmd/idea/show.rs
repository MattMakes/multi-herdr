//! `horch idea show <id> [--in-flight] [--json]`: 1 idea and what you
//! approve, and with `--in-flight` the changed files and the diffs of each
//! other feature in flight (`docs/specs/fleetd/design.md`, "Ideas";
//! "Between features"; MAP.md of fleetd-p6, Q13). The sub-orchestrator runs
//! it at plan time. It opens the ledger read-only.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{bail, Result};
use clap::Args;
use horch_core::fleetd::ideas::application::queries::{
    idea_view, in_flight_diffs, BranchDiffs, IdeaView, InFlightFeature,
};
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger;
use horch_core::taskledger::git::Git;
use serde::Serialize;

#[derive(Args)]
pub struct ShowArgs {
    /// The idea id.
    pub id: String,
    /// Also print the diffs of the features in flight, to build on them.
    #[arg(long)]
    pub in_flight: bool,
    /// Print the result as JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Serialize)]
struct Shown {
    #[serde(flatten)]
    idea: IdeaView,
    #[serde(skip_serializing_if = "Option::is_none")]
    in_flight: Option<Vec<InFlightFeature>>,
}

/// The git reads of the project repository.
struct RepoDiffs {
    git: Git,
    repo: PathBuf,
}

impl BranchDiffs for RepoDiffs {
    fn changed_files(&self, primary: &str, branch: &str) -> Result<Vec<String>> {
        let range = format!("{primary}...{branch}");
        let out = self
            .git
            .run(&self.repo, &["diff", "--name-only", &range, "--"])?;
        Ok(out.lines().map(str::to_string).collect())
    }

    fn diff(&self, primary: &str, branch: &str) -> Result<String> {
        let range = format!("{primary}...{branch}");
        self.git.run(&self.repo, &["diff", &range, "--"])
    }
}

/// `horch idea show`.
pub fn run(ctx: &RuntimeContext, args: ShowArgs) -> Result<ExitCode> {
    let Some(store) = taskledger::open_existing_read_only(ctx)? else {
        bail!("no idea {}: the ledger does not exist", args.id);
    };
    let conn = store.conn();
    let Some(idea) = idea_view(conn, &ctx.paths.fleet_root, &args.id)? else {
        bail!("no idea {}", args.id);
    };
    let in_flight = if args.in_flight {
        let diffs = RepoDiffs {
            git: Git::new(&ctx.bins.harness.git),
            repo: ctx.paths.project()?,
        };
        in_flight_diffs(conn, &diffs, &args.id)?
    } else {
        None
    };
    let shown = Shown { idea, in_flight };
    if args.json {
        println!("{}", serde_json::to_string_pretty(&shown)?);
    } else {
        print!("{}", text(&shown));
    }
    Ok(ExitCode::SUCCESS)
}

fn text(shown: &Shown) -> String {
    let i = &shown.idea;
    let mut out = format!(
        "{}  {}  idea {}\nFeature id: {}\n",
        i.id, i.feature, i.state, i.feature_id
    );
    section(&mut out, "Idea", i.idea.as_deref());
    out.push_str("\nAmendments:\n");
    if i.amendments.is_empty() {
        out.push_str("  none\n");
    }
    for a in &i.amendments {
        out.push_str(&format!("--- {}\n{}", a.name, with_newline(&a.text)));
    }
    match &i.assumptions {
        Some(a) => {
            out.push_str(&format!("\nAssumptions (sha256 {}):\n", a.sha256));
            out.push_str(&with_newline(&a.text));
        }
        None => out.push_str("\nAssumptions:\n  none yet\n"),
    }
    section(&mut out, "Plan verdict", i.plan_verdict.as_deref());
    if let Some(features) = &shown.in_flight {
        out.push_str("\nIn flight:\n");
        if features.is_empty() {
            out.push_str("  no other feature\n");
        }
        for f in features {
            out.push_str(&format!("--- {} ({})\nFiles:\n", f.feature, f.branch));
            for file in &f.files {
                out.push_str(&format!("  {file}\n"));
            }
            out.push_str(&with_newline(&f.diff));
        }
    }
    out
}

fn section(out: &mut String, title: &str, text: Option<&str>) {
    out.push_str(&format!("\n{title}:\n"));
    match text {
        Some(t) => out.push_str(&with_newline(t)),
        None => out.push_str("  none yet\n"),
    }
}

fn with_newline(text: &str) -> String {
    if text.is_empty() || text.ends_with('\n') {
        text.to_string()
    } else {
        format!("{text}\n")
    }
}
