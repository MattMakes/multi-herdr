# The ledger owns its stored window type

The ledger stores a launch's native auto-compact window decision in its
own type, `execution::legacy::LedgerWindow`
(`crates/horch-core/src/execution/legacy.rs`), at the store boundary
(review finding 4). `compaction::window` converts to and from it, and the
`From<&WindowDecision> for LedgerWindow` conversion lives in
`crates/horch-core/src/compaction/window.rs`. `execution` never imports
`compaction`.

## Consequences

- `compaction::window::WindowSource`, where a launch's window came from,
  is not persisted and has no serde derives. A rename for display reasons
  cannot change what the ledger stores.
- The serde names of `LedgerWindow` are the wire format. Renaming a Rust
  item must not change them.
