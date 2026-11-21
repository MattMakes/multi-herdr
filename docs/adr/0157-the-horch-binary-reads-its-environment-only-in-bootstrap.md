# The horch binary reads its environment only in bootstrap

The `horch` binary reads its process environment in exactly 1 place,
`bootstrap::context_with_backend` (`crates/horch/src/bootstrap.rs`).
`main` calls it once and passes the resulting `RuntimeContext` to every
command. A command that needs a different value (a pinned state dir, a
freshly created workspace) changes its own copy, never the process
environment.
