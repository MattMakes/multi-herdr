pub mod finish;
pub mod git;
pub mod import;
pub mod map;
pub mod model;
pub mod orchestrator;
pub mod outbox;
pub mod prestage;
pub mod runs;
pub mod schema;
pub mod sql_words;
pub mod status;
pub mod stop;
pub mod store;
pub mod supervisor;
pub mod tasks;
pub mod timer;
pub mod triage;
pub mod util;
pub mod watchdog;
pub mod workers;
pub mod worktree_ops;

pub use store::Store;

use anyhow::Result;

use crate::runtime::RuntimeContext;

pub fn open(ctx: &RuntimeContext) -> Result<Store> {
    Store::open(&ctx.paths.ledger_db)
}

pub fn open_existing(ctx: &RuntimeContext) -> Result<Option<Store>> {
    if !ctx.paths.ledger_db.is_file() {
        return Ok(None);
    }
    open(ctx).map(Some)
}

pub fn open_existing_read_only(ctx: &RuntimeContext) -> Result<Option<Store>> {
    if !ctx.paths.ledger_db.is_file() {
        return Ok(None);
    }
    Store::open_read_only(&ctx.paths.ledger_db).map(Some)
}
