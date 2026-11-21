# Context readers treat harness data as untrusted

The telemetry and context readers (`crates/horch-core/src/telemetry/`) read
data that other programs wrote, such as OpenCode's `opencode.db`, and run
inside sandboxed workers. They treat that data as outside input. They
validate it before use, read what it says by name, and keep working when
they cannot write.

## Consequences

- The OpenCode context reader puts the session id into the SQL text. An id
  that is not `ses_<alnum>` is refused before it reaches the SQL.
- `telemetry/readers.rs` matches `FreeUsageLimitError` by name. The `error`
  field of an OpenCode message row is external data from OpenCode's
  database, not a Rust error this code produced, and its name is the only
  signal. The ARC-22 scan in `tests/arch_scan.rs` allows this match. Each
  entry in that allow list carries the reason it is not a Rust error value,
  and no entry is added without one.
- In a sandboxed Codex worker the context marker cache dir cannot be
  created or written (a read-only parent). `telemetry::context::read_current`
  still returns a reading when the cache is unwritable.
