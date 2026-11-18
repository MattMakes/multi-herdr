//! `horch ideas [--json]`: the board of every idea, its state and what it
//! waits on, then the open needs-you items (`docs/specs/fleetd/design.md`,
//! "Ideas"; OR-3). It opens the ledger read-only.

use std::process::ExitCode;

use anyhow::Result;
use clap::Args;
use horch_core::fleetd::ideas::application::queries::{idea_board, IdeaBoard};
use horch_core::runtime::RuntimeContext;
use horch_core::taskledger;

#[derive(Args)]
pub struct BoardArgs {
    /// Print the board as JSON.
    #[arg(long)]
    pub json: bool,
}

/// `horch ideas`.
pub fn run(ctx: &RuntimeContext, args: BoardArgs) -> Result<ExitCode> {
    let board = match taskledger::open_existing_read_only(ctx)? {
        Some(store) => idea_board(store.conn())?,
        None => IdeaBoard {
            ideas: Vec::new(),
            needs_you: Vec::new(),
        },
    };
    if args.json {
        println!("{}", serde_json::to_string_pretty(&board)?);
    } else {
        print!("{}", text(&board));
    }
    Ok(ExitCode::SUCCESS)
}

/// 1 line for each idea, then the section "Needs you".
fn text(board: &IdeaBoard) -> String {
    let mut out = String::new();
    if board.ideas.is_empty() {
        out.push_str("No idea in flight.\n");
    }
    for i in &board.ideas {
        out.push_str(&format!(
            "{}  {}  idea {}  waits on {} since {}\n",
            i.id, i.feature, i.state, i.waits_on, i.since
        ));
    }
    out.push_str("\nNeeds you:\n");
    if board.needs_you.is_empty() {
        out.push_str("  nothing\n");
    }
    for n in &board.needs_you {
        out.push_str(&format!("  {}  {}  since {}\n", n.key, n.text, n.since));
    }
    out
}
