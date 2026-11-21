# herdr's live answer under a deadline decides what a pane is

horch asks herdr whether a pane exists and what it runs, and trusts that
answer over any file it recorded. Every such call has a deadline, because
herdr can accept a call and never answer. In
`crates/horch/src/cmd/messaging.rs` `inbox` and `peek` wrap every herdr
call in a deadline (`pane_list_timeout`). A call killed at the deadline
reads every pane as unknown, so a hung herdr never keeps pending mail from
printing.

## Consequences

- herdr can hang while its server restarts, which is exactly when
  `restore::adopt_restored_identity` runs. Its `pane_get` call uses a 2 s
  timeout.
- A mailbox pane id file can outlive its herdr pane. The courier in
  `cmd/mail.rs` treats a pane that herdr does not answer `pane_get` for as
  no target, and `deliver_roles` skips a role whose pane herdr no longer
  answers for.
- herdr pane ids have the form `<workspace>:p<n>`.
  `telemetry::workspace_of` uses herdr's answer, else the recorded one, and
  a pane herdr no longer reports still names its workspace through its id.
- herdr reports the caller's pane in its internal form (`p1`), while the
  mailbox registers the public form (`w1:p1`). `messaging::may_acknowledge`
  compares both forms. A pane outside the fleet (`caller` unset) or a fleet
  with no registered orchestrator may acknowledge the orchestrator's mail.
  Any other pane that is not the registered orchestrator's may not, so a
  resumed worker pane cannot consume it (#125).
