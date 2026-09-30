# Telemetry sources: where each harness writes token usage

| | |
|---|---|
| Date | 2026-09-29 |
| Author | the fleet orchestrator (cloud session) |
| Cited as | [S] by `ai_docs/designs/2026-09-28-fleet-telemetry-design.md` |
| Claude Code | 2.1.284 (`env -u ANTHROPIC_API_KEY /opt/node22/bin/claude --version`), native binary, 243 MB |
| Codex | `@openai/codex` 0.158.0 + `@openai/codex@0.158.0-linux-x64` (`codex --version` = `codex-cli 0.158.0`) |
| OpenCode | `opencode-ai` 1.18.33 + `opencode-linux-x64` 1.18.33 (`opencode --version` = `1.18.33`) |
| pi | `@mariozechner/pi-coding-agent` / `pi-ai` 0.73.1 (last release, 2026-05-07) and `@earendil-works/pi-coding-agent` / `pi-ai` 0.87.1 (current name) |
| Prime Agent | not inspectable here: `npm view prime-agent` returns 404 |

Provenance tags on every claim:

- **[observed]**: seen in a real file or command output in this container.
- **[source]**: read from shipped harness code (npm JS, or `strings` of a binary). The search term is given.
- **[operator]**: only known from the operator's Mac. Not re-verified here. Treat it as unverified.

Method notes. Packages came from `npm pack` into the scratch dir. Binaries were read with `strings -n 6` and fixed-string `grep`. Transcripts were read with a python key-path walker that skips `content`, `text`, `input`, `thinking` and `signature`. It prints only key paths, types and counts. No message text, prompt, email or account id was copied. The Codex linux-x64 package unpacks to 444 MB (tarball 162 MB). It was packed in the same batch before its size was checked, so it was read anyway.

**Conflicts at a glance** (details in the last section): Claude subagent transcripts carry start-of-stream usage, not final usage. pi's `cacheWrite` includes a 1h subset. The Codex `token_usage_record` per-response rule and its `turn_id` key are not confirmable from the binary. OpenCode still stores SQLite, not JSON files, so there is no storage conflict.

---

## 1. Ledger fields

Source: `crates/horch-core/src/ledger.rs`, `crates/horch/src/cmd/spawn.rs`, `crates/horch/src/cmd/worker.rs`, `crates/horch-core/src/launch.rs`, `crates/horch-core/src/mailbox.rs`. All claims in this section are **[source]** (repo code, read in this container).

`Record` (ledger.rs:35):

| Field | Type | Note |
|---|---|---|
| `record_id` | string | Minted at spawn (`mint_uuid`), always known |
| `session_id` | string or null | Null until known. Codex and OpenCode reveal it after launch |
| `agent` | string | claude / codex / opencode / pi / prime |
| `tier` | string | The teammate name, still spelled `tier` on disk |
| `model` | string | |
| `effort` | string, optional | Skipped when absent |
| `phase` | Phase, optional | Skipped when absent |
| `role` | string | Pane role |
| `status` | string | `working` or `done` |
| `task` | string | |
| `history` | array of `{at, event, text}` | |
| `created_at`, `updated_at` | string | `%Y-%m-%dT%H:%M:%SZ`, UTC |

- **state_root** (ledger.rs:113): `$HORCH_STATE_DIR` if non-empty. Else `${XDG_STATE_HOME:-$HOME/.local/state}/horch`.
- **Per-project file** (ledger.rs:145): `<state_root>/<slug>.json`. `slug` maps every byte that is not ASCII alphanumeric to `-`, like `tr -c 'A-Za-z0-9' '-'`. The project is `$HORCH_PROJECT_DIR`, else the cwd.
- **Lock** (ledger.rs:170): `create_dir("<slug>.json.lock")`. It retries every 100 ms. After 150 tries (~15 s) it removes the lock as stale. The guard removes the dir on drop.
- **Session id harvest** (worker.rs:225-290): a background thread polls every 3 s, 60 times. Codex: newest `rollout-*` under `~/.codex/sessions` whose text holds `"cwd":<project>`. OpenCode: `opencode session list`. Prime: the newest `.jsonl` in the pane's own session dir. The result goes in via `ledger.set_session`.
- Claude gets a caller-minted id (`Session::Fresh`). Codex and OpenCode run `Session::Unmanaged` and are harvested (worker.rs, launch.rs:20-28).
- **Mailbox** (mailbox.rs:80): `<temp_dir>/herdr-orchestration-<workspace_id>/`, holding `<role>.id` (pane id) and `<role>.brief.json`.

