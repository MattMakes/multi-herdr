# A wake is an outbox row first and a send second

`taskledger::outbox` is the wake outbox (trace-fixes C3, D7). `outbox::write`
adds the wake row in the transaction of the ledger change that causes it and
never sends inside that transaction. `outbox::drain` sends after the commit:
it gives each undelivered row to a sender, oldest first, and sets
`delivered_at` only when the send took it. A failed send leaves the row for
the next drain, and a rolled-back change leaves no wake.

## Consequences

- `write` returns `Some(id)` for a new row and `None` when a row with the
  same dedup key exists already.
- `watchdog::flag_unclaimed` records the flag of an unclaimed spawn and
  returns its `unclaimed-spawn` wake in the caller's transaction, `None` when
  a flag exists already. Its subject is `asked:<worker>`, the subject of the
  pre-claim sweep in `workers::sweep_preclaim`, so the dedup key keeps the 2
  passes from waking the orchestrator twice about 1 worker.
- 1 drain runs at a time per ledger file: 2 drains would read the same
  undelivered rows and send each wake twice. A drain that finds the lock held
  sends nothing and reports busy.
- The held drain lock (`outbox::Held`) unlocks explicitly on drop. A flock
  belongs to the open file description, and a child forked by another thread
  holds a copy of the descriptor until its exec. Only an unlock, not a close,
  frees the lock at once.
