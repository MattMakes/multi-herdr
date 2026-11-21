# horch cost is informational and never prices unknowns as zero

`horch cost` (`crates/horch/src/cmd/cost.rs`) reports what a fleet run
cost, per worker, read from transcripts. It is spend telemetry to see where
tokens go, and never an input to routing, model or teammate choice. No
field of a report recommends anything.

## Consequences

- Every worker in the project's ledger is priced from its own harness's
  transcript (see `horch_core::usage`). Sessions that cannot be priced (no
  session id, no transcript, an unread harness, an unknown model) are
  listed separately and never folded into a total as zero, so a total is
  never silently understated.
- With no filter the command writes 3 window reports (30d, 7d, 24h, ending
  now). A window counts each call by its own time, so a session that
  started before the window shows only its calls inside it.
- The same pass audits skills: which skills each worker actually loaded,
  and which of the skills its teammate file expects it to use it never
  touched.
