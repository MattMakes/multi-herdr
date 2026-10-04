# D11 orchestrator-ownership report

## What changed

- `teammates/orchestrator.md` and `teammates/orchestrator-codex.md`: the
  "== Own the product ==" block, verbatim from the plan. Claude persona: it sits
  before "== horch:skill-creator ==". Codex persona: it ends the file.
- `teammates/_base/fleet-orchestrator.md`: the pool line now reads
  "claude, codex, opencode-zen, google, local" (`POOL_GOOGLE` in `routing/quota.rs`).
- `crates/horch-core/tests/golden_prompts.rs`: twelfth sanctioned change
  (`own_product`, spliced into `only_fable`); the usage-limits text lists `google`.
  No golden file changed.

## Checks

- `just gate` is green. `horch teammates --check` passes (41 teammates).
- `golden_prompts`: 6 of 6 pass. The Claude briefing is pinned byte for byte.
  The Codex test pins it equal outside the tier block, so the block is in both.

## Decisions and gotchas

- The block must stay inside the persona region, because the Codex test strips
  that region. A block placed elsewhere would break that equality test.
- The plan named no separate Codex golden test, so none was added.

## Follow-ups

None.
