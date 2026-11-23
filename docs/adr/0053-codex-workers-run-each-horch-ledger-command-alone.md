# Codex workers run each horch ledger command alone

A `horch ledger` command after `;` or `&&`, or with a pipe, is not matched
by the codex execpolicy prefix rule, runs in the sandbox and cannot open
the ledger (#181). So every codex worker briefing carries the line
`LEDGER_ALONE` ("Run each horch ledger command alone."), and
`crates/horch-core/src/roster/validation.rs` checks for it.

## Consequences

- Without the line, `teammates --check` names each codex worker that lacks
  it, and no claude worker.