Worker pane environment (`export_brief`, worker.rs:70-96):

| Variable | Always set |
|---|---|
| `HORCH_ROLE`, `HORCH_TEAMMATE`, `HORCH_AGENT`, `HORCH_MODEL` | yes |
| `HORCH_RECORD_ID`, `HORCH_SESSION_ID` (may be empty), `HORCH_RESUME` (`1`/`0`) | yes |
| `HORCH_TASK`, `HORCH_PROJECT_DIR` | yes |
| `HORCH_STATE_DIR`, `HORCH_CLAUDE_BIN`, `HORCH_CODEX_BIN`, `HORCH_TEAMMATES_DIR` | only when the brief carries them |

`launch::apply_env` also sets `CLAUDE_CODE_SUBAGENT_MODEL` and the teammate's `env:` map. Every launch removes `ANTHROPIC_API_KEY` (launch.rs:42). `HORCH_WORKSPACE_ID` is read by `spawn`, `tell` and `tile`, but `export_brief` does not set it.

**Orchestrator:** `crates/horch/src/cmd/recipes.rs:424` launches the orchestrator pane with `Session::Unmanaged` and writes no ledger record. horch therefore has no record and no session id for the orchestrator. **[source]**

## 2. C1: Claude Code transcripts

Files:

- Main thread: `~/.claude/projects/<cwd-slug>/<sessionId>.jsonl` **[observed]** (this session, 163 lines at read time).
- Subagents: `~/.claude/projects/<cwd-slug>/<sessionId>/subagents/agent-<agentId>.jsonl`, plus `agent-<agentId>.meta.json` with keys `agentType, description, toolUseId, spawnDepth, requestShape, requestNonInteractive` **[observed]** (3 files).
- The same dir also holds `tool-results/` and `ccr-tip.json` **[observed]**.
- Subagent paths can be nested: `Wg(t)` builds `subagents/<agentTranscriptSubdirs.get(t)>/agent-<t>.jsonl`, and the path parser accepts extra segments after `subagents` **[source]** (search `"subagents"`). A flat `subagents/*.jsonl` glob can miss nested files.

Line types in the main file **[observed]**: `assistant` 43, `user` 25, `attachment` 51, `queue-operation` 8, `atis-latch` 13, `last-prompt` 13, `mode` 9, `cost-state` 1.

Assistant line key paths **[observed]** (main file, 43 lines):

| Path | Seen | Note |
|---|---|---|
| `type`, `uuid`, `parentUuid`, `timestamp`, `sessionId`, `version`, `cwd`, `gitBranch`, `isSidechain`, `userType`, `entrypoint` | 43/43 | |
| `requestId`, `apiBlockIndex`, `effort`, `perTurnEffort`, `advisorModel`, `serverClassifierRequest` | 42/43 | absent on the `<synthetic>` line |
| `message.id`, `message.model`, `message.stop_reason` | 43/43 | |
| `message.usage.input_tokens`, `output_tokens`, `cache_creation_input_tokens`, `cache_read_input_tokens` | 43/43 | int |
| `message.usage.cache_creation.ephemeral_5m_input_tokens`, `.ephemeral_1h_input_tokens` | 43/43 | int |
| `message.usage.output_tokens_details.thinking_tokens` | 42/43 | null on the synthetic line |
| `message.usage.server_tool_use.web_search_requests`, `.web_fetch_requests` | 43/43 | |
| `message.usage.service_tier`, `speed`, `inference_geo`, `iterations[]` | 42/43 | |

Repeats per `message.id` **[observed]**:

