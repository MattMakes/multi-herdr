# A broken harness binary is judged by harness, not by pool

A pool is shared by harnesses. A `pi` teammate on an `anthropic/` model
draws on the `claude` pool. So a binary that crashes is recorded per
harness, in `HarnessHealth` in `crates/horch-core/src/routing/quota.rs`,
not per pool. `QuotaView::assess` returns `broken` for a harness whose last
`--version` probe failed, whatever its pool says. Routing never chooses a
binary that cannot start, and other harnesses on the same pool stay usable.

## Consequences

- `HarnessHealth.error` is set when `--version` ran and failed, and is
  cleared when it next succeeds.
- `pool_for` maps a teammate to its pool from its agent and model (section
  11.1). `pi` and `prime` models map by their provider prefix.
- Prime runs Anthropic models on a credential horch cannot see. They are
  counted against the claude pool: the conservative choice.
