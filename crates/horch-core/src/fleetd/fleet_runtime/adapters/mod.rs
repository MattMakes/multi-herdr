//! The adapters layer of fleet runtime: I/O: SQLite, files and processes.

pub mod channel_client;
pub mod channel_server;
pub mod clock_store;
pub mod config_file;
pub mod cursors_store;
pub mod effects;
pub mod fleetd_row;
pub mod job_result_file;
pub mod jobs_store;
pub mod old_watchdog;
pub mod peer_process;
pub mod polite_stops_store;
pub mod protected_files;
pub mod request_spool;
pub mod request_wire;
pub mod requests_store;
pub mod scans;
pub mod sqlite;
pub mod test_fleetd;
pub mod token_file;
pub mod watchdog_duties;