- Main file: 21 distinct ids over 43 lines. 17 ids repeat, up to 3 lines each. Every line holds exactly 1 content block, and `apiBlockIndex` counts 0, 1, 2 within an id. So there is one line per content block.
- Usage on repeats: identical in 17 of 17 repeated ids. It did not grow. Main-thread lines carry final values: `stop_reason` is set, and `thinking_tokens` is present.

**Subagent lines differ** **[observed]**, all 3 files, 2.1.284, subagents still running at read time:

- Every line has `isSidechain: true`, an `agentId`, and `attributionAgent`. Main-file lines have `isSidechain: false` and no `agentId`.
- `message.stop_reason` is null on every subagent assistant line. `output_tokens_details` and `server_tool_use` are absent.
- `output_tokens` never exceeds 41 per message (sums 383 to 962 per file). One subagent tool call had a 20,642-character input. So the value is the stream-start snapshot, not the final count.
- Repeats are identical. No later line with final usage was appended for any id.

The recorded subagent `output_tokens` is therefore a large under-count. Thinking tokens are missing. Input and cache fields are known at stream start and look plausible.

A possible final source is the Agent tool result. Its schema has `totalTokens`, `totalToolUseCount`, `totalDurationMs` and `usage{input_tokens, output_tokens, cache_creation_input_tokens, cache_read_input_tokens}` **[source]** (search `totalToolUseCount`). No subagent had finished in this session, so this is not observed.

`<synthetic>` and API errors:

- `al="<synthetic>"` is the model name on locally made assistant messages **[source]**.
- 1 line in the main file has `model: "<synthetic>"`, `isApiErrorMessage: true`, `error: "server_error"`, no `apiErrorStatus`, no `requestId`, and all four usage counts 0 **[observed]**.
- The error builder sets `isApiErrorMessage:!0, apiError, apiErrorParams, apiErrorIsTransient, quotaLimits, error, errorDetails` **[source]** (search `isApiErrorMessage:!0`).
- `apiErrorStatus` is set only when the cause is an HTTP API error (`FN()`: `s.apiErrorStatus = e.status`) **[source]**. It is optional.
- Rate limits produce `error:"rate_limit"` (11 sites) with `quotaLimits` or `apiError:"model_requires_usage_credits"` **[source]** (search `error:"rate_limit"`). A real rate-limit line was not seen here.

Other usage-bearing line: `cost-state` has `sessionId`, `totalCostUSD`, durations, and `modelUsage.<model>.{inputTokens, outputTokens, thinkingTokens, cacheReadInputTokens, cacheCreationInputTokens, webSearchRequests, costUSD}` **[observed]** (keys only). There is no 5m/1h split.

**Repo reader:** `crates/horch-core/src/usage.rs` `read_claude` reads only `<sid>.jsonl`. `find_claude_transcript` never looks in `<sid>/subagents/`, so subagent usage is ignored. It keeps the last record per `message.id`, skips `<synthetic>`, and splits `cache_creation.ephemeral_1h_input_tokens` out of `cache_creation_input_tokens`. It does not read `thinking_tokens`. **[source]** (repo)

## 3. C2: Claude OpenTelemetry

All **[source]**, from `strings` of the 2.1.284 binary.

