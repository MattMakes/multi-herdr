# Queued tasks are planned greedily in claim order

`tasks::Board::plan` lists every queued task in claim order (priority, then
seq) with why it cannot start, `None` when it can. The tasks are chosen
greedily: each one is checked against the running tasks and the startable
ones before it. A startable task holds its slot only when its persona has a
live or starting worker, so no task waits behind one that nobody will claim.

## Consequences

- In `tasks::Board::wait`, review tasks change no code. They never conflict
  with running tasks on owned files and never wait for an exclusive task.
  An exclusive task waits for them instead.
