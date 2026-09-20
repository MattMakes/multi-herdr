# Claude Code: env vars and settings for fleet efficiency

- Researcher: researcher-1. Date: 2026-09-18. Brief: `ai_docs/plans/env-research/01-claude-code.md`.
- **Version.** The brief scopes 2.1.274. But `claude` on PATH (`~/.local/bin/claude`) now resolves to **2.1.276**, which auto-updated at 07:02 today (`~/.local/share/claude/versions/2.1.276`, Mach-O arm64 Bun binary). Every `claude` teammate launched from now on runs 2.1.276. I checked all evidence below in 2.1.276. Every env var, settings key and flag cited is also present in the 2.1.274 binary (`strings | grep -c` > 0 for each, including `bashOutputMaxChars`, `taskOutputMaxChars`, `promptSuggestionEnabled`, `maxEffortLevel`, `--autocompact` and `--exclude-dynamic-system-prompt-sections`).
- **Raw env-var count (step 1 regex, verbatim command):** **1267** names in 2.1.274 (`/tmp/claude-envvars.txt` = `/tmp/claude-envvars-2.1.274.txt`) and 1271 in 2.1.276. The regex over-matches (`MAX_TREE_IMAGE_BYTES`, `CLAUDE_PROJECT_DIRO` and similar are not env vars), so I used two cleaner sources as the "exists" evidence:
  - the typed env registry (`NAME:()=>getter` export map): **1228** names in 2.1.274, **1223** in 2.1.276 (`/tmp/claude-registry-*.txt`);
  - a hard-coded "safe env" allowlist `Za` of **208** public env vars (`/tmp/claude-za-set.txt`). `TJe()` checks it in two places: when a settings `env` block is applied (allowlisted keys apply from any source; trusted sources such as user settings apply every key via `Object.assign`), and when non-allowlisted env keys in settings are flagged.
  - 2.1.274→2.1.276 registry diff: only internal names changed (`CLAUDE_CONTEXT_COLLAPSE*`, `*_PARKED_PERMISSION` removed; `CLAUDE_CODE_DISABLE_ATTRIBUTION_CROSS_REPO` and `CLAUDE_CODE_FORCE_TERMINAL_IMAGES` added). **None of the brief's candidates changed.**
- **Settings keys:** 639 schema entries with `.describe()` text were extracted from the binary (`/tmp/claude-settings-describe.txt`). Defaults quoted below come from those strings or from the code constants.
- **Verification methods** (shared-context numbering): **M1** = in the installed binary (registry, `Za` allowlist, or code read); **M2** = official docs (URL); **M3** = `claude --help` (saved to `/tmp/claude-help-276.txt`).
- **Models checked:** `claude-opus-5`, `claude-fable-5-1`, `claude-sonnet-5`. All three have `context.window: 1e6, native_1m: true`, `max_output_tokens {default: 64000, upper: 128000}` and `default_effort: "high"` in the binary's model table.

---

## Headline findings

1. **The bundle-dump fix is a settings key, not the env var.** In 2.1.276 the Bash tool's inline cap is `maxResultSizeChars = bashOutputMaxChars ?? 30000` (code `X1e()`). The binary's own schema text says `BASH_MAX_OUTPUT_LENGTH` "on its own only sizes the read-back window". With `bashOutputMaxChars` set, output past the cap is saved to a file and the model receives a short preview plus the path, so nothing is lost. Default is 30000 chars, clamped to 4000–128000. `taskOutputMaxChars` (default 32000) does the same for `TaskOutput`.
2. **Prompt suggestions cost tokens in every fleet pane.** The server flag `tengu_chomp_inflection` is `true` for this account (cached in `~/.claude.json`). After each turn, the interactive main thread therefore forks the **whole conversation on the same model** (`querySource:"prompt_suggestion"`, `cacheSafeParams`, `skipCacheWrite`) to predict the next user prompt. Nobody reads those suggestions in a horch pane. Fix: `promptSuggestionEnabled: false` or `CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION=false`.
3. **Effort is the biggest per-token lever, and the binary has cost multipliers for it.** The model table's `effort_cost_index` (relative to `high` = 1, used by the CLI's own "uses more tokens per task" warning) gives these values:
   - Opus 5: low 0.67, medium 0.76, **xhigh 1.6**, max 1.7.
   - Fable 5.1: low 0.75, medium 0.86, **xhigh 1.38**, max 1.74.
   - Sonnet 5: low 0.47, medium 0.74, **xhigh 2.41**, max 5.59.
