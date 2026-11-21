# Finish steps run in a detached process group

A task's finish steps run detached from the agent's command (#160 phase 2,
`crates/horch/src/cmd/taskledger_finish.rs`,
`crates/horch-core/src/taskledger/finish/detach.rs`). `horch ledger finish`
starts `horch ledger finish --run-steps` through
`taskledger_finish::spawn_run_steps` in its own process group and returns
at once. The steps' result goes to the worker as durable mail. The agent
harness ends a command by signalling its process group, and that signal
must not end the detached finish.

## Consequences

- The run file names the detached process by its pid and start time.
  `detach::record` fails when the start time cannot be read, because the
  process could not be told apart from a later one.
- `taskledger_finish::detached_run` clears the run file and the
  `FINISH_WAIT` before it mails the result. A worker that reads the mail
  and runs finish again at once must find no detached finish still
  recorded.
- On Unix `cmd::taskledger::tools` runs finish steps with the absolute
  `/bin/sh`, because a herdr pane's PATH may not reach a shell.
- `taskledger::run_steps` picks the steps of a new run from `--steps` as
  given, else the steps of `--verify`, else of `HORCH_VERIFY`, else of the
  default profile. An unparseable `HORCH_VERIFY` is an error, not a silent
  default: a default would change which checks gate landing without anyone
  noticing.
