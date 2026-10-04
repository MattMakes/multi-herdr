# R1 review-safety: cross-vendor review of the process and state safety work

Read-only review. No branch, no commits, no file writes outside /tmp.

## GOAL

A second model family reviews the riskiest changes of this run and reports
findings ranked by consequence, each with file:line and a failure scenario.

## SCOPE

On branch `design-skills` (read it in `/Users/mascott/projects/mh-wt/integ-ds`,
or `git show design-skills:<path>`):
- execution state guards: `crates/horch-core/src/execution/store.rs`
  (`mark_starting`, `mark_running`, `end_live`), `execution/service.rs`,
  `competition/coordinator.rs` `end_candidate`;
- pid identity: `crates/horch-core/src/procid.rs`, `evaluation/scheduler.rs`,
  `evaluation/validator.rs`, `harness/prime.rs`, `fsx.rs`, `telemetry/lock.rs`;
- git env scrub: `crates/horch-marketplace/src/git.rs` and every git caller.
Reports: `ai_docs/reports/design-skills/{flake-and-durable,pid-identity}.md`.

## REPORT

Send findings to the orchestrator with `horch tell orchestrator` (one
message per 3 findings at most; Simplified Technical English), then
`horch done` with the full ranked list. "No findings" is a valid result if
you say what you checked.