- Metrics: `claude_code.session.count`, `lines_of_code.count`, `pull_request.count`, `commit.count`, `cost.usage` (USD), `token.usage` (tokens), `code_edit_tool.decision`, `active_time.total`.
- Span names: `claude_code.llm_request`, `claude_code.tool`, `tool.execution`, `tool.blocked_on_user`, `interaction`, `hook`, `compaction`, `subagent.spawn`, `bash.subprocess`, `mcp.rpc`.
- `claude_code.token.usage` is added 4 times per response, with `type` = `input`, `output`, `cacheRead`, `cacheCreation`. The other attributes are `model`, optional `speed:"fast"`, `query_source`, `effort` (search `type:"cacheCreation"`). The value is `cache_creation_input_tokens`, so **there is no 5m/1h split**. **No thinking-token type.**
- Log event `api_request`: `model, input_tokens, output_tokens, cache_read_tokens, cache_creation_tokens, cost_usd, cost_usd_micros, duration_ms, ttft_ms, request_id, client_request_id, speed, query_source, effort` (search `rs("api_request",{model:e`). There is no TTL split and no thinking field.
- Log event `api_error`: `model, error, status_code, duration_ms, attempt, request_id, speed, query_source, effort`. Also `api_retries_exhausted`, `api_retry`, `api_refusal`, `user_prompt`, `tool_result`, `tool_decision`.
- Identity attributes: `organization.id`, `user.email`, and `user.account_uuid` (gated by `OTEL_METRICS_INCLUDE_ACCOUNT_UUID`). Also `user.id`, `user.account_id`, `session.id`, `app.version` (search `"organization.id"`).
- Env surface: `CLAUDE_CODE_ENABLE_TELEMETRY`, `OTEL_METRICS_EXPORTER`, `OTEL_LOGS_EXPORTER`, `OTEL_TRACES_EXPORTER`, `OTEL_EXPORTER_OTLP_*`, `OTEL_EXPORTER_PROMETHEUS_HOST/PORT`, `OTEL_METRICS_INCLUDE_{SESSION_ID,VERSION,ACCOUNT_UUID,ENTRYPOINT,REPOSITORY,RESOURCE_ATTRIBUTES}`, `OTEL_LOG_USER_PROMPTS`, `OTEL_LOG_TOOL_DETAILS`, `OTEL_LOG_RAW_API_BODIES`, and others.

This supports D1. OTel loses the cache TTL split and thinking tokens, which the transcript has (section 2). It also attaches identity attributes to every record.

## 4. X1: Codex rollouts

Version: 0.158.0, a musl static Rust binary at `vendor/x86_64-unknown-linux-musl/bin/codex`, 287 MB. No rollout exists in this container (`~/.codex` is absent) **[observed]**. Everything below is from `strings` (**[source]**) unless marked otherwise.

- **Path:** `<CODEX_HOME>/sessions/YYYY/MM/DD/rollout-<timestamp>-<uuid>.jsonl`.
  - `sessions`, `archived_sessions`, `rollout-` and a `%Y/%m/%d` format string are present **[source]**.
  - The full pattern with the uuid suffix is **[operator]**: `ai_docs/reports/model-guide-2026-09.md` and the operator's files.
  - horch's `codex::sessions_dir` hardcodes `~/.codex/sessions`, not `$CODEX_HOME` (codex.rs:37) **[source]** (repo).
  - There is also a `codex.sqlite` state DB (strings `codex.sqlite`, `rollout DB stale row sample`). Its schema was not examined.
- **Line types** (`RolloutItemWire` variants, search `struct variant RolloutItemWire::`): `session_meta`, `response_item`, `turn_context`, `event_msg`, `compacted`, **`token_usage_record`**, `inter_agent_communication`, `world_state`, `realtime_item`, `retained_context`. Each line wraps a `payload`.
- **`token_usage_record` exists.** The struct is `TokenUsageRecord with 8 elements`, and field names next to it are `session_id`, `response_id`, `usage`, `thread_token_usage`.
  - Another string run lists `session_id root_turn_id response_id usage thread_token_usage started_at completed_at duration_ms` (8 names). That suggests `root_turn_id`, not `turn_id`. This is inferred from string adjacency, not confirmed.
  - That there is exactly one record per `response_id`, and that it lands after the last `token_count`, is **[operator]**.
- **`TokenUsage`** (7 fields): `input_tokens, cached_input_tokens, cache_write_input_tokens, output_tokens, reasoning_output_tokens, total_tokens, codex_rollout_budget_units`.
- **`token_count`** is an `event_msg` whose payload is `TokenCountEvent` (2 fields: `info`, `rate_limits`). `info` is `TokenUsageInfo` (3 fields): `total_token_usage`, `last_token_usage`, `model_context_window`. `total_token_usage` is the running thread total. `last_token_usage` is the latest response.
- **Cached input inside input:** OTel emits both `codex.turn.token_usage.input_tokens` and `codex.turn.token_usage.non_cached_input_tokens`. This implies `input_tokens` includes cached tokens **[source]**, indirect. The exact arithmetic is **[operator]** plus the repo comment in usage.rs.
- **`rate_limits`** (snake case `RateLimitSnapshot`, 10 fields): `limit_id`, `limit_name`, `primary`, `secondary`, `credits`, `plan_type`, `rate_limit_reached_type`, `spend_control_reached`, `normal_model_slug`, and one more.
  - Each window (`RateLimitWindow`, 3 fields) has `used_percent`, `window_minutes`, `resets_at`.
  - `resets_in_seconds` does not occur in 0.158.0 (0 hits). A backend struct has `reset_after_seconds`.