4. **Default auto-compaction is late on these models.** The code path resolves the "auto" window to the model's full context (1M) for native-1M models on first-party auth. The threshold is `window − min(maxOutput, 20000) − 13000`, which comes to ≈**967k tokens**.
   - Server overrides are absent: `autoCompactWindowsCache` is null and `tengu_amber_redwood2/3` are empty.
   - The docs agree independently (model-config page): native-1M models "such as Sonnet 5, the Fable models, and Opus 4.7 and later on the Anthropic API, compact before the window fills, at about 967K tokens by default."
   - The levers are `CLAUDE_CODE_AUTO_COMPACT_WINDOW`, `autoCompactWindow` and `--autocompact` (100k–1M). The compaction benchmark thread should pick the value.
5. **`DISABLE_TELEMETRY` / `DO_NOT_TRACK` / `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` are not neutral.** Any of them makes `vL()` true, which makes `Lh()` true and `IN()` false, which disables GrowthBook (`ZD()`). The CLI then runs on client-default feature flags instead of the server-tuned ones the operator's sessions use. They save no model tokens.
6. **`DISABLE_NON_ESSENTIAL_MODEL_CALLS` does not exist** in 2.1.274 or 2.1.276 (0 hits).
7. **Status of the other background "side" model calls:**
   - Currently **off** here: away summary (`awaySummaryEnabled:false`), narration (`CLAUDE_CODE_ENABLE_NARRATION` unset and `tengu_pewter_kite_ms` = 0), and background memory extraction (`tengu_passport_quail` = false).
   - `side_question` (`/btw`) runs only when the user triggers it.
   - Prompt suggestion (#2) is the only one on by default in every pane.
   - `agent_summary` and `agent_classifier` were not traced; see the Unverified list.

---

## TOP 10 (ranked)

The "Where" column uses these names:
- **overlay**: the horch `--settings` JSON overlay. Settings keys can't go in a teammate `env:` map.
- **env:**: the teammate `env:` map.
- **user settings**: `~/.claude/settings.json`. The orchestrator pane only gets settings from here or from a `horch fleet` launch flag.

Token-effect sizes are rough (≈3.5–4 chars/token for logs, ≈3 for minified JS).

| # | Name (exact) | Controls | Verified by | Default (2.1.276) | Recommended: worker | Recommended: orchestrator | Where | Token effect | Quality effect | Verdict |
|---|---|---|---|---|---|---|---|---|---|---|
| 1 | `bashOutputMaxChars` (settings) | Characters of a Bash/PowerShell result the model gets inline. Overflow goes to a file, and the model gets a preview plus the path. | M1: schema text "How many characters of a successful Bash or PowerShell command's output Claude receives inline (default 30000; values clamp to 4000-128000). Output past this is saved to a file…". Code `X1e(){return OWt(Ke().bashOutputMaxChars)??zhn}`, `zhn=30000`, clamp `G9r=4000`, `gle=128000`, used as Bash `maxResultSizeChars`. M2: code.claude.com/docs/en/tools-reference (`BASH_MAX_OUTPUT_LENGTH` "reads back from the working file… If you set the `bashOutputMaxChars` setting, Claude Code ignores this variable"); changelog v2.1.261 "Added `bashOutputMaxChars` and `taskOutputMaxChars` settings…". | 30000 chars (≈7.5–10k tokens per call) | 16000 | 8000 | overlay (every teammate, including `teammates/orchestrator.md`) | **Medium–large, tool-output side.** Caps any single Bash result at ≈2–4k tokens instead of ≈10k. Directly fixes the JS-bundle dump. | Neutral to slight risk: nothing is lost (spilled to file), but long test logs may need one extra `Read`. | **RECOMMEND** |
| 2 | `promptSuggestionEnabled` (settings) / `CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION` (env) | After-turn fork that predicts the user's next prompt | M1: schema "When false, prompt suggestions are disabled"; `ext()` checks env first, then flag `tengu_chomp_inflection` (cached **true** here), then setting. `tbr()` runs only for `repl_main_thread` and skips when the terminal reports blur, the cache is cold, or there are fewer than 2 assistant turns. Env var is in registry and `Za`. | Enabled (flag on, setting absent) | `false` | `false` (optional; the only pane a human might read) | env: `CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION=false` in each teammate file, orchestrator.md included (no code change needed), or `promptSuggestionEnabled:false` in the overlay | **Small–medium, input side.** One full-context cache read (0.1× input price) on the pane's own model plus a few output tokens per turn end. | Neutral: no one consumes the suggestion in a horch pane. | **RECOMMEND** |
| 3 | `--effort` per teammate (already set by horch; tuning) plus `maxEffortLevel` / `modelSettings.<model>.maxEffortLevel` (settings cap) | Reasoning/effort level sent per request | M3: `--effort <level> … (low, medium, high, xhigh, max)`. M1: `effort_cost_index` per model (above); `maxEffortLevel` schema "Anything above it (an /effort or /model pick, --effort, CLAUDE_CODE_EFFORT_LEVEL, a model default) is clamped to it…". M2: code.claude.com/docs/en/model-config (levels table; "`xhigh` \| Deeper reasoning at higher token spend"; resolution order "1. An explicit choice: the `CLAUDE_CODE_EFFORT_LEVEL` environment variable, launching with `--effort`, or `/effort`…"); changelog v2.1.267 adds `maxEffortLevel`. The docs give no numeric multiplier; the binary does. | Model default `high` for all three. What the fleet actually runs (`teammates/*.md` `effort:`, passed as `--effort`, which beats the operator's saved `modelSettings` per the docs' resolution order): **every Claude teammate is `xhigh`**. That is `sonnet` and `qa-engineer` (Sonnet 5, 2.41× high); `opus`, `backend-developer`, `frontend-developer`, `designer`, `architect-reviewer`, `staff-engineer`, `product-lead`, `researcher` and `orchestration-worker` (Opus 5, 1.6×); and `orchestrator` (Fable 5.1, 1.38×). The operator's `modelSettings.claude-fable-5-1: medium` is therefore overridden in the fleet. | `sonnet`, `qa-engineer`: `high`, the biggest single cut at 2.41× → 1.0. Implementers (`backend-developer`, `frontend-developer`, `opus`): `high` (1.6× → 1.0). Keep `xhigh` for `architect-reviewer`, `staff-engineer` and design/debug roles. | Fable `xhigh` → `high` saves 1.38× → 1.0. Its own plan and design reasoning is the fleet's highest-stakes thinking, so decide this one deliberately. | teammate `effort:` field (horch `--effort`); caps via overlay `maxEffortLevel` | **Large, output side** (thinking plus text). For Sonnet 5, xhigh costs 2.41× high by the CLI's own index. | Risks lower quality on hard reasoning; that is why this is per-role, not global. | **TUNE CAREFULLY** |
| 4 | `CLAUDE_CODE_AUTO_COMPACT_WINDOW` (env) / `autoCompactWindow` (settings) / `--autocompact` (flag) | Context size at which auto-compaction fires | M1: `Sv()` resolution order env → settings → clientdata → experiment → model-default. `WZ(…,w2e=1e5,XJe=1e6)`. Threshold `kPe(e)=e−13000` on effective window `T6 = window − min(maxOutput, 20000)`. M3: `--autocompact <auto\|tokens> Auto-compact window size (auto, or 100k–1M tokens)`. M2: code.claude.com/docs/en/model-config ("at about 967K tokens by default"; the env var "takes precedence over the command, the flag, and the setting"). | "auto" is the full 1M window for opus-5, fable-5-1 and sonnet-5 on first-party auth, so it fires at **≈967k** (M1 and M2 agree). If the resolved window falls below 1M (1M-credits latch, `CLAUDE_CODE_DISABLE_1M_CONTEXT`, a 3P provider), these models drop to a 200k window, firing at ≈167k. | Value belongs to the benchmark thread. Lever: `env:` or `--autocompact`. | Same lever. Orchestrator is long-lived, so this is its most important knob (see note). | env: / overlay / a horch `--autocompact` arg (orchestrator.md too) | **Large, input side** for long sessions: every turn re-reads the whole context. | Risks lost detail at each compaction. Binary warns: "Overriding auto may result in high token usage, especially when resuming long sessions." | **TUNE CAREFULLY** |
| 5 | `taskOutputMaxChars` (settings) | Characters of a background task's output that `TaskOutput` returns inline | M1: schema "(default 32000; values clamp to 4000-128000). Longer output is cut to its most recent characters with the path of the full output file…" | 32000 chars | 16000 | 8000 | overlay / user settings | **Medium, tool-output side** (background builds and watchers) | Neutral: tail kept, full output file path given | **RECOMMEND** |
| 6 | `DISABLE_AUTOUPDATER` (env) | Background self-update of the `claude` binary | M1: registry and `Za`. M2 not obtained (the docs agent did not reach the setup page; secondary sources say it "only stops the background check; `claude update` still works"). | Auto-update on | `1` | `1` | env: (every Claude teammate file, orchestrator.md included) | None directly | **Helps reproducibility.** Today the binary changed 2.1.274→2.1.276 between writing the brief and running it, so panes spawned later run a different build from earlier ones. Update deliberately between fleets. | **RECOMMEND** |
| 7 | `CLAUDE_CODE_FILE_READ_MAX_OUTPUT_TOKENS` (env) | Maximum tokens one `Read` may return | M1: `r(){let e=a.CLAUDE_CODE_FILE_READ_MAX_OUTPUT_TOKENS;…}` with fallback `gtr=25000`. Over the cap, Read errors ("File content (N tokens) exceeds maximum…") or auto-paginates (`truncatedByTokenCap`). In `Za`. | 25000 tokens | Leave at default | 10000 | env: (orchestrator.md) | **Medium, tool-output side**, orchestrator only | Slight risk: extra paginated reads | **TUNE CAREFULLY** |
| 8 | `# Compact instructions` section in the project CLAUDE.md | What the compaction summary must preserve | M2: code.claude.com/docs/en/costs, verbatim example "# Compact instructions / When you are using compact, please focus on test output and code changes" | none | Keep: role, assigned brief path, files touched, decisions made, open `horch tell` questions, report path | Same, plus roster, who-is-doing-what, and pending integrations | project `CLAUDE.md` (or the teammate prompt), outside my edit scope | None directly; lets #4 be lowered with less loss | **Helps**: the quality-side complement to an earlier compaction | **RECOMMEND** alongside any #4 change |
| 9 | `--exclude-dynamic-system-prompt-sections` (flag) | Moves cwd, env info, memory paths and git status out of the system prompt into the first user message | M3: "Improves cross-user prompt-cache reuse. Only applies with the default system prompt (ignored with --system-prompt). (default: false)" | off | Trial on one teammate type, measure `cache_creation_input_tokens` at spawn | off (single long session; nothing to share) | horch launch arg (needs a Rust change; there is no env equivalent) | **Medium, input side at spawn**, but only if several workers share an identical tools+system prefix. Git status changes constantly in a fleet. | Neutral: same info, different position | **TUNE CAREFULLY** (measure first) |
| 10 | `CLAUDE_CODE_ENABLE_TELEMETRY` + `OTEL_METRICS_EXPORTER` / `OTEL_LOGS_EXPORTER` / `OTEL_EXPORTER_OTLP_*` (env) | OpenTelemetry metrics and events (token and cost counters) | M1: all in `Za`. `OTEL_LOG_USER_PROMPTS`, `OTEL_LOG_TOOL_CONTENT` and `OTEL_LOG_ASSISTANT_RESPONSES` exist and should stay unset. | off | Only during the measurement phase: `CLAUDE_CODE_ENABLE_TELEMETRY=1`, `OTEL_METRICS_EXPORTER=otlp` to a local collector. Never `console`: it would print into the TUI pane. | same | env: | None (measurement instrument, not a saving) | Neutral. Content-logging vars stay off (privacy). | **RECOMMEND for measurement only.** The cheaper alternative is the per-message `usage` already in `~/.claude/projects/*/<session>.jsonl`. |

### Notes on the top 10

- **Rows 1 and 5 need a horch change or a teammate settings file.** They are settings keys, and horch's auto-built `--settings` overlay only carries `enabledPlugins` and `statusLine` today (`launch.rs` ~L290–L326). A teammate that names its own `settings:` file replaces the overlay entirely, so that file must also carry these keys. Putting them in `~/.claude/settings.json` would apply them to every session the operator runs, including workers and non-fleet sessions.
- **Rows 1 and 7 go together.** With a low `bashOutputMaxChars` the model gets a spill-file path and will often `Read` it. The default 25k-token Read cap lets ~100k chars through, so the tokens just move from Bash to Read. Set `CLAUDE_CODE_FILE_READ_MAX_OUTPUT_TOKENS` alongside the Bash cap, at least for the orchestrator.
- **Row 2 can ship today** through the teammate `env:` map with no code change.
- **`CLAUDE_CODE_EFFORT_LEVEL` beats `--effort`.** The binary says "CLAUDE_CODE_EFFORT_LEVEL=… overrides effort this session". Don't put it in `env:` maps unless you intend to override the teammate `effort:` field; keep using `effort:`.
- **`CLAUDE_AUTOCOMPACT_PCT_OVERRIDE` can only lower the threshold.** It is `min(floor(effWindow × pct/100), effWindow − 13000)`, and it is internally named `testPctOverride`. Prefer the window lever in row 4.

---

## Other verified levers (not top 10)

| Name | Verified | Default | Finding | Verdict |
|---|---|---|---|---|
| `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE` (env) | M1 `mtt()` / `kPe()`; `Za` | unset | Percentage of the *effective* window (window − min(maxOutput, 20k)). Only lowers the threshold. Named `testPctOverride` in code. | TUNE CAREFULLY; prefer #4 |
| `precomputeCompactionEnabled` (settings) | M1 schema | on (buffer fraction `tengu_amber_rokovoko` = 0.2) | Precomputes the summary in the background at ≈80% of the window, so the summary call is spent even if the session ends before the threshold. Matters only once #4 is lowered. | TUNE CAREFULLY with #4 |
| `autoMemoryEnabled` (settings) / `CLAUDE_CODE_DISABLE_AUTO_MEMORY` (env) | M1 schema; `uqt()` | on | Loads the memory instructions and `MEMORY.md` into every worker's system prompt (≈1–3k tokens, cached). All fleet panes share one memory dir (same cwd), so a worker's memory write changes every later pane's system prompt. Background extraction (`extract_memories`, 5-turn fork every `tengu_bramble_lintel` = 7 turns) is currently **off**: gate `tengu_passport_quail` = false. Disabling loses the operator's memories (e.g. "token economy is principled"), which workers use. | TUNE CAREFULLY (quality risk if disabled) |
| `promptCacheTtl` / `subagentPromptCacheTtl` (settings); `CLAUDE_CODE_PROMPT_CACHE_TTL` / `CLAUDE_CODE_SUBAGENT_PROMPT_CACHE_TTL` / `ENABLE_PROMPT_CACHING_1H` (env) | M1 `gLn()` / `UFn()` | Subscriber, not on overage: main thread (`repl_main_thread*`) gets **1h**; subagents 5m unless the server allowlist includes them. The cached allowlist adds `prompt_suggestion`, `away_summary`, `extract_memories`, `agent_classifier`, `rolling_compact`. On overage or API key: 5m everywhere. | Idle panes already get a 1h main-thread cache. Only if the fleet runs on API-key or overage billing: `ENABLE_PROMPT_CACHING_1H=1` for the orchestrator (long idle gaps). | No change now |
| `skillListingBudgetFraction` (settings; operator has 0.5) | M1 schema: "default: 0.01 = 1%"; `SMe()` budget = `window × 4 chars/token × fraction` | 0.01 | This is a ceiling, not a reservation. On 1M-window models, 0.01 already allows 40,000 chars (≈10k tokens); 0.5 allows 2,000,000 (no cap). Lower values save tokens only when the listing is larger than the budget, and they do it by shortening skill descriptions (risk: wrong or missed skill triggering). Workers with `disableBundledSkills` and phase-only skills sit well under 40k chars, so 0.5 vs 0.01 is a no-op for them. `skillListingMaxDescChars` (default 1536) still caps each entry. | Leave at 0.5; measure the orchestrator listing with `/context` before changing |
| `SLASH_COMMAND_TOOL_CHAR_BUDGET` (env) | M1 `SMe()`; `Za` | unset | Absolute character budget that overrides the fraction | Not needed |
| `MAX_MCP_OUTPUT_TOKENS` (env) | M1: `u(){let e=a.MAX_MCP_OUTPUT_TOKENS;…return M}`, `M=25000`; server override `tengu_velvet_ibis.mcp_tool` cached `{}`; `Za`. M2: code.claude.com/docs/en/mcp ("the default maximum is 25,000 tokens") | 25000 tokens; overflow saved to `rawOutputPath` | Applies only to teammates that declare MCP servers. The orchestrator runs `mcp_servers: {}`, and most workers get `--strict-mcp-config`. For any teammate with a chatty MCP server (e.g. a browser), 10000 via its `env:`. | TUNE CAREFULLY (per MCP-using teammate) |
| `CLAUDE_CODE_SUBAGENT_MODEL` (env; horch already sets it) | M1 `kJ()`; M2 code.claude.com/docs/en/sub-agents ("doesn't change the model the built-in Explore and Plan subagents run on"); changelog v2.1.251 "now sets default rather than overriding everything" | `inherit` | `IH()` falls back to it when an `Agent`-tool call does not pin a model. `inherit` means the parent's model. Since 2.1.251 it is a default, not an override: agent frontmatter or an explicit `model` wins. The Opus-5 main thread × N subagents is the multiplier to watch. | Keep; only a default for Agent-tool subagents |
| `CLAUDE_CODE_MAX_OUTPUT_TOKENS` (env) | M1 `dKe()` → `WZ(…, n.default, n.upperLimit)` | 64000 (upper 128000) | You pay only for tokens actually generated, so lowering saves nothing unless replies hit the cap, and then they truncate and retry. Also sets the compaction summary buffer (`min(maxOutput, 20000)`). | SKIP (no saving) |
| `MAX_THINKING_TOKENS` (env) | M1 `pnr()`; M2 code.claude.com/docs/en/costs ("Adaptive-reasoning models ignore nonzero budgets, so use effort levels there instead") | unset | On Opus 5, Fable 5.1 and Sonnet 5 a value `>0` is ignored; `0` turns thinking off (except on Fable). | SKIP (policy for 0; >0 is a no-op) |
| `CLAUDE_CODE_DISABLE_EXPLORE_PLAN_AGENTS` (env) | M1 registry, `Za` | unset | Removes the built-in Explore/Plan agent entries (small listing saving) but loses a cheap delegation route | Not recommended |
| `ENABLE_TOOL_SEARCH` (env) | M1 `EYe()` | tool search **on** (`tst`) for first-party | Tool schemas are already deferred; setting it false would load them all | Leave default |
| `systemPromptSnapshot` (settings) | M1 schema | true | Keeps the system prompt byte-stable across requests (good for cache hits) | Leave default |

## Verified but token-neutral (brief candidates)

None of these change model tokens. Listed so the orchestrator doesn't re-research them. All M1 (registry and/or `Za`).

| Name | Default | Note |
|---|---|---|
| `BASH_MAX_OUTPUT_LENGTH` | 30000, cap 150000 (`wYn`) | "On its own only sizes the read-back window". Superseded by `bashOutputMaxChars` (#1). Not the inline cap. |
| `TASK_MAX_OUTPUT_LENGTH` | 32000 | Same relationship to `taskOutputMaxChars` (#5) |
| `BASH_DEFAULT_TIMEOUT_MS` / `BASH_MAX_TIMEOUT_MS` | 120000 / 600000 ms (`fe`, `ge`) | Wall-clock only. Raising the default can let a hung command block a worker longer. |
| `MCP_TIMEOUT` / `MCP_CONNECT_TIMEOUT_MS` / `MCP_TOOL_TIMEOUT` | 30000 / 5000 / per-transport default | Startup and tool timeouts; no token effect |
| `DISABLE_ERROR_REPORTING` | off | Sentry only |
| `DISABLE_COST_WARNINGS`, `DISABLE_BUG_COMMAND`, `DISABLE_FEEDBACK_COMMAND`, `CLAUDE_CODE_DISABLE_FEEDBACK_SURVEY`, `DISABLE_INSTALLATION_CHECKS` | off | UI only. Optional quiet-pane hygiene. |
| `CLAUDE_CODE_DISABLE_TERMINAL_TITLE` | off | Terminal escape sequences only. Could matter for herdr pane titles; check before setting. |
| `CLAUDE_CODE_IDE_SKIP_AUTO_INSTALL`, `USE_BUILTIN_RIPGREP` | off / builtin | Startup only |
| `CLAUDE_CODE_SKIP_*` (`_BEDROCK_AUTH`, `_VERTEX_AUTH`, `_FOUNDRY_AUTH`, `_MANTLE_AUTH`, `_PLUGIN_MCP_SERVERS`, `_PROMPT_HISTORY`, …) | off | Provider auth / plugin MCP; not relevant to first-party token use |
| `CLAUDE_CODE_MAX_TOOL_USE_CONCURRENCY` / `CLAUDE_CODE_MAX_CONCURRENT_SUBAGENTS` | 10 / 20 | Parallelism only |
| `cleanupPeriodDays` | 30 | Transcript retention. Keep ≥ the benchmark window; transcripts are the free measurement source. |
| `statusLine` | operator script | Local command; no API tokens |
| `includeCoAuthoredBy` | deprecated → `attribution` | Commit text only |
| `outputStyle` | default | Changes the system prompt. A terse custom style *might* cut output, but that is unmeasured. Unverified effect. |

---

## Unverified / folklore

- **`DISABLE_NON_ESSENTIAL_MODEL_CALLS`.** Absent from both installed binaries (0 matches in strings, registry and `Za`). The docs agent found it only in a GitHub issue title, so it may have existed in an older build. Treat as folklore for 2.1.27x. The real per-feature switches are `promptSuggestionEnabled`, `awaySummaryEnabled` and `CLAUDE_CODE_ENABLE_NARRATION`.
- **Live auto-compact value in a horch pane.** Code and docs agree on ≈967k, but I did not run `/autocompact` in a live pane. I skipped `claude -p` because it would fire the operator's herdr `SessionStart` hook from a stray session. The account latch `longContext1mCreditsBlocked` would drop the value to 200k. Sonnet 5 also has a per-surface table (`remote_cowork` / `local-agent` → 500k); I did not trace which surface a horch pane reports. One `/autocompact` in each tier's pane settles both.
- **`agent_summary` and `agent_classifier` side calls.** Both fork or call a model. They appear tied to the background-agent view and thread recap, not the plain REPL loop, but I did not trace their gates.
- **`--effort` vs `modelSettings.<model>.effortLevel`.** The docs' resolution order puts an explicit `--effort` (step 1) ahead of saved per-model levels, and the binary confirms `CLAUDE_CODE_EFFORT_LEVEL` beats `--effort`. I did not observe this live.
- **`effort_cost_index` numbers** are the CLI's own shipped estimates (shown in its effort-change warning), not measurements of this fleet.
- **Prompt-suggestion cost** assumes herdr panes do not report terminal focus-out. If they do, unfocused panes already skip it (`unfocused` / `bg_unattached`).
- **Env boolean parsing.** Whether `CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION=0` parses like `false` is not checked; use the literal `false`.
- **Telemetry-off side effect.** The GrowthBook-off claim is from code (`ZD(){return!a.DISABLE_GROWTHBOOK&&IN()}`, `IN(){return!Lh()}`, `Lh()` includes `vL()`). The docs partly corroborate it: `DISABLE_TELEMETRY` "also disables Remote Control's feature-flag fetch". I did not run a session with it set to diff the behaviour.
- **Token-size estimates** use ≈3–4 chars/token and are not measured.
- **Docs gaps.** The docs agent's WebFetch truncated the large `env-vars` and `settings-reference` pages, and once fabricated entries past the cut. It discarded those, and I use only rows it confirmed on fully read pages. Binary evidence (M1) is primary throughout.

## SKIP list

| Name | Reason |
|---|---|
| `DISABLE_PROMPT_CACHING`, `DISABLE_PROMPT_CACHING_{OPUS,SONNET,FABLE,HAIKU}` | Policy: disables caching. Every turn would re-bill the full prefix at full input price (≈10× the cache-read price). |
| `FORCE_PROMPT_CACHING_5M` | Drops the main thread from 1h to 5m cache, so idle panes (waiting on the orchestrator > 5 min) re-write their whole prefix |
| `CLAUDE_CODE_DISABLE_THINKING`, `MAX_THINKING_TOKENS=0`, `alwaysThinkingEnabled:false`, `CLAUDE_CODE_DISABLE_ADAPTIVE_THINKING`, `DISABLE_INTERLEAVED_THINKING` | Policy: disables or reshapes thinking (quality). Fable 5.1 declares `rejects_disabled_thinking`, and the docs say `CLAUDE_CODE_DISABLE_ADAPTIVE_THINKING` "doesn't apply" to Fable, Sonnet 5 or Opus 4.7+. |
| `DISABLE_AUTO_COMPACT`, `DISABLE_COMPACT`, `autoCompactEnabled:false` | Context grows until the hard block at window − 3000 (`ctt()`). `DISABLE_COMPACT` also kills manual `/compact`. |
| `CLAUDE_CODE_DISABLE_1M_CONTEXT`, `CLAUDE_CODE_MAX_CONTEXT_TOKENS` | `DISABLE_1M_CONTEXT` is a blunt, fixed 200K version of #4. The docs say native-1M models then "compact at the 200K boundary". It also switches off 1M everywhere, and the binary warns that for some models "the 200K limit isn't enforced… set CLAUDE_CODE_AUTO_COMPACT_WINDOW". `MAX_CONTEXT_TOKENS` is honoured only with `DISABLE_COMPACT`. Use #4. |
| `DISABLE_TELEMETRY`, `DO_NOT_TRACK`, `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC`, `DISABLE_GROWTHBOOK` | No token saving. They switch off server feature flags (see Headline 5), so fleet panes would diverge from the operator's tuned sessions. Use `DISABLE_AUTOUPDATER` (#6) for version pinning instead. |
| `--bare` / `CLAUDE_CODE_SIMPLE` | Skips hooks (the herdr `SessionStart` state hook), CLAUDE.md and auto-memory. Removes the operator's tuned context wholesale. |
| `disableAllHooks` | Kills `herdr-agent-state.sh` and the status line; breaks fleet state tracking |
| `ENABLE_TOOL_SEARCH=false` | Would load every deferred tool schema into each request |
| Lowering `skillListingBudgetFraction` now | No measured cost at 0.5 (see above). Saving comes only from truncating skill descriptions. |
| `CLAUDE_CODE_MAX_OUTPUT_TOKENS` lowering | No saving; causes truncation and retries |

---

## Orchestrator vs worker

- **Where settings land.** The orchestrator (Fable 5.1, `xhigh`, long-lived, context-precious) is launched by `horch fleet` from `teammates/orchestrator.md`. It goes through the same launch builder as workers (`launch.rs` header comment; orchestrator launch test at `launch.rs:752`), so it has its own `env:` map and `--settings` overlay. It also inherits the operator's shell and `~/.claude/settings.json`, and runs with `inherit_plugins: false` and `mcp_servers: {}`. Every recommendation above can therefore be scoped per pane, orchestrator included, through teammate frontmatter. Settings keys (#1, #5) still need horch's auto-built overlay to learn to carry them, or a teammate `settings:` file.
- **What matters most for the orchestrator.** In order:
  1. Small inline caps: `bashOutputMaxChars` / `taskOutputMaxChars` ≈ 8000, plus `CLAUDE_CODE_FILE_READ_MAX_OUTPUT_TOKENS` ≈ 10000 so the spill file isn't simply re-read. Every oversized result stays in its context for the rest of the fleet run.
  2. The auto-compact window (#4) plus `# Compact instructions` (#8). The ≈967k default means it can re-read ~1M tokens per turn before compacting. That is the largest single cost, and the benchmark thread should size it.
  3. Its own effort (`xhigh`, 1.38× high).
  4. Prompt suggestions are optional here, since a human may read them.
- **What matters most for workers.** Workers are short-lived and usually finish far below any compaction threshold, so the compaction window rarely matters for them. Their levers are effort per role (#3: `sonnet` / `qa-engineer` xhigh → high is the single biggest cut, then the Opus implementers), prompt suggestions off (#2), and a moderate Bash cap (≈16000).
- **All panes.** `DISABLE_AUTOUPDATER=1` keeps all panes of one fleet on one binary.
- **Put orchestrator-only values in `orchestrator.md`'s `env:`, not the shell.** Workers are also spawned from the operator's shell, and `apply_env` only layers the teammate `env:` map on top. Anything placed in the shell profile or `~/.claude/settings.json` `env` block (e.g. `CLAUDE_CODE_FILE_READ_MAX_OUTPUT_TOKENS=10000`) reaches every worker and every non-fleet session too.

## Docs cross-check (step 3)

Done by a delegated docs fetch against code.claude.com, the pages the old docs.claude.com URLs redirect to. It confirmed items only from fully read pages.

- **Agree with the binary (M2 added above):**
  - `BASH_MAX_OUTPUT_LENGTH` is read-back only and ignored when `bashOutputMaxChars` is set (tools-reference).
  - `bashOutputMaxChars` / `taskOutputMaxChars` were added in v2.1.261.
  - The ≈967K default threshold (model-config).
  - `CLAUDE_CODE_AUTO_COMPACT_WINDOW` takes precedence over the command, flag and setting.
  - `MAX_MCP_OUTPUT_TOKENS` default 25,000 (mcp).
  - `BASH_DEFAULT_TIMEOUT_MS` 120000 / `BASH_MAX_TIMEOUT_MS` 600000 (env-vars).
  - `CLAUDE_CODE_MAX_CONCURRENT_SUBAGENTS` default 20 (sub-agents).
  - `maxEffortLevel` added in v2.1.267.
  - Adaptive models ignore nonzero `MAX_THINKING_TOKENS` (costs).
  - `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1` / `autoMemoryEnabled` (memory).
  - `ENABLE_TOOL_SEARCH=false` loads everything (mcp).
  - `cleanupPeriodDays` default 30.
- **Docs add:**
  - The `# Compact instructions` CLAUDE.md block (costs).
  - "Agent teams use approximately 7x more tokens than standard sessions when teammates run in plan mode" (costs). This describes Claude Code's built-in agent-teams feature (`CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS`), not horch panes. Context only.
  - `MCP_TOOL_TIMEOUT` unset means "the 28-hour default".
  - The MCP startup timer is "the greatest of three values: 60 seconds, the tool timeout…, and `MCP_TIMEOUT`". The binary's `Kc()` default is 30000; token-neutral either way.
- **Docs silent or not fetched, binary-only (M1):**
  - `CLAUDE_CODE_FILE_READ_MAX_OUTPUT_TOKENS`, `CLAUDE_CODE_MAX_OUTPUT_TOKENS`
  - `DISABLE_AUTOUPDATER`, `DISABLE_COST_WARNINGS`, `DISABLE_INTERLEAVED_THINKING`, `DISABLE_PROMPT_CACHING*`
  - `CLAUDE_CODE_DISABLE_TERMINAL_TITLE`, `CLAUDE_CODE_IDE_SKIP_AUTO_INSTALL`, `USE_BUILTIN_RIPGREP`
  - `promptSuggestionEnabled`, `CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION`
  - the `effort_cost_index` numbers.
- **Docs vs binary:** `DISABLE_BUG_COMMAND`. The docs route `/bug` through `DISABLE_FEEDBACK_COMMAND`, but the binary still lists `DISABLE_BUG_COMMAND` in `Za`. Token-neutral; no action.
- **Changelog 2.1.x lines relevant here:**
  - v2.1.251: "`CLAUDE_CODE_SUBAGENT_MODEL` now sets default rather than overriding everything"; "`/effort` now saves per-model default settings".
  - v2.1.261: output-cap settings.
  - v2.1.267: `maxEffortLevel`.
  - No renames or removals found for the candidates. Changelog coverage for 2.1.200–2.1.260 was incomplete.
