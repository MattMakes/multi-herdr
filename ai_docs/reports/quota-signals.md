# Quota signals: evidence report [Q]

- Date: 2026-09-29
- Author: the fleet orchestrator (cloud session)
- Repo commit read: `a4dcee2`
- Cited by: `ai_docs/designs/2026-09-28-fleet-telemetry-design.md` as [Q]

Versions inspected:

| harness | version | where |
|---|---|---|
| Claude Code | 2.1.284 | `/opt/node22/bin/claude` -> `/opt/claude-code/bin/claude`, 243,059,896 bytes. `env -u ANTHROPIC_API_KEY claude --version` printed `2.1.284 (Claude Code)` |
| Codex | 0.158.0 | `npm pack @openai/codex` + `@openai/codex@0.158.0-linux-x64`. Binary `vendor/x86_64-unknown-linux-musl/bin/codex` |
| OpenCode | 1.18.33 | `npm pack opencode-ai` + `opencode-linux-x64@1.18.33`. Binary `bin/opencode` |
| pi (asked for) | 0.73.1 | `npm pack @mariozechner/pi-coding-agent` (latest on that name) |
| pi (what the operator runs) | 0.85.1 | `npm pack @earendil-works/pi-coding-agent@0.85.1` (latest is 0.87.1) |

Provenance tags:

- **[observed]**: seen in a real file or command output in this container.
- **[source]**: read from shipped code (npm JS, or `strings` of a binary). The search term is named.
- **[operator]**: known only from the operator's Mac on 2026-09-28. It cannot be re-verified here.

Safety: no model turn, no control request to a real claude, no usage endpoint call, no credential
file read. `ANTHROPIC_API_KEY` was stripped from the one `claude --version` run. The downloaded
codex ran only `--version`, `app-server --help`, `generate-ts` and `generate-json-schema`, with
`HOME` and `CODEX_HOME` set to an empty scratch dir.

## 1. Claude

### 1a. Control protocol and `get_usage`

Envelope, from the protocol's own zod schema with descriptions [source, term
`type:R("control_request")`, `type:R("control_response")`]:

| message | fields |
|---|---|
| request | `{"type":"control_request","request_id":<string>,"request":{"subtype":<string>,...}}`. `request_id` is "chosen by the sender, unique among its in-flight requests" |
| success reply | `{"type":"control_response","response":{"subtype":"success","request_id":<string>,"response":{...}}}`. `response.response` is "the success payload, shaped as documented for the answered request's subtype" |
| error reply | `{"type":"control_response","response":{"subtype":"error","request_id":<string>,"error":<string>}}` |
| cancel | `{"type":"control_cancel_request","request_id":<string>}` |

The success variant also carries optional `pending_permission_requests` and similar siblings of
`response` [source, term `subtype:R("success"),request_id`].

`get_usage` exists [source, term `get_usage`, 11 hits]:

- Request schema: `{subtype:"get_usage", skip_behaviors?: boolean}`. The description: "Requests the
  structured /usage data: session cost/usage totals plus claude.ai plan rate-limit utilization when
  available. Experimental - the response shape may change."
- `skip_behaviors`: "Skip the scan of local transcripts ... For callers that need only the plan rate
  limits, such as a usage meter". A non-boolean is rejected with `get_usage: skip_behaviors must be
  a boolean`.
- The SDK client wraps it as
  `usage_EXPERIMENTAL_MAY_CHANGE_DO_NOT_RELY_ON_THIS_API_YET({skipBehaviors})`.
- The handler calls the same function as the `/usage` slash command. That function reads the usage
  endpoint (section 1d) and, unless skipped, scans local transcripts. Nothing on this path calls the
  model [source, handler `ze=async(e,o)=>{...T3t(...)}`].
- Some reads are answered from a cached snapshot: `Usage read answered from a snapshot <n>s old;
  endpoint not asked` [source].
- A different handler answers `get_usage is not supported in this context`. It sits next to
  `[bridge:repl]` log strings, so it looks like the remote-control bridge, not `-p` stream-json mode
  [source; inference].

Response payload (`response.response`) schema [source, term `rate_limits_available:H()`]:

```
session: {total_cost_usd, total_api_duration_ms, total_duration_ms,
          total_lines_added, total_lines_removed, model_usage}