- **Errors:** `CodexErrorInfo` values include `context_window_exceeded`, `session_budget_exceeded`, `usage_limit_exceeded`, `rate_limit_exceeded` and `server_overloaded`. `codex_error_info` is a field name, and `ErrorEvent` has 2 fields.
  - Which event carries it (`error` vs `task_complete.error`) is **[operator]**.
  - `task_complete` is the tag of `TurnCompleteEvent` (7 fields).
- **`turn_context`** (`TurnContextItem`, 24 fields) includes `model`, `effort`, `approval_policy`, `sandbox_policy`, `collaboration_mode` and `current_date`.

**The under-count.** `read_codex` in usage.rs keeps the last `total_token_usage` it sees **[source]** (repo). The final response of a turn can be recorded by a `token_usage_record` (with `thread_token_usage`) after the last `token_count`. If so, the last `total_token_usage` stops one response short, and the reader misses it. The mechanism fits the shipped record types. The ordering and the size (205k tokens in one sampled file) are **[operator]**.

**Codex OTel.** Exporter kinds are `statsig`, `otlp-http`, `otlp-grpc` (`OtelExporterKind::OtlpHttp` 4 fields, `OtlpGrpc` 3 fields) **[source]**. No console or file exporter string was found. Local collection would need an OTLP listener. The `OtelConfigToml` (10 fields) includes `log_user_prompt` and `trace_exporter`.

## 5. O1: OpenCode

Storage is **SQLite, not JSON files.** JSON files appear only as a one-time migration source (`storage/session/{info,message,part}/*.json` globbed into the DB) **[source]**. There is no design conflict here.

- **Path:** `<xdg data>/opencode/opencode.db`.
  - Running `opencode session list` with a scratch `HOME`/`XDG_DATA_HOME` created `.../share/opencode/opencode.db`, `-wal` and `-shm` **[observed]**. This makes no model call.
  - The name depends on the build channel: `opencode.db` for `latest`/`beta`/`prod` or with `OPENCODE_DISABLE_CHANNEL_DB=1`, else `opencode-<channel>.db`. `OPENCODE_DB` overrides the name or path **[source]** (search `opencode.db`).
- **WAL:** `PRAGMA journal_mode = WAL`, `synchronous = NORMAL`, `busy_timeout`, `wal_checkpoint(PASSIVE)` **[source]**. `pragma journal_mode` on the created DB returns `wal` **[observed]**.
- **Tables** (via `pragma table_info` on the created DB **[observed]**, matching the `CREATE TABLE` DDL in the binary **[source]**):
  - `message(id PK, session_id NOT NULL → session.id, time_created, time_updated, data TEXT NOT NULL)`
  - `part(id PK, message_id → message.id, session_id, time_created, time_updated, data)`
  - `session(id, project_id, workspace_id, parent_id, slug, directory, path, title, version, share_url, summary_*, metadata, cost, tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, tokens_cache_write, revert, permission, agent, model, time_created, time_updated, time_compacting, time_archived)`
  - Indexes: `message_session_time_created_id_idx`, `part_message_id_id_idx`, `part_session_idx`, `session_*`. **There is no index on `time_updated`.**
- **Second message store:** `session_message(id, session_id, type, seq, time_created, time_updated, data)` also exists **[observed]**. It is written from durable session events **[source]** (`insert(...).values({id, session_id, type, seq, ...})`). Which path the default TUI uses in 1.18.33 was not determined. This is a risk to track.
- **`message.data`:** the message JSON with `id` and `sessionID` removed (`{id:X, sessionID:J, ...Y}` then `data:Y`) **[source]**. Assistant messages have:
  - `role:"assistant"`, `cost`
  - `tokens:{input, output, reasoning, cache:{read, write}}`
  - `modelID`, `providerID`
  - `time:{created, completed}`: `completed` is set when the step ends (search `assistantMessage.time.completed=Date.now()`).
