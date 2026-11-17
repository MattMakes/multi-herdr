//! The adapters layer of fleet runtime: I/O: SQLite, files and processes.

pub mod clock_store;
pub mod config_file;
pub mod cursors_store;
pub mod effects;
pub mod fleetd_row;
pub mod jobs_store;
pub mod old_watchdog;
pub mod scans;
pub mod sqlite;
pub mod watchdog_duties;