subscription_type: 'pro'|'max'|'team'|'enterprise'|null
rate_limits_available: boolean   // false for API key, Bedrock, Vertex, or no profile scope
rate_limits: null | {
  five_hour, seven_day, seven_day_oauth_apps, seven_day_opus, seven_day_sonnet:
      null | {utilization: number|null  /* "Percentage of the window used, 0-100." */,
              resets_at: string|null    /* "ISO 8601 timestamp" */}
  model_scoped?: [{display_name /* e.g. 'Fable' */, utilization, resets_at}]
  extra_usage?: null | {is_enabled, monthly_limit, used_credits, utilization, currency?}
}
behaviors: null | {day, week}
```

- `utilization` is 0-100 and `resets_at` is ISO 8601 [source, schema descriptions]. When the reading
  is seeded from response headers, the code multiplies the header fraction by 100 and converts epoch
  seconds to ISO, so the output scale stays 0-100 [source, term `utilization:s.utilization*100`].
- `limits` array: the documented `get_usage` schema has no `limits` key. But the builder spreads the
  raw endpoint body into `rate_limits` (`{...m, model_scoped:p}`), and drops `limits` only for a
  header-seeded reading. So a live `ok` reply very likely carries `rate_limits.limits` inside
  `rate_limits`, not beside it [source; runtime shape not confirmable here].
- The endpoint body's `limits[]` rows are `{kind, group, percent, resets_at, severity, is_active,
  scope:{model?:{display_name}, surface?:{display_name}}}` [source, term `is_active:_o()`]. `kind`
  is e.g. `session`, `weekly_all`, `weekly_scoped`: "Classify a row on this, never on a label."
  `percent` is 0-100. `severity` is e.g. `normal`, `warning`, `critical`. `is_active` is "the
  server's headline pick".
- No identity field (`account_uuid`, `email`) is in the documented reply. Whether the raw spread
  leaks any is not confirmable here.

R2 status: the envelope is now documented in shipped code. The payload is at `.response.response`,
and `rate_limits` is a direct key of it. What stays unconfirmed is only the live reply (passthrough
keys, and whether `-p` answers before any user message).

### 1b. Stream-json `rate_limit_event`

Schema [source, term `type:R("rate_limit_event")`]: `{type:"rate_limit_event", rate_limit_info,
uuid, session_id}`, "emitted when rate limit info changes".

`rate_limit_info`: `status` (`allowed` | `allowed_warning` | `rejected`), `resetsAt` (int),
`rateLimitType` (`five_hour`, `seven_day`, `seven_day_opus`, `seven_day_sonnet`,
`seven_day_overage_included`, `overage`), `utilization`, and `unifiedWindows.{five_hour, seven_day,
seven_day_overage_included}.{utilization, resetsAt}`. The description says utilization is "the
fraction of the window used (usually 0-1 ... values above 1 occur ...); resetsAt is unix epoch
seconds". The values come from `anthropic-ratelimit-unified-*` response headers, so the event
appears only after a real API response, that is, during a model turn [source].

### 1c. 429 transcript line

- Rate-limit errors are built by `Qo({content, error:"rate_limit", ...})`, which sets
  `isApiErrorMessage:!0` [source, term `error:"rate_limit"`].
- The transcript writer copies `apiErrorStatus` (and `apiErrorCode` when set) [source, term
  `apiErrorStatus:`].
- The synthetic model id is `al="<synthetic>"` [source, term `"<synthetic>"`].
- Message text: ``You've hit your ${label}${resets}`` with `resets = " · resets <time>"`. Labels:
  `five_hour` "session limit", `seven_day` "weekly limit", `seven_day_opus` "Opus limit",
  `seven_day_sonnet` "Sonnet limit", `seven_day_overage_included` "Fable limit", `overage` "usage
  credit limit" [source, term `bme={five_hour:`]. A parser maps these prefixes back to
  `session`/`weekly`/`model`.
- The operator saw a line reading like "You've hit your weekly limit · resets Oct 2 at 7am
  (America/Phoenix)" [operator]. The reset is local wall-clock text, not a timestamp. No 429 line
  exists in this container's `~/.claude/projects` [observed].

### 1d. `/api/oauth/usage`

The strings `/api/oauth/usage`, `/api/oauth/usage?at_wall=1&skip_spend=1` and
`/api/oauth/usage?cedar_ember=1&skip_spend=1` exist [source, term `api/oauth/usage`]. The read map
is `Q4={plain:"/api/oauth/usage", at_wall:..., cedar_ember:...}`, and the `get_usage`/`/usage` path
uses `plain`. It sends the OAuth bearer token. Not called.

### 1e. statusLine input

The statusLine JSON includes `rate_limits` when known: `{five_hour:{used_percentage, resets_at},
seven_day:{...}}`, plus `spend_limit` on a gateway [source, term `used_percentage:Vgt(`].
`used_percentage` is `round(utilization*1000)/10`, so 0-100 with one decimal. `resets_at` is
compared with `Math.floor(Date.parse(..)/1000)`, so it is epoch seconds [source]. It only holds what
the session has seen in response headers. The design rejects it because the operator's status line
already uses the slot [S]; nothing here contradicts that.

### 1f. Model-scoped windows

- Fixed keys: `seven_day_opus`, `seven_day_sonnet`, `seven_day_oauth_apps` [source].
- Server rows with `kind:"weekly_scoped"` and `scope.model.display_name` (e.g. "Fable") become
  `rate_limits.model_scoped[]`, filtered by an allowlist. An epoch `resets_at` is converted to ISO
  [source, term `model_scoped projection failed`].
- The header window for this is `seven_day_overage_included`, labelled "Fable limit" in 429 text
  [source].

### Claude operator facts

| fact | tag |
|---|---|
| Claude weekly window 100% used, resets 2026-10-02 14:00Z | [operator] |
| Claude extra usage enabled, at 7.5% of the monthly limit | [operator] |
| 429 line "You've hit your weekly limit · resets Oct 2 at 7am (America/Phoenix)" | [operator] |

## 2. Codex

Version 0.158.0. The codex binary's own `app-server generate-ts` and `generate-json-schema` output
was used as the main source [observed]; strings of the binary back it up [source].

- `codex app-server` exists, with `daemon`, `proxy`, `generate-ts` and `generate-json-schema`
  subcommands [observed, `app-server --help`].
- Handshake: `initialize` request with params `{clientInfo:{name, version, title?}, capabilities?}`.
  Only `clientInfo` is required. Then the client notification `{"method":"initialized"}` [observed,
  `InitializeParams`, `ClientNotification.ts`].
- `account/rateLimits/read`: `{id, method, params?: GetAccountRateLimitsParams | null}`. The params
  are `supportsLunaReserve?` and `excludeResetCreditDetails?` ("Skip the separate reset-credit
  detail lookup for background usage polls"). There is no thread or turn id [observed,
  `ClientRequest.json`, `GetAccountRateLimitsParams.ts`].
- `GetAccountRateLimitsResponse` [observed]:
  - `ordinaryUsageAllowed: boolean|null`: "Backend permission for ordinary included usage ... Null
    means unavailable; clients must not infer recovery from percentages or reset times."
  - `rateLimits: RateLimitSnapshot`: "backward-compatible single-bucket view".
  - `rateLimitsByLimitId: {[limit_id]: RateLimitSnapshot}|null`, e.g. `codex`.
  - `rateLimitResetCredits`, `accountId: string|null`, `rateLimitUpsell`.
- `RateLimitSnapshot`: `limitId, limitName, normalModelSlug, primary, secondary, credits{hasCredits,
  unlimited, balance}, individualLimit, spendControlReached, planType, rateLimitReachedType`
  [observed].
- `RateLimitWindow`: `usedPercent` (int32, required), `windowDurationMins` (int64 or null),
  `resetsAt` (int64 or null) [observed, JSON schema]. The unit of `resetsAt` is not written in the
  schema. Epoch seconds is the likely unit (the operator's reading and the design fixture both use
  it), but it is not confirmable here.
- `account/rateLimits/updated` notification: `{rateLimits: RateLimitSnapshot}`. "Sparse rolling
  rate-limit update. Clients should merge available values into the most recent
  `account/rateLimits/read` response or refetch." [observed].
- `planType` values include `prolite`, `pro`, `promax`, `plus`, `free`, `team`, `enterprise`,
  `unknown` and more [observed, `PlanType.ts`].
- `rateLimitReachedType`: `rate_limit_reached`, `workspace_owner_credits_depleted`,
  `workspace_member_credits_depleted`, `workspace_owner_usage_limit_reached`,
  `workspace_member_usage_limit_reached` [observed].
- Errors: `CodexErrorInfo` includes `usageLimitExceeded` and `rateLimitExceeded` (camelCase on the
  app-server). The snake_case `usage_limit_exceeded` and `codex_error_info` exist in the binary for
  the rollout/core side [source, terms `usage_limit_exceeded`, `codex_error_info`]. User text starts
  "You've hit your usage limit" with plan-specific tails [source].
- Rollout snapshot (`TokenCountEvent.rate_limits`): `RateLimitSnapshot` with `limit_name,
  normal_model_slug, primary, secondary, credits, spend_control_reached, plan_type,
  rate_limit_reached_type`. Windows are `RateLimitWindow{used_percent, window_minutes, resets_at}`
  [source, terms `RateLimitWindowused_percentwindow_minutesresets_at`, `TokenCountEvent`].
  `resets_in_seconds` has 0 hits in 0.158.0 [source]. Older rollouts written by older versions may
  still carry it.
- Backend wire shape (what `/wham/usage` returns): `RateLimitWindowSnapshot` with `used_percent,
  limit_window_seconds, reset_after_seconds` and a 4th field, under
  `primary_window`/`secondary_window` [source]. Codex converts this to the shape above.
- Endpoint strings: `/wham/usage`, `/api/codex/usage`, `/wham/usage/thread_usage/query`,
  `/wham/rate-limit-reset-credits` [source, term `wham/usage`]. Not called.

Positional windows. `primary` and `secondary` are two nullable slots. Each carries its own
`windowDurationMins`. The TUI labels by duration ("5-hour limits", "Weekly limits") [source]. The
operator saw the slots change meaning by plan:

| fact | tag |
|---|---|
| Codex weekly 99% used, resets 2026-10-03 19:15Z | [operator] |
| `plan_type` `prolite`, a single 7-day window in `primary` | [operator] |
| Rollouts from 2026-09-11: primary = 5h, secondary = 7d | [operator] |

So windows must be identified by `windowDurationMins`/`window_minutes` (300 = 5h, 10080 = 7d), never
by slot.

## 3. OpenCode

Version 1.18.33.

- `FreeUsageLimitError` has 1 hit [source, term `FreeUsageLimitError`]. The client never raises it.
  The OpenCode Zen server returns it. The client's session-retry classifier checks
  `e.data.responseBody?.includes( "FreeUsageLimitError")` on an `APIError`.
- Then it reports `GO_UPSELL_MESSAGE` = "Free usage exceeded, subscribe to Go", with action
  `{reason:"free_tier_limit", title:"Free limit reached", link:"https://opencode.ai/go"}` [source].
- The branch is reached only for an error already judged retryable (or 5xx). The retry loop then
  retries up to `RETRY_MAX_RETRIES=5` with backoff (factor 2, 25% jitter, no-header max 30 s)
  [source]. A free-limit hit can therefore look like repeated retries before the session fails.
- The paid sibling `GoUsageLimitError` carries `metadata.limitName` and a `retry-after` header ("...
  It will reset in <n> hours") [source]. The free error carries no reset time.
- No quota or usage endpoint for Zen free models was found. The only `/usage` paths in the binary
  are OpenAI SDK `/organization/usage/*` routes [source, term `/usage`]. Zen base URLs:
  `opencode.ai/zen/v1`, `opencode.ai/zen/go/v1`.
- Free models: without an OpenCode login the `opencode` provider keeps only models with `cost.input
  === 0` and uses `apiKey:"public"`. The picker shows "free" when `provider.id==="opencode"` and
  `cost.input===0` [source]. The bundled catalog names end in `-free` (for example
  `nemotron-3-ultra-free`, `nemotron-3.5-lightning-free`, `glm-free`). `big-pickle` has no suffix
  but is served free the same way [source]. Free means "cost 0", not "no limit".

## 4. Local (pi + ollama)

| package | version | `engines.node` |
|---|---|---|
| `@mariozechner/pi-coding-agent` (as asked) | 0.73.1 (latest) | `>=20.6.0` [observed] |
| `@earendil-works/pi-coding-agent` (operator's install, per repo reports) | 0.85.1 | `>=22.19.0` [observed] |

- The repo's own reports place the operator's pi at
  `/opt/homebrew/lib/node_modules/@earendil-works/pi-coding-agent`, version 0.85.1 [observed,
  `ai_docs/reports/env-research/pi-ollama-prime.md`]. The 22.19 floor belongs to that package. The
  `@mariozechner` name is an older line and does not show it.
- pi cannot start on the operator's machine: nvm default Node 22.9 < 22.19 [operator]. The container
  has Node 22.22.2, so this is not reproducible here.
- `--version`: both READMEs list `-v`, `--version` "Show version" [observed].
- Ollama: add a provider in `~/.pi/agent/models.json`:
  `{"providers":{"ollama":{"baseUrl":"http://localhost:11434/v1",
  "api":"openai-completions","apiKey":"ollama","models":[{"id":"..."}]}}}`. "The `apiKey` is
  required but Ollama ignores it." The model is then named `ollama/<id>` [observed, `docs/models.md`
  in both packages]. `teammates/pi.md` uses `ollama/qwen3.8` and points to the same file.
- No quota exists for this pool. Its only signals are health: pi starts, and ollama answers.

## 5. Routing points

`crates/horch/src/cmd/spawn.rs` (384 lines) [observed]:

| point | lines |
|---|---|
| `Roster::is_spawnable(&teammate)` on the resume branch | 99 |
| `Roster::is_spawnable(&teammate)` on the fresh branch | 112 |
| Final gate, `Roster::model_is_spawnable(&plan.model, &plan.teammate.name)?` | 136 (comment 131-135) |
| Ledger write: comment 163, `resume_with_phase` 164-165, `add_with_phase` 166-177, `set_effort` 178 | 163-178 |

The design's "line 136" and "164-178" match.

`crates/horch-core/src/teammates.rs` [observed]:

- `ORCHESTRATOR_TIERS = [("fable","opus"), ("astra","codex-sol")]` at 45.
- `reserved_tier(model)` at 65-73 matches a model segment against those tiers.
- `is_spawnable` at 786-788 delegates to `model_is_spawnable`.
- `model_is_spawnable` at 796-806. The top-tier refusal is its `bail!`: "'{who}' runs on {model},
  and the {tier} tier is reserved for the orchestrator; ... Use {instead}." `{instead}` is advice in
  the error text. horch does not act on it.
- `enum Agent { Claude, Codex, Opencode, Pi, Prime, None }` at 202-211.

No teammate substitution: `grep -rni 'fallback\|substitut' crates/` finds only unrelated uses
(placeholder substitution in prompts, tile id substitution, rollout-file and jq fallbacks, the
"generic fallbacks" roster heading) [observed]. A refused spawn stops. Nothing picks another
teammate.

`crates/horch/src/cmd/recipes.rs` `FleetFlavor` at 75-85: `Opus` (`opus`), `Fable` (`fable`),
`Astra` (`gpt-6-astra`), `Sol` (`gpt-5.6-sol`), models at 109-116 [observed]. Only Fable and Astra
hold a reserved tier (test at 534-539).

Teammates (frontmatter `agent` / `model`) and derived pool [observed]:

| pool | teammates |
|---|---|
| claude | `_template` (opus), architect-reviewer (opus), backend-developer (opus), designer (opus), frontend-developer (opus), opus (opus), product-lead (opus), researcher (opus), staff-engineer (opus), qa-engineer (sonnet), sonnet (sonnet), orchestrator (fable), orchestration-orchestrator (fable), orchestration-worker (no model) |
| codex | codex-luna (gpt-5.6-luna), codex-terra (gpt-5.6-terra), codex-sol / codex-network / codex-reviewer (gpt-5.6-sol), orchestrator-codex (gpt-6-astra) |
| opencode-zen | opencode-lightning (`opencode/nemotron-3.5-lightning-free`), opencode-ultra (`opencode/nemotron-3-ultra-free`), opencode-pickle (`opencode/big-pickle`) |
| local | pi (`ollama/qwen3.8`) |
| unmapped | prime (agent `prime`, model `anthropic/claude-opus-5-5`); smoke (agent `none`) |

The pool follows `agent` alone, except `prime`: its model is an Anthropic model, and which
credential it bills was not inspected here. The design's conservative mapping (`prime` +
`anthropic/*` -> `claude`, design line 379) is the safe reading of that gap.

## 6. Normalization table

| source | scale | reset format | window identification |
|---|---|---|---|
| Claude `get_usage` `rate_limits.<key>` | 0-100 (`utilization`) | ISO 8601 string | fixed key: `five_hour`, `seven_day`, `seven_day_opus`, `seven_day_sonnet`, `seven_day_oauth_apps` |
| Claude `get_usage` `rate_limits.model_scoped[]` | 0-100 | ISO 8601 (converted from epoch if needed) | `display_name` (e.g. Fable) |
| Claude endpoint `limits[]` (passthrough, unconfirmed) | 0-100 (`percent`) | ISO 8601 | `kind` + `scope`, never the label |
| Claude stream-json `rate_limit_event` | 0-1, can exceed 1 | epoch seconds (int) | `rateLimitType` / `unifiedWindows` key |
| Claude statusLine `rate_limits` | 0-100, 1 decimal | epoch seconds | fixed key `five_hour`, `seven_day` |
| Claude 429 transcript | none (text) | local wall-clock text with IANA zone | label in text: "session", "weekly", "Opus", "Sonnet", "Fable" limit |
| Codex `account/rateLimits/read` | 0-100 integer (`usedPercent`) | int64, epoch seconds likely (unit unstated) | `windowDurationMins`, not slot |
| Codex rollout `rate_limits` | 0-100 (`used_percent`) | `resets_at` (0.158.0); older files may have `resets_in_seconds` | `window_minutes`, not slot |
| Codex backend `/wham/usage` (not used) | 0-100 | `reset_after_seconds` relative | `limit_window_seconds` |
| OpenCode Zen free | none | none | none: only the `FreeUsageLimitError` string |
| pi + ollama | none | none | health only |

## 7. Conflicts with the design

1. `get_usage` is a control request with no model turn: **consistent** [source]. Whether `claude -p
   --input-format stream-json` answers it before any user message is **not confirmable here** (no
   real claude was driven).
2. The `get_usage` fixture in Appendix A has the **wrong nesting**. It closes `rate_limits` after
   `seven_day` and puts `seven_day_opus`, `seven_day_sonnet` and `limits` beside it. In shipped
   code, `seven_day_opus`/`seven_day_sonnet` are inside `rate_limits`, and `limits` (if present) is
   spread inside `rate_limits` too [source].
3. The fixture uses `severity:"ok"`. Shipped values are `normal`, `warning`, `critical` [source].
   The fixture's `limits` row also lacks `group`.
4. The fixture omits `rate_limits.model_scoped`, which is where the shipped reply puts the Fable
   weekly window in a documented form [source].
5. R2 ("envelope not documented") is **out of date**. The shipped schema documents
   `control_response.response.{subtype, request_id, response}`. The parser's "first nested object
   with `rate_limits`" still works, but `.response.response` is now a known path.
6. `account/rateLimits/read` exists and needs no thread or turn: **consistent** [observed]. The
   request in the design matches the schema (`params: null` is allowed; `initialize` needs only
   `clientInfo`).
7. Scales: `get_usage` 0-100 ISO, stream-json 0-1 epoch, Codex 0-100 epoch: **consistent**, with one
   note. Codex `resetsAt` is int64 but its unit is not stated in the schema, so "epoch seconds" is
   **not confirmable here**.
8. pi's 22.19 floor: **consistent** for `@earendil-works/pi-coding-agent` 0.85.1, but **not** for
   `@mariozechner/pi-coding-agent` (0.73.1, `>=20.6.0`). The design should name the
   `@earendil-works` package.
9. `prime` (`anthropic/claude-opus-5-5`): the design maps it to `claude` "conservatively"; its real
   credential type is **not confirmable here**.
