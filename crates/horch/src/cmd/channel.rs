//! The worker-report channel relay exposed as an MCP stdio server.

use anyhow::{Context, Result};
use clap::ValueEnum;
use horch_core::execution::records::Ledger;
use horch_core::messaging::channel::{self, Output, ServeOptions};
use horch_core::runtime::RuntimeContext;
use horch_core::workspace::herdr::Herdr;

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Agent {
    Claude,
    Codex,
}

/// Run the channel relay until its MCP input reaches EOF.
pub fn serve(ctx: &RuntimeContext, agent: Agent) -> Result<()> {
    let workspace = ctx
        .herdr
        .workspace
        .as_ref()
        .context("HORCH_WORKSPACE_ID is unset")?
        .to_string();
    let roster = super::load_roster(ctx, None)?;
    let instructions = roster
        .base("channel")
        .context("base 'channel' does not exist in _base/")?
        .body
        .clone();
    let socket = channel::socket_path(&ctx.paths.state_root, &workspace);
    let output = match agent {
        Agent::Claude => Output::ClaudeNotification,
        Agent::Codex => {
            let ctx = ctx.clone();
            Output::CodexQueue {
                codex_bin: ctx.bins.harness.codex.clone(),
                thread: Box::new(move || thread_id(&ctx)),
            }
        }
    };
    channel::serve(
        ServeOptions {
            socket,
            output,
            instructions,
            version: env!("CARGO_PKG_VERSION").to_string(),
            connection_timeout: channel::CONNECTION_TIMEOUT,
        },
        std::io::stdin().lock(),
        std::io::stdout(),
    )
}

/// Resolve the current orchestrator thread for one Codex queue delivery.
fn thread_id(ctx: &RuntimeContext) -> Result<String> {
    if let Some(record_id) = ctx
        .worker
        .as_ref()
        .and_then(|worker| worker.record_id.as_deref())
    {
        return Ledger::open_in(ctx)?
            .get(record_id)?
            .session_id
            .filter(|id| !id.is_empty())
            .context("the ledger record has no session id");
    }

    let pane = ctx
        .herdr
        .pane
        .as_ref()
        .context("HORCH_RECORD_ID and HERDR_PANE_ID are unset")?;
    let pane = Herdr::with_bin(&ctx.bins.harness.herdr).pane_get(pane.as_str())?;
    pane.agent_session
        .as_ref()
        .and_then(agent_session_id)
        .context("herdr did not report an agent session for HERDR_PANE_ID")
}

/// Read the session id shapes supported by herdr's pane protocol.
fn agent_session_id(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(id) if !id.is_empty() => Some(id.clone()),
        serde_json::Value::Object(fields) => ["session_id", "id", "ref"]
            .into_iter()
            .filter_map(|key| fields.get(key)?.as_str())
            .find(|id| !id.is_empty())
            .map(str::to_owned),
        _ => None,
    }
}
