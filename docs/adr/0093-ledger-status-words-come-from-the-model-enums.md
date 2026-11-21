# Ledger status words come from the model enums, not SQL literals

Ledger SQL never spells a status word. A query puts `sql_words::word` or
`sql_words::list` in with `format!`, built from the enums in
`taskledger::model`, so a word added to an enum reaches every reader (#240). A
misspelled literal is valid SQL that matches 0 rows. A missing enum variant
does not compile.

## Consequences

- `tasks.held_reason` and `workers.end_reason` are free text with no `CHECK`
  list. Their words live in the `HeldReason` and `EndReason` enums, so a new
  word needs no table rebuild. Version 7 (docs/specs/trace-fixes.md section
  4) adds every field of the trace-fixes plan in 1 migration, so no 2
  tickets race on the version: the ticket fields of a task (C1), the run
  check, PR decline and orchestrator respawn of a run (C5, C7), the run gates
  (C5), the wake outbox (C3), the handoff of a worker (C4) and the session
  records that replace the JSON session ledger (D10, TRF-45).
- A held task stays `queued` and records `held_reason` (version 3, #168)
  instead of taking a new status, so the status `CHECK` list keeps its words.
  `attempt_floor` is the attempt count at the last operator requeue:
  `max_attempts` counts only the attempts after it.
- `runs::Run::steps` fails when the run's stored `finish_steps` hold a word
  this binary does not know. A finish step must never be skipped because an
  older or newer binary cannot read its name.