- **Double-count risk:** `step-finish` parts also carry `tokens` and `cost` **[source]**. Summing messages and parts would double-count.
- **Free-tier error:** detected by `responseBody.includes("FreeUsageLimitError")`, which maps to `reason:"free_tier_limit"` **[source]**. It is a string in an HTTP error body, not a class name.
- No `sqlite3` CLI is installed in this container **[observed]**. python3's sqlite 3.45.1 was used instead.

## 6. pi

The package moved. `@mariozechner/pi-coding-agent` stops at 0.73.1 (2026-05-07). The operator runs `@earendil-works/pi-coding-agent` 0.85.1 **[operator]** (`ai_docs/reports/env-research/pi-ollama-prime.md`). The current version is 0.87.1 **[observed]** (`npm view`). Both were read. All claims are **[source]** unless marked.

- **Session dir:** `--session-dir` wins, then `PI_CODING_AGENT_SESSION_DIR`, then the `sessionDir` setting (main.js:384).
  - The default is `<agentDir>/sessions/--<cwd with / \ : as ->--/`.
  - `agentDir` is `$PI_CODING_AGENT_DIR` or `~/.pi/agent` (config.js).
- **File name:** `<ISO timestamp with : and . as ->_<sessionId>.jsonl` (session-manager.js:713 in 0.87.1, :502 in 0.73.1). `CURRENT_SESSION_VERSION = 3`.
- **Lines:**
  - The header is `{type:"session", version, id, timestamp, cwd, parentSession}`.
  - Each message is `{type:"message", id, parentId, timestamp, message}`.
  - `message.role` is `user`, `assistant` or `toolResult` (0.87.1 adds `system`).
- **Assistant message:** `provider`, `model`, `responseModel?`, `responseId?`, `usage`, `stopReason`, `timestamp`.
- **`Usage`:** `input, output, cacheRead, cacheWrite, totalTokens, cost{input, output, cacheRead, cacheWrite, total}`. Two fields are new in 0.87.1:
  - `cacheWrite1h?`: a **subset of** `cacheWrite`. The Anthropic adapter fills it from `cache_creation.ephemeral_1h_input_tokens`.
  - `reasoning?`: a subset of `output`.
- **Tool results:** 0.87.1 `ToolResultMessage` has `usage?: Usage`, documented as "Usage from the tool execution itself … Not part of main LLM context accounting". pi's footer adds it to the totals. In 0.73.1 `ToolResultMessage` has **no** `usage` field.
- **Node engine:** 0.87.1 needs `>=22.19.0`. 0.73.1 needs `>=20.6.0`, and its pi-ai needs `>=20.0.0`. This container has Node v22.22.2 **[observed]**. The operator's default Node is 22.9.0 **[operator]**.
- No pi session exists here (`~/.pi` is absent) **[observed]**.

## 7. Prime Agent

- horch gives each Prime pane `<state_root>/prime/<role-slug>-<uuid>/` with `d.sock` and `sessions/` (prime.rs:39) **[source]** (repo). It passes `--daemon-socket` and `--session-dir` (worker.rs).
- The ledger `session_id` for Prime is the **absolute path** of the newest `*.jsonl` in that `sessions/` dir (`prime::find_session`, worker.rs:258) **[source]** (repo). `usage::find_pi_session` accepts a path as the id.
- Format assumption: pi-like. `usage.rs` reads Prime with `read_pi` ("as pi") **[source]** (repo). The claim that Prime is pi-derived rests on its `PI_*` env vars (`PI_CACHE_RETENTION`, `PI_OFFLINE`) **[operator]**.
- Session files are named `<session-uuid>.jsonl` under `~/.prime/agent/sessions` **[operator]**, which differs from pi's `<timestamp>_<uuid>.jsonl`.
- Unknown: Prime's line shape, whether `usage` has `cacheWrite1h`/`reasoning`, and how sub-agent (RLM) children record usage. Prime 0.9.4 is not on the npm registry (404) **[observed]**, so none of this can be checked here.

