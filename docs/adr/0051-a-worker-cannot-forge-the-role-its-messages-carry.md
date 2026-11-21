# A worker cannot forge the role its messages carry

The `[<role>]` tag is the messaging channel's 1 trust boundary: a worker
line carries a `[<role>]` tag, and everything else is the human operator.
`crates/horch-core/src/messaging/message.rs` decides what `horch tell` types
into a pane, as opposed to how it types it. A worker's message always
starts with its own tag, even when the worker forgot it. A sandboxed pane's
role comes from its parent process, never from the pane
(`crates/horch-core/src/messaging/report.rs`).

## Consequences

- A message longer than `MAX_INLINE` (600 characters) is not typed at all.
  It is spooled to a file, and the pane gets 1 short tagged line that names
  the file. A short line is far less exposed to a busy TUI splitting it,
  and even a split one starts with the tag.
- `spool_dir` keeps the messages delivered by reference in 1 directory per
  workspace under `state_root/messages` (`RuntimeContext::paths.state_root`).
  A fleet never lists another fleet's messages, because role names repeat
  across fleets.
- The sandboxed agent of a `horch_access: report` pane
  (`docs/security/sandboxed-pane-horch-access.md`, C1) reaches no horch
  state: no state root, no ledger folder, no relay or herdr socket. Its
  `horch worker` parent, which runs outside the sandbox, serves 1 Unix
  socket for the pane, and the `horch` CLI in the pane talks only to it
  (`ENV`, `HORCH_REPORT_SOCKET`).
- The report protocol is 1 request per connection. The client writes 1 JSON
  line of at most `MAX_REQUEST` (16 KiB) bytes, then reads 1 JSON `Reply`
  line. `{"verb":"progress","text":..}` makes the parent record ledger
  progress for its own worker row. `{"verb":"tell","to":..,"text":..}` makes
  the parent tell the brief's controller, or else its report target
  (`target`), tagged with the brief's role. Any other `to` is refused.
  `{"verb":"done","text":..}` makes the parent run `horch done` for its own
  record. Every other verb or key, a line over the limit and an empty text
  are refused. So a sandboxed agent cannot act as another worker.
- `report::send` accepts an empty reply as success only for a `done`
  request. A `horch done` that works closes the pane, and the parent dies
  with it before it can answer. Every other verb needs an answer and fails
  on an empty reply.
