//! `horch worker` - the port of `scripts/herdr-fleet/worker.sh`.
//!
//! `horch spawn` writes a brief (the resolved teammate, model, task, session and
//! record ids) into the workspace mailbox and then runs this command in the
//! fresh pane. `execution::lifecycle::run_worker` registers the role, renders
//! that teammate's briefing, launches the agent CLI and records how it ended.
//!
//! The bash version `exec`ed the agent. There is no `exec` on Windows, so the
//! agent runs as a child and its exit status is propagated.

use std::path::Path;
use std::process::ExitCode;

use anyhow::Result;
use horch_core::execution::lifecycle::{run_worker, PaneWorker};
use horch_core::runtime::RuntimeContext;

pub fn worker(ctx: &mut RuntimeContext, role: &str) -> Result<ExitCode> {
    // The process's working directory, not an environment variable: the
    // agent starts here, and a relative roster path resolves from here.
    let enter_dir = |dir: &Path| Ok(std::env::set_current_dir(dir)?);
    let code = run_worker(&mut PaneWorker::new(ctx, role, &enter_dir))?;
    Ok(match code {
        0 => ExitCode::SUCCESS,
        code => ExitCode::from(code.clamp(1, 255) as u8),
    })
}
