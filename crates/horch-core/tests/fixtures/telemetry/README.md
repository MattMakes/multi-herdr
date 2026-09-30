# Telemetry fixture corpus

Synthetic data only. No line comes from a real transcript. Field names and
nesting follow the evidence reports, which read the shipped harness code:
`ai_docs/reports/telemetry-sources.md` [S] and `ai_docs/reports/quota-signals.md` [Q].

`SENTINEL-CONTENT`, `sentinel@example.invalid` and `acct_SENTINEL` mark text
that must never reach a telemetry or quota file (TEL-11).

| path | models | what it exercises |
|---|---|---|
| `claude/-work-alpha/11111111-....jsonl` | Claude Code 2.1.284 | a `message.id` repeated once per content block with identical usage; a `<synthetic>` 429 line (`error: "rate_limit"`); `thinking_tokens`; a 1h cache write; a line with no `cache_creation` object |
| `claude/-work-alpha/11111111-.../subagents/agent-a1.jsonl` | 2.1.284 | subagent lines (`isSidechain: true`, `agentId`); one id whose usage GROWS on its repeat (start-of-stream snapshot, [S] C1) |
| `claude/-work-alpha/11111111-.../subagents/workflow-x/agent-a2.jsonl` | 2.1.284 | a nested subagent directory ([S] C1) |
| `claude/-work-alpha/22222222-....jsonl` | 2.1.284 | a plain worker |
| `codex/sessions/2026/09/27/rollout-...-01a0....jsonl` | Codex 0.157.1 | `token_usage_record` per `response_id`, the last one AFTER the last `token_count` (the under-count); a `rate_limits` snapshot; `task_complete.error.codex_error_info: usage_limit_exceeded` |
| `codex/sessions/2026/09/11/rollout-...-01b0....jsonl` | Codex 0.150.0 | positional windows: primary = 5h, secondary = 7d (the old plan shape); no records, so the `token_count` fallback |
| `codex/sessions/2026/09/12/rollout-...-01c0....jsonl` | Codex 0.149.0 | no `token_usage_record` (older CLI): the fallback path; `rate_limits: null` |
| `pi/sessions/--work-beta--/..._3333....jsonl` | pi 0.87.1 (`@earendil-works`) | assistant usage; `toolResult` usage (`tool_nested`); `cacheWrite1h` as a subset of `cacheWrite`; `reasoning` |
| `prime/prime-1-4444/sessions/4444....jsonl` | Prime Agent 0.9.4 (assumed pi-like, [S] section 7) | the ledger stores this file's path as the session id |
| `opencode/opencode.sql` | OpenCode 1.18.33 | `session`/`message`/`part` subset; a completed message, an incomplete one (skipped), a user message (skipped), a message in a session no ledger names, and a `step-finish` part that repeats tokens (never summed) |
| `ledgers/-work-alpha.json`, `ledgers/-work-beta.json` | horch | 2 projects; the orchestrator record; 1 record per agent; 1 without a session id; 1 `agent: none`. `{STATE}` is replaced by the state root when installed |
| `probes/claude-get-usage-*.jsonl` | 2.1.284 | `get_usage` control responses; `rate_limits` holds `seven_day_opus`, `model_scoped` and `limits` ([Q] 1a, which corrects the design's Appendix A) |
| `probes/codex-ratelimits-*.json` | 0.158.0 | `account/rateLimits/read` results; `swapped` puts the week in the first slot |
| `quota/*.json` | horch | one `quota.json` per pool-state case, observed at 2026-09-28T17:55:00Z |

`EXPECTED.md` is the oracle for the readers and the end-to-end scenario. Its
numbers are computed by hand from the lines above.
