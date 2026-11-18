//! The adapters layer of task execution: I/O: SQLite, files and processes.

pub mod attempt_files;
pub mod ingested_store;
pub mod leases_store;
pub mod outbox_reader;
pub mod outbox_writer;
pub mod result_reader;
pub mod sqlite;
pub mod task_files;
pub mod verdict_file;
pub mod waits_store;
