# Courier redelivery is bounded and never floods the orchestrator

The courier in `crates/horch/src/cmd/mail.rs` (`OrchestratorPush`) sends a
message's full text to the orchestrator 1 more time only when the message
had no first push or its first push can have missed (no reachable pane, or
`/login`). All other unread mail is summarised in an occasional single
digest line. A digest is not mail and is never redelivered. This follows
the incident of 2026-11-05: a reachable orchestrator holding 25 unread
lines was flooded by full-text resends. Such an orchestrator now gets
digests only: no full text again and no NOTE.

## Consequences

- A held pane can miss hours of scheduled retries. `mail::redeliver_due`
  skips the missed slots (`first_attempt_after`) instead of replaying them
  on consecutive courier polls.
- `redeliver_due` anchors the next retry to the time of the actual
  attempt, failed pushes included, so a slow herdr call or a suppressed
  pane cannot build a backlog of due retries.
