# Test goldens and oracles are frozen captures of the old output

The golden files and behaviour oracles of the horch-core integration tests
are captures of what the previous code produced. A test only reads them. It
never rewrites or refreshes them from the current output, because a test
that rewrote its own expected file could not fail. A diff against a golden
or an oracle means the behaviour changed. This covers the briefing goldens
in `crates/horch-core/tests/golden/` (`tests/golden_prompts.rs`), the
`assess_pool` golden (`tests/r_routing_assess.rs`), the quota reply parser
golden (`tests/r_routing_parsers.rs`) and the A0 oracles in
`crates/horch-core/tests/oracles/` (`tests/baseline_oracles.rs`, ARC-01).

## Consequences

- The briefing goldens were captured from the implementation where every
  prompt was a Rust `const` or `format!`. They prove that moving the
  briefings into `teammates/` changed no text. Each is compared byte for
  byte against what the roster renders now. If a `.md` file is edited, the
  tests fail. Prompt text is a deliberate change, not a side effect of
  touching code.
- A sanctioned briefing change is a named literal block in the test, with
  its reason. The test asserts that the old strings the block replaces are
  still present in the golden, so a golden overwritten with today's output
  fails rather than passes. The named block is what makes the change
  reviewable.
- The orchestrator briefing's sanctioned differences are all listed in
  `the_orchestrator_briefing_differs_only_where_sanctioned`. A
  hand-maintained list of 5 tiers became the `{roster}` placeholder. The
  fleet no longer pre-spawns 4 idle workers for the orchestrator to
  inherit. The orchestrator is told workers never run on Fable or Astra and
  to write for Opus/Codex-Sol readers. `horch` now places panes itself, so
  the orchestrator is no longer told how to choose one.
- The worker briefing's sanctioned differences are pinned in
  `every_worker_briefing_differs_only_where_sanctioned`.
- Both base briefings gained the Simplified Technical English section
  (`ste_section`), inserted whole and pinned in those same 2 tests.
- The `assess_pool` golden holds state, reason, worst window and headroom
  for every reading in the quota fixtures. The parser golden holds the
  parsers' output for odd replies as well as real ones. Both were written
  by the code before the #153 refactor.
- The A0 oracles record what the pre-refactor code does. Later phases move
  the call sites in `baseline_oracles.rs`, never the expected files.
  `HORCH_BLESS=1` writes the oracles. It was for the A0 baseline only.
- `World::new` in `baseline_oracles.rs` points `HORCH_STATE_DIR` and
  `HORCH_DATA_DIR` into the temp world, so no launch reads the host's real
  state (a Codex v1 model catalog, for one). These keys stay out of
  `PINNED`: `pinned_header` writes `PINNED` into every frozen oracle as
  `_pinned_env`, so a new key there would change every oracle.
