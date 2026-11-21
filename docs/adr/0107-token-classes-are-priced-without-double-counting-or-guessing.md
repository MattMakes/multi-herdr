# Token classes are priced without double counting or guessing

`TokenClasses` in `crates/horch-core/src/telemetry/mod.rs` keeps 1 field
per token class (TEL-03). `usage::Tokens` (`crates/horch-core/src/usage.rs`)
is the 1 shape every harness's token counts are converted into. In both,
`input` is fresh (uncached) input only. `reasoning` is already inside
`output` for every harness, so it is reported for information and never
priced or added again. An event whose model has no
price stays unpriced. It is never counted as $0, so a missing price stays
visible instead of understating spend.

## Consequences

- Codex reports cached tokens inside its input count: a Codex `TokenUsage`
  object's `input_tokens` includes `cached_input_tokens`. So
  `telemetry::readers::codex_tokens`
  (`crates/horch-core/src/telemetry/readers.rs`) subtracts
  `cached_input_tokens` from `input_tokens`. Counting both would bill cache
  reads twice. Claude and pi already report cached tokens separately.
- `TokenClasses::total` counts every billed token and leaves `reasoning`
  out, since it is inside `output`.
- `TokenClasses::priced`, the pricing shape, drops `reasoning`: it is
  priced as output.
- The snapshot in `crates/horch-core/src/telemetry/collect.rs` counts events
  with no price (an unknown model) in `unpriced_events`, never as $0.
- `Event::price_if_unset` prices at read time only an event stored without
  a cost (read-time pricing, section 8.2). A stored cost is the price at
  collection time and is kept. A model `prices` does not know stays
  `None`.
- `readers::pi_line` and `readers::pi_tokens` read 1 pi or Prime Agent
  session line. `cacheWrite` includes `cacheWrite1h` in pi 0.87 ([S] pi),
  so the 1h part is split out. Anthropic field spellings are accepted too.
