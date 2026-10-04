# D18 flake-and-durable: cmp_05 root cause, one replace function, doc warnings

Branch `ds/flake-and-durable`. Commits:

- `Execution: Keep a worker's state when the spawner records its pane late`
- `Fsx: Delete replace_durable; write_atomic is the replace discipline`
- `Docs: Wrap the bare URLs that cargo doc warns about`
- `Competition: Drop clones of Copy values; name the CAS read-tree publish`
- `Tests: Plan coordinator rounds from a fixed round id`
- `Reports: Add the D18 flake-and-durable report`

## 1. cmp_05: a product bug, not a flaky test

### Symptom

`cmp_05_e2e_candidates_in_dataset_workspace` sometimes saw 1
`candidate.completed` instead of 2 under load. The run exited 0 and the round
was `DECIDED`.

### Root cause

`ExecutionService::spawn` (`execution/service.rs`) does these steps in order:
split the pane, `pane run` the worker command, then
`ExecutionStore::mark_starting`. The worker starts when `pane run` answers.
`mark_starting` set the record to `Starting` unconditionally. When the spawner
was slow to get the ledger lock, the worker had already written `Running`,
and a fast agent had already run `horch done` (`Done`). `mark_starting` then
took the record back to `Starting` and cleared `finished_at`. `horch done`
had closed the pane. The coordinator then saw a `Starting` record with no
pane. It read the record again, still saw `Starting`, and recorded
`candidate.failed {pane_vanished}` for a candidate that had finished.

In cmp_05, candidate B does not commit, so it finishes fastest and is the one
that is lost.

### Evidence

| Run | Load | Result |
|---|---|---|
| Before: `cmp_05` alone, 30 times, 18 `yes` processes | 42 | 0 of 30 failed |
| Before: the full `dataset` suite (21 tests), 8 times, 18 `yes` processes | 44 to 52 | 1 of 8 failed: `cmp_05` |
| Before: `cmp_05` with a temporary 3 s sleep before `mark_starting` | any | 3 of 3 failed (both candidates lost, `REJECTED: no_eligible`, exit 6) |
| After: `cmp_05` with the same temporary 3 s sleep | any | 5 of 5 passed |
| After: `cmp_05` alone, 40 times, 18 `yes` processes (mark_starting fix only) | 47 | 0 of 40 failed |
| After: `cmp_05` alone, 40 times, 18 `yes` processes (final code) | 42 | 0 of 40 failed |
| After: the full `dataset` suite, 12 times, 18 `yes` processes (final code) | 41 to 46 | 0 of 12 failed |

The event trail of the failure before the fix (temporary logging, removed):

```
02:13:19.863 candidate.spawned A
02:13:22.946 candidate.spawned B      (emitted after mark_starting)
02:13:23.689 candidate.completed A
02:13:24.077 candidate.failed B {"kind": "pane_vanished"}
...
02:13:27.973 round.completed           exit 0, DECIDED: winner A
```

The 3 s gap between the 2 spawns shows how slow the spawner was.

The test's deadline and poll were not the cause. The candidate deadline is
60 s, and the failure came 1.1 s after the spawn.

### Fix

The fix is in the store. The orchestrator approved option 1, and it owns
`execution/store.rs` for this unit.

