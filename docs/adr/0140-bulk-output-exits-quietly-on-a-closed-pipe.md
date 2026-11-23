# Bulk output exits quietly on a closed pipe

Commands that write bulk output (`horch sessions` and the like) write
through `output::print` (`crates/horch/src/output.rs`), not `println!`.
`horch sessions | head` closes the pipe early. Rust ignores SIGPIPE, so the
write returns `BrokenPipe` and `println!` panics with a backtrace, an
alarming result for an ordinary shell idiom. Reading a long ledger through
`head` or `less` is exactly what these commands are for.

## Consequences

- `output::print` exits 0 quietly on `BrokenPipe` and 1 on any other write
  error.
