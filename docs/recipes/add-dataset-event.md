# Recipe: add a dataset event

## When

The dataset mode (`multi-herdr-dataset`) must record a new fact or a new
step of a round. Every state change of a round is an event in
`<state root>/multi-herdr/<project slug>/events/YYYY-MM-DD.jsonl`, and every
view of a round is a fold over those events.

## Before you start

- `ai_docs/designs/2026-10-02-dataset-competition-design.md` section 4.1
  (the event log) and section 5 (round states).
- [command-flow.md](../command-flow.md), diagram 3: where in a round each
  event happens.
- `ai_docs/reports/arch-refactor-dataset/b5-promotion.md`: the last event
  kind added (`operator.promote`).

In the steps, `<Kind>` is the Rust name (`OperatorPromote`) and `<kind.name>`
the dotted name (`operator.promote`).

## Steps

1. In `crates/horch-core/src/measure/event.rs`, add 1 line to the
   `event_kinds!` block, with a doc comment:

   ```rust
   /// What the event means and who writes it.
   OperatorPromote => "operator.promote",
   ```

   and a payload struct with the same name, deriving `Debug, Clone,
   PartialEq, Serialize, Deserialize`. Do not set `deny_unknown_fields` on
   any payload. A field you add to an EXISTING payload needs
   `#[serde(default)]`, so events written before your change still parse
   (model: `publish` and `validation_ids` on `PromotionStarted`). Check:
   `cargo build -p horch-core`.
2. In `crates/horch-core/tests/measure.rs`, add 1 sample of the new kind to
   `sample_kinds()`, in `EventKind::KNOWN` order (the order of the
   `event_kinds!` block). Check: `mea_02_envelope_roundtrip_every_kind`
   passes. It fails with "one sample per known kind" when the sample is
   missing or out of order.
3. Project it. In `crates/horch-core/src/measure/projection.rs`,
   `apply_round` matches every kind with no catch-all, so the build fails
   until you add an arm. In the arm, move the state with `step(r.state,
   RoundEvent::<Kind>)?` and store the payload on the `RoundView`. Return
   `Err(...)` for an event that does not fit the round (model: the
   `OperatorPromote` arm refuses a round without a winner); the fold records
   it as an anomaly and does not apply it. Check: `cargo build -p horch-core`.
4. Give the state table the event. In
   `crates/horch-core/src/competition/state.rs`: add a `RoundEvent` variant,
   its key string in the `match` that names each key, and 1 `TABLE` row
   `(from, "<kind.name>", to)` for every state the event may arrive in. Use a
   self-loop row (`from` equals `to`) when it does not move the state (model:
   `candidate.frozen`). An event in a state with no row is an anomaly.
   Check: `table_has_no_duplicate_keys`,
   `mea_05_full_lifecycles_fold_without_anomalies` and
   `mea_05_invalid_transition_is_an_anomaly_and_not_applied`.
5. Write the event. Use a `Recorder` (`JsonlRecorder` in
   `crates/horch-core/src/measure/recorder.rs`) and `NewEvent` with a stable
   `idempotency_key` such as `<step>:<round>:<label>`, so a resumed
   coordinator writes it once. The writer is the coordinator
   (`crates/horch-core/src/competition/coordinator.rs`) for a step of the
   round, or a command in `crates/horch/src/dataset/` for an operator action
   (model: `crates/horch/src/dataset/promote.rs`). Check: a test that runs
   the step twice and finds 1 event (model: `mea_04_duplicate_key_noop`).
6. Replay. `multi-herdr-dataset rebuild <experiment>` folds the events again
   and compares the result byte for byte with the live fold. Check:
   `mea_05_rebuild_equals_live` and `mea_05_prop_replay_identical`.
7. Run the gate: `HORCH_REQUIRE_GIT=1 HORCH_REQUIRE_SQLITE=1 just gate`.

## Files this recipe touches

| file | change | required or optional |
|---|---|---|
| `crates/horch-core/src/measure/event.rs` | `event_kinds!` line and payload struct | required |
| `crates/horch-core/tests/measure.rs` | 1 sample in `sample_kinds()` | required |
| `crates/horch-core/src/measure/projection.rs` | `apply_round` arm | required |
| `crates/horch-core/src/competition/state.rs` | `RoundEvent` variant, key, `TABLE` rows | required |
| `crates/horch-core/src/competition/coordinator.rs` or `crates/horch/src/dataset/<command>.rs` | the writer | required |
| `crates/horch-core/src/dataset/export.rs` | export fields | only if the export must carry the new fact |

## Tests and oracles

- Must pass: `mea_02_envelope_roundtrip_every_kind`,
  `mea_02_unknown_kind_preserved`, `table_has_no_duplicate_keys`, the
  `mea_05_*` fold tests.
- Goldens: `crates/horch-core/tests/golden/export-1.0.0.jsonl`
  (`dataset_export.rs`) and `worker-run-1.0.0.json` (`measure.rs`) are
  written once and never re-blessed. A new kind does not change them, because
  the export and the worker run read the projection, not the raw events. If
  your change must alter an export row, that is a new export schema version;
  ask first.

## Schema version

`EVENT_SCHEMA_VERSION` in `event.rs` is `1.0.0`. The dataset design says a
change to an event format bumps it. B5 added the kind `operator.promote` and 2
`#[serde(default)]` fields to `promotion.started` without a bump, because
old events still parse and an old reader keeps an unknown kind as
`EventKind::Unknown` (`mea_02_unknown_kind_preserved`). Renaming or removing
a field, or changing its type, is a format change: bump the version and ask
first.

## Worked example

- `git show --stat 7ccc935` (B3): the round state table in
  `competition/state.rs` and the projection that uses it.
- `git show --stat 56f5f18` (B5): `operator.promote` in `measure/event.rs`,
  its arm in `measure/projection.rs`, 1 sample in `tests/measure.rs`.
- `git show --stat d77e93a` (B5): the writer, `crates/horch/src/dataset/promote.rs`.
