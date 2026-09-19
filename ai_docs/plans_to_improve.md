# Plans to improve fleet efficiency: env vars and settings per harness

Date: 2026-09-18. Author: orchestrator (Fable 5.1), synthesizing four researcher reports:

- `ai_docs/reports/env-research/claude-code.md` (Claude Code 2.1.274/2.1.276)
- `ai_docs/reports/env-research/codex-opencode.md` (Codex CLI 0.154.0, OpenCode 1.18.2)
- `ai_docs/reports/env-research/pi-ollama-prime.md` (pi 0.85.1, Ollama 0.32.15, Prime Agent 0.9.4)
- `ai_docs/reports/env-research/compaction-benchmarks.md` (MRCR v2, GraphWalks, RULER evidence per model; harness compaction defaults)

Every item below was verified against the installed binary, the official docs for that version, or `--help` by the researcher. Items they could not verify are listed in the reports' "unverified" sections and are not recommended here. Items marked "applied" in section 1 have shipped on the `fleet-efficiency-plan` branch; everything else is the plan.

Ground rule (from the operator's standing guidance): measure, then cut narrowly. Never drop the operator's tuned settings wholesale. Every change lands per teammate, in `teammates/*.md`, never in the operator's shell profile, `~/.claude/settings.json`, `~/.codex/config.toml`, or the global OpenCode config.

## 1. Summary of decisions

1. **Compact earlier, per tier, with a compaction-instructions block.** The premise holds: every harness compacts at or beyond the point where measured accuracy has already fallen. Claude Code compacts native-1M models at about 967k tokens; third-party MRCR v2 shows Opus 5 dropping below 90% of its short-context score in the 128K–256K bin and Sonnet 5 in the 32K–64K bin. Anthropic's own agentic evals of these models compact at 200k. Thresholds are in section 2.
2. **Effort is the biggest per-token lever and it is set too high for the cheap tiers.** Every Claude teammate runs `xhigh`. By the CLI's own cost index, Sonnet 5 at `xhigh` costs 2.41 times `high`. Drop `sonnet` and `qa-engineer` to `high`, and the Opus implementers to `high`. Keep `xhigh` where judgment is the product.
3. **Fix six correctness bugs before tuning anything.** They cost tokens or accuracy today regardless of thresholds (section 4). Two are blockers: pi cannot start, and OpenCode silently ignores the effort horch passes.
4. **Cap tool output inline.** Today's JS-bundle dump into the orchestrator's context was a settings gap, not an env var gap: the Bash inline cap is `bashOutputMaxChars`, which needs horch's settings overlay to learn one new key.
5. **Turn off background model calls nobody reads in a pane.** Section 3.6 inventories 39 of them across the five harnesses with a verified switch or a verified "none exists" for each. The ones on today: Claude Code's prompt-suggestion fork (full context, pane's own model, every turn), Claude's session title (sends the horch briefing to Haiku with no caching), Claude's subagent progress summaries (forks each background subagent every 30 seconds), Codex's auto recap, OpenCode's title call on your paid Bedrock model, and Prime's auto-refine, which sends up to 80k characters to Opus 5 and can rewrite the global Prime harness dir from inside a worker session. Three cannot be turned off: the Codex thread title, Claude's agent classifier while the agents view is open, and Prime's daemon recap, which is dormant without a Prime credential.
6. **Strip inherited tool surface from the non-Claude harnesses.** Measured on Codex: four flags cut input tokens per request by about 35%. On OpenCode free workers, the operator's Playwright MCP server adds about 5k tokens of schema to every request and is sent to endpoints that train on input.
7. **Agent-to-agent messages use Simplified Technical English.** Section 3.7. Applied to both base prompts on 2026-09-18.
8. **claude.ai-synced skills are off in every fleet Claude pane.** Section 3.1 row 11. Applied in horch on 2026-09-19 via `syncClaudeAiSkills: false` in the settings overlay; a live check dropped the orchestrator's skill list from 21 entries to 10.

## 2. Compaction thresholds, with the evidence

### 2.1 What the evidence says

Third-party MRCR v2 8-needle (Context Arena, max effort) and vendor numbers disagree on level by 20 to 30 points at long lengths but agree on shape: flat, then a decline beginning between 64K and 256K. MRCR and GraphWalks are the hardest retrieval tests; RULER and agentic-coding evals look gentler on the same models. No source measures how MRCR-style decline maps onto coding-agent quality, so these thresholds are a judgment, not a derivation.

| model (tier) | window | first drop below 90% of short-context score | harness default trigger | source quality |
|---|---|---|---|---|
| Opus 5 (opus tiers, specialists, Prime) | 1M | 128K–256K bin | Claude Code 967k; Prime 983k | third party, max effort only |
| Sonnet 5 (sonnet, qa-engineer) | 1M | 32K–64K bin | 967k | third party, max effort only |
| Fable 5.1 (orchestrator) | 1M | no measured curve. Proxy: Mythos 5 GraphWalks BFS 91% at 256k, 79% at 1M | 967k | unknown, proxy only |
| gpt-5.6-sol (codex-sol) | 272k used | 128K–256K bin | 244,800 | third party full curve; vendor only above 256K |
| gpt-5.6-terra (codex-terra) | 272k used | 64K–128K bin; below 70% by 128K–256K | 244,800 | same |
| nemotron-3-ultra (opencode-ultra) | 1M | RULER base checkpoint 256K–512K; served checkpoint single point 94.7 at 1M | 968,000 | first party RULER, no MRCR |
| nemotron-3.5-lightning (opencode-lightning) | 262k | nothing measured below 256K | 230,144 | sparse |
| big-pickle (opencode-pickle) | 200k (160k input) | unknown, model undisclosed | 140,000 | unknown |
| Qwen3.8-27B Q4 (pi) | 262k | 64K–128K bin (full precision) | 245,760 | third party, up to 128K only |

Two other facts shape the decision:

- Higher effort does not buy long-context accuracy. At the 64K–128K bin Qwen3.8 scores lower at xhigh than at low; the same non-monotonic pattern shows for GPT-5.5 and Opus 4.6. So `xhigh` is not a substitute for compacting earlier.
- Anthropic's API-side compaction defaults to 150k and its own BrowseComp runs compact at 200k, while Claude Code's `auto` picks 967k on cost and continuity grounds. Both are Anthropic's numbers; the fleet's goal is accuracy, so it follows the eval practice.

### 2.2 What a compaction costs

One summarization request over the whole context (mostly cache reads at about a tenth of input price if the cache is warm), a lossy summary, and re-injection of at most five recently read files under 5,000 tokens each. Tool results, exact error text, and conversational state that is not on disk survive only as far as the summary captures them. Claude Code also precomputes the summary at about 80% of the window, so lowering the window moves that background call earlier too.

For a **worker** the better answer is usually to not need compaction at all: the fleet already prefers fresh sessions with a written briefing, and `horch done` summaries plus the ledger are the durable notes. The threshold is a safety net for a worker that runs long, not the plan.

