# Model and effort guide: September 2026

This is why each teammate runs on the model and effort level it does. It is
horch's counterpart to [ImCesar/cezaar#40], extended past Claude to all five
harnesses horch launches. Update it through the `tune-fleet` skill
(`.claude/skills/tune-fleet/SKILL.md`), not by hand-editing teammate files in
isolation.

Researched on 2026-09-23 and 2026-09-24. **Unverified** marks a figure that
first-party pages did not confirm: the egress proxy blocked openai.com and
opencode.ai, so those vendors are known only from secondary sources. Re-check
every unverified row before relying on it.

[ImCesar/cezaar#40]: https://github.com/ImCesar/cezaar/issues/40

## The rules this guide applies

1. **Effort follows the role, not the model.** This is the #40 table:
   - builders run low or medium (the brief carries the thinking);
   - reviewers and judges run high (a missed finding costs a whole review
     round);
   - researchers run medium;
   - runners run low;
   - the orchestrator keeps xhigh.
2. **Say the effort out loud.** An unset effort inherits whatever the operator
   last chose. For Claude that is `/effort` or settings; for Codex it is
   `~/.codex/config.toml`. That is a setting nobody chose, and it changes
   under the fleet.
3. **Levels are not comparable across vendors.** Codex "high" is not Claude
   "high". Compare cost and outcome with `horch cost`, not level names.
4. **Measure before retuning.** Every change below is a starting point, not a
   result. `horch cost` reports what a run actually cost per teammate, harness
   and family, and which skills each worker actually loaded.

## Claude Code (`agent: claude`)

`--model <alias>` with `fable`, `opus`, `sonnet` or `haiku`, so a teammate
follows new releases. `--effort low|medium|high|xhigh|max`.

**Precedence** (cezaar#41): env `CLAUDE_CODE_EFFORT_LEVEL` > `--effort` >
settings `effortLevel` > the model default. The env var can come from the
shell or from `env` in `~/.claude/settings.json`. Either way it overrides
every pane, and `horch doctor` warns about both, and about `maxEffortLevel`.

**Haiku 4.5 has no effort setting.** `--check` refuses one, and a `haiku`
model override drops the flag.

**Escape hatch:** `ANTHROPIC_DEFAULT_{OPUS,SONNET,HAIKU}_MODEL` pins what an
alias resolves to, without editing any teammate.

| model | input | output | cache read | 5m / 1h cache write | source |
|---|---|---|---|---|---|
| claude-fable-5-1 | $10 | $50 | 0.025× input | 1.25× / 2× input | cezaar#49 pricing table |
| claude-opus-5-5 | $4 | $20 | 0.05× input | 1.25× / 2× | cezaar#49 |
| claude-opus-5 | $5 | $25 | 0.1× | 1.25× / 2× | cezaar#49 |
| claude-sonnet-5 | $2 | $10 | 0.1× | 1.25× / 2× | cezaar#49 |
| claude-haiku-4-5 | $1 | $5 | 0.1× | 1.25× / 2× | cezaar#49 |

All prices are per MTok, API-equivalent.

**Baseline** (cezaar#40, run #24/#25, every worker at an inherited high effort):
- builders (Sonnet 5): $8.70;
- reviewers (Opus 5): $16.27;
- judges (Opus 5): $7.22;
- curator: $1.12.

That is $33.31 in total, or $26.57 repriced to Opus 5.5. Cache reads were
60-70% of every session's cost, which is why Opus 5.5's cheaper cache reads
matter more than its lower list price.

| teammate | model | was | now | why |
|---|---|---|---|---|
| orchestrator | opus (default flavor) / fable | xhigh | xhigh | The one seat that does the reasoning for everyone else |
| backend-, frontend-developer, designer, opus, sonnet | opus / sonnet | xhigh | medium | Builders on a written brief. #40 says low; medium because our briefs are not always complete specs |
| researcher | opus | xhigh | medium | #40 researchers: exploration without maximum rigor |
| architect-reviewer, qa-engineer | opus / sonnet | xhigh | high | #40 reviewers/judges: 71% of worker spend was review, but a missed finding costs a round |
| staff-engineer, product-lead | opus | xhigh | high | Plans and product calls; the orchestrator above is already xhigh |

**Default orchestrator is now Opus** (`horch fleet`). Fable is one word away
(`horch fleet fable`) for work that needs it. Fable and Astra stay reserved
from every worker.

## Codex CLI (`agent: codex`)

- **Model:** `-c model="<slug>"`. There is no alias mechanism, so slugs are
  literal and must be bumped by hand.
- **Effort:** `-c model_reasoning_effort="<level>"`, one of
  `none|low|medium|high|xhigh|max`. Astra has no `none`.
- **Refused by `--check`:**
  - `minimal`: an API error on the gpt-5.6 models
    (ai_docs/reports/env-research/codex-opencode.md);
  - `ultra`: fans out client-side and multiplies spend.
- **Found on 2026-09-24:** neither codex worker set effort, so both ran at
  the operator's `config.toml` value, `medium`. An offered codex teammate must
  now state one.

| model | tier | input / cached / output | benchmarks | source |
|---|---|---|---|---|
| gpt-6-astra | top; orchestrator only | $10 / $1 / $50; cache write $12.50 | Terminal-Bench 4.0 57.9% (Sol 37.3%) | openai.com/index/gpt-6-astra (via yottalabs.ai, computingforgeeks.com) |
| gpt-5.6-sol | flagship | $4 / $0.40 / $20; **promo until 2026-11-21**, launch price $5 / $30 | SWE-bench Pro 64.6%; Terminal-Bench 2.1 88.8% at max | techjacksolutions.com, layerlens.ai |
| gpt-5.6-terra | everyday | **unverified:** $2 / $12, or $2.50 / $15; cached about 90% off | SWE-bench Pro 63.4% | layerlens.ai, axis-intelligence.com |
| gpt-5.6-luna | cheapest | **unverified:** $0.20 / $1.20, or $1 / $6 | Terminal-Bench 2.1 84.7% | layerlens.ai |

`horch cost` uses the lower of each disputed pair. Override with
`--pricing <file.json>`.

| teammate | model | was | now | why |
|---|---|---|---|---|
| orchestrator-codex | astra (or sol via `horch fleet sol`) | xhigh | xhigh | Orchestrator |
| codex-sol | gpt-5.6-sol | inherited (medium) | medium | Builder; now explicit |
| codex-terra | gpt-5.6-terra | inherited (medium) | low | "Executes exactly what is asked", so the thinking is in the brief (#40 builders low) |
| **codex-luna** (new) | gpt-5.6-luna | – | low | #40's "runner": commands, renames, lookups |
| **codex-reviewer** (new) | gpt-5.6-sol | – | high | Cross-vendor review of Claude-built changes. A second model family shares fewer blind spots |

## OpenCode (`agent: opencode`)

- **Model:** `--model provider/model`.
- **Effort** is the build agent's `variant`, set through the
  `OPENCODE_CONFIG_CONTENT` overlay. The TUI horch launches has no `--variant`
  flag, and 1.18.2 silently swallows one.
- **The free `opencode/*` models define no variants** (`variants: {}`), so
  `--check` refuses `effort` on them.

| model | notes | trains on input | source |
|---|---|---|---|
| opencode/big-pickle | Stealth coding model, 200K context, widely believed to be GLM-4.6. SWE Atlas QnA 50.8% | yes | opencode issue #4276; github.com/PhillipChaffee/big-pickle-swe-atlas |
| opencode/nemotron-3.5-lightning-free | NVIDIA 30B MoE (3B active), 1M context. SWE-bench Verified 51.56 | logged (NVIDIA trial terms) | huggingface.co/nvidia |
| opencode/nemotron-3-ultra-free | NVIDIA 550B MoE (55B active), 1M context. **Reported failing on Zen** (anomalyco/opencode#30951) | logged | freellm.net |

Other notes:
- **One paid call per free session:** every free session makes one
  title-generation call on the operator's paid `small_model`.
- **The Zen free list is unverified.** It also offers GPT-5 Nano, MiMo V2
  Flash, MiniMax M2.5 and Space Bunny.

| teammate | was | now | why |
|---|---|---|---|
| opencode-lightning | minimal (no-op) | – | No variants on the model |
| opencode-pickle, opencode-ultra | high (no-op) | – | No variants on the model |

**Before trusting opencode-ultra:** run a 1-turn check first. If Zen still
rejects it, mark it `hidden: true` with the issue link.

## pi (`agent: pi`) and Prime Agent (`agent: prime`)

- **Model:** `--model provider/id`.
- **Effort:** `--thinking off|minimal|low|medium|high|xhigh|max`.
- **Prime Agent** (PrimeIntellect-ai/prime-agent) is built on pi, so the flags
  match.

| teammate | model | was | now | why |
|---|---|---|---|---|
| pi | ollama/qwen3.8 (27B) | high | low | The long-context benchmark scores xhigh below low on Qwen3.8 (env-research/compaction-benchmarks.md). A local model also pays for every thinking token in wall-clock time |
| prime | anthropic/claude-opus-5 → **claude-opus-5-5** | high | medium | Opus 5.5 is cheaper and scores better (#41). Long exploratory runs multiply thinking tokens (#40 researchers) |

**Known pi issues:**
- `--thinking off` does not turn thinking off on Ollama without a
  `thinkingLevelMap`.
- `/v1/chat/completions` can hang on `qwen3.8:27b`. Use Ollama's `/api/chat`,
  or `qwen3.6:27b` (SWE-bench Verified 77.2, fits 24 GB).

## Where each harness records usage (for `horch cost`)

| harness | file | tokens |
|---|---|---|
| claude | `~/.claude/projects/<cwd-slug>/<session-id>.jsonl` | Assistant `message.usage` (`input_tokens`, `cache_creation_input_tokens`, `cache_read_input_tokens`, `output_tokens`, and `cache_creation.ephemeral_{5m,1h}_input_tokens`). Streaming repeats a message, so dedup by `message.id` |
| codex | `$CODEX_HOME/sessions/YYYY/MM/DD/rollout-*-<session-id>.jsonl` (a pane's private home links the operator's `sessions/`) | `event_msg` / `token_count` with `info.total_token_usage` (running). It is emitted twice. `cached_input_tokens` is *inside* `input_tokens`, and reasoning is inside `output_tokens`. Compaction usage is missing (openai/codex#47003) |
| pi | `~/.pi/agent/sessions/<cwd>/…<session-id>….jsonl` | Assistant messages carry `usage` (`input`, `output`, `cacheRead`, `cacheWrite`). **Field names unverified**; the reader accepts both these and the Anthropic spellings |
| prime | the `--session-dir` horch gives each pane | Same format as pi. The ledger's session id is the session file path |
| opencode | `~/.local/share/opencode/opencode.db` (SQLite) | Not read by `horch cost` yet: the schema moves between versions, and adding a SQLite dependency for three $0 models is not worth it. Listed as "not priced" |

## Sources

- ImCesar/cezaar#40, #41, #49: the Claude role table, effort precedence,
  baseline, and pricing.
- ai_docs/reports/env-research/codex-opencode.md, pi-ollama-prime.md,
  compaction-benchmarks.md, side-calls.md.
- openai.com/index/gpt-5-6, openai.com/index/gpt-6-astra (via the secondary
  sources named in the tables); openai/codex#47003; getagentseal/codeburn#1380.
- opencode.ai/docs/zen (unreachable, so secondary sources only);
  anomalyco/opencode#30951, #36141, #4276.
- earendil-works/pi docs (settings.md, sessions.md); PrimeIntellect-ai/prime-agent
  (usage.md).
