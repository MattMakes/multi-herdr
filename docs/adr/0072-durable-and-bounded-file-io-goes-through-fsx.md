# Durable and bounded file I/O goes through fsx

Every write that must survive a crash goes through
`crates/horch-core/src/fsx.rs`. `write_atomic` writes a temp file in the
same directory, calls `sync_all`, renames it over the target, then fsyncs
the parent directory (`sync_dir`) so the rename itself is on disk.
`create_immutable` is the write-once variant for content-addressed files.
The same module reads the settings files that a repository or the operator
controls (`read_regular_bounded`), without blocking and without unbounded
memory.

## Consequences

- `sync_dir` fsyncs the directory on unix so a rename or create inside it
  is durable. Windows cannot open a directory for sync, so there it is a
  no-op.
- `create_immutable` writes the bytes to a temp file and hard-links it to
  the target. The link is atomic and fails when the target exists, so a
  reader sees no file or the whole file, never an empty or half-written
  one. An existing file with the same bytes is accepted. One with
  different bytes is an `FsxError::Conflict`. A reader once polled the
  judge's `output.json` between its create and its write, parsed an empty
  answer and ran the judge again.
- `create_new_file` makes a file private from its first moment: on unix
  the mode is set by `open`, so the file never exists with umask
  permissions.
- `write_temp` calls `set_mode` after the create, because the umask may
  have cleared mode bits the caller asked for.
- `read_regular_bounded` opens the file non-blocking on unix, requires a
  regular file and reads at most `READ_LIMIT` (1 MiB) bytes. A FIFO
  planted at a settings path then opens at once instead of waiting for a
  writer. A link is followed. `Ok(None)` means there is nothing to read:
  no file, no access, or not UTF-8 text.
