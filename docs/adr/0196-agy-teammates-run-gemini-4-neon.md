# The agy teammates run Gemini 4 Neon, and `neon` is a quota scope

Since 2026-11-21 `spec-writer`, `ticket-writer` and `audio-reviewer` run
`gemini-4-neon` on the Antigravity CLI (`agy`). Before that they ran
`gemini-4-argon`. The operator asked for the change. `hark llm` follows it:
`AGY_DEFAULT_MODEL` in `skills/hark/engine/hark/surfaces/_llm_agy.py` is
`gemini-4-neon-high`.

`neon` is a model family in `crates/horch-core/src/routing/quota.rs`,
beside `argon`. This record adds to
[0063](0063-antigravity-quota-windows-are-scoped-by-model-group.md), which
still holds for Argon.

Nobody has checked the Neon facts below on a machine with `agy`. The
container that made this change could not reach `antigravity.google`. They
come from secondary sources, and the code is written to be safe if they
are wrong.

## Consequences

- `model_family` tests `neon` and `argon` before `gemini`, so
  `gemini-4-neon` is limited by `neon` windows.
- In `parse_agy_usage`, a bucket id that starts `gemini-neon-` scopes to
  `neon`. A shared `gemini-*` bucket scopes to `gemini`, to `argon` when the
  reply has no own Argon group, and to `neon` when it has no own Neon
  group. The reply shape known today (agy 1.4.10, Pro plan) has an Argon
  group and no Neon group, so the shared Gemini buckets limit a Neon
  teammate. If agy reports a Neon group under the guessed id, that group
  limits it instead.
- If agy reports Neon under another bucket id, one of 2 things happens. An
  id that starts `gemini-` counts as a shared Gemini bucket, so an empty
  Neon bucket would also stop the Flash teammate, and the Argon ones when
  the reply has no Argon group. Any other id is unscoped and limits every
  Antigravity model. Both are conservative. Fix the prefix in `quota.rs`
  when the real id is known.
- `audio-reviewer` runs at `medium` effort. It ran at `high`, because the
  audio probe ran at high and a missed defect costs a fix round. The fleet
  now caps every teammate at medium unless the operator signs off, so
  `high` returns only with that sign-off.
- The audio probe (`docs/specs/hark/agy-audio-probe.md`) ran on
  `gemini-4-argon-high`. Whether Neon hears a WAV through agy's
  file-reading tool is not known. Re-run the probe on Neon before a review
  from `audio-reviewer` is trusted. It has no fallback.
- `effort_problem` (`crates/horch-core/src/roster/effort.rs`) accepts
  `xhigh` on antigravity for a `gemini-4-neon` model only. The harness
  levels in `harness/capabilities.rs` stay `low`, `medium`, `high`.
  `EFFORT_SUFFIXES` gains `-xhigh`, so the id `gemini-4-neon-xhigh` states
  its effort. No shipped teammate uses `xhigh`.
- `builtin_prices` in `crates/horch-core/src/usage.rs` prices the Neon ids
  at $3.00 input, $15.00 output and $0.15 cached input per million tokens,
  with cache writes at the input price. The Argon entries stay, so old
  transcripts keep their price. `PRICES_AS_OF` in
  `crates/horch/src/cmd/cost.rs` is 2026-11-21: that is the day the Neon
  rows were added. The other rows were not checked again on that day.
- `fake-antigravity` reports a "Gemini 4 Neon" group with the bucket
  `gemini-neon-wtu-weekly`, and the scenario `agy_neon_exhausted` empties
  it. That shape is the guess below, not a capture. The captured fixtures
  (`agy-usage-pro.json`, `agy-usage-ultra.json`, `agy-1.4.10.json`) are
  unchanged and hold no Neon group.

## Not verified

| Fact | Source | State |
|---|---|---|
| `agy models` on agy 1.5.11 lists `gemini-4-neon-low`, `-medium`, `-high` and `-xhigh`; the base id is `gemini-4-neon` | <https://github.com/router-for-me/models/issues/104>, and a search summary of <https://antigravity.google/docs/cli/headless/> | secondary, not checked |
| `agy --model gemini-4-neon --effort <level>` takes `low`, `medium`, `high` and `xhigh` | inferred from the ids above | a guess |
| Neon costs $3.00 input, $15.00 output and $0.15 cached input per million tokens on the Gemini API | <https://apidog.com/blog/gemini-4-neon-pricing/> | secondary, not checked |
| `agy /usage` names Neon's group "Gemini 4 Neon" and its buckets `gemini-neon-*` | by analogy with "Gemini 4 Argon" and `gemini-argon-wtu-weekly` | a guess |
| Neon hears audio through agy's file-reading tool | none | not known |

## How to verify on the operator's Mac

1. `agy --version`, then `agy models`. The list must hold
   `gemini-4-neon-medium`. Note whether it holds `gemini-4-neon-xhigh`.
2. `agy -p "/usage" --output-format json --print-timeout 60s`. This is the
   command `routing/quota_probe.rs:probe_google` runs, and it runs no model
   turn. Read `command.data.groups[]`: is there a Neon group, and what are
   its `name` and the `id` of each bucket? Save the reply as a fixture
   beside `crates/horch-core/tests/fixtures/telemetry/probes/agy-usage-pro.json`.
3. `horch quota --refresh`, then `horch quota`. The google row must show a
   `neon` window with `source` `usage`.
4. `horch route spec-writer` must print `SPAWN`. `horch route
   audio-reviewer` must print `SPAWN` too.
5. `agy --model gemini-4-neon --effort medium -p "Reply OK." --output-format
   json` must answer with `status` `SUCCESS`. Run it again with `--effort
   xhigh` to check that level.
6. Re-run the audio probe of `docs/specs/hark/agy-audio-probe.md` with
   `--model gemini-4-neon-medium`, the level `audio-reviewer` runs at.
7. Compare the prices with Google's own price page.

When a fact differs, fix the code and this table in the same change, and
add a dated line to `docs/live-checks/harnesses.md`.