For the **orchestrator** compaction is the dangerous event: its "who owns what" map is conversational unless written down. Its threshold is set higher, and its compaction-instructions block must name the map.

### 2.3 Thresholds to apply

| tier | knob | value | resulting trigger | reasoning |
|---|---|---|---|---|
| all Claude workers on Opus 5 | `CLAUDE_CODE_AUTO_COMPACT_WINDOW` in teammate `env:` | `200000` | about 167k | Matches Anthropic's own 200k eval practice; sits below the 128K–256K drop bin's upper edge while leaving room for a briefing, phase skills, and a real task |
| `sonnet`, `qa-engineer` (Sonnet 5) | same | `150000` | about 117k | Sonnet 5 degrades earliest. The 100k floor would trigger at 67k and thrash on ordinary tasks; 150k is the compromise. These tiers do short grunt work and should rarely reach it |
| orchestrator (Fable 5.1) | same, in `teammates/orchestrator.md` `env:` | `300000` | about 267k | No curve exists for Fable; the Mythos proxy holds 91% at 256k. A fleet run's orchestrator context is precious and its compaction is lossy, so it gets the most room that the proxy supports |
| codex-sol | `-c model_auto_compact_token_limit=200000` in teammate `args:` | 200000 | 200k | Default 244,800 is inside Sol's 90% drop bin |
| codex-terra | same, `150000` | 150k | Default is inside Terra's 70% bin; Terra does grunt work and should finish well below this |
| opencode-ultra | `{"compaction":{"prune":true}}` in `OPENCODE_CONFIG_CONTENT` | prunes tool output older than the last 40k tokens | Evidence for Ultra below 512K is weak and the served checkpoint scores well at 1M; pruning old tool output is the lower-risk lever. Do not lower `limit.context` yet |
| opencode-lightning, opencode-pickle | leave the trigger; consider `prune` after the Ultra trial | as shipped | No evidence to move them; `compaction.reserved` is ignored for lightning anyway |
| pi (Qwen3.8) | `contextWindow: 131072` on the qwen3.8 entry in a fleet-owned pi models.json | about 115k | Measured drop at 64K–128K; the local model also writes its own summary, so keep it inside the range it handles well. Must stay at or below Ollama's `num_ctx` (262144 today) |
| prime (Opus 5 via API) | `modelOverrides.claude-opus-5.contextWindow: 200000` in a fleet-owned prime models.json | about 184k | Same reasoning as Opus workers. Also guards against the unverified question of whether the 1M window is reachable on this API path; 200k is safe either way |

On precision: the Opus worker trigger (about 167k) and the Terra trigger (150k) both sit inside the bin where the drop is first seen, and the bins are 2x wide, so the evidence cannot say whether a given point inside the bin is before or after the drop. For Opus the tiebreaker is Anthropic's own 200k eval practice. For Terra, 150k is a compromise between its 64K–128K 90% point and not thrashing on ordinary tasks. Treat both as starting values to be measured, not derived optima.

Why the window variable rather than `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE`: the percentage variable also exists and is verified. It takes a percentage of the effective window, can only lower the trigger, and is named `testPctOverride` inside the binary, which suggests a test hook rather than a supported surface. The window variable is documented, takes an absolute token count that maps directly onto the benchmark bins, and is what the `/autocompact` command and settings key use. One thing the percentage variable has that the window variable is not documented to have: the docs say it applies to subagents as well as the main conversation. For `staff-engineer`, which delegates through the Agent tool, add `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE=20` alongside the window variable as a trial and check a subagent transcript to see which one governed.

Companion change for every Claude tier: a `# Compact instructions` block telling the summary to preserve role, assigned brief path, files touched, decisions made, open questions sent via `horch tell`, and the report path. The orchestrator's version adds the roster and the ownership map. This is the quality-side half of compacting earlier. Where it lives is an open design question: the docs describe the heading as a project `CLAUDE.md` feature, but this repo has no CLAUDE.md and a fleet runs in arbitrary target projects, so a block here would only help fleets working on multi-herdr. The instructions must travel with the worker. Two candidate mechanisms: put the block in the teammate base prompt in `teammates/_base/` (the summarization request does include the system prompt, so this plausibly works, but it is not verified that the heading is honored outside CLAUDE.md), or have horch write a CLAUDE.md fragment into the target cwd at spawn (verified mechanism, but it touches the target repo). Resolve with one test before step 5 of the rollout.

Validation before flipping defaults: one `/autocompact` in a live pane of each Claude tier to confirm the window took effect (the researcher ran it in `-p` mode only), and a check of the session transcripts under `~/.claude/projects` for how often fleet workers actually reached the new trigger.

## 3. Top 10 per harness

Column "where" names the landing place. "env:" and "args:" are the per-teammate maps in `teammates/*.md`. "overlay" is horch's auto-built `--settings` JSON, which today carries only plugin disabling and the status line and needs one small Rust change to accept extra keys.

### 3.1 Claude Code (opus, sonnet, all eight specialists, and the orchestrator)

