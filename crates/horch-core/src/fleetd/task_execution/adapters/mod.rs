//! The adapters layer of task execution: I/O: SQLite, files and processes.

pub mod ingested_store;
pub mod leases_store;
pub mod result_reader;
pub mod sqlite;
pub mod task_files;
pub mod verdict_file;
pub mod waits_store;
