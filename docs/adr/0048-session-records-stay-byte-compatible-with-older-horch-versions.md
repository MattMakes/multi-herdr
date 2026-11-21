# Session records stay byte-compatible with older horch versions

The on-disk session record is `LedgerRecordV1` in
`crates/horch-core/src/execution/legacy.rs`. It is the pre-refactor
`ledger::Record` serde, field for field and in the same order, plus
optional keys that are skipped when empty. A record without the new keys
serializes byte-identical to before. The struct has no
`deny_unknown_fields`, so an old binary reads a new ledger and ignores what
it does not know. The typed view, `Execution` in `model.rs` (Spec A §4,
architecture design §4.3), loses nothing: `store::to_execution` and
`store::from_execution` give every record back unchanged.

## Consequences

- `status` stays the legacy `"working"` / `"done"` string. The typed
  `ExecutionStatus` goes under the key `state`, as a nested object with its
  own internal `state` tag, for example
  `"status": "done", "state": {"state": "launch_failed", "stage": "split",
  "reason": "no such pane"}`. The nesting means the outer key and the inner
  tag never clash.
- When `state` is set, the store writes `status` as
  `state.legacy_status()`. `ExecutionStatus::legacy_status` maps every
  terminal state (`Done`, `Failed`, `LaunchFailed`) to `"done"`, so an old
  reader never shows a failed launch as a live worker. A reader takes
  `LedgerRecordV1::execution_status`: `state` when present, else the status
  derived from `status`.
- `ExecutionKind::legacy_kind` writes candidates and judges as `"worker"`,
  because every reader that predates those kinds treats them as workers.
- The fields of `LedgerRecordV1` are never reordered. `routing` (ARC-14),
  absent before A5, stays in the place A5 wrote it, so those ledgers keep
  their bytes.
- The teammate's name is still spelled `tier` on disk. Ledgers written by
  earlier versions carry that key, and the built-in teammates kept the old
  tier ids as their names precisely so those records still resolve.
- A record from before A5 has no routing. `plan::resume_routing` rebuilds
  it from `via`, so such records stay resumable.
- `Execution` keeps timestamps as the ledger's text, not parsed times, so a
  record written by any earlier version keeps its bytes.
- Timestamps use the UTC format the bash implementation wrote
  (`date -u +%Y-%m-%dT%H:%M:%SZ`, `now` in `records.rs`). Lexicographic
  order then equals chronological order, which the record sorting relies on.
- A `session_id` of `""` (the bash spelling for none) reads as
  `SessionState::Unavailable` and writes back as `""`.
- `store::slug` turns a project path into a filename exactly like
  `tr -c 'A-Za-z0-9' '-'`. It works on bytes, as `tr` does, so a multi-byte
  character becomes one `-` per byte and slugs computed by the bash version
  still resolve. The slug rule decides which file an existing ledger lives
  in. Changing it would orphan every recorded session.