## 8. Empirical checks

| What ran | What was seen |
|---|---|
| `env -u ANTHROPIC_API_KEY claude --version` | `2.1.284 (Claude Code)` |
| Key-path walk of this session's main transcript | 163 lines; 43 assistant; 21 ids; repeats identical; one block per line; 1 `<synthetic>` `server_error` line |
| Same walk on 3 subagent transcripts | all `isSidechain:true` with `agentId`; `stop_reason` null; `output_tokens` ≤ 41; no `thinking_tokens` |
| `ls` of `<sid>/` and `subagents/` | `agent-<id>.jsonl` + `agent-<id>.meta.json`; also `tool-results/`, `ccr-tip.json` |
| `strings` + grep on claude, codex and opencode binaries | as cited per section |
| `npm view` / `npm pack` for codex, opencode-ai, both pi scopes; `npm view prime-agent` | versions above; prime-agent 404 |
| `codex --version`, `opencode --version` (scratch HOME) | `codex-cli 0.158.0`, `1.18.33` |
| `opencode session list` (scratch HOME, no model call) | created `opencode.db` + `-wal` + `-shm`; `journal_mode` = `wal`; tables as in O1 |
| `ls ~/.codex ~/.pi ~/.prime ~/.local/state/horch` | all absent; no Codex, pi or Prime data here |
| `which sqlite3` | absent |

## 9. Conflicts with the design

Each design statement below is checked against the evidence above.

1. **"Claude usage dedupe by `message.id`, last record wins."**
   - Holds for main-thread files: repeats are identical and final **[observed]**.
   - **Conflict for subagent files:** each line carries the stream-start usage (`stop_reason` null, `output_tokens` ≤ 41, no `thinking_tokens`), and no final line follows **[observed]**, 2.1.284, running subagents. The last record per id under-counts output and reasoning.
   - The finished Agent tool result carries `totalTokens` and `usage` **[source]**, but no finished result has been seen yet.
   - Test this on a completed subagent before relying on either.
2. **"`<synthetic>` skipped."** No conflict. Synthetic lines have zero usage **[observed]**. Rate-limit synthetic lines use `error:"rate_limit"` **[source]**. `apiErrorStatus` is optional, and it was absent on the one observed synthetic line.
3. **"Subagents at `<sid>/subagents/*.jsonl`."** Partial conflict. Nested subdirectories under `subagents/` are supported **[source]**, so a flat glob can miss files. `*.meta.json` files sit next to the transcripts **[observed]**.
4. **"Codex `token_usage_record`, 1 per `response_id`."** Not contradicted, and not confirmed. The type and its `response_id`/`usage`/`thread_token_usage` fields exist in 0.158.0 **[source]**. One per response and "after the last token_count" are **[operator]**. The design fixture uses `turn_id`, but binary strings suggest `root_turn_id` (inferred).
5. **"Codex `input_tokens` includes `cached_input_tokens`."** No conflict. OTel's separate `non_cached_input_tokens` supports it **[source]**, indirectly.
6. **"OpenCode `message` table with `session_id`, `time_updated`, `data`."** No conflict **[observed]**. Risks:
   - There is no index on `time_updated`.
   - A second `session_message` table exists.
   - `step-finish` parts duplicate tokens.
   - The DB name varies by channel and `OPENCODE_DB`.
7. **"pi usage fields `input/output/cacheRead/cacheWrite`."** The names match **[source]**. **Conflict with the design's mapping** `cacheWrite -> cw5m`: in pi 0.87.1, `cacheWrite` includes the new `cacheWrite1h` subset, so 1h writes would be priced as 5m. `ToolResultMessage.usage` exists only in the `@earendil-works` line (0.87.1), not in `@mariozechner` 0.73.1.
8. Design R8 (pi needs Node ≥22.19) matches `@earendil-works` 0.87.1 **[source]**. It does not match `@mariozechner` 0.73.1 (≥20.6.0). The design should name the `@earendil-works` package.
