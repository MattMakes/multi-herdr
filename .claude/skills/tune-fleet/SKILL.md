---
name: tune-fleet
description: Use when retuning the horch fleet roster - which model and effort each teammate runs at, which skills it is expected to use - from measured run cost and skill use, or when a model release, price change or new harness version means the model guide needs re-checking.
---

# Tune the fleet

Retune `teammates/*.md` from evidence, not from benchmarks alone. This skill
is for an operator session opened in this repository. Fleet workers do not
load it.

The two halves of the evidence are:
- `horch teammates --matrix [--json]`: what is configured.
- `horch cost [--json]`: what a run cost, and which skills each worker loaded.

The standing record is `ai_docs/reports/model-guide-<yyyy-mm>.md`. Every
change you make must end up explained there, and in a `#` comment next to the
changed line.

## 1. Collect the configuration

```bash
horch teammates --matrix --json > /tmp/matrix.json
horch teammates --check
```

Note every row whose `effort_is_explicit` is false, and every `unknown` price.
A codex teammate with no effort inherits the operator's `config.toml`. That is
a finding in itself.

## 2. Collect the measurements

Run from the project directory the fleet ran in. Its ledger is per project.

```bash
horch cost --since <run start, e.g. 2026-09-24> --json > /tmp/cost.json
horch cost --since <run start> --reprice claude-sonnet-5
horch cost --since <run start> --session claude:<orchestrator session id>
```

Read these from the report:
- **Cost by family** (build / review / research / plan) and **by harness.**
  Compare with the baseline in the model guide; cezaar#40's was reviewers and
  judges at 71% of worker spend.
- **Cache-read share.** When it is above about 60%, the cache-read price
  matters more than the list price.
- **The reprice column.** It shows what the same tokens would cost on another
  model. Reprice only within one vendor, because token counts are not
  comparable across tokenizers.
- **The skills table.** It lists expected-but-never-loaded skills and loaded
  skills nobody expected.
- **Not priced.** Say why each session is missing. Never treat one as $0.

Where available, use more than one run. One run is an anecdote.

## 3. Re-verify the model facts

Model facts go stale faster than the roster. Before changing a level, check
each harness in use for these:
- **Latest models and tiers.** Claude aliases (`fable`/`opus`/`sonnet`/`haiku`)
  track releases on their own. Codex slugs, `provider/model` ids and the
  opencode free list do not.
- **Prices per MTok:** input, cached input or cache read, cache write, and
  output. Note any promotional price and its end date.
- **Valid effort levels, and each model's default.** Check the flag or config
  key still works. The opencode TUI once silently swallowed `--variant`.
- **Known bugs.** Examples: a model that fails on its provider, or a thinking
  switch that does nothing.

Cite a source URL for every fact. Mark anything you could not confirm on a
first-party page as **unverified**. Then:
- copy the model guide to a new month's file, or update this month's;
- update the price table in `crates/horch-core/src/usage.rs` (`builtin_prices`)
  to match, including its "as of" date in `cmd/cost.rs`.

## 4. Decide

Each rule below comes from cezaar#40 or from measurement in this repo:

1. **Effort follows the role, not the model.**
   - builders: medium, or low when the brief is a complete spec;
   - reviewers and planners: high;
   - researchers: medium;
   - runners: low;
   - the orchestrator: xhigh.
2. **Lower effort** where a role is expensive and its output passes review
   the first time.
3. **Raise effort** where a role's output keeps failing review rounds. One
   extra round usually costs more than the higher level. For a one-off, prefer
   `horch spawn --effort` over a file change.
4. **Move work down a tier** (opus → sonnet, sol → terra → luna) when plans
   are complete enough that the cheaper model follows them. The fix for a
   cheap model failing is usually a better plan, not a bigger model.
5. **Levels are not comparable across vendors.** Compare cost and outcome,
   never level names.
6. **Skills:**
   - For an expected skill that is never loaded, sharpen its description
     first, because descriptions are what the briefing shows. Drop it from
     `skills:` or `plugin_skills` only if it truly does not fit the role.
   - A skill loaded often but not expected is a candidate for `skills:`.
   - A plugin skill that should never be used belongs outside
     `plugin_skills`. Listing the others there switches it off.
7. **Local and free tiers are judged on quality, not cost.** pi costs
   wall-clock time. The opencode free tier costs confidentiality.
8. **Keep the reserved tiers reserved.** Never give a worker `fable` or
   `astra`.

## 5. Edit

For each change, edit the teammate's frontmatter:
- set the new `model:` or `effort:` value;
- add a `#` comment directly above it that gives the reason and the number
  behind it, e.g. `# medium -> low: 3 runs, 0 review rounds failed, -38%`;
- update the matching row of the effort table in `teammates/README.md`, and
  the teammate table in `README.md`.

## 6. Verify

```bash
horch teammates --check          # the level is valid for the agent
cargo test --workspace           # argv tests pin some levels; update them
horch smoke fleet                # spends no tokens
horch doctor                     # no env var or setting overrides the new levels
```

## 7. Record

Write `ai_docs/reports/tuning-<yyyy-mm-dd>.md` with these sections:
- the runs measured: ledger path, `--since`, session count;
- a before/after table per changed teammate: model, effort, cost per
  session, review rounds;
- the facts re-verified, with sources, and any that stayed unverified;
- what to measure next time to confirm or revert each change.
