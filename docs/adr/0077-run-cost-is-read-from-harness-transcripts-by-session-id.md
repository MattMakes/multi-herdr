# Run cost is read from harness transcripts by session id

`crates/horch-core/src/usage.rs` reports what a fleet run actually cost,
read back from each harness's own transcripts (ImCesar/cezaar#49, extended
past Claude). Attribution is exact: the ledger holds each worker's session
id, and every harness names its transcript after that id. Nothing is
matched by time. The token readers live in `telemetry::readers`, shared
with the telemetry collector. `usage.rs` keeps the prices and the skill
scan.

Each harness's transcript and token record:

- claude: `~/.claude/projects/*/<sid>.jsonl`, assistant `message.usage`,
  streamed several times per `message.id`.
- codex: `<codex home>/sessions/**/rollout-*-<sid>.jsonl`,
  `token_usage_record` per response. Before 0.157, `token_count` totals.
- pi: `~/.pi/agent/sessions/**/*<sid>*.jsonl`, assistant `usage` per
  message.
- prime: the session file path the ledger stores, read as pi.
- opencode: `opencode.db`, via the `sqlite3` CLI, completed assistant
  messages.

## Consequences

- Prices are per million tokens and dated. `--pricing <file.json>`
  overrides any row without a rebuild.
- `find_claude_transcript` searches every directory under
  `~/.claude/projects/` for `<sid>.jsonl` instead of re-deriving Claude
  Code's cwd slug. The slug rule belongs to Claude Code, and the session
  id alone is unique.
- In a Claude Code transcript, streaming writes several records for 1 API
  response, each with the same `message.id` and usage that only grows, so
  the last record per id is the response. `<synthetic>` records are Claude
  Code's own, not API calls. Skill loads are `Skill` tool calls, counted
  once per `tool_use` id.
- In a Codex rollout, `token_count` events carry a running
  `total_token_usage` and are written twice each, so the last one is the
  session. Newer rollouts nest the same fields elsewhere
  (getagentseal/codeburn#1380), hence a depth-first key search.
- Usage from Codex auto-compaction is missing from the totals
  (openai/codex#47003), so a compacted session reads low.
- A Codex session that switched models is priced entirely as its last
  model. The totals are not split per turn.
- A Codex skill load is a shell or tool call that reads a
  `skills/<name>/SKILL.md`.
- `builtin_prices` takes Anthropic prices from cezaar#49
  (platform.claude.com pricing).
- Fable 5.2 is $8 / $40 per Mtok, cache reads $0.20, cache writes 1.25x
  and 2x (anthropic.com/claude/fable and the Fable 5.2 migration guide,
  read 2026-11-09). The `claude-fable-5-1` row stays because old
  transcripts name it.
- Sonnet 5.5 is $2 / $10 per Mtok, cache hits $0.20, cache writes 1.25x
  and 2x (platform.claude.com/docs/en/about-claude/pricing, read
  2026-10-04).
- OpenAI publishes the cache write price of `gpt-6-astra`: $12.50 per
  Mtok, that is 1.25x input.
- `codex-auto-review` has no price row, on purpose. It is Codex's approval
  reviewer (`approvals_reviewer = "auto_review"`; a rollout with
  `thread_source: guardian_review` names it). Codex's models cache lists
  it hidden, and OpenAI publishes no per-token price for it
  (learn.chatgpt.com/docs/pricing, read 2026-10-06). Its events stay
  unpriced.
- agy names a model once per thinking level (`agy models`, 2026-10-05),
  for example `gemini-3.1-pro-high` and `gemini-3.1-pro-low`. Each Gemini
  name gets its own price row.
- `MODEL_ALIASES` tracks what Claude Code currently resolves an alias to.
  Claude Code 2.1.321 resolves `fable` to Fable 5.2: a `claude -p --model
  fable` result names `claude-fable-5-2` (seen 2026-11-09). Claude Code
  resolves `sonnet` to Sonnet 5.5: its transcripts name
  `claude-sonnet-5-5` (seen 2026-10-04).
