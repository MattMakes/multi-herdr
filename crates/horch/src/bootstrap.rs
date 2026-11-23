use anyhow::Result;
use horch_core::runtime::{ProcessEnv, RuntimeContext};

pub fn context() -> Result<RuntimeContext> {
    context_with_backend(None)
}

pub fn context_with_backend(explicit_backend: Option<&str>) -> Result<RuntimeContext> {
    let ctx = RuntimeContext::from_env_with_backend(&ProcessEnv, explicit_backend)?;
    horch_core::clock::install(ctx.settings.now, ctx.settings.now_unparsable.as_deref());
    Ok(ctx)
}
