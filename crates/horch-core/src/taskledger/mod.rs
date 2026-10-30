//! The task ledger: orchestrators queue tasks, workers claim them, and every
//! finish, checkin and stop is recorded (docs/specs/task-ledger.md, #28).
//!
//! 1 SQLite file per machine ([`store::default_path`], `HORCH_LEDGER_DB`
//! overrides it). Every read-then-write runs in [`store::Store::immediate`].

pub mod finish;
pub mod git;
pub mod import;
pub mod map;
pub mod model;
pub mod runs;
pub mod schema;
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

/// Open the ledger this context names. It creates the file when it does not
/// exist; a caller that only reads uses [`open_existing`].
pub fn open(ctx: &RuntimeContext) -> Result<Store> {
    Store::open(&ctx.paths.ledger_db)
}

/// Open the ledger this context names only when its file exists. A ledger
/// that does not exist has no runs, and a read must not create it (#199).
pub fn open_existing(ctx: &RuntimeContext) -> Result<Option<Store>> {
    if !ctx.paths.ledger_db.is_file() {
        return Ok(None);
    }
    open(ctx).map(Some)
}
