//! The one place the `horch` binary reads its process environment.
//!
//! `main` calls [`context`] once and passes the [`RuntimeContext`] to every
//! command. A command that needs a different value (a pinned state dir, a
//! freshly created workspace) changes its own copy, never the process
//! environment.

use anyhow::Result;
use horch_core::runtime::{ProcessEnv, RuntimeContext};

/// Read the process environment into a [`RuntimeContext`], and pin the
/// shared clock to its `HORCH_NOW`.
pub fn context() -> Result<RuntimeContext> {
    let ctx = RuntimeContext::from_env(&ProcessEnv)?;
    horch_core::clock::install(ctx.settings.now, ctx.settings.now_unparsable.as_deref());
    Ok(ctx)
}
