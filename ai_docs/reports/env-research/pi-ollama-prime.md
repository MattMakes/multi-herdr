# Env / config research: pi + Ollama (qwen3.8) and Prime Agent (claude-opus-5)

Researcher: researcher-4, 2026-09-18. Brief: `ai_docs/plans/env-research/03-pi-ollama-prime.md`.
This is a report only: no settings, teammate files, models or Rust were changed.

## Header

| Item | Value |
|---|---|
| pi | **0.85.1** (`@earendil-works/pi-coding-agent`), bundle `/opt/homebrew/lib/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js`. **Does not start under the shell's default Node; see Blocker.** Version read with `/opt/homebrew/bin/node <cli.js> --version`. |
| Ollama | **0.32.15** (`ollama --version`), binary `/Applications/Ollama.app/Contents/Resources/ollama`. The server is run by Ollama.app, not by a shell. |
| Prime Agent | **0.9.4** (`prime-agent --version`), bundle `/opt/homebrew/lib/node_modules/prime-agent/dist/bundle/cli.js`. The binary is `prime-agent` (horch's `prime_bin()` default). There is no `prime` on PATH. |
| Hardware | Apple **M5 Max**, 40-core GPU, Metal 4, **128 GiB** unified memory (`sysctl hw.memsize`). Ollama sees `total_vram="107.5 GiB"` (server log). `iogpu.wired_limit_mb: 0` (the OS default). |
| Node on PATH | `~/.nvm/versions/node/v22.9.0/bin/node` (nvm default = 22.9.0) |

### `ollama show qwen3.8` summary
- arch `qwen35` (hybrid: 16 attention layers + 64 recurrent layers, per the load log), **27.3B** params, **Q4_K_M**, embedding 5120, requires Ollama 0.32.12.
- **Max context length: 262144.**
- Capabilities: completion, vision (CLIP projector 460.73M), tools, **thinking**.
- Modelfile params: `temperature 1`, `top_k 20`, `top_p 0.95`, `min_p 0`, `presence_penalty 0`, `repeat_penalty 1`, `draft_num_predict 4` (speculative draft).
- Size: 17 GB on disk. `ollama ps` shows 18 GB, `100% GPU`, **CONTEXT 262144** when loaded by a pi-shaped `/v1` request (live, today).
- Memory at 262144 (server log, same load): weights 15.3 GiB, **KV cache 16 GiB** (f16), draft KV 1 GiB, recurrent state 0.75 GiB. That totals about 33 GiB, well inside 107.5 GiB.
- `ollama ps` at research start: nothing loaded. I loaded the model for the live tests below and then ran `ollama stop qwen3.8`, so the end state matches the start.

### Enumeration counts
| Source | Command | Count | Notes |
|---|---|---|---|
| pi bundle (cli.js + chunks + index.js) | `grep -aoE '(PI_\|OLLAMA_\|OPENAI_\|ANTHROPIC_)[A-Z0-9_]+' \| sort -u` | **92** | **0 `OLLAMA_*`**. Some matches are JS constants, not env vars (`PI_BY_TWO`, `PI_CLASS`, `*_ENV`). The env vars pi actually reads are the documented ones in `docs/environment-variables.md` and `pi --help`. |
| Ollama binary | `strings \| grep -oE 'OLLAMA_[A-Z0-9_]+' \| sort -u` | **31 raw** | Garbled by Go string concatenation (e.g. `OLLAMA_EDITOROLLAMA_MODELSLLAMA_ARG_FIT`). The clean method-3 list is `ollama serve --help`: **21** (19 `OLLAMA_*` + `LLAMA_ARG_FIT`, `LLAMA_ARG_FIT_TARGET`). The server "server config" log line also shows `OLLAMA_DEBUG_LOG_REQUESTS`, `OLLAMA_EDITOR`, `OLLAMA_GO_TEMPLATE`, `OLLAMA_NOHISTORY`, `OLLAMA_REMOTES`. |
| Prime bundle (all `dist/bundle/*.js`) | `grep -aoE '(PRIME_\|PI_\|ANTHROPIC_)[A-Z0-9_]+' \| sort -u` | **104** | About 24 are `PRIME_AGENT_INTERNAL_*` or build internals (do not set). The 3 `ANTHROPIC_*_COST_MULTIPLIER` names are JS constants. `RLM_MAX_DEPTH` / `RLM_DEPTH` are real env reads but fall outside the regex (found separately). |

Verification methods: **M1** = present in the installed binary/bundle (code line quoted). **M2** = official docs (URL or shipped `docs/*.md` of the installed package). **M3** = `--help`. **LIVE** = a request I ran against the local Ollama today.

---

## BLOCKER: pi cannot start under the shell's default Node

**Repro (exact):**
```
$ pi --version            # in a normal zsh (nvm default = v22.9.0)
... ~150 KB of minified bundle source printed to stderr ...
TypeError: webidl.util.markAsUncloneable is not a function
Node.js v22.9.0
exit=1
```
The same happens for any pi launch, including the horch pane command. The stack trace points into `dist/bundle/chunks/chunk-JVUZSMYM.js` (bundled undici).

**Cause:** `package.json` has `"engines": { "node": ">=22.19.0" }`, but `#!/usr/bin/env node` resolves to nvm's default **v22.9.0**. The command below works, which proves the package itself is intact:
```
$ /opt/homebrew/bin/node /opt/homebrew/lib/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js --version
0.85.1
```

**Node versions on disk vs `>=22.19.0`:**

| Path | Version | Satisfies? |
|---|---|---|
| `/opt/homebrew/bin/node` (Cellar `node/25.5.0`) | v25.5.0 | **YES (the only one)** |
| `/opt/homebrew/opt/node@22`, `@23`, `@24`, `@25` | all symlink to `Cellar/node/25.5.0` → v25.5.0 | yes (same binary) |
| `/opt/homebrew/opt/node@16` | broken (missing `icu4c@75` dylib) | no |
| `/usr/local/bin/node` | v18.13.0 | no |
| nvm `v22.10.0` | v22.10.0 | **no** (newest nvm install) |
| nvm `v22.9.0` (default) | v22.9.0 | no |
| nvm `v20.17.0`, `v18.x` (5), `v16.19.0`, `v15.14.0` | — | no |

**Fix options (operator's choice; I applied neither):**
1. **Upgrade the nvm default:** `nvm install 22 && nvm alias default 22` installs the latest 22.x (≥22.19). Panes started after this pick it up. It affects every Node tool in the operator's shell.
2. **Point horch at a wrapper:** set `HORCH_PI_BIN` (read by `agent::pi_bin()`, `crates/horch-core/src/agent.rs:38`) to a script such as
   ```sh
   #!/bin/sh
   exec /opt/homebrew/bin/node /opt/homebrew/lib/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js "$@"
   ```
   Set `HORCH_PI_BIN` in the environment of the horch process that builds the command (operator shell / `horch fleet`). I did not verify whether setting it in `pi.md`'s `env:` map takes effect before `pi_bin()` is read.

Prime is unaffected: it requires `>=22.8.0` and runs under v22.9.0.

---

## Key findings (read these first)

1. **Silent-truncation trap: not active on this machine today, but fragile.** Ollama 0.32.15 picks the default `num_ctx` from VRAM tiers (`<24 GiB → 4k, 24–48 → 32k, ≥48 → 256k`). With 107.5 GiB visible that gives **262144** (server log `msg="vram-based default context" total_vram="107.5 GiB" default_num_ctx=262144`; live `ollama ps` CONTEXT 262144). pi's `models.json` says `contextWindow: 262144`, so the two agree.
   - **pi never sends `num_ctx`.** The bundle has 0 hits for `num_ctx`, `keep_alive` or `11434`.
   - **Ollama's `/v1` endpoint ignores `options.num_ctx` and `keep_alive` in the body** (LIVE: sent `options.num_ctx: 32768` and `keep_alive: "90s"`; `ollama ps` still showed CONTEXT 262144 and "4 minutes from now").
   - So the only context controls are server-side `OLLAMA_CONTEXT_LENGTH`, the app's context slider (currently 0 = auto), or a Modelfile variant.
   - **Invariant to keep:** pi `contextWindow` ≤ Ollama's effective `num_ctx`. pi decides when to compact from `contextWindow`. If the server's context were ever smaller, pi would never compact and Ollama would drop context without any error. The same happens on a machine with less VRAM, or if someone sets `OLLAMA_CONTEXT_LENGTH` or moves the app slider.
2. **pi's `maxTokens` is silently dropped by Ollama.** For a custom `openai-completions` provider, pi sends `max_completion_tokens` by default. Ollama only reads `max_tokens`.
   - LIVE: with `max_completion_tokens: 5`, Ollama produced 66 tokens (`finish: stop`). With `max_tokens: 5` it produced 5 (`finish: length`).
   - The Ollama binary has no `json:"max_completion_tokens` tag.
   - Fix: `compat.maxTokensField: "max_tokens"`.
3. **`--thinking off` on pi does not turn Qwen thinking off.** With no level set, pi sends no `reasoning_effort`, and Ollama thinks by default for thinking models.
   - LIVE: a request with no `reasoning_effort` returned 113 chars of `reasoning`.
   - Only `thinkingLevelMap: {"off": "none"}` makes `off` real.
4. **The `developer` role works on Ollama.** pi sends its system prompt as `role: developer` because `reasoning: true` and `supportsDeveloperRole` defaults to true for unknown hosts.
   - LIVE: a developer message saying "reply PINEAPPLE" overrode the user request. The worker briefing is therefore not being dropped, and no compat change is needed.
5. **Prime's kernel output is the big token sink, and there is no knob for it.** Each `ipython` call returns up to **65,536 chars each** of stdout, stderr and result, plus 65,536 of background output. The start is kept and the rest truncated (`[... output truncated at 65536 chars ...]`). That is about 16k tokens per stream and roughly 64k tokens per call at worst. `maxOutputChars` is internal only. The lever is the briefing text in `teammates/prime.md`: "print slices/summaries, not whole objects".
6. **Prime on Opus 5 almost never compacts.** The catalog `contextWindow` is 1,000,000, so compaction fires at about 983,616 tokens. If the API's real limit is lower, long runs hit a context error first. A `modelOverrides` entry setting `contextWindow` to 200000 in `~/.prime/agent/models.json` is the documented lever, and that value is safe under either limit.
7. **Prime prompt caching is on by default** (5-minute ephemeral `cache_control` on the system prompt, the last tool and the last message). `PI_CACHE_RETENTION=long` switches to a 1h TTL, which costs 2× instead of 1.25× on writes.
8. **Prime `--thinking` on Opus 5 is adaptive thinking plus `output_config.effort`** (`minimal`/`low`→`low`, `medium`, `high`, `xhigh`, `max`). `thinkingBudgets` is ignored for this model.

---

## Part A: pi (0.85.1)

### What horch already passes (`crates/horch-core/src/launch.rs` `pi_family_command`, `skills.rs` `native_args`)
`pi --model ollama/qwen3.8 --thinking high [--tools …] [--exclude-tools …] --no-extensions --no-skills --no-prompt-templates --no-themes --skill <bundle>/skills --session-id <id> -- "<prompt>"`

- The pane runs pi in **interactive** mode (no `-p`).
- The `pi.md` teammate sets `inherit_plugins: false` and `effort: high`.
- The phase skill catalog horch adds is about 117 tokens (`horch skills --phase implementation`). `_base/fleet-worker.md` plus the `pi.md` body is about 4 KB. The startup prompt is therefore tiny compared with 262k; it only becomes a problem if `num_ctx` falls to the 4k tier.

### The request pi sends to Ollama (M1 `chunks/openai-completions-EKZT2IH2.js` `detectCompat` / `buildParams`; LIVE-confirmed)
- `POST http://localhost:11434/v1/chat/completions`, `api: "openai-completions"` (from `~/.pi/agent/models.json`).
- System prompt as `role: "developer"` (reasoning model with the default `supportsDeveloperRole: true`). **Honored by Ollama (LIVE).**
- `reasoning_effort: "<level>"` when `--thinking` ≠ off, taking `thinkingLevelMap[level]` if set, else the level name. **Nothing** is sent for `off`.
- `max_completion_tokens: <maxTokens>`, the default `maxTokensField`. **Ignored by Ollama (LIVE).**
- `store: false`, `stream: true`, `stream_options.include_usage: true`.
- No `num_ctx`, no `keep_alive`, no `options`. `prompt_cache_key` is sent only to api.openai.com or when retention is `long` (Ollama ignores it).
- Ollama has **no special compat detection** in pi (`detectCompat` has no ollama/11434 branch), so every generic default applies.

### Config surfaces (M2 shipped `docs/`)
- `~/.pi/agent/settings.json` (global; **does not exist today**) and `<cwd>/.pi/settings.json` (project; needs trust).
- `~/.pi/agent/models.json` (exists; providers `ollama` → `qwen3.8` with `contextWindow 262144`, `maxTokens 32000`, and `qwen3:8b`).
- Config dir override: `PI_CODING_AGENT_DIR`. Sessions: `--session-dir` > `PI_CODING_AGENT_SESSION_DIR` > `sessionDir`. Default location `~/.pi/agent/sessions/--<path>--/*.jsonl` (none exist; pi has evidently never run successfully here).
- **Compaction** (docs/compaction.md, settings.md): on by default. It triggers when `contextTokens > contextWindow − compaction.reserveTokens` (default 16384), checked between tool batches, and keeps the most recent `compaction.keepRecentTokens` (default 20000). The summary is generated by the **same model** (local qwen3.8), with prompt-cache writes disabled for that call.
- **Thinking levels:** `off, minimal, low, medium, high, xhigh, max`. Without a `thinkingLevelMap`, `xhigh`/`max` are clamped. Ollama accepts `high/medium/low/max/none` (M2 openai-compatibility checklist). LIVE: `minimal` and `xhigh` were also accepted without error (thinking stayed on).
- **Tool output truncation:** `bash` keeps the **last 2000 lines or 50 KB** (full output goes to a temp file), and the same 2000-line / 50 KB limit applies to `read`. This is hard-coded (M1 `Output is truncated to last ${2e3} lines or ${51200/1024}KB`); there is no setting.
- **Context files:** pi loads `AGENTS.md` / `CLAUDE.md` from `~/.pi/agent/`, every parent directory, and the cwd. **None exist on this machine's path today.**
- **Project trust:** in interactive mode pi *prompts* when the cwd has project-local `.pi` settings/resources or `.agents/skills`. An unattended pane would wait on that prompt. `--no-approve` / `-na` ignores those resources for one run (M3, docs/security.md).

## Part B: Ollama (0.32.15) behind pi

### Current server config (server log, `routes.go` "server config", last server start 2026-08-25)
`OLLAMA_CONTEXT_LENGTH:0` (→ VRAM tier → 262144), `OLLAMA_NUM_PARALLEL:1`, `OLLAMA_KEEP_ALIVE:5m0s`, `OLLAMA_MAX_LOADED_MODELS:0` (→ 3×GPUs), `OLLAMA_MAX_QUEUE:512`, `OLLAMA_FLASH_ATTENTION:false` (but the new engine logs `flash_attn = auto` → "warmup: flash attention is enabled"), `OLLAMA_KV_CACHE_TYPE:` (→ f16), `OLLAMA_HOST:127.0.0.1:11434`, `OLLAMA_NO_CLOUD:false`. The Ollama.app settings DB has `context_length = 0` (auto) and `think_enabled = 0` (that setting applies to the app's chat UI only; the API default is thinking on).

### The `num_ctx` accuracy trap (brief item B3)
- **Default in 0.32.15:** VRAM-based. `ollama serve --help`: "OLLAMA_CONTEXT_LENGTH … (default: 4k/32k/256k based on VRAM)". docs.ollama.com/context-length: "< 24 GiB VRAM: 4k context, 24-48 GiB VRAM: 32k context, >= 48 GiB VRAM: 256k context". **On this machine that is 262144.** (The GitHub `docs/faq.mdx` still says "4096", which is out of date for this version.)
- **qwen3.8 maximum:** 262144 (`ollama show`).
- **How pi sets it:** pi does not. It has no `num_ctx` code, and `/v1` ignores `options.num_ctx` (LIVE). Ollama's docs say: "The OpenAI API does not have a way of setting the context size for a model", and recommend a Modelfile with `PARAMETER num_ctx` (docs.ollama.com/api/openai-compatibility).
- **Evidence pi-shaped traffic ran at full context:** on 2026-09-09, `/v1/chat/completions` loads of qwen3.8 logged `new slot, n_ctx = 262144 … truncated = 0`. The `n_ctx 2048/8192` lines in the log come from *other* clients (`/api/chat` with explicit `num_ctx`, and nomic-embed's `n_ctx_train=2048`), not from pi.
- **Where server knobs live:** Ollama.app launches the server, so a teammate `env:` map **cannot** reach any `OLLAMA_*` variable. The mechanism is `launchctl setenv OLLAMA_X value` followed by restarting Ollama.app (docs.ollama.com/faq, "Setting environment variables on Mac"), or the app's context slider for context length.
- **Unknown:** what happens when a prompt *does* exceed `num_ctx` for this hybrid model. The log says "KV cache shifting is not supported for this context", which could mean an error or truncation of old messages (see Unverified).

### Qwen thinking on/off (brief item B5)
- **Toggle:** over `/v1`, `reasoning_effort` / `reasoning.effort` accepts `high|medium|low|max|none`, and `none` disables thinking (M2). With the field absent, thinking is **on** (M2 "Thinking is enabled by default in the CLI and API for supported models"; LIVE).
- **pi mapping:** `--thinking <level>` sends that level string. `off` sends nothing, so thinking stays on unless `thinkingLevelMap.off = "none"`.
- **Tradeoff:** thinking improves multi-step and tool-planning accuracy on a 27B model, at the cost of output tokens and wall-clock time. On local hardware that is time, not money. Thinking tokens also count against `max_tokens`, so the fix for that flag (below) caps a runaway thinking loop.
- Keep thinking **on** for fleet workers (policy). Allow `off` only as an explicit per-task effort.

### Top 10: pi + Ollama

| # | Name (exact) | Harness | Controls | Verified by | Default (installed) | Recommended: worker / orchestrator | Lives in | Token effect | Accuracy effect | Verdict |
|---|---|---|---|---|---|---|---|---|---|---|
| 1 | `compat.maxTokensField: "max_tokens"` (provider- or model-level) | pi → Ollama | Which field carries pi's `maxTokens` | M1 `detectCompat` → `maxTokensField: useMaxTokens ? "max_tokens" : "max_completion_tokens"` (false for Ollama); M2 Ollama checklist lists only `max_tokens`; M1 Ollama binary has no `max_completion_tokens` tag; **LIVE** 66 vs 5 tokens | `max_completion_tokens` (ignored → output unbounded up to the context) | `"max_tokens"` with the existing `maxTokens: 32000` / n/a (orchestrator never runs on pi) | pi `models.json` (`~/.pi/agent/models.json`, or the fleet dir from #8) | Output: caps runaway generations and thinking loops at 32k (large on bad turns, zero normally) | Neutral (32k is generous); helps by stopping loops | **RECOMMEND** |
| 2 | `thinkingLevelMap: {"off": "none"}` (model-level on `qwen3.8`) | pi → Ollama | Makes pi `--thinking off` actually disable Qwen thinking | M1 code: `if(!options?.reasoningEffort && model.reasoning && compat.supportsReasoningEffort){ let offValue=model.thinkingLevelMap?.off; typeof offValue=="string" && (params.reasoning_effort=offValue) }`; M2 Ollama `"none"`; **LIVE** absent → reasoning present | omitted → `off` still thinks | Add it; keep teammate `effort: high` / n/a | pi `models.json` | Output: large saving *only when* a teammate or task uses `effort: off`; zero at `high` | Neutral at current settings; makes effort levels truthful | **RECOMMEND** (correctness) |
| 3 | `OLLAMA_CONTEXT_LENGTH` | Ollama server | Default `num_ctx` for every request without `options.num_ctx` (all pi requests) | M3 `ollama serve --help`; M2 docs.ollama.com/context-length; log `default_num_ctx=262144`; **LIVE** `ollama ps` CONTEXT 262144 | 0 → VRAM tier → **262144** here | Pin to **262144** (= pi `contextWindow`); if #5 is lowered, never set this below it / unaffected | `launchctl setenv OLLAMA_CONTEXT_LENGTH 262144` + restart Ollama.app, or the app slider | None today; stops a future silent truncation | **Helps** (locks the invariant; today's value only holds by VRAM detection) | **RECOMMEND** (accuracy guard, no behavior change today) |
| 4 | `OLLAMA_KEEP_ALIVE` | Ollama server | How long qwen3.8 (about 33 GiB incl. KV) stays loaded after the last request | M3 help; M2 faq; log `OLLAMA_KEEP_ALIVE:5m0s`; **LIVE** per-request `keep_alive` over `/v1` ignored, so the server env is the only lever | `5m` | `30m` (or `-1` for the length of a fleet run, then restore) / unaffected | `launchctl setenv` + restart app | No billed tokens. Avoids a full re-prefill of the whole context (llama-server prompt cache, 8 GiB limit, is lost on unload) after any >5 min idle, e.g. waiting on the orchestrator: large *compute/latency* saving on long sessions | Neutral | **RECOMMEND** (the cost is about 33 GiB staying resident) |
| 5 | `contextWindow` (qwen3.8 entry) | pi | pi's **compaction trigger** (not Ollama's context) | M2 pi docs/models.md ("Context window size in tokens", default 128000), docs/compaction.md trigger formula; M1 | 262144 (set in models.json; 128000 if omitted) | **131072** (compacts at about 114.7k) / n/a. Must stay ≤ Ollama `num_ctx` | pi `models.json` | Input: up to about 2× fewer prompt tokens per turn late in long sessions; much shorter prefill | Mixed: helps against long-context degradation on a 27B Q4 model; risks losing detail in the summary (written by the same local model). Ollama docs: agents/coding tools "at least 64000" | **TUNE CAREFULLY** |
| 6 | `PI_OFFLINE=1` | pi | Skips startup network operations: update check, package updates, install/update telemetry | M3 `pi --help` ("Disable startup network operations"); M2 docs/environment-variables.md | unset | `1` / n/a | teammate `env:` in `pi.md` | None | Neutral; faster, fully local start, which matches `pi.md`'s "nothing leaves the machine" | **RECOMMEND** |
| 7 | `OLLAMA_NUM_PARALLEL` | Ollama server | Concurrent requests per loaded model; memory scales with it | M3 help; M2 faq ("Required RAM will scale by OLLAMA_NUM_PARALLEL * OLLAMA_CONTEXT_LENGTH"); log `OLLAMA_NUM_PARALLEL:1` | 1 | Keep `1` for a single pi worker. Set it to the number of concurrent pi workers (2–3) only if the fleet runs several; each extra slot adds about 16 GiB KV at 262144 (8 GiB at 131072) / unaffected | `launchctl setenv` + restart app | None on tokens; throughput (otherwise the second pi worker queues behind the first) | Neutral; risks memory pressure against other loaded models | **TUNE CAREFULLY** |
| 8 | `PI_CODING_AGENT_DIR` | pi | Config dir (models.json, settings.json, auth, sessions default) | M3 help; M2 docs/environment-variables.md | `~/.pi/agent` | A fleet-owned dir holding a copy of `models.json` (+ #1, #2, #5) and a `settings.json` for compaction tuning, so fleet tuning never edits the operator's global pi config (the same pattern as the private `CODEX_HOME`) / n/a | teammate `env:` in `pi.md` | Indirect (it is where #1/#2/#5 land safely) | Neutral; risk of drift from the operator's `models.json` | **TUNE CAREFULLY** |
| 9 | `--no-context-files` / `-nc` | pi (also Prime) | Stops loading AGENTS.md/CLAUDE.md from `~/.pi/agent`, parent dirs and cwd | M3 help; M2 docs/usage.md "Context Files" | discovery on | `-nc` for workers on repos whose AGENTS.md is written for other agents / n/a | teammate `args:` | Input: equal to the context-file size, on every turn (**0 today**: no such files on this path) | Risk: drops real project conventions if the target repo's AGENTS.md matters | **TUNE CAREFULLY** (per-project) |
| 10 | `--no-approve` / `-na` | pi | Ignores project-local `.pi` settings/resources for the run, which stops the interactive trust **prompt** from stalling an unattended pane | M3 help; M2 docs/security.md, settings.md "Project Trust" | prompt (`defaultProjectTrust: "ask"`) in interactive mode | `-na` / n/a | teammate `args:` or `pi_family_command` | None | Helps (no hung worker); matches `inherit_plugins: false` intent | **TUNE CAREFULLY** (only matters on repos with `.pi/` resources; none here) |

**Ready-to-apply `models.json` delta for #1 and #2 (recommendation only; not applied):**
```json
{ "providers": { "ollama": {
    "compat": { "maxTokensField": "max_tokens" },
    "models": [ { "id": "qwen3.8", "thinkingLevelMap": { "off": "none" }, "...": "existing fields unchanged" } ]
} } }
```
`teammates/README.md` ("Running pi on local models") carries the same snippet and would need the same change.

**Alternative to #3 when the fleet's context should differ from other Ollama clients:** `ollama create qwen3.8-fleet` from a Modelfile with `FROM qwen3.8` + `PARAMETER num_ctx 131072`. This is the documented `/v1` route. `pi.md` would then use `model: ollama/qwen3.8-fleet`. TUNE CAREFULLY; it creates a model, so it is the operator's call.

---

## Part C: Prime Agent (0.9.4, `anthropic/claude-opus-5`)

### What horch already passes
`prime-agent --model anthropic/claude-opus-5 --thinking high --no-extensions --no-skills --no-prompt-templates --no-themes --skill <bundle>/skills --daemon-socket <state>/prime/<role>-<uuid>/d.sock --session-dir <state>/prime/<role>-<uuid>/sessions -- "<prompt>"` (interactive; `crates/horch-core/src/prime.rs` stops the per-pane daemon on exit).

### Findings
- **Auth:** `~/.prime/agent/auth.json` is empty (`{}`), and `ANTHROPIC_API_KEY` is set in the shell, so model calls use the API key (pay-per-token).
- **Thinking** (M1 `anthropic-LMVIEUPS.js` `supportsAdaptiveThinking` includes `"opus-5"`, `mapThinkingLevelToEffort`):
  - Levels map to `thinking: {type: "adaptive", display: "summarized"}` + `output_config: {effort}`, with `minimal`/`low`→`low`, `medium`→`medium`, `high`→`high`, `xhigh`→`xhigh`, `max`→`max`. The catalog `thinkingLevelMap` enables `xhigh` and `max`.
  - `off` → `thinking: {type: "disabled"}`.
  - **`thinkingBudgets` is ignored for Opus 5.** It only feeds `budget_tokens` on non-adaptive models.
  - Without `--thinking`, the settings default is **`xhigh`** (docs/settings.md); horch overrides it with `high`.
- **Prompt caching** (M1 `getCacheControl`, lines 6284-6292, and `cache_control` on the system blocks, the last tool and the last message):
  - **On by default**, `{type: "ephemeral"}` (5 min). `PI_CACHE_RETENTION=long` adds `ttl: "1h"`.
  - Cost constants in the bundle: read 0.1×, 5-min write 1.25×, 1-h write 2×.
  - No env var turns caching off (`"none"` is reachable only through an SDK option).
- **Max output:** `max_tokens = min(catalog maxTokens 128000, 32000) = 32000` (M1 `buildBaseOptions`). Thinking counts against it.
- **Context / compaction:** catalog `contextWindow: 1e6` (M1). Compaction (same settings as pi: `compaction.enabled` true, `reserveTokens` 16384, `keepRecentTokens` 20000) therefore fires at about **983,616** tokens. That is effectively never for a worker; each turn re-reads the whole history (at 0.1× when cached).
- **Kernel output (the big sink)** (M1 `chunk-TOACIHN2.js` `DEFAULT_MAX_OUTPUT_CHARS = 65536`, `executeInner`, `MAX_BACKGROUND_OUTPUT_CHARS = 64 * 1024`):
  - Per `ipython` call: stdout, stderr and the `result` repr are *each* cut at 65,536 chars (the start is kept), plus up to 65,536 chars of background-task output.
  - The only override of `maxOutputChars` is internal (autonomous gates, 6000).
  - **No env var or setting controls it.** Fleet lever: add a line to the `teammates/prime.md` body such as "print `len()`/`head()`/`.info()`/slices, never whole files or DataFrames; write big outputs to a file and read the part you need". That text belongs in the teammate file, not in Rust.
- **Recursive sub-agents (`rlm`)** (M1):
  - Depth is resolved in this order: persisted chat state, global setting `rlmMaxDepth`, env **`RLM_MAX_DEPTH`**, default **2**.
  - A spawn fails when `RLM_DEPTH >= RLM_MAX_DEPTH`. So 2 allows children and grandchildren, 1 allows children only, and 0 allows none.
  - Every child is a fresh Opus 5 session, so depth multiplies tokens.
  - This variable is not in the docs.
- **Built-in skills:** `--no-skills` (which horch passes) **also removes Prime's built-in skills** (docs/skills.md:101): `edit`, `compact`, `goal`, `refine`, `websearch`, `agent-message`, and others. `edit` is the exact-string replace helper ("instead of rewriting the whole file"). Without it the model edits files through raw Python I/O, and rewriting whole files costs output tokens. `--skill <path>` is additive even with `--no-skills` (docs/skills.md:37).
- **Session dir contents:** `<session-uuid>.jsonl` files (observed under `~/.prime/agent/sessions`). Artifacts go to `~/.prime/agent/session-artifacts/<id>/`. The kernel venv is shared at `~/.prime/agent/kernel-venv` (`PRIME_AGENT_KERNEL_VENV` overrides it).
- **Daemon:** `idleEvictionMinutes` (default 90, read only from the global settings.json) evicts idle worker trees, losing kernel state. `PRIME_AGENT_INTERNAL_*` variables are supervisor internals; never set them.
- **Telemetry / traces:** `telemetry.enabled` defaults to true and sends pseudonymous aggregates; the docs say it sends no prompts, paths or code. `PRIME_AGENT_TELEMETRY=0` turns it off. Trace upload (`/traces`, `PRIME_AGENT_TRACES_*`, `PRIME_API_KEY`) is **opt-in and off** (no `PRIME_API_KEY` set).

### Top 10: Prime Agent

| # | Name (exact) | Harness | Controls | Verified by | Default (installed) | Recommended: worker / orchestrator | Lives in | Token effect | Accuracy effect | Verdict |
|---|---|---|---|---|---|---|---|---|---|---|
| 1 | `modelOverrides` → `"claude-opus-5": {"contextWindow": N}` under provider `anthropic` | prime | Compaction trigger (`N − reserveTokens`) | M2 Prime docs/models.md §Per-model Overrides ("supports … `contextWindow`, `maxTokens` …"); M1 catalog `contextWindow: 1e6` | 1,000,000 (compaction at about 983.6k) | **200000** (compacts at about 184k). This is the safe value under either API limit. Raise it (e.g. 400000) only after Opus 5's ~1M context is confirmed reachable (Unverified #3). If the real limit is below the catalog's 1M, a long Prime run **today** hits an API context error before compaction can fire / n/a | `~/.prime/agent/models.json` (or the fleet dir from #6) | Input: **large** on long runs (caps per-turn history; cached reads are still 0.1× of a huge base) | Mixed: compaction loses detail; very long contexts also degrade. Needs measurement | **TUNE CAREFULLY** |
| 2 | `PI_CACHE_RETENTION=long` | prime | Anthropic `cache_control` TTL 5m → 1h | M1 `resolveCacheRetention`: `process.env.PI_CACHE_RETENTION === "long"` → `ttl: "1h"`; M2 Prime docs/usage.md env table | unset → 5m ephemeral (caching ON) | `long` for Prime workers that regularly idle >5 min between model calls (long kernel jobs, waiting on the orchestrator) / n/a | teammate `env:` in `prime.md` | Input: every write costs 2× instead of 1.25×, and each >5-min gap stops costing a full 1.25× re-write. Break-even is about one such gap per hour of cached prefix | Neutral | **TUNE CAREFULLY** (measure cache_read vs cache_write in the session JSONL first) |
| 3 | `RLM_MAX_DEPTH` (env) / `rlmMaxDepth` (global setting, wins over env) | prime | Recursive sub-agent depth | M1 `_resolveRlmMaxDepth` (chat → global setting → `process.env.RLM_MAX_DEPTH` → `{maxDepth: 2}`); error `RLM recursion depth limit reached (RLM_DEPTH=…, RLM_MAX_DEPTH=…)`. **Undocumented; may change** | 2 | **1** (the worker may delegate once; the fleet orchestrator is already the top-level delegator) / n/a | teammate `env:` in `prime.md` | Output + input: **large** when the model fans out (each child is a full Opus 5 context) | Risk: slightly less capability on huge exploratory tasks. `0` would remove Prime's main strength, so do not use 0 | **TUNE CAREFULLY** |
| 4 | `--thinking` level (teammate `effort`) | prime | Opus 5 adaptive `output_config.effort` | M3 help (`off … max`); M1 `mapThinkingLevelToEffort` | horch passes `high` (Prime's own default is `xhigh`) | Keep `high` for the Prime tier (long exploratory work); `medium` for a narrow, well-specified Prime task / n/a | `teammates/prime.md` `effort:` (already wired) | Output: medium-large | `medium` risks shallower multi-step reasoning; never `off` (policy) | **TUNE CAREFULLY** |
| 5 | `--skill /opt/homebrew/lib/node_modules/prime-agent/skills/edit` | prime | Restores the built-in exact-string `edit` skill that `--no-skills` removes | M2 Prime docs/skills.md:37 ("`--skill <path>` … additive even with `--no-skills`"), :101 ("`--no-skills` also excludes built-in skills"); M1 file exists | absent (removed by horch's `--no-skills`) | Add for Prime workers that edit code / n/a | teammate `args:` in `prime.md` | Output: medium (targeted replaces instead of full-file rewrites); the catalog adds a line of metadata | Helps (edits require exactly one match, so fewer accidental rewrites) | **TUNE CAREFULLY** (a one-line flag, but it is a horch default; path is install-specific) |
| 6 | `PRIME_AGENT_CODING_AGENT_DIR` (+ `PRIME_AGENT_KERNEL_VENV` pointing at the existing venv) | prime | Config dir (settings.json, models.json, telemetry id) | M2 Prime docs/usage.md env table; docs/skills.md (kernel venv) | `~/.prime/agent` | A fleet-owned dir carrying #1 and the compaction/rlm settings without touching the operator's global config. Set `PRIME_AGENT_KERNEL_VENV=~/.prime/agent/kernel-venv` to avoid a second kernel bootstrap. Auth still comes from `ANTHROPIC_API_KEY` / n/a | teammate `env:` in `prime.md` | Indirect | Neutral; risk of drift | **TUNE CAREFULLY** |
| 7 | `PI_OFFLINE=1` (≡ `--offline`) | prime | Skips startup network operations: update check, package checks, Prime Inference model refresh | M3 help (`--offline`); M2 Prime docs/usage.md, providers.md ("Set `PI_OFFLINE=1` to skip network refreshes") | unset | `1` / n/a | teammate `env:` in `prime.md` | None | Neutral; faster start | **RECOMMEND** |
| 8 | `idleEvictionMinutes` | prime daemon | Idle eviction of worker trees and passivation of idle children (kernel state lost) | M2 Prime docs/settings.md §Daemon | 90 | `"off"` for fleet workers (horch already stops each pane's daemon when the pane closes) / n/a | global `settings.json` only | Input/output: avoids re-deriving lost kernel state after long waits (small-medium, occasional) | Helps (state kept) | **TUNE CAREFULLY** (global-only key; affects the operator's own Prime use unless #6 is used) |
| 9 | `PRIME_AGENT_TELEMETRY=0` | prime | Pseudonymous usage analytics | M2 Prime docs/settings.md ("`PRIME_AGENT_TELEMETRY=0 prime-agent`"; no prompts/paths/code sent) | enabled | `0` if the operator wants zero egress beyond the model API / n/a | teammate `env:` | None | Neutral | **TUNE CAREFULLY** (hygiene, not a token lever) |
| 10 | `--no-context-files` / `-nc` | prime | Same as the pi row #9 | M3 help | discovery on | Per project / n/a | teammate `args:` | Input: equal to the context-file size per turn (0 today) | Risk: drops project conventions | **TUNE CAREFULLY** |

**Kernel-output lever (no knob, so not a table row):** each `ipython` call can return about 16k tokens *per stream* before truncation. A one-sentence output-discipline instruction in `teammates/prime.md` is the only way to control the largest Prime token sink.

---

## Skip list (verified, with reasons)

| Item | Harness | Reason |
|---|---|---|
| Small `OLLAMA_CONTEXT_LENGTH` (or app slider below pi `contextWindow`) | Ollama | **Policy:** silently truncates model context; pi would never compact. |
| `compaction.enabled: false` | pi, prime | **Policy / accuracy:** pi would overflow num_ctx (silent truncation); Prime would grow to the API limit. |
| Thinking `off` as a default (`effort: off`, `reasoning_effort: "none"` everywhere) | pi, prime | **Policy:** disables thinking. |
| `OLLAMA_KV_CACHE_TYPE=q8_0/q4_0` | Ollama | Quality risk: docs say high-GQA models like Qwen "may see a larger impact". No memory pressure here (about 33 GiB used of 107.5 GiB). |
| `OLLAMA_FLASH_ATTENTION=1` | Ollama | No-op: the engine already runs `flash_attn = auto` → "flash attention is enabled" (server log). |
| `compat.supportsDeveloperRole: false` | pi | Not needed: LIVE test shows Ollama honors `developer`. (Harmless if set, since `system` is also supported.) |
| `samplingParams: {"options": {"num_ctx": …}, "keep_alive": …}` | pi | Does not work: LIVE, `/v1` ignores both. |
| `PI_CACHE_RETENTION` for pi+Ollama | pi | No-op: Ollama ignores `prompt_cache_key` and `prompt_cache_retention`; llama-server's prefix cache is automatic. |
| `thinkingBudgets` / `compat.thinkingTokenBudgetField` for qwen3.8 | pi | Ollama's `/v1` accepts no `thinking_budget` field; cap thinking via `max_tokens` (#1). |
| `thinkingBudgets` for Opus 5 | prime | Ignored: Opus 5 uses adaptive thinking (M1). |
| `OLLAMA_MAX_LOADED_MODELS`, `OLLAMA_MAX_QUEUE`, `OLLAMA_HOST`, `OLLAMA_LOAD_TIMEOUT`, `OLLAMA_SCHED_SPREAD`, `OLLAMA_GPU_OVERHEAD` | Ollama | No token or accuracy effect for one local worker; defaults are fine. |
| `OLLAMA_DEBUG=1` / `OLLAMA_DEBUG_LOG_REQUESTS` | Ollama | Diagnostic only: writes prompts to the server log. Useful once to verify prompt rendering, not for running. |
| `OLLAMA_NO_CLOUD=1` | Ollama | Neutral on tokens. Optional privacy hardening, consistent with `pi.md` ("nothing leaves the machine"); qwen3.8 is local anyway. |
| `/traces`, `PRIME_AGENT_TRACES_API_KEY`, `PRIME_AGENT_TRACES_BASE_URL`, `PRIME_API_KEY` | prime | **Policy:** uploads session traces to an external endpoint. Opt-in and currently off; keep it off. |
| `--autonomous-max-tokens` (default 80000) and other `--autonomous-*` | prime | Only apply with `--autonomous`, which horch does not use. |
| `PRIME_AGENT_KERNEL_PYTHON` | prime | No token effect; the managed venv works. |
| `compaction.reserveTokens` / `keepRecentTokens` changes | pi, prime | Keep the defaults (16384 / 20000). Move the trigger with `contextWindow` (pi #5, Prime #1) instead. |
| `PI_TELEMETRY=0` | pi | Already covered by `PI_OFFLINE=1` (pi docs: offline disables install/update telemetry). |

## Unverified / folklore (not in any top list)

1. What Ollama 0.32.15 does when a `/v1` prompt exceeds `num_ctx` for this hybrid model (KV shifting is disabled): an error, or silently dropping the oldest messages? It only matters if the #3/#5 invariant is broken.
2. Whether `reasoning_effort` `low/medium/high/max` changes qwen3.8's thinking length or only switches it on. LIVE showed `minimal`/`xhigh` are accepted (no 400), but I did not measure trace lengths.
3. Whether Opus 5's catalog 1M context is reachable without a beta header. No `context-1m` string exists in Prime's Anthropic provider. This decides whether Prime #1 must stay at 200000 or can go higher. A context-length error in the session JSONL of any long Prime run would settle it.
4. pi `retry.provider.timeoutMs` "SDK default" (folklore: OpenAI SDK 10 min) compared with local prefill time for about 250k tokens on this machine.
5. With the llama-server engine, whether `OLLAMA_NUM_PARALLEL>1` gives every slot the full `num_ctx` (docs say total context multiplies) and whether the prompt cache is per slot.
6. Whether `HORCH_PI_BIN` set in `pi.md`'s `env:` map is applied before `agent::pi_bin()` is read.
7. `--goal-token-budget` as a hard per-worker token cap (only exists with `--goal`; semantics not checked).
8. Size of Prime's default system prompt plus the `ipython` tool description (not measured; needs an API call).
9. Qwen's own recommended sampling for thinking mode (folklore: temp 0.6). The installed Modelfile ships `temperature 1, top_k 20, top_p 0.95`. Do not override without an eval.
10. Whether `PI_CACHE_RETENTION` also reaches `rlm` child sessions (they use the same provider path, so probably yes).

## Method notes
- LIVE tests: 6 small `/v1/chat/completions` requests to qwen3.8 (≤66 output tokens each), then `ollama stop qwen3.8`. No Ollama settings, models or config files were changed.
- Secrets: `models.json` `apiKey` redacted in my reads; only the *name* `ANTHROPIC_API_KEY` was checked, not its value.