- `mark_starting` records the pane always. It moves the state to `Starting`
  only from `Planned`, or from `LaunchFailed` (a spawn's recovery).
- The orchestrator asked for an audit of every other state writer. I guarded
  each one that could move a record back:

| Writer | Caller | Could move back | Change |
|---|---|---|---|
| `mark_starting` | `ExecutionService::spawn` | `Running`/`Done`/`Failed` → `Starting` | guarded (above) |
| `set_state(Running)` | the worker, `PaneWorker::set_running` | a coordinator `Failed(TimedOut)` or `Failed(Cancelled)` → `Running`, which leaves a phantom live record after the pane kill | new `mark_running`: keeps `Done` and `Failed`; still revives `LaunchFailed`, because the worker does run |
| `set_state(Failed)` | `Coordinator::end_candidate` (timeout, budget cancel) | a `Done` written after the coordinator's look → `Failed(TimedOut)`: a second way to lose a completion | new `end_live`: writes an end only over `Planned`/`Starting`/`Running`, and returns the state it leaves. When the record had ended, the coordinator records that end (`classify` of the re-read record). |
| `set_state(LaunchFailed)` | `ExecutionService::launch_failed` | a worker's end → `LaunchFailed` when `pane run` errored but the command ran | `end_live` |
| `set_state(LaunchFailed)` | the coordinator's adoption of an abandoned `Planned` record | a record that ran since the look → `LaunchFailed` | `end_live`; when the record moved on, the next look records it |
| `record_exit` | the worker | no: it already keeps a terminal state | none |
| `recover_abandoned` | every spawn | no: only `Planned` without a pane | none |
| `Ledger::done` (`horch done`) | the agent | no: `set_legacy_status("done")` keeps a terminal state | none |
| `Ledger::insert`, `resume_with_phase` | spawn, resume | reopen on purpose | none |
| `supersede_orchestrators` | fleet | no: filters live records | none |
| judge `set_state` (`judging.rs`) | the coordinator only, in sequence | no: one process writes the judge record, in order | none |

### Tests

- `arc_16_mark_starting_keeps_what_the_worker_recorded`
  (`crates/horch-core/tests/execution_plan.rs`). This is the 3 s sleep repro
  made deterministic with a hook. `FastWorker` wraps `FakeWorkspace`: its
  `pane_run` writes `Running`, then `Running`, `Done` or `Failed`, before
  the spawner calls `mark_starting`. Red before the fix (`left: Starting,
  right: Running`), green after.
- `arc_16_guarded_writers_keep_an_end`: a table for `mark_running` and
  `end_live` from each state.
- `startup_failure_records_failed` (used by 2 `arc_18` tests) called
  `mark_starting` on the finished fixture record (`status: done`) to set up a
  `Starting` record. A real spawn always calls it on a `Planned` record. The
  setup now sets `Planned` first. The assertions did not change.
- Coverage limit: no in-process coordinator test for the `end_candidate`
  path. The `tests/coordinator.rs` world needs about 150 lines of setup, and
  a completed candidate starts a judge that `NoJudge` refuses. The store
  table covers `end_live`. The e2e loop covers the coordinator.

## 2. `replace_durable` and `write_atomic`

`replace_durable` was `write_atomic(path, bytes, mode)` and nothing else:
temp file, fsync, rename, directory fsync. The semantics are identical, so I
kept one name: `write_atomic`. It has 7 callers. `replace_durable` had 0
callers.

The design (§2.3) also named the wrong writers for `replace_durable`. I
checked each one:

- The manifest (`experiments/<exp>/manifest.json`) is written once, with
  `create_immutable` (`horch/src/dataset/preflight.rs`).
- Export files are written once, with `create_immutable`
  (`horch-core/src/dataset/export.rs`).
- No code writes round projection files. `DatasetPaths::round_file` has no
  caller outside tests. Projections are folded from the events on each read.

Changes:

- `fsx::replace_durable` is deleted. `mea_09_replace_durable` is now
  `mea_09_write_atomic_replaces_durably`, and it tests `write_atomic`.
  `sec_05_permissions` uses `write_atomic`.
- Design §2.2: the 0600 writers are `write_atomic` and `create_immutable`.
- Design §2.3: the `write_atomic` row names its real writers. The
  `create_immutable` row adds the manifest, `run.json`, judge output and
  exit, and export files.
- Design MEA-09 test map row: the new test name.
- I did not edit history: the design's B1 "Created" row (line 958), the
  `arc-refactor-dataset` plans and reports, and the D16 report still say
  `replace_durable`. They record what was true then.

## 3. cargo doc

`cargo doc` warned on bare URLs at `crates/herdr-install/src/main.rs:3` and
`crates/herdr-docs-sync/src/main.rs:3` (D16 report). The 3 URLs on those
lines are now `<...>` autolinks. Result: `cargo doc --workspace --no-deps`
prints 0 warnings.

## 4. Fixes the orchestrator added (from the D17 report)

- `measure/event.rs`: the `PromotionStarted::publish` doc names
  `update_ref_cas` and `update_ref_cas_read_tree`, and calls `merge_ff_only`
  the legacy name. The doc is true once D17 merges.
- `tests/coordinator.rs`: both tests plan from `fixed_round()`, a fixed
  `RoundId`. The planner seeds the exploration slot with `sha256(round_id)`,
  so a minted id changed the plan between runs. The tests passed 3 of 3.
- Clippy `clone_on_copy`: the report named 3 lines, and the lines had moved.
  Clippy found 6 in the 2 files: `coordinator.rs` (2, `diff_digest`) and
  `judging.rs` (4, `reason` and digests). All 6 are fixed. `horch-core`
  clippy now reports 0 `clone_on_copy`.

## Gotchas

- At the default load, 30 runs of `cmp_05` alone did not reproduce the
  failure. The full suite under load did, and the 3 s sleep made it
  deterministic. A narrow race needs a widened window, not more loops.
- Other worktrees' `dataset` suites ran at the same time and added load.
  Detached `multi-herdr-dataset watch` and `horch tile` processes (ppid 1)
  from tests outlive their test for a few seconds. That is by design.

## Follow-ups (outside my scope)

- Other `horch-core` clippy warnings (not gate failures): `type_complexity`
  in `measure/redact.rs:170` and `harness/inventory.rs:242`;
  `cloned_ref_to_slice_refs` in `harness/claude_plugins.rs:288, 303`,
  `tests/vcs.rs:348`, `tests/promotion.rs:942`; `nonminimal_bool` in
  `tests/baseline_oracles.rs:88`; `large_enum_variant` in
  `competition/promotion.rs:152`; doc markdown lints in
  `roster/operator.rs:60-64` and `workspace/layout.rs:11-15`;
  `unnecessary_sort_by` in `harness/codex.rs:351`; `filter_next` style in
  `workspace/tile.rs:1668`.

- Design layout line 88 lists `experiments/<exp>/rounds/<round>.json` as a
  round projection file. No code writes it. Either delete the line and
  `DatasetPaths::{rounds_dir, round_file}`, or build the cache.
- A candidate's `horch done` spawns `horch tile` or `horch balance` against
  the dataset workspace (`arrange::settle_after_close`). The spawn request
  sets `tiling: Disabled`, but the candidate's own context decides the
  subcommand. The coordinator owns that layout, so the settle step is wasted
  work there.
- `fake-herdr` still breaks its state lock after 10 s of age or 30 s of
  waiting. A slow holder under heavy load can lose the state. I saw no
  evidence of it in this unit.
- A leaked `horch worker codex-sol-1` and its fake `codex` from a
  `tel-fast-end` test in `/Users/mascott/projects/mh-wt/followups` have run
  for 12 h.
