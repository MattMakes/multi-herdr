use std::path::Path;
use std::process::ExitCode;

use anyhow::Result;
use horch_core::execution::lifecycle::{run_worker, PaneWorker};
use horch_core::runtime::RuntimeContext;

pub fn worker(ctx: &mut RuntimeContext, role: &str) -> Result<ExitCode> {
    let enter_dir = |dir: &Path| Ok(std::env::set_current_dir(dir)?);
    let code = run_worker(&mut PaneWorker::new(ctx, role, &enter_dir))?;
    Ok(match code {
        0 => ExitCode::SUCCESS,
        code => ExitCode::from(code.clamp(1, 255) as u8),
    })
}
