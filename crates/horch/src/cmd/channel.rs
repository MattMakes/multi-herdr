//! The worker-report channel relay exposed as an MCP stdio server.

use anyhow::{Context, Result};
use horch_core::execution::records::SessionLedger;
use horch_core::harness::HarnessKind;
use horch_core::messaging::channel::{self, ServeOptions};
use horch_core::runtime::RuntimeContext;
use horch_core::workspace::PaneHost;

/// Run the channel relay until its MCP input reaches EOF.
pub fn serve(ctx: &RuntimeContext, agent: HarnessKind) -> Result<()> {
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
    let thread_ctx = ctx.clone();
    let output = agent.channel_output(
        ctx.bins.harness.codex.clone(),
        Box::new(move || thread_id(&thread_ctx)),
    )?;
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
        return SessionLedger::open_in(ctx)?
            .get(record_id)?
            .session_id
            .filter(|id| !id.is_empty())
            .context("the ledger record has no session id");
    }

    if ctx.herdr.backend == horch_core::runtime::Backend::Hc {
        anyhow::bail!("hc reports no agent session; HORCH_RECORD_ID is needed");
    }

    let pane = ctx
        .herdr
        .pane
        .as_ref()
        .context("not inside a herdr pane or a Herdr app terminal (HERDR_PANE_ID and HC_TERMINAL_ID are unset) and HORCH_RECORD_ID is unset")?;
    let pane = PaneHost::of(ctx).pane_get(pane.as_str())?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use horch_core::runtime::MapEnv;

    #[test]
    fn hcb_11_channel_thread_id_on_hc_requires_record_id() {
        let tmp = tempfile::tempdir().unwrap();
        let env = MapEnv::new(tmp.path())
            .with("HOME", &tmp.path().to_string_lossy())
            .with("HC_TERMINAL_ID", "t1");
        let ctx = RuntimeContext::from_env(&env).unwrap();
        let err = thread_id(&ctx).unwrap_err().to_string();
        assert_eq!(
            err,
            "hc reports no agent session; HORCH_RECORD_ID is needed"
        );
    }
}
