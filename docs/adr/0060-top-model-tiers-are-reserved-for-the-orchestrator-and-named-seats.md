# Top model tiers are reserved for the orchestrator and named seats

`ORCHESTRATOR_TIERS` in `crates/horch-core/src/roster/teammate.rs` reserves
the top model tiers for the orchestrator and the named creative seats, and
names the ordinary teammate to reach for instead of each: `fable` (instead,
`opus`) and `astra` (instead, `codex-sol`). `horch spawn` refuses ordinary
workers on either tier, whichever flavor is orchestrating. Only the exact
names in `TOP_TIER_SEATS` may start there, and each seat may use only its
assigned tier. The orchestrator writes every other brief for an
Opus/Codex-Sol reader.

## Consequences

- The 1 exception is the operator's own `horch spawn --model`
  (`ModelSource::Operator`). It may put any worker on a reserved tier, and
  the orchestrator passes it only when the operator asks. A top-tier seat
  still runs only on its own tier.
- `TOP_TIER_SEATS` lists the creative council's top-tier seats and the
  tier each runs on: `fable-creative` (fable), `astra-creative` (astra) and
  `game-design-juicer` (fable). They are the only workers `horch spawn`
  starts on a reserved tier from a teammate file, and only by name: never
  as a fallback, never picked by a router or planner, never in a
  competition. A teammate file cannot add a seat. Only this list can.
- `SEATS_WITH_FALLBACKS` (`game-design-juicer`) is the operator's exception
  to the seat rules. Such a seat may name `fallbacks`, a fallback may share
  the seat's pool when it runs on another model family (Fable and Opus have
  separate scoped usage windows), and a substituted launch runs on the
  fallback's ordinary model.
- `reserved_tier` matches a model's name segments rather than the whole
  slug, because Claude and Codex name their models differently and both
  keep changing them. `fable`, `claude-fable-5-2` and `gpt-6-astra` are all
  reserved, while `opus` and `gpt-6.2-sol` are not. A version bump stays
  reserved without a code edit.
- `Roster::model_is_spawnable` (`crates/horch-core/src/roster/validation.rs`)
  applies the rule to the model a spawn is actually about to launch, not
  only to the teammate file. `plan_launch`
  (`crates/horch-core/src/execution/plan.rs`) runs it as the last gate
  before launch, after the resume rules. It is the only gate that sees what
  will actually start. `horch spawn --resume` takes its model from the
  ledger record, so checking the file alone would leave a stale or
  hand-edited record able to start a second top-tier session behind an
  innocent-looking tier name. Only the operator's `--model`, or the resume
  of a record it started, may name a reserved tier.