| # | setting | value | where | effect | quality risk | verdict |
|---|---|---|---|---|---|---|
| 1 | `effort:` per teammate | `sonnet`, `qa-engineer`: `high`. `opus`, `backend-developer`, `frontend-developer`, `product-lead`, `researcher`, `designer`: `high`. Keep `xhigh`: `staff-engineer`, `architect-reviewer`, `orchestrator` | teammate frontmatter (already wired to `--effort`) | Large, output side. CLI cost index relative to `high`: Sonnet 5 xhigh 2.41, Opus 5 xhigh 1.6, Fable 5.1 xhigh 1.38 | Real on hard reasoning, which is why it is per role. Hard-reasoning roles keep xhigh | TUNE CAREFULLY, apply the role split above |
| 2 | `CLAUDE_CODE_AUTO_COMPACT_WINDOW` | per section 2.3 | env: | Large, input side, on long sessions only | Lossy summary; mitigated by the compaction-instructions block | TUNE CAREFULLY, apply with the block |
| 3 | `bashOutputMaxChars` | workers `16000`, orchestrator `8000` (default 30000) | overlay (needs horch change) | Medium to large, tool-output side. Overflow spills to a file with a preview and path, nothing lost | Occasional extra `Read` of the spill file | RECOMMEND |
| 4 | `CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION=false` | `false` | env: (every Claude teammate, orchestrator optional) | Small to medium per turn: removes a full-context fork on the pane's own model after every turn. The fork already skips when the terminal reports focus-out; whether herdr panes report that is unverified, so the saving may be smaller than it looks | None; no one reads suggestions in a pane | RECOMMEND, ships today with no code change |
| 5 | `taskOutputMaxChars` | workers `16000`, orchestrator `8000` | overlay | Medium, tool-output side for background tasks | Tail kept, full file path given | RECOMMEND |
| 6 | `CLAUDE_CODE_FILE_READ_MAX_OUTPUT_TOKENS` | orchestrator `10000` (default 25000); workers default | env: in `orchestrator.md` | Medium, orchestrator only. Without it the Bash spill file is simply re-read in full | Extra paginated reads | TUNE CAREFULLY, orchestrator only |
| 7 | `DISABLE_AUTOUPDATER=1` | `1` | env: (all Claude teammates and orchestrator) | None directly. The binary moved from 2.1.274 to 2.1.276 during this research; panes in one fleet should run one build | None. Update deliberately between fleets | RECOMMEND |
| 8 | `# Compact instructions` block | see section 2.3 | project `CLAUDE.md` | None directly; makes #2 safe | Helps | RECOMMEND with #2 |
| 9 | `MAX_MCP_OUTPUT_TOKENS` | `10000` only on teammates that declare an MCP server (frontend-developer with a browser) | env: on those teammates | Medium on chatty MCP tools; irrelevant elsewhere since the orchestrator runs no MCP and workers use strict config | Truncated tool output past the cap, saved to a file | TUNE CAREFULLY, per MCP-using teammate |
| 10 | Telemetry as a measuring instrument: `CLAUDE_CODE_ENABLE_TELEMETRY=1` with `OTEL_METRICS_EXPORTER=otlp` to a local collector | measurement phase only | env: | None; it is how before/after gets measured. Cheaper alternative: the per-message `usage` fields already in the session JSONL | Keep `OTEL_LOG_*` content vars unset | RECOMMEND for measurement only |
| 11 | `syncClaudeAiSkills` | `false`; a teammate opts back in with `inherit_claudeai_skills: true` | overlay, in every Claude pane whatever `inherit_plugins` says (horch emits it in both the plain and the skill-bundle overlay) | About 2,300 tokens of listing per Claude pane: the 11 `anthropic-skills:*` entries synced from claude.ai. Given through `--settings`, it hides them for that launch only and moves nothing on disk (ai_docs/reports/claudeai-synced-skills.md) | None; no synced skill serves a fleet worker or the orchestrator | RECOMMEND, applied |

Also worth knowing: `--exclude-dynamic-system-prompt-sections` improves cross-session cache reuse but needs a Rust change and a measurement; `skillListingBudgetFraction` at the operator's 0.5 is a no-op for phase-only workers, so leave it.

### 3.2 Codex CLI (codex-sol, codex-terra)

A codex pane's private `CODEX_HOME` symlinks the operator's real `config.toml`, so nothing may be written there. Everything lands in teammate `args:` as `-c key=value` or `--disable <feature>`.

| # | setting | value | where | effect (measured on terra, input tokens per request, baseline 16,122) | quality risk | verdict |
|---|---|---|---|---|---|---|
| 1 | `--disable plugins` | | args: | about 2,664 fewer | None for coding; fleet phase skills verified intact | RECOMMEND |
| 2 | `--disable apps` | | args: | about 1,706 fewer alone, about 2,900 with #1 | None | RECOMMEND |
| 3 | `-c web_search="disabled"` | terra only | args: | about 2,456 fewer | Loses live doc lookup; terra gets facts from the brief. Sol keeps the default | TUNE CAREFULLY, RECOMMEND for terra |
| 4 | `effort:` made explicit | sol `low` is the catalog default, terra `medium`; both run `medium` today only because the operator's shared config says so. Set `medium` on both explicitly | teammate frontmatter | Stops drift with the operator's personal default | None at `medium`. Never `minimal` (hard API error) or `none` | TUNE CAREFULLY, make explicit |
| 5 | `-c mcp_servers.node_repl.enabled=false -c mcp_servers.blender.enabled=false` | | args: (operator-specific names) | about 780 fewer, and two fewer MCP processes per pane | None for coding | RECOMMEND |
| 6 | `--disable image_generation` | | args: | about 435 fewer | None for coding | RECOMMEND |
| 7 | `-c model_auto_compact_token_limit=N` | per section 2.3 | args: | Long sessions only | Lossy summary | TUNE CAREFULLY |
| 8 | `-c tool_output_token_limit=6000` | trial on terra (default 10000) | args: | Tool-output side; truncation is visible, not silent | May lose the useful part of a long log | TUNE CAREFULLY |
| 9 | `-c history.persistence="none"` | | args: | None; keeps fleet briefings out of the operator's prompt history | None | RECOMMEND |
| 10 | `-c notify=[]` | | args: | None; stops the operator's per-turn notify command firing for every worker turn | None, but it is the operator's setting | TUNE CAREFULLY, ask |

All four main cuts together measured 10,422 input tokens per request against 16,122, about 35% less. The prefix is cached, so the saving is a smaller uncached write once and smaller cached reads on every later request, plus later compaction. The composite `args:` line for terra is in the report; it was not launch-tested as a whole.

### 3.3 OpenCode (opencode-lightning, opencode-pickle, opencode-ultra)

These models cost nothing, so "cheaper" here means less context pressure, fewer compactions, faster turns, and less of the operator's data sent to endpoints that train on input.

