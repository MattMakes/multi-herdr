# Workers reach horch by bare name on PATH

The briefing tells the agent to run `horch tell`, `horch note` and
`horch done` by bare name. `PaneWorker` in
`crates/horch-core/src/execution/lifecycle.rs` therefore prepends the
running horch binary's directory to the agent's PATH. Without it a worker
launched from a build directory would have no channel at all.

## Consequences

- `horch_command` relies on `std::process::Command` searching the PATH
  given to the child, not the parent's. The prepended PATH also decides
  which `horch` runs.
- The smoke teammate (`run_smoke`) is a fake agent that verifies the
  machinery (spawn, register, ledger, tell, self-close) without spending any
  tokens. It invokes `horch` subprocesses by bare name on purpose: that is
  exactly how a real agent calls them from its shell tool, so the check also
  proves `horch` is reachable on the PATH the briefings promise.
- `run_smoke` sets `HORCH_TELL_GRACE_MS=0`. The smoke orchestrator is a
  plain shell where no agent ever shows, so `done`'s report must not wait
  for one (TELL_GRACE).
