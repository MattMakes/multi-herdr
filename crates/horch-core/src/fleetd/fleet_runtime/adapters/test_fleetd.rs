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

pub fn serve_session_records(paths: &Paths) -> Result<ChannelServer> {
    let db = session_db(paths);
    serve_session_records_at(&db, &session_fleet_root(paths, &db))
}

pub fn serve_session_records_at(db: &Path, fleet_root: &Path) -> Result<ChannelServer> {
    if let Some(dir) = db.parent() {
        std::fs::create_dir_all(dir)?;
    }
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