| # | setting | value | where | effect per request | quality risk | verdict |
|---|---|---|---|---|---|---|
| 1 | `OPENCODE_DISABLE_CLAUDE_CODE_SKILLS=1` | | env: | about 8.5k chars of skill listing removed (16 Claude-only or personal skills, including the operator's herdr skills) | Helps; also stops personal skill descriptions going to training endpoints. Use this, not the broad `OPENCODE_DISABLE_CLAUDE_CODE`, which also drops the project CLAUDE.md fallback | RECOMMEND |
| 2 | `{"mcp":{"playwright":{"enabled":false}}}` | | `OPENCODE_CONFIG_CONTENT` in env: (horch merges its skills paths into it) | about 21k chars of tool schema removed | None for non-UI work. Keep context7 (about 1.2k tokens, helps accuracy) | RECOMMEND |
| 3 | `effort:` on all three opencode files | remove, or have horch warn | teammate files, horch roster check | None today: the TUI has no `--variant` flag and drops it silently, and all three models declare no variants | The risk is the false belief that lightning is tuned fast and ultra deep | TUNE CAREFULLY, fix the lever (section 4) |
| 4 | `{"compaction":{"prune":true}}` | trial on ultra first | `OPENCODE_CONFIG_CONTENT` | Large on long sessions: clears tool output older than the most recent 40k tokens once 20k is prunable | Model may re-read files it lost | TUNE CAREFULLY |
| 5 | `{"small_model":"opencode/nemotron-3.5-lightning-free"}` | per worker, its own free model | `OPENCODE_CONFIG_CONTENT` | Removes one paid call per session: titles currently go to the operator's paid Bedrock model, carrying the horch briefing | None (titles only) | RECOMMEND |
| 6 | `{"permission":{"doom_loop":"deny"}}` | trial on free workers | `OPENCODE_CONFIG_CONTENT` | Caps runaway identical tool loops; horch's `--auto` currently approves the loop guard | A legitimate third identical poll gets denied | TUNE CAREFULLY |
| 7 | `OPENCODE_DISABLE_AUTOUPDATE=1` | | env: | None; version stability within a fleet | None | RECOMMEND |
| 8 | `OPENCODE_DISABLE_SHARE=1` | | env: | None; a project `opencode.json` in an OSS repo can override the operator's global `share: disabled`, and env beats config | None | RECOMMEND |
| 9 | `OPENCODE_DISABLE_EXTERNAL_SKILLS=1` | operator's call | env: | about 12.5k chars removed (includes #1); also drops the operator's 23 cross-harness `~/.agents/skills` | Loses possibly useful `dev-*` skills; the standing guidance says keep the operator's own skills unless asked | TUNE CAREFULLY, opt in |
| 10 | `{"tool_output":{"max_bytes":24000}}` | trial on lightning (default 51,200) | `OPENCODE_CONFIG_CONTENT` | Tool-output side; overflow goes to a file with a preview | Model works from a preview | TUNE CAREFULLY |

### 3.4 pi with Ollama (pi tier, Qwen3.8-27B Q4_K_M, local)

The model is Qwen3.8 27B, not Qwen3 8B. Ollama's `/v1` endpoint ignores per-request context length and keep-alive, so those are server-side only: `launchctl setenv` plus an Ollama.app restart, which a teammate `env:` map cannot reach.

| # | setting | value | where | effect | quality risk | verdict |
|---|---|---|---|---|---|---|
| 1 | `compat.maxTokensField: "max_tokens"` on the ollama provider | | fleet-owned pi `models.json` | Caps runaway generations and thinking loops at the existing 32k; today pi sends a field Ollama ignores, so output is unbounded (live-verified: 66 tokens produced under a cap of 5) | None | RECOMMEND (correctness) |
| 2 | `thinkingLevelMap: {"off":"none"}` on qwen3.8 | | fleet-owned pi `models.json` | Makes `--thinking off` real; today Qwen keeps thinking | None at current `high`; makes effort levels truthful | RECOMMEND (correctness) |
| 3 | `OLLAMA_CONTEXT_LENGTH=262144` | pin the value VRAM detection already picks | `launchctl setenv` + restart Ollama.app | None today; locks the invariant pi `contextWindow` ≤ Ollama `num_ctx`. If the server context ever fell below pi's window, pi would never compact and Ollama would drop context silently | Helps | RECOMMEND (guard) |
| 4 | `OLLAMA_KEEP_ALIVE=30m` | (default 5m) | same | No billed tokens. Avoids a full re-prefill of up to 262k tokens after any idle over 5 minutes, such as waiting on the orchestrator | Keeps about 33 GiB resident on a 128 GiB machine | RECOMMEND |
| 5 | `contextWindow: 131072` on qwen3.8 | per section 2.3 | fleet-owned pi `models.json` | Up to half the prompt tokens per turn late in long sessions; much shorter prefill | Summary written by the same local model; must stay ≤ `num_ctx` | TUNE CAREFULLY |
| 6 | `PI_OFFLINE=1` | | env: in `pi.md` | None; skips update checks and install telemetry, matching "nothing leaves the machine" | None | RECOMMEND |
| 7 | `OLLAMA_NUM_PARALLEL` | keep `1` unless several pi workers run at once; each extra slot adds about 16 GiB of KV cache at 262k | server env | Throughput only | Memory pressure | TUNE CAREFULLY |
| 8 | `PI_CODING_AGENT_DIR` | a fleet-owned config dir holding the models.json changes above, same pattern as the private `CODEX_HOME` | env: in `pi.md` | Where #1, #2, #5 land without editing the operator's global pi config | Drift from the operator's models.json | TUNE CAREFULLY |
| 9 | `--no-context-files` | per project | args: | Equal to the size of any AGENTS.md or CLAUDE.md found on the path (zero today) | Drops project conventions | TUNE CAREFULLY |
| 10 | `--no-approve` | | args: | None; stops pi's interactive project-trust prompt from stalling an unattended pane on repos with `.pi/` resources | None | TUNE CAREFULLY |

### 3.5 Prime Agent (prime tier, Opus 5 via the Anthropic API key, pay per token)

| # | setting | value | where | effect | quality risk | verdict |
|---|---|---|---|---|---|---|
| 1 | `modelOverrides.claude-opus-5.contextWindow` | `200000` | fleet-owned prime `models.json` | Large on long runs: compaction today fires at about 983k, effectively never | Lossy summary; the Python kernel state survives compaction | TUNE CAREFULLY |
| 2 | output discipline line in `teammates/prime.md` | "print lengths, heads, slices and summaries, never whole files or DataFrames; write big output to a file and read the slice you need" | teammate body | Each `ipython` call can return about 16k tokens per stream before truncation and there is no knob for it. This is Prime's largest sink | Helps | RECOMMEND |
| 3 | `RLM_MAX_DEPTH=1` | (default 2) | env: in `prime.md` | Large when the model fans out: each recursive child is a fresh Opus 5 context. The fleet orchestrator is already the top-level delegator | Slightly less capability on huge exploratory tasks. Never 0 | TUNE CAREFULLY |
| 4 | `--skill /opt/homebrew/lib/node_modules/prime-agent/skills/edit` | | args: in `prime.md` | Restores the exact-string edit skill that horch's `--no-skills` removes; without it the model rewrites whole files through raw Python | Helps | RECOMMEND (path is install-specific) |
| 5 | `PI_CACHE_RETENTION=long` | only if Prime workers idle over 5 minutes between calls | env: | Cache TTL 1h at 2x write cost versus 1.25x; break-even is about one long gap per hour | None | TUNE CAREFULLY, measure cache reads vs writes in the session JSONL first |
| 6 | `effort:` | keep `high` for the tier; `medium` for narrow, well-specified Prime tasks | teammate frontmatter | Output side, medium to large. On Opus 5 this maps to adaptive thinking plus an effort level; thinking budgets are ignored | Shallower multi-step reasoning at medium | TUNE CAREFULLY |
| 7 | `PI_OFFLINE=1` | | env: | None; skips network refreshes at start | None | RECOMMEND |
| 8 | `PRIME_AGENT_CODING_AGENT_DIR` plus `PRIME_AGENT_KERNEL_VENV=~/.prime/agent/kernel-venv` | fleet-owned config dir | env: | Where #1 lands without touching the operator's global config; the venv pointer avoids a second kernel bootstrap | Drift | TUNE CAREFULLY |
| 9 | `idleEvictionMinutes: "off"` | | global settings only, or the fleet dir from #8 | Avoids re-deriving lost kernel state after long waits | Helps | TUNE CAREFULLY (global key) |
| 10 | `PRIME_AGENT_TELEMETRY=0` | | env: | None; hygiene | None | TUNE CAREFULLY |

## 3.6 Background and side model calls: inventory and off switches

Requirement from the operator: every harness feature that makes a model call the worker did not ask for (recaps, prompt suggestions, session titles, away summaries, narration, memory extraction, thread classifiers) must be off in fleet panes. Nobody reads their output in a pane, and each one spends tokens on the pane's model or on an off-tier model. The follow-up researcher verified every row on 2026-09-18 against the installed versions: Claude Code 2.1.276 (also 2.1.274), Codex 0.154.0, OpenCode 1.18.2, pi 0.85.1 and Prime Agent 0.9.4. Evidence for each row (binary offsets, source lines, command output): `ai_docs/reports/env-research/side-calls.md`.

| harness | feature | what it does | current state | off switch | status |
|---|---|---|---|---|---|
| Claude Code | prompt suggestion | forks the full conversation on the pane's model after each turn to predict the next prompt | ON (server flag) | `CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION=false` in env: | verified, turn off |
| Claude Code | session title (`generate_session_title`) | on the first user prompt of a pane, sends that prompt (the horch briefing) to the small fast model (Haiku 4.5 unless `ANTHROPIC_SMALL_FAST_MODEL` is set), with no prompt caching, to name the session | ON in every interactive pane | `CLAUDE_CODE_DISABLE_TERMINAL_TITLE=1` in env:. The REPL title store is built with `disabled: CLAUDE_CODE_DISABLE_TERMINAL_TITLE`. Side effect: Claude Code also stops setting the terminal title | verified, turn off |
| Claude Code | `agent_summary` (progress line for a running subagent) | every 30 s while a background subagent runs, forks that subagent's full context on the subagent's model to write a 3-5 word status for the tasks panel. It skips when the subagent transcript has not changed | ON in every interactive pane. Agent-tool subagents run in the background by default, and the summary gate is `(coordinator mode or fork-subagent gate) and interactive`. The fork-subagent gate is on by default. `fork` subagents and `context: fork` skills always summarize, and the switch does not stop the forked-skill path. No horch phase skill and no operator plugin skill uses `context: fork` today | `CLAUDE_CODE_FORK_SUBAGENT=false` in env:. This is the only client switch. It also removes the `fork` subagent type and its system-prompt paragraph, and the Agent tool shows `run_in_background` again. Subagents still run in the background by default | verified, operator decides: the only client switch also removes the `fork` subagent type, which fleet sessions use today |
| Claude Code | `agent_classifier` and `agent_namer` (state and label for the agents view) | classifies the session state (working, needs reply, done, failed) and gives it a 2-4 word label, on the small fast model, after each turn and at most once a minute during a turn | OFF in a plain pane: the REPL surface uses a heuristic and makes no model call. It becomes a model call in `--bg` sessions, and in every session while Claude Code's agents view (FleetView) is open: the view touches `~/.claude/sessions/.fleetview-heartbeat`, and that marks all sessions "watched". The file does not exist today | no client switch exists in 2.1.276. The knobs are server-only (`tengu_classifier_disabled_surfaces`). Guard: do not keep the agents view open while the fleet runs | verified, off today; no switch |
| Claude Code | away summary (recap after returning) | forks the full conversation on the pane's model to recap what happened while the user was away | default ON in 2.1.276. OFF in fleet panes today, because every Claude teammate loads the operator's `awaySummaryEnabled: false` (no teammate sets `setting_sources`) | `CLAUDE_CODE_ENABLE_AWAY_SUMMARY=false` in env: as a guard. The CLI reads it before any settings file, so it holds even if a teammate restricts setting sources later | verified, keep off; add the env guard |
| Claude Code | narration | spoken or streamed narration of activity | OFF (`CLAUDE_CODE_ENABLE_NARRATION` unset, server interval 0) | leave unset; pin `CLAUDE_CODE_ENABLE_NARRATION=false` in env: as a guard | verified, keep off |
| Claude Code | background memory extraction (`extract_memories`) | per-turn fork that writes auto-memory | OFF (server gate `tengu_passport_quail` is false; the opt-in CCR path `CLAUDE_CODE_POST_TURN_MEMORY` is unset) | no extraction-only switch exists in 2.1.276. `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1` exists, but it equals `autoMemoryEnabled: false` and also removes memory reads (quality cost). Guard: none needed while the server gate is off | verified, keep off |
| Claude Code | `auto_dream` (memory consolidation) | forks the conversation to merge memory files across sessions | OFF (server flag `tengu_onyx_plover.enabled` is false) | none needed while the flag is off. A guard would be `autoDreamEnabled: false`, which is a settings key (overlay, Rust change) | verified, keep off |
| Claude Code | `memdir_relevance` (memory recall selector) | a side call for each user prompt that picks the relevant memory files | OFF (needs server flag `tengu_moth_copse` or env `CLAUDE_MEMORY_STORES`; both are off) | keep `CLAUDE_MEMORY_STORES` unset | verified, keep off |
| Claude Code | tool-use summaries (`tool_use_summary_generation`) | labels each finished batch of tool calls on the small fast model | OFF (opt-in: `CLAUDE_CODE_EMIT_TOOL_USE_SUMMARIES` is unset) | leave it unset | verified, keep off |
| Claude Code | `rolling_compact` and precomputed compaction | `rolling_compact` has no call site in 2.1.274 or 2.1.276; the name is only in the server's 1h-cache allowlist. Precompute prepares the compaction summary in the background before the trigger | precompute OFF (server flag `tengu_sepia_moth` is false) | none needed now. If the flag turns on: `precomputeCompactionEnabled: false`, a settings key (overlay, Rust change) | verified, off today; decide with section 2.3 |
| Claude Code | auto-mode permission classifier (`auto_mode`) | a classifier model decides tool calls that need permission, and reviews subagent hand-backs | ON for every teammate with `permission_mode: auto` (most Claude specialists) | a teammate `permission_mode:` other than `auto`. This is a safety control that the operator chose, not a recap | verified, operator decides |
| Claude Code | user-, tool- or config-triggered sources: `/btw` (`side_question`), `/rename` with no name, remote-session titles, `/insights`, `/feedback`, `/model` validation, MCP date parsing, `claude auto-mode` and `claude plugin eval`, artifact comments, WebFetch and WebSearch internals, prompt and agent hooks, `/compact` | run only when a user types the command, the worker calls the tool, or the operator configures the hook | idle (artifacts are off: `disableArtifact: true`) | none needed | verified |
| Codex | automatic thread title | on the first user message of a thread with no name, the TUI starts a hidden ephemeral thread that makes a title of at most 36 characters from the first 960 bytes of that message (the horch briefing). It runs on `gpt-5.6-luna` at low effort when signed in with ChatGPT (true here), else on the pane's model | ON (observed: today's horch worker threads have generated names in `~/.codex/session_index.jsonl`) | does not exist in 0.154.0: no config key, feature or flag controls it. The only gate is "thread has no name" (`tui/src/app/thread_routing.rs:1954-1987`) | verified, cannot turn off; one small luna call per new pane |
| Codex | automatic recap (`tui.auto_recap`) | when the pane loses focus, after at least 3 turns and 3 minutes idle, starts a hidden thread on the pane's own model to write a catch-up of at most 40 words. It repeats after every 2 more turns | ON by default, and reachable: the Codex TUI turns on focus reporting, and herdr forwards pane focus events | `-c tui.auto_recap=false` in args: (parse-checked with `codex debug prompt-input`). Manual `/recap` stays available | verified, turn off |
| Codex | `/rename` title suggestion and manual `/recap` | the same hidden-thread calls as the two rows above | only when a user types the command | none needed | verified |
| Codex | `memories` feature | background memory model calls | OFF (`memories stable false`) | keep off; pin `--disable memories` in args: | verified, keep off |
| Codex | guardian auto-review, and the Guardian v2 classifier | a reviewer model decides approval requests instead of the user; v2 adds a risk classifier on each tool call | OFF for all codex teammates: `permission_mode: auto` gives `-a never`, and the review runs only with `-a on-request`. But the operator's global config sets `approvals_reviewer = "auto_review"`, so a codex teammate moved to `plan` or `acceptEdits` would start it. The v2 feature `guardianv2` is off | guard: `-c approvals_reviewer="user"` in args: (parse-checked) | verified, off today; add the guard |
| Codex | compaction (remote v2) | one summary request when the window fills | ON by policy | tune `model_auto_compact_token_limit` per section 2.3; do not disable | verified; decide with section 2.3 |
| Codex | startup prewarm, and ambient suggestions | prewarm opens the websocket with `generate=false` (connection setup, no inference). Ambient suggestions are a desktop-app feature | prewarm ON; ambient suggestions do not exist in CLI 0.154.0 | none needed | verified, no action |
| Codex | `notify` command | not a model call; spawns the operator's notify script per turn | ON | `-c notify=[]` in args: | verified, operator decides |
| OpenCode | session title generation | on the first step of every top-level session (not subagent sessions), sends "Generate a title for this conversation:" plus the whole first user message (the horch briefing) to the `title` agent | ON. It runs on `small_model`, which is the operator's paid Bedrock `amazon-bedrock/qwen.qwen3-coder-30b-a3b-v1:0`, whatever the worker's own provider is | `{"agent":{"title":{"disable":true}}}` in the teammate's `OPENCODE_CONFIG_CONTENT` (env:). A disabled agent is deleted from the registry, and the title call returns when the agent is missing. Checked with `opencode debug agent title`: "Agent title not found". No `OPENCODE_DISABLE_TITLE` or `title: false` exists in 1.18.2 | verified, turn off (the `small_model` redirect in 3.3 #5 is then only a fallback) |
| OpenCode | other `small_model` uses | `getSmallModel` has two callers: the title call, and the name for a project copy (git worktree) in the TUI "Create copy" action | project-copy name: idle (user action only) | none needed | verified |
| OpenCode | `summary` agent | hidden native agent; the docs say it creates session summaries | no call site in 1.18.2 (`SessionSummary` only computes file diffs) | none needed. Optional guard: `"summary":{"disable":true}` in the same JSON | verified, no action |
| OpenCode | compaction agent | summarizes history on overflow or on an explicit compact | ON, on the session's main model unless `agent.compaction.model` is set | keep on by policy (section 2). Do NOT set `agent.compaction.disable`: the call site has no null check, so compaction would throw | verified, keep on |
| OpenCode | `general` and `explore` subagents, `opencode agent create` | subagents run only when the model calls the `task` tool (they use the caller's model, and skip title generation); `agent create` is a user command | idle until used | none needed | verified |
| OpenCode | share and sync | uploads sessions, not a model call | OFF (operator global), guarded by `OPENCODE_DISABLE_SHARE=1` | env: | verified, keep off |
| pi | branch summary | summarizes the abandoned branch when the session tree is navigated | OFF in fleet panes: it runs only when a human picks "Summarize" at `/tree`, or an RPC or extension client asks for it; fleet workers do not navigate the tree | none needed. Optional guard: `branchSummary.skipPrompt: true` in pi settings.json | verified, no action |
| pi | session title or naming | would name the session with a model call | does not exist in 0.85.1: names come only from `/name`, `--name`/`-n`, or the `setSessionName` API | none needed | verified, does not exist |
| pi | extension-initiated calls | extensions can call the model or replace summaries through hooks | OFF: horch passes `--no-extensions` because the pi teammate sets `inherit_plugins: false` | keep `inherit_plugins: false` | verified, keep off |
| pi | any other side call | | none found: the only model call sites in 0.85.1 are the main turn, compaction and branch summary | none needed | verified, does not exist |
| Prime | auto-refine (the harness reviews and edits itself) | every 25 assistant messages (20-minute cooldown) and after each compaction, sends up to 40k characters of conversation to the pane's own model (Opus 5, a different system prompt, so no cache reuse) and asks whether to refine. If yes, a second call with up to 80k characters plans edits to prompt notes, memory, skills and subagent specs, and can write them to the global harness dir, where they change later sessions | ON: `autoRefine.enabled` defaults to true, and the operator's settings do not set it. `--no-skills` does NOT stop it; it only hides the `refine` skill from the model | `"autoRefine": {"enabled": false}` in a Prime settings.json. No flag or env var exists. Landing: the fleet-owned agent dir from 3.5 #8 (`PRIME_AGENT_CODING_AGENT_DIR` in env: plus a settings.json there) | verified, turn off |
| Prime | daemon status recap (the recap line and verdict in the agents view) | every 25 s while a session works, and 2 s after a turn ends, sends the last 8 messages to Prime Intellect's `qwen/qwen3-30b-a3b-instruct-2507` | DORMANT: it runs only if a Prime credential exists (`PRIME_API_KEY`, `prime login`, or `/login`). None exists on this host | does not exist in 0.9.4: no setting, flag or env var. Guard: do not set `PRIME_API_KEY` for Prime teammates and do not run `prime login` on the fleet host | verified, dormant; no switch |
| Prime | session title or naming | would name the session with a model call | does not exist in 0.9.4: names come from `/name`, `/rename` or `prime-agent rename` | none needed | verified, does not exist |
| Prime | `refine`, `goal` and heartbeat skills, `/refine`, `/goal`, `--goal`, cron jobs | refinement, goal continuation or scheduled prompts | OFF: `--no-skills` (from `inherit_plugins: false`) drops the bundled skills, horch never passes `--goal`, the slash commands need a human, and no cron job or heartbeat exists | keep `inherit_plugins: false`; do not add `--goal` to args: | verified, keep off |
| Prime | autonomous mode and `/btw` | continuation prompts with no human; a side question from a human | OFF (autonomous mode needs `/autonomous` or a CLI setting); `/btw` is idle | none needed | verified |
| Prime | agent trace upload | uploads session files to Prime Intellect; not a model call | OFF: `agentTraces.enabled` defaults to false, and no Prime credential exists | none needed | verified, keep off |
| Prime | telemetry | not a model call | ON, pseudonymous | `PRIME_AGENT_TELEMETRY=0` | verified, operator decides |
| Ollama | none: the server makes no calls of its own | | | | verified |

Where the switches land: these need only teammate-file changes (`env:` or `args:`): Claude `CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION=false`, `CLAUDE_CODE_DISABLE_TERMINAL_TITLE=1`, `CLAUDE_CODE_FORK_SUBAGENT=false`, `CLAUDE_CODE_ENABLE_AWAY_SUMMARY=false`, `CLAUDE_CODE_ENABLE_NARRATION=false`; Codex `-c tui.auto_recap=false`, `--disable memories`, `-c approvals_reviewer="user"`, `-c notify=[]`; OpenCode `{"agent":{"title":{"disable":true}}}` inside the teammate's `OPENCODE_CONFIG_CONTENT` (horch passes that JSON through and adds only `skills.paths`, `skills.rs:279`). These need a horch Rust change: any Claude settings-overlay key (`precomputeCompactionEnabled`, `autoDreamEnabled`, or a `promptSuggestionEnabled` / `awaySummaryEnabled` pin), because `launch.rs:288-318` builds the overlay from a fixed key set, and a teammate `settings:` file replaces the overlay and drops the plugin off-switches. None of those keys is needed today, because each one has an env equivalent above or its server flag is off. Prime `autoRefine.enabled: false` needs a settings.json in a fleet-owned agent dir: a one-time file, or a Rust change if horch must manage it. No off switch exists for: the Codex automatic thread title, the Claude `agent_classifier` (a model call only while the agents view is open), and the Prime daemon recap (dormant without a Prime credential).

Follow-up brief: `ai_docs/plans/env-research/05-side-calls.md` (done 2026-09-18).

## 3.7 Agent-to-agent messages use Simplified Technical English

Requirement from the operator: every message an agent sends to another agent through `horch tell`, `horch assign`, `horch done`, and the DONE and BLOCKED lines follows Simplified Technical English (ASD-STE100 style). Reasons: these messages are typed into another model's terminal and re-read on every later turn, so short unambiguous sentences cost fewer tokens and misread less often, and cheaper tiers parse them more reliably.

Rules to put in the base prompts (`teammates/_base/fleet-worker.md` and `teammates/_base/fleet-orchestrator.md`, plus the codex base variants):

- One instruction or one fact per sentence. Sentences of at most 20 words. Procedural sentences at most 20 words, descriptive at most 25.
- Active voice, present tense. Name the actor: "researcher-1 wrote the report to path X", not "the report was written".
- Use the same word for the same thing every time. Never a synonym for variety.
- No idioms, no metaphors, no hedging phrases ("it seems", "sort of", "basically").
- Paths, commands, flags, and identifiers verbatim, one per sentence where possible.
- Lists for parallel items, one item per line, no nested lists.
- Report messages open with the role tag and one of: `ready`, `DONE:`, `BLOCKED:`, `NOTE:`, `QUESTION:`, then one sentence stating the outcome, then details.
- Numbers as digits. Units stated. No approximations where a count is known.
- Warnings before the action they refer to, not after.

Implementation task: `ai_docs/plans/env-research/06-ste-base-prompts.md`. Prompts are data, so this is a teammate-file change only, no Rust.

## 4. Bugs found that cost accuracy or tokens today

Fix these regardless of the tuning above. Ordered by severity.

1. **pi cannot start.** Its package requires node 22.19 or newer; the shell's nvm default is 22.9.0 and pi crashes with a `webidl.util.markAsUncloneable` TypeError before its first token. Homebrew's node 25.5.0 runs it. Operator's choice: `nvm install 22 && nvm alias default 22`, or set `HORCH_PI_BIN` to a wrapper that runs pi's cli.js with `/opt/homebrew/bin/node`. Whether `HORCH_PI_BIN` in `pi.md`'s `env:` map is read early enough is unverified; the shell env is the safe place. Prime needs only node 22.8 and is unaffected.
2. **OpenCode drops the effort horch passes.** horch launches the TUI, which has no `--variant` option; yargs swallows it silently, and all three free models declare no variants. The `effort:` values on the three opencode teammates are fiction. Fix: horch's roster check should reject or warn on `effort` for TUI-launched opencode teammates, and the three files should drop the field and their comments about speed and depth.
3. **OpenCode `--pure` does not drop MCP servers.** Every free worker starts the operator's Playwright and context7 servers and sends their schemas to training endpoints. Fix is 3.3 #2; consider making horch add it when `inherit_plugins: false`.
4. **pi's max-tokens field is ignored by Ollama, and `--thinking off` does not stop thinking.** Fixes are 3.4 #1 and #2.
5. **Codex per-worker config cannot go in the private `CODEX_HOME`.** It symlinks the operator's real `config.toml`. Every Codex recommendation above is therefore an `args:` entry. Related: neither codex teammate sets `effort:`, so both silently track the operator's personal default.
6. **Prime's `--no-skills` also removes its built-in `edit` skill.** Fix is 3.5 #4.
7. **Codex writes its `.system` skills into the horch skill bundle** because the private home points `skills` at the bundle. Roughly token-neutral, but codex writes into a fleet-owned directory, and 72 stale private homes sit under `~/.local/state/horch/codex-home/`. Worth a cleanup task.
8. **Prime auto-refine edits shared state from inside a worker.** Every 25 assistant messages and after each compaction, Prime asks Opus 5 whether to refine itself and can then write prompt notes, memory, skills, and subagent specs into the global Prime harness dir, changing every later session. `--no-skills` does not stop it. Fix: `autoRefine.enabled: false` in a fleet-owned Prime settings.json (3.5 #8). This is both a token sink and a reproducibility bug.
9. **Claude Code settings keys cannot be set per teammate today.** horch's overlay carries only `enabledPlugins` and `statusLine`, and a teammate that names its own `settings:` file replaces the overlay wholesale. Needed for 3.1 #3 and #5: a `settings_overlay:` map on the teammate that horch merges into its overlay. Partly addressed: the overlay now also accepts `skillOverrides`, fed by the teammate list field `disabled_skills` (each entry `"off"`, merged per key with the operator's own overrides), and carries `syncClaudeAiSkills: false` (3.1 #11). For a teammate with a phase or skills, the skill bundle merges horch's keys into the teammate's `settings:` file rather than being replaced by it. A general `settings_overlay:` map is still open.

## 5. Options skipped, and why

Grouped by the reason they were rejected. Full lists with evidence are in each report's skip section.

**Would sacrifice quality (policy):**
- Disabling prompt caching in any harness (`DISABLE_PROMPT_CACHING*`, `FORCE_PROMPT_CACHING_5M`). Every turn would re-bill the full prefix at full input price.
- Disabling or reshaping thinking: `CLAUDE_CODE_DISABLE_THINKING`, `MAX_THINKING_TOKENS=0`, `alwaysThinkingEnabled: false`, Codex `model_reasoning_effort="none"`, pi/Prime `--thinking off` as a default, Ollama `reasoning_effort: none`. Fable 5.1 rejects disabled thinking outright.
- Disabling auto-compaction anywhere. Context grows to the hard block, or on Ollama truncates silently.
- `CLAUDE_CODE_DISABLE_1M_CONTEXT`: a blunt, fixed 200k version of the window lever that also switches off 1M everywhere. The window env var does the same job per tier.
- Lowering Ollama `num_ctx` below pi's declared window, or lowering Codex `project_doc_max_bytes`: both truncate silently.
- `OLLAMA_KV_CACHE_TYPE=q8_0/q4_0`: quantized KV cache degrades high-GQA models like Qwen, and there is no memory pressure to justify it.
- Codex `effort: minimal` (hard API error on these models) and `ultra` for workers (client-side automatic delegation multiplies spend).
- Lowering `CLAUDE_CODE_MAX_OUTPUT_TOKENS` or OpenCode's output token max: saves nothing unless replies hit the cap, then truncates and retries.

**Would drop the operator's tuned settings wholesale:**
- Claude `--bare`, `disableAllHooks`, an empty `--setting-sources` (kills the herdr state hook and status line), Codex `--ignore-user-config`, Codex `shell_environment_policy.inherit = "core"` (strips the `HERDR_*` and `HORCH_*` variables that `horch tell` needs, so it breaks the fleet), OpenCode `OPENCODE_DISABLE_PROJECT_CONFIG` and the broad `OPENCODE_DISABLE_CLAUDE_CODE`.
- `OPENCODE_DISABLE_EXTERNAL_SKILLS` as a default (kept as an opt-in in 3.3 #9).

**Look like savings but are not:**
- `DISABLE_TELEMETRY`, `DO_NOT_TRACK`, `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC`: save no model tokens and switch off the server feature flags, so fleet panes would diverge from the operator's sessions. Version pinning is what people usually want from these; `DISABLE_AUTOUPDATER` does that.
- `DISABLE_NON_ESSENTIAL_MODEL_CALLS`: does not exist in 2.1.27x. The real per-feature switches are prompt suggestions (on), away summary (already off), narration (off), memory extraction (off).
- `BASH_MAX_OUTPUT_LENGTH`: only sizes the read-back window; the inline cap is the settings key.
- Codex `--disable multi_agent`, `browser_use`, `computer_use`, `goals`, `sleep_tool`: measured zero change.
- Codex `model_verbosity` and `model_reasoning_summary`: already at their lowest by catalog default.
- Lowering `skillListingBudgetFraction`: no measured cost at the current value for phase-only workers; the only saving would come from truncating skill descriptions, which risks wrong triggering.
- `OLLAMA_FLASH_ATTENTION=1`: already on via auto.
- `ENABLE_TOOL_SEARCH=false`: would load every deferred tool schema into each request.

**Not applicable or already handled by horch:** `CLAUDE_CODE_SUBAGENT_MODEL`, private `CODEX_HOME`, `OPENCODE_PURE`, the pi `--no-*` flags, sandbox and approval flags.

## 6. Rollout plan

Each step is one worker task with its own brief. Order matters: measure first, fix bugs second, then tune.

1. **Baseline measurement.** A qa-engineer or sonnet worker records, for one representative fleet run: per-tier input, output, cache-read and cache-write tokens from the Claude session JSONL under `~/.claude/projects`; Codex `codex exec --ephemeral --json` input tokens per request; `opencode debug skill --pure` and `opencode mcp list --pure` listing sizes; `horch skills` context estimates. Written to `ai_docs/reports/env-research/baseline.md`. Without this the after-numbers mean nothing.
2. **Bug fixes (section 4).** Operator decides the pi node fix. Rust changes: `settings_overlay:` on the teammate (item 8), an opencode `effort` roster check (item 2), and optionally MCP disabling under `inherit_plugins: false` for opencode (item 3). Teammate-file changes: codex explicit `effort`, prime `edit` skill, pi models.json in a fleet dir. Cleanup of stale codex homes.
3. **Ship the RECOMMEND rows that need no Rust.** Claude: prompt suggestions off, session title off, away-summary and narration env guards, autoupdater off. Codex: the four `--disable`/`-c` cuts, history off, `tui.auto_recap=false`, `--disable memories`, `approvals_reviewer="user"`. OpenCode: the composite env from 3.3 plus `agent.title.disable`. pi: `PI_OFFLINE`, the models.json corrections, the two Ollama server pins. Prime: `PI_OFFLINE`, output-discipline line, `RLM_MAX_DEPTH=1`, `autoRefine.enabled: false` in the fleet-owned settings.json. All switches are listed with landing places at the end of section 3.6.
4. **Effort split (3.1 #1).** One commit, then re-measure output tokens per tier on the same task mix.
5. **Compaction thresholds (section 2.3) with the compaction-instructions block.** Verify with `/autocompact` in a live pane per tier; watch the thrash guard message in transcripts.
6. **Trials, one at a time, each with a before/after:** OpenCode `prune` on ultra, `doom_loop` deny, Codex `tool_output_token_limit`, Prime `PI_CACHE_RETENTION`, pi `contextWindow` 131072, and `--exclude-dynamic-system-prompt-sections`.
7. **After-measurement** against the step 1 baseline, written next to it. Anything that saved less than it cost in quality (re-reads, retries, wrong answers on the fixed task mix) gets reverted.

## 7. Open questions for the operator

1. pi node fix: shell-wide nvm default, or a `HORCH_PI_BIN` wrapper? (section 4 item 1)
2. Keep the orchestrator at `xhigh`? Fable at `xhigh` costs 1.38 times `high` and it is the fleet's highest-stakes thinking. This plan keeps it.
3. Codex `notify`: fleet workers currently trigger your per-turn notification command. Silence them?
4. `OPENCODE_DISABLE_EXTERNAL_SKILLS`: drop your 23 cross-harness `~/.agents/skills` from free workers too, or keep them?
5. Ollama server pins require an Ollama.app restart and keep about 33 GiB resident for 30 minutes after the last pi call. Acceptable?
6. Claude subagent progress summaries: the only switch, `CLAUDE_CODE_FORK_SUBAGENT=false`, also removes the `fork` subagent type. Fleet sessions use fork today. Keep the summaries and the fork type, or drop both? (3.6)
7. Claude auto-mode permission classifier: it runs a model call per gated tool use for every teammate with `permission_mode: auto`. It is a safety control you chose; the plan leaves it on. Confirm.
8. Is Opus 5's 1M window reachable on Prime's API-key path? A context error in any long Prime session transcript settles it and decides whether Prime's window can go above 200k.
