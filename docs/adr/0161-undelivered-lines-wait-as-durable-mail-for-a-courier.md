# Undelivered lines wait as durable mail for a courier

A line horch cannot be sure it delivered is queued as durable mail, and a
courier process (`crates/horch/src/cmd/mail.rs`) redelivers it.
`messaging::enqueue_durable` queues the line, and the courier starts either
after the push or at once, depending on the caller. The order of queue,
push and courier start is chosen so that no line is lost and none is
delivered twice (#67, #177).

## Consequences

- For ordinary lines `Outgoing::courier_after` starts the courier only
  after the push, as before #67. An unbounded push can outlast the
  courier's first redelivery and deliver the same line twice.
- Inside a push budget (`horch done`), `messaging::queue_as` starts the
  courier before `done` marks the record done, so a process killed after
  that point still leaves the mail with a courier. The courier's first
  redelivery is 30 s after the enqueue, after the push budget ends, so it
  cannot double-deliver.
- Because `horch done` starts the courier before its own push, the
  first-push record can appear after the courier first sees the mail. Such
  mail is `IfUnpushed`: owed while no first push is recorded.
  `OrchestratorPush::push` sends the owed full-text resend only at the first
  redelivery slot (30 s after creation), so a `done` push still running is
  not doubled.
- When the courier cannot start, `enqueue_durable` with `take_back` takes
  the done report back out. `done` then fails with nothing queued, and its
  retry queues the line once. Every other durable line (a wake) stays
  queued: a wake that is neither queued nor pushed is lost (#67).
- `Courier::settle_empty`: with the queue empty, the courier releases its
  lock, looks at the queue once more and takes the lock back if mail is
  pending. A message enqueued while this courier was deciding to exit found
  the lock held, so its own courier exited at once. Without the second look
  that mail would wait for the next courier.
- `taskledger_watch::pass` shares 1 push budget across every wake of a
  watchdog pass (#67, #177). An orchestrator pane that is not ready cannot
  stall the pass. Undelivered wakes stay queued and the courier delivers
  them.
- `compact::report` sends 1 untagged line to the mailbox's `orchestrator`
  pane, not through `messaging::tell`, which would add a `[<role>]` tag. It
  queues the line as durable orchestrator mail first (#177), so the courier
  delivers it when no orchestrator pane is registered yet or the push
  fails. A queue that fails is printed, and the line is still typed.
- Task-ledger wakes follow the outbox order: write the row, send, mark
  delivered. `taskledger_wake::send_row` keys each wake's mail by the
  outbox row id (`mail::keyed`), so a replay after a crash before
  `mark_delivered` reuses the first mail file and writes no 2nd one.
- `taskledger_wake::reviews_in` (C3): when the last review of a parent is
  in, it writes the keyed `reviews-in` row, tells the author to run finish,
  then marks the row delivered. A crash before the mark sends the row to
  the orchestrator on the next drain. A failed tell to the author sends it
  to the orchestrator at once.
- An flock stays held while any copy of its file descriptor is open. A
  process spawned by another thread around the moment the courier lock is
  dropped holds a copy until it execs, so the release can come
  milliseconds late. Tests of `mail::acquire_lock` poll up to 5 s for the
  lock instead of expecting it at once.
