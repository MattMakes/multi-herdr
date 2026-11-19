//! An in-process fleetd for the tests of a client of the session records
//! (T-884): the channel server with only the handler of `session_records`,
//! on a ledger and a fleet root in temporary folders. The `fleetd` row
//! names this process, so the client of the channel accepts the server.
//!
//! A test holds the server after its temporary folder, so that the server
//! drops first. Never point it at the live ledger.

use std::path::Path;
use std::time::Duration;

use anyhow::Result;

use crate::execution::store::{apply_session_records, session_db, session_fleet_root};
use crate::fleetd::fleet_runtime::adapters::channel_server::ChannelServer;
use crate::fleetd::fleet_runtime::adapters::token_file;
use crate::fleetd::fleet_runtime::application::requests::Handlers;
use crate::fleetd::fleet_runtime::domain::ledger::Generation;
use crate::fleetd::fleet_runtime::domain::requests::Kind;
use crate::runtime::Paths;

/// Serve `session_records` for the session records of a process with
/// `paths`, where [`crate::execution::store::ExecutionStore`] sends them.
pub fn serve_session_records(paths: &Paths) -> Result<ChannelServer> {
    let db = session_db(paths);
    serve_session_records_at(&db, &session_fleet_root(paths, &db))
}

/// Serve `session_records` for the ledger `db` on the fleet root
/// `fleet_root` until the server drops.
pub fn serve_session_records_at(db: &Path, fleet_root: &Path) -> Result<ChannelServer> {
    if let Some(dir) = db.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // The ledger and its schema first.
    drop(crate::taskledger::Store::open(db)?);
    let conn = rusqlite::Connection::open(db)?;
    conn.busy_timeout(Duration::from_secs(10))?;
    let pid = std::process::id();
    let start = horch_platform::process::start_time(pid)
        .ok_or_else(|| anyhow::anyhow!("the start time of process {pid} is unknown"))?;
    conn.execute(
        "INSERT OR REPLACE INTO fleetd (id, generation, pid, pid_start, started_at, fleet_clock_s, clock_ticked_at)
         VALUES (1, 1, ?1, ?2, ?3, 0, ?3)",
        rusqlite::params![pid, start as i64, crate::taskledger::util::now()],
    )?;
    std::fs::create_dir_all(fleet_root)?;
    token_file::ensure(fleet_root)?;
    let mut handlers = Handlers::new();
    handlers.insert(Kind::SessionRecords, |tx, p| apply_session_records(tx, p));
    ChannelServer::start(fleet_root, conn, Generation(1), handlers)
}
