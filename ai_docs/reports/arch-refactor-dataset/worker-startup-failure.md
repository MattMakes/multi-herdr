# U35 worker-startup-failure report

## Change

`run_worker` (`crates/horch-core/src/execution/lifecycle.rs`): after `load_brief`
succeeds, an error from `enter_context` or `register` calls
`steps.agent_exited(&brief, None)`, then returns the original error.

## Decisions

- Failure state: `Failed(AgentExited{code: None})`. It is the state a launch
  failure already gets. `record_exit` documents `None` as "never started".
  No new `FailureKind`, no new store setter, no serialization golden changed.
- The recording is best effort. A recording error is logged to stderr.
- `set_running` failure: unchanged. It is logged, the agent starts, and the
  agent's exit ends the record.
- `load_brief` failure records nothing: the record id is unknown.
- `WorkerSteps` has no new method, so no other implementor changes.

## Tests (2 added, in `crates/horch-core/tests/execution_plan.rs`)

- `arc_18_register_failure_records_failed`
- `arc_18_enter_context_failure_records_failed`

Both start from a `Starting` record, check the state is `Failed(AgentExited{None})`,
`finished_at` is set, `launch` never runs, and the original error returns.

## Gotchas

- `record_exit` leaves an orchestrator record unchanged, so an orchestrator
  that fails at startup still stays live.
- Panes: the pane stays open after the failure. The worker process exits with an error.
