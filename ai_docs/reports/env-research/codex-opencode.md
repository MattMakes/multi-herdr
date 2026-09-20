# Env vars and config for fleet efficiency: Codex CLI and OpenCode

Researcher: researcher-3. Date: 2026-09-18. Brief: `ai_docs/plans/env-research/02-codex-opencode.md`.
Report only. No teammate, settings, or Rust files were edited.

Four findings change how the orchestrator should read everything else:

1. **A codex pane's private `CODEX_HOME` does not give it a private `config.toml`.** `build_private_home` (`crates/horch-core/src/codex.rs:185`) symlinks every entry of `~/.codex` except `rules/`, and that includes `config.toml`. Writing a key "into the worker's config.toml" edits the operator's real file. Per-worker settings belong in teammate `args:` (`-c key=value`, `--disable <feature>`) or in a profile file (see A.2).
2. **OpenCode's `effort` → `--variant` does nothing, for two separate reasons.** horch launches the TUI (`opencode --model … --prompt …`), and the TUI command defines no `--variant` option. Its handler forwards only `{continue, sessionID, agent, model, prompt, fork, auto}`, and yargs accepts the unknown flag silently (`opencode --variant zzz --version` exits 0). Separately, all three free models report `variants: {}`, and the bundle hard-codes `big-pickle` to return no variants. So `effort: high` / `effort: minimal` in `teammates/opencode-*.md` currently has no effect.
3. **Neither codex worker sets `effort:`, so both take `model_reasoning_effort = "medium"` from the operator's shared config.toml.** The catalog default is `low` for gpt-5.6-sol and `medium` for gpt-5.6-terra. If the operator changes their own default, every codex worker changes with it.
4. **`--pure` drops OpenCode plugins but not MCP servers.** Every free-tier worker carries the operator's playwright MCP (26 tools, 21,080 chars of schema) and context7 (2 tools, 4,937 chars). They also carry 16 `~/.claude/skills` entries that are Claude-only or personal, sent on every request to endpoints that may train on them.

---

# Part A: Codex CLI

## A.0 Version and binary

- `codex --version | head -1` → **`codex-cli 0.154.0`**
- Binary: `/opt/homebrew/Caskroom/codex/0.154.0/bin/codex` (Mach-O arm64, Rust). `/opt/homebrew/bin/codex` symlinks to it. The latest upstream tag is `rust-v0.155.0`. All source citations below are pinned to `rust-v0.154.0`.
- Models (from `codex debug models`, the installed catalog):

| model | default effort | client effort levels | API-accepted efforts (from the error text) | context_window / max | tool-output truncation | default verbosity / summary |
|---|---|---|---|---|---|---|
| gpt-5.6-sol | **low** | low, medium, high, xhigh, max, ultra | none, low, medium, high, xhigh, max | 272,000 / 872,000 | 10,000 tokens | low / none |
| gpt-5.6-terra | **medium** | low, medium, high, xhigh, max, ultra | none, low, medium, high, xhigh, max | 272,000 / 872,000 | 10,000 tokens | low / none |
| gpt-6-astra (orchestrator) | medium | low … max, ultra | (not probed) | 272,000 / 872,000 | 10,000 tokens | low / none |

`ultra` exists only in the client catalog ("Maximum reasoning with automatic task delegation"), and the API does not accept it. `minimal` is **rejected**: `codex exec -m gpt-5.6-terra -c model_reasoning_effort="minimal"` produced `turn.failed … Unsupported value: 'minimal' is not supported with the 'gpt-5.6-terra' model`. A codex teammate with `effort: minimal` would fail every turn.

## A.1 Raw enumeration

- `strings <binary> | grep -oE '(CODEX|OPENAI)[A-Z0-9_]+' | sort -u` → **86 raw matches**. Many are concatenated strings (e.g. `CODEX_API_KEYOPENAI_API_KEY`); splitting them leaves **82 unique names**.
- None of those env vars controls reasoning, verbosity, compaction, tool output, or context. Those are all config keys. The env vars that touch the fleet at all are `CODEX_HOME` (horch already sets it), `CODEX_SQLITE_HOME` (state-DB location), `OPENAI_BASE_URL`, and `CODEX_OSS_BASE_URL`/`CODEX_OSS_PORT`. The rest are auth, proxy, sandbox-internal (`CODEX_SANDBOX*`, `CODEX_ESCALATE_SOCKET`), or telemetry/testing.
- The full top-level `ConfigToml` field list was taken from the binary (serde field-name string). The keys relevant here are `model_context_window`, `model_auto_compact_token_limit`, `model_auto_compact_token_limit_scope`, `shell_environment_policy`, `notify`, `include_permissions_instructions`, `include_apps_instructions`, `include_environment_context`, `compact_prompt`, `project_doc_max_bytes`, `tool_output_token_limit`, `history`, `hide_agent_reasoning`, `show_raw_agent_reasoning`, `model_reasoning_effort`, `plan_mode_reasoning_effort`, `model_reasoning_summary`, `model_verbosity`, `personality`, `service_tier`, `features`, `plugins`, `apps`, `memories`, `analytics`, `feedback`, `check_for_update_on_startup`, and `experimental_compact_prompt_file`. **`disable_response_storage` is not in 0.154.0.**
- Help: `codex --help`, `codex exec --help`, `codex resume --help`, `codex features list`, `codex debug --help`. There is no `codex config` subcommand. Relevant flags: `-c/--config key=value`, `--enable/--disable <FEATURE>` ("Equivalent to `-c features.<name>=…`"), `-p/--profile <CONFIG_PROFILE_V2>` ("Layer $CODEX_HOME/<name>.config.toml on top of the base user config"), and `--search`. `--disable` and `-p` both exist on the root command and on `resume`.

## A.2 Where codex settings can go (corrects the brief)

| landing place | per-worker? | verified | note |
|---|---|---|---|
| `$CODEX_HOME/config.toml` in the private home | **No.** It is a symlink to `~/.codex/config.toml` | `codex.rs:12-17, 185-222` | Writing there changes every codex session on the machine |
| teammate `args:` with `-c key=value` / `--disable <feature>` | Yes | `--help`; `codex_command` appends `teammate.args` after its own flags (`launch.rs:380`) | Zero Rust. horch already uses this for effort. Each flag takes one value, so none of them swallows the trailing prompt |
| `-p <name>` → `$CODEX_HOME/<name>.config.toml` | Yes, if horch or the operator puts the file in `~/.codex` before launch (the private home symlinks it in) | `--help` (root and `resume`); docs "Profiles" (learn.chatgpt.com/docs/config-file/config-advanced) | Docs: profile files use top-level keys. Whether nested tables such as `[features]` or `[mcp_servers.x]` work in a profile file is **unverified**, so `-c` is the safe form |
| teammate `env:` | Yes | – | Only useful for the few real env vars. Nothing in the top-10 is an env var |

This follows the token-economy memory: per-key overrides for the worker only, and the operator's `config.toml` stays untouched.

## A.3 Measurements (installed codex 0.154.0, cwd = this repo)

Method: `codex exec --ephemeral --json -m gpt-5.6-terra -c model_reasoning_effort="low" -c sandbox_mode="workspace-write" -c approval_policy="never" "Reply with exactly: OK"`, with the herdr env unset so the SessionStart hook stays quiet. `input_tokens` was read from `turn.completed`. Each row is one sample, and the absolute numbers depend on the operator's installed plugins, skills, and MCP servers.

| configuration | input tokens / request | Δ vs baseline |
|---|---|---|
| baseline (fleet `auto` perms) | 16,122 | – |
| `--disable apps` | 14,416 | −1,706 |
| `--disable plugins` | 13,458 | −2,664 |
| `--disable apps --disable plugins` | 13,124 (13,313 on a rerun) | ≈ −2,900 |
| … + `--disable image_generation` | 12,878 | −435 more |
| … + `-c web_search="disabled"` (instead of image_generation) | 10,857 | −2,456 more |
| all four (apps, plugins, image_generation, web_search) | 10,422 | **−5,700 (≈35%)** |
| all four + `mcp_servers.node_repl/blender.enabled=false` | 9,642 | −780 more |
| `--disable` multi_agent / browser_use / computer_use / goals / sleep_tool (each, on top of apps+plugins) | 13,313 | **0** (no change) |

Where the characters went, from `codex debug prompt-input` (model-visible developer/user messages only, no tool schemas): under auto perms the input is 20,314 chars. That breaks down as `<skills_instructions>` 11,140, `<recommended_plugins>` 3,489 (a list of uninstalled plugins), `<plugins_instructions>` 1,014, `<apps_instructions>` 646, multi-agent block 2,264, `<environment_context>` 904, and `<permissions instructions>` 584. With apps and plugins disabled it is 11,453 chars. The `<permissions instructions>` block is 8,190 chars only without auto perms, which the fleet never uses.

**Fleet skills survive `--disable plugins`.** I built a horch-style private home (symlinks plus `skills → <bundle>/skills`), ran `codex debug prompt-input --disable apps --disable plugins`, and the listing still had all four `horch:*` phase skills.

How to read the savings: the fixed prefix is cached. A cut therefore saves the uncached write once, then cached-read tokens on every later request in the session, and it frees context-window space, which pushes auto-compaction later. The dollar effect per request is small. Across a long worker session (100+ requests) it adds up to hundreds of thousands of cached-read tokens.

## A.4 Codex top-10

| # | setting | verdict | token effect | landing |
|---|---|---|---|---|
| 1 | `features.plugins` → `--disable plugins` | RECOMMEND (workers) | −2,664 input/request | teammate `args:` |
| 2 | `features.apps` → `--disable apps` | RECOMMEND (workers) | −1,706 input/request (≈ −2,900 together with #1) | teammate `args:` |
| 3 | `web_search = "disabled"` | TUNE CAREFULLY (RECOMMEND for codex-terra) | −2,456 input/request | teammate `args:` |
| 4 | `model_reasoning_effort` via teammate `effort:` | TUNE CAREFULLY (make it explicit) | output/reasoning side | teammate `effort:` (existing lever) |
| 5 | `mcp_servers.<name>.enabled = false` (the operator's node_repl, blender) | RECOMMEND (workers) | −780 input/request; also no MCP processes spawned per pane | teammate `args:` |
| 6 | `features.image_generation` → `--disable image_generation` | RECOMMEND (workers) | −435 input/request | teammate `args:` |
| 7 | `model_auto_compact_token_limit` | TUNE CAREFULLY | long sessions: input side | teammate `args:` |
| 8 | `tool_output_token_limit` | TUNE CAREFULLY | tool-output side | teammate `args:` |
| 9 | `history.persistence = "none"` | RECOMMEND (workers) | none (hygiene) | teammate `args:` |
| 10 | `notify = []` | TUNE CAREFULLY | none (process overhead) | teammate `args:` |

### 1. `features.plugins` (`--disable plugins`)
- **Controls:** loading of installed Codex plugins (skills, MCP servers, and apps they contribute) plus the `<plugins_instructions>` block. The operator has 10 enabled (sites, visualize, google-calendar, slack, documents, pdf, spreadsheets, presentations, template-creator, browser). None of them is needed for fleet coding work.
- **Verified by:** method 3 (`codex features list` → `plugins  stable  true`; `--help`: "--disable <FEATURE> … Equivalent to `-c features.<name>=false`") and method 2 (`codex-rs/core/config.schema.json` `features`). Measured, see A.3.
- **Default:** `true`.
- **Recommended:** workers `--disable plugins`. Orchestrator: same, unless the operator uses plugin skills from the orchestrator pane.
- **Where:** teammate `args: [..., "--disable", "plugins"]`.
- **Token effect:** medium, input side: −2,664 tokens/request (skills listing −3.7K chars, plugin instructions, `<recommended_plugins>` list, plugin-provided tools).
- **Accuracy:** neutral to helps (fewer irrelevant skills and tools to choose from). Fleet phase skills verified intact. Risk: a frontend worker that wanted the `browser` plugin loses it. If that matters, use per-plugin `-c 'plugins."<name>@<marketplace>".enabled=false'` for the rest (`plugins` is a keyed table in the schema).
- **Verdict:** RECOMMEND.

### 2. `features.apps` (`--disable apps`)
- **Controls:** ChatGPT app/connector integrations, the `<apps_instructions>` block, and (measured) the `<recommended_plugins>` list.
- **Verified by:** method 3 (`codex features list` → `apps  stable  true`) and method 2 (docs: "`features.apps` … Enable app (connector) integrations (stable; on by default)", learn.chatgpt.com/docs/config-file/config-reference).
- **Default:** `true`.
- **Recommended:** workers and orchestrator: `--disable apps`. Slack/Calendar connectors have no fleet use.
- **Where:** teammate `args:`.
- **Token effect:** small to medium, input side: −1,706 tokens/request alone, ≈ −2,900 combined with #1 (they overlap).
- **Accuracy:** neutral.
- **Verdict:** RECOMMEND.

### 3. `web_search`
- **Controls:** the native Responses `web_search` tool: `disabled | cached | indexed | live`.
- **Verified by:** method 2 (schema `WebSearchMode` enum; docs: "Web search mode (default: `"cached"` …)") and method 1 (`web_search` in the ConfigToml strings). Measured −2,456 tokens. The feature flags `web_search_cached` and `web_search_request` are **deprecated**, so use this top-level key.
- **Default:** `"cached"`.
- **Recommended:** codex-terra ("executes exactly what is asked") `-c web_search="disabled"`. codex-sol: optional (TUNE). Orchestrator and any researcher-type codex teammate: keep the default.
- **Where:** teammate `args: [..., "-c", "web_search=\"disabled\""]`.
- **Token effect:** medium, input side (the tool definition is large: `web_search_tool_type = text_and_image`).
- **Accuracy:** risks lower accuracy on tasks that need current external docs. Implementation workers get their facts from the brief and the repo, and the fleet has researcher roles for lookups.
- **Verdict:** TUNE CAREFULLY (RECOMMEND for codex-terra).

### 4. `model_reasoning_effort` (existing horch lever: teammate `effort:`)
- **Controls:** Responses API reasoning effort.
- **Verified by:** method 2 (docs; schema `ReasoningEffort`), method 1 (catalog via `codex debug models`), and the live API error listing accepted values.
- **Default:** catalog default is sol `low` and terra `medium`. The **effective value today is `medium` for both**, because neither `codex-sol.md` nor `codex-terra.md` sets `effort:` and the shared `~/.codex/config.toml` sets `model_reasoning_effort = "medium"`.
- **Recommended:** set `effort:` explicitly in both teammate files so the fleet no longer drifts with the operator's personal default. `medium` keeps current behaviour. `low` for codex-terra is a candidate to trial, not a default. Never `minimal` (hard API error), never `none` (disables thinking, SKIP by policy), and avoid `ultra` for workers (client-side automatic delegation multiplies spend). The orchestrator's `xhigh` is fine.
- **Where:** teammate `effort:` (horch already maps it to `-c model_reasoning_effort=…`).
- **Token effect:** output/reasoning side. Size by level is **not measurable from my probe**: a 41-person Josephus question gave 63–289 reasoning tokens across levels with no monotonic trend. Every level answered correctly, so the task was too easy to separate levels.
- **Accuracy:** a higher level helps on hard tasks. Pinning the level keeps quality stable when the operator's setting changes.
- **Verdict:** TUNE CAREFULLY (make it explicit, and validate `effort` against the model's accepted list).

### 5. `mcp_servers.<name>.enabled = false` for the operator's node_repl and blender
- **Controls:** per-server MCP enablement. The operator's config defines `node_repl` (ChatGPT app browser REPL, `startup_timeout_sec = 120`) and `blender`. Both load in every codex worker because config.toml is shared.
- **Verified by:** method 2 (schema `mcp_servers.*.enabled`; the operator's own config already uses `enabled = false` on another server) and measurement (−780 tokens/request).
- **Default:** enabled (as configured by the operator).
- **Recommended:** workers `-c mcp_servers.node_repl.enabled=false -c mcp_servers.blender.enabled=false`. Orchestrator: operator's choice.
- **Where:** teammate `args:`. The server names are operator-specific, so this belongs in the operator's teammate files (or a profile), not in horch defaults.
- **Token effect:** small, input side, −780 tokens/request (measured). I did not establish why it is this small (see A.5 #9). The bigger win is startup: two fewer MCP processes per codex pane.
- **Accuracy:** neutral for coding work.
- **Verdict:** RECOMMEND.

### 6. `features.image_generation` (`--disable image_generation`)
- **Controls:** the image-generation tool.
- **Verified by:** method 3 (`codex features list` → `image_generation  stable  true`) and measurement.
- **Default:** `true`.
- **Recommended:** workers `--disable image_generation` (a designer-type codex teammate would keep it).
- **Where:** teammate `args:`.
- **Token effect:** small, input side, −435 tokens/request.
- **Accuracy:** neutral for code work.
- **Verdict:** RECOMMEND.

### 7. `model_auto_compact_token_limit` (+ `model_auto_compact_token_limit_scope`)
- **Controls:** the token count at which Codex auto-compacts history.
- **Verified by:** method 1 (ConfigToml strings) and method 2 (schema: "Token usage threshold triggering auto-compaction of conversation history"; docs; source `protocol/src/openai_models.rs::auto_compact_token_limit`).
- **Default:** 90% of the resolved context window, so **244,800 tokens** for sol, terra, and astra (272,000 × 9/10). In `total` scope a configured value is `min(config, 90% of window)`, which means it can only lower the trigger, not raise it. A hard cap also applies at `effective_context_window_percent` (95%) = 258,400.
- **Recommended:** leave at the default for now. If long codex sessions prove costly, trial `-c model_auto_compact_token_limit=200000` on codex-terra first.
- **Where:** teammate `args:`.
- **Token effect:** medium on long sessions only (every request re-sends the whole context, mostly as cached reads).
- **Accuracy:** risk. Earlier compaction replaces detail with a summary.
- **Verdict:** TUNE CAREFULLY.

### 8. `tool_output_token_limit`
- **Controls:** per-tool-output token budget stored in history.
- **Verified by:** method 1 (ConfigToml strings) and method 2 (schema: "Token budget applied when storing tool/function outputs in the context manager"; source `models-manager/src/model_info.rs` maps it onto the model's `truncation_policy`). Truncation inserts a visible "tokens truncated" marker (string present in the binary), so it is not silent.
- **Default:** 10,000 tokens (catalog `truncation_policy: {mode: tokens, limit: 10000}` for all three models).
- **Recommended:** default for now. `6000` is a trial value for codex-terra if test and build logs dominate its context.
- **Where:** teammate `args:`.
- **Token effect:** small to medium, tool-output side, only on large outputs.
- **Accuracy:** risk of losing the relevant tail or middle of long logs. The model can re-run the command with `tail`/`grep`.
- **Verdict:** TUNE CAREFULLY.

### 9. `history.persistence`
- **Controls:** whether prompts are appended to `$CODEX_HOME/history.jsonl` (records are `{session_id, ts, text}`, the up-arrow prompt history). This is not what resume uses; resume reads rollouts under `sessions/`, which the ledger harvests.
- **Verified by:** method 2 (schema `History.persistence: save-all | none`, default `save-all`; docs "History Persistence").
- **Default:** `"save-all"`. The file is shared through the symlink: 77 of its current 480 entries are fleet briefings.
- **Recommended:** workers `-c history.persistence="none"`. Orchestrator: leave it (the operator may want to recall it).
- **Where:** teammate `args:`.
- **Token effect:** none.
- **Accuracy:** neutral. It keeps fleet briefings out of the operator's personal history.
- **Verdict:** RECOMMEND.

### 10. `notify`
- **Controls:** an external command spawned on turn completion. The operator's shared config runs a Computer Use client on every turn end, so today it fires for every codex worker turn.
- **Verified by:** method 2 (schema `notify`; docs "Notifications & Telemetry").
- **Default:** unset upstream. The operator's config sets it.
- **Recommended:** workers `-c notify=[]` if the operator does not want desktop notifications from fleet panes.
- **Where:** teammate `args:`.
- **Token effect:** none (one process spawn per turn per pane).
- **Accuracy:** neutral.
- **Verdict:** TUNE CAREFULLY (it is the operator's setting, so ask).

**Composite args for codex-terra (all verified individually; the composite was not launch-tested):**
`args: ["--dangerously-bypass-hook-trust", "--disable", "apps", "--disable", "plugins", "--disable", "image_generation", "-c", "web_search=\"disabled\"", "-c", "mcp_servers.node_repl.enabled=false", "-c", "mcp_servers.blender.enabled=false", "-c", "history.persistence=\"none\""]`, plus `effort: medium`. codex-sol is the same without `web_search`.

## A.5 Codex: unverified / folklore (not in the top list)

1. **Cost per effort level.** Reasoning tokens are billed as output. My single-sample probe could not separate levels, and I found no installed source with per-level multipliers.
2. **`service_tier = "flex"`.** The schema description mentions `flex`, but none of these models advertises it (`service_tiers` lists only `priority` = "Fast … increased usage"). Availability under ChatGPT auth is unknown.
3. **Whether the operator's TUI `/fast` toggle persists `service_tier` into the shared config.toml.** If it does, every codex worker would silently switch to priority tier (1.5× usage for sol/terra). I did not test it, to avoid writing to the operator's config. `-c service_tier="default"` does work on terra (verified live) if a guard is wanted.
4. **No lever found to stop codex scanning `~/.agents/skills` (23 skills) and `$CODEX_HOME/skills/.system` into every worker.** `--enable skip_host_skill_discovery` (under development) had no effect in my test.
5. **Nested tables inside a `-p` profile file** (`[features]`, `[mcp_servers.x]`). The docs only say "top-level keys".
6. **`--disable multi_agent` has no effect.** The multi-agent v2 block (2,264 chars) stays. That appears to come from the model catalog (`multi_agent_version: v2`), and I found no config key to remove it.
7. **`personality`** (the operator sets `pragmatic`). It may change the base-instruction size. Not measured.
8. **`compact_prompt` / `experimental_compact_prompt_file`.** A leaner compaction prompt might save tokens, but quality is unmeasured.
9. **Why the MCP saving (A.4 #5) is only 780 tokens.** One guess is that MCP tools are deferred or summarized. Not established: `tool_search` shows as *removed* in `codex features list`.

## A.6 Codex: skip list

| item | reason |
|---|---|
| `model_reasoning_effort = "none"` | Disables thinking (policy). |
| effort `minimal` | Hard API error on sol/terra (verified). |
| effort `ultra` for workers | Client-side automatic task delegation multiplies spend. Reserve it for explicit orchestrator use. |
| `service_tier = "priority"` / `"fast"` | "Increased usage" (1.5× sol/terra, "2x speed, increased usage" astra) for latency, not quality. |
| `include_permissions_instructions = false` | Only 584 chars under fleet auto perms, and the model needs to know its sandbox. |
| `include_environment_context = false` | 904 chars. cwd, date, and shell are useful facts. |
| `include_apps_instructions = false` | Subsumed by `--disable apps`. |
| `shell_environment_policy.inherit = "core"` / `"none"` | **Breaks the fleet.** Default is `inherit = all`, `ignore_default_excludes = true` (source `config/src/shell_environment_policy.rs:135-136`). horch run from inside a codex worker reads `HERDR_PANE_ID`, `HORCH_WORKSPACE_ID`, `HORCH_STATE_DIR`, and `HORCH_PROJECT_DIR`, and `core` would strip them, so `horch tell/note/done` would fail. No token effect either, since the env is not in context. |
| `shell_environment_policy.ignore_default_excludes = false` | Security hygiene only (drops `*KEY*`/`*SECRET*`/`*TOKEN*` vars). Could break `gh` or other tools that read tokens from env. No token effect. |
| `model_context_window` raised toward 872,000 | Bigger per-request bills and long-context degradation. |
| `model_verbosity` | Already `low` by catalog default, so nothing to cut. |
| `model_reasoning_summary` | Already `none` by catalog default. |
| `hide_agent_reasoning` / `show_raw_agent_reasoning` | UI only. No token or accuracy effect. |
| `disable_response_storage` | Does not exist in 0.154.0 (absent from the ConfigToml strings and schema). |
| `project_doc_max_bytes` (lowering) | Default 32,768. Lowering it silently truncates AGENTS.md (policy). This repo has no AGENTS.md anyway. |
| `--disable` multi_agent / browser_use / computer_use / goals / sleep_tool | Measured 0 token change. |
| `check_for_update_on_startup`, `CODEX_HOME`, sandbox/approval flags | Already set by horch (`launch.rs:354`, `codex.rs:77`, `teammates.rs:309`). |
| `OPENAI_BASE_URL`, `CODEX_OSS_*`, `CODEX_SQLITE_HOME` | No token or accuracy effect for ChatGPT-auth workers. |
| `analytics.enabled`, `feedback.enabled`, `otel` | Privacy/telemetry only. No token effect. |
| `codex exec --ignore-user-config` | exec-only (horch launches the TUI), and it drops the operator's tuned settings wholesale (policy). |
| `memories` feature | Already off (`memories  stable  false`). |

---

# Part B: OpenCode

## B.0 Version and binary

- `opencode --version | head -1` → **`1.18.2`**
- Binary: `/Users/mascott/.opencode/bin/opencode` (Mach-O arm64, a Bun-compiled single file with the JS bundle embedded; not a symlink). Paths from `opencode debug paths`: config `~/.config/opencode`, state `~/.local/state/opencode`, cache `~/.cache/opencode`.
- Models (from `opencode models opencode --verbose`):

| model | context | input | output | variants | cost |
|---|---|---|---|---|---|
| opencode/nemotron-3.5-lightning-free | 262,144 | – | 262,144 | **{}** | 0 |
| opencode/big-pickle | 200,000 | 160,000 | 32,000 | **{}** (hard-coded `return {}` in the bundle's variant transform) | 0 |
| opencode/nemotron-3-ultra-free | 1,000,000 | – | 128,000 | **{}** | 0 |

Because these models cost $0, "cheaper in tokens" here means less context pressure, fewer compactions, lower latency, and less data sent to endpoints that train on it. It does not mean dollars.

## B.1 Raw enumeration

- `strings <binary> | grep -oE 'OPENCODE_[A-Z0-9_]+' | sort -u` → **82 names** (one is the artefact `OPENCODE__`).
- Present in the binary but **not** in the docs env table: `OPENCODE_DISABLE_EXTERNAL_SKILLS`, `OPENCODE_PURE`, `OPENCODE_DISABLE_SHARE`, `OPENCODE_DISABLE_PROJECT_CONFIG`, `OPENCODE_DISABLE_EMBEDDED_WEB_UI`, `OPENCODE_DISABLE_CHANNEL_DB`, `OPENCODE_DISABLE_FFF`, `OPENCODE_EXPERIMENTAL_CODE_MODE`, and `OPENCODE_EXPERIMENTAL_REFERENCES`, among others. `OPENCODE_DISABLE_EXTERNAL_SKILLS` is described in the built-in `customize-opencode` skill text shipped inside the binary.
- Help: `opencode --help` (the TUI default command has **no `--variant`**), `opencode run --help` (has `--variant`, `--thinking`, `--format json`), `opencode debug {config,skill,agent,paths}`, `opencode mcp list`, `opencode models --verbose`.
- Docs: https://opencode.ai/docs/cli/ (env table), https://opencode.ai/docs/config/ (precedence, compaction, share, autoupdate, snapshot, instructions, tools, mcp), https://opencode.ai/docs/agents/, https://opencode.ai/docs/models/, https://opencode.ai/docs/zen/.

**Precedence (docs/config):** remote → global `~/.config/opencode/opencode.json` → `OPENCODE_CONFIG` → project `opencode.json` → `.opencode/` → **`OPENCODE_CONFIG_CONTENT`** → managed. The fleet's inline JSON therefore beats a project's own `opencode.json`. That matters for public or OSS repos that ship one. Unknown top-level keys are rejected with `ConfigInvalidError` (built-in customize-opencode skill text), so a typo in the teammate JSON fails the launch.

## B.2 Measurements

I made no live model calls. Every baseline request would have sent the operator's personal skill descriptions to a free endpoint that may train on them. Everything below is measured locally, as chars (≈4 chars/token).

| what | measurement |
|---|---|
| Skill listing a worker sees (`opencode debug skill --pure`, with horch's `skills.paths` bundle in `OPENCODE_CONFIG_CONTENT`) | 44 skills: 23 `~/.agents/skills`, 16 `~/.claude/skills`, 4 fleet bundle, 1 built-in. ≈13,346 chars of name + description |
| … with `OPENCODE_DISABLE_CLAUDE_CODE_SKILLS=1` | 28 skills (the 16 `~/.claude` ones gone), ≈4,885 chars. Fleet bundle intact. (The char count was measured with the broad `OPENCODE_DISABLE_CLAUDE_CODE=1`, which removes the same 16 skills. `_SKILLS=1` alone was verified without the bundle: 40 → 24.) |
| … with `OPENCODE_DISABLE_EXTERNAL_SKILLS=1` | 5 skills (4 fleet + built-in), ≈819 chars. **Fleet bundle intact** |
| MCP under `--pure` (`opencode mcp list --pure`) | context7 and playwright both **connected**. `--pure` does not drop MCP |
| MCP tool schema sizes (direct `tools/list`) | playwright 26 tools / 21,080 chars. context7 2 tools / 4,937 chars |
| `OPENCODE_CONFIG_CONTENT='{"mcp":{"playwright":{"enabled":false}}}'` | `opencode mcp list` shows playwright `disabled`, context7 connected (verified) |

## B.3 OpenCode top-10

| # | setting | verdict | token effect | landing |
|---|---|---|---|---|
| 1 | `OPENCODE_DISABLE_CLAUDE_CODE_SKILLS=1` | RECOMMEND | ≈ −8.5K chars (~2.1K tok)/request | teammate `env:` |
| 2 | `mcp.playwright.enabled = false` | RECOMMEND (non-UI workers) | ≈ −21K chars (~5K tok)/request | `OPENCODE_CONFIG_CONTENT` in teammate `env:` |
| 3 | teammate `effort:` → `--variant` is inert | TUNE CAREFULLY (fix the lever) | none today | teammate file / horch |
| 4 | `compaction.prune = true` | TUNE CAREFULLY | long sessions: large, input side | `OPENCODE_CONFIG_CONTENT` |
| 5 | `small_model` (per worker) | RECOMMEND | removes one paid off-tier call per session | `OPENCODE_CONFIG_CONTENT` |
| 6 | `permission.doom_loop = "deny"` | TUNE CAREFULLY | caps runaway identical tool loops | `OPENCODE_CONFIG_CONTENT` |
| 7 | `OPENCODE_DISABLE_AUTOUPDATE=1` | RECOMMEND | none (stability) | teammate `env:` |
| 8 | `OPENCODE_DISABLE_SHARE=1` (or `"share":"disabled"` pinned) | RECOMMEND | none (data guard) | teammate `env:` |
| 9 | `OPENCODE_DISABLE_EXTERNAL_SKILLS=1` | TUNE CAREFULLY (operator decides) | ≈ −12.5K chars (~3.1K tok)/request | teammate `env:` |
| 10 | `tool_output.max_bytes` / `max_lines` | TUNE CAREFULLY | tool-output side | `OPENCODE_CONFIG_CONTENT` |

### 1. `OPENCODE_DISABLE_CLAUDE_CODE_SKILLS`
- **Controls:** loading of `.claude/skills` (global `~/.claude/skills` and project) into OpenCode's skill tool listing.
- **Verified by:** method 2 (docs/cli: "Disable loading `.claude/skills`"), method 1 (bundle: `disableClaudeCodeSkills`), and measurement (16 skills removed).
- **Default:** unset, so Claude skills are loaded.
- **Recommended:** all three opencode workers `1`. The orchestrator is not opencode.
- **Where:** teammate `env: {OPENCODE_DISABLE_CLAUDE_CODE_SKILLS: "1"}`.
- **Token effect:** medium, input side, ≈8.5K chars (~2.1K tokens) per request.
- **Accuracy:** helps. The 16 skills are Claude-Code-specific or personal: `herdr-orchestrator` (orchestrator instructions offered to a worker), `herdr-worker`, docx/xlsx/pptx/pdf, morning brief, anki, a meeting-talk writer, and others. It also stops the operator's personal skill descriptions going to endpoints that train on input. Prefer this over the broad `OPENCODE_DISABLE_CLAUDE_CODE`, which also drops the **project `CLAUDE.md`** instruction fallback. The bundle builds the instruction list as `["AGENTS.md", ...(!disableClaudeCodePrompt ? ["CLAUDE.md"] : []), "CONTEXT.md"]`, and losing that hurts accuracy in repos that only have CLAUDE.md.
- **Verdict:** RECOMMEND.

### 2. `mcp.<name>.enabled = false` (playwright)
- **Controls:** whether an MCP server from the operator's global config starts and exposes tools.
- **Verified by:** method 1 (schema in bundle: `mcp: Record<string, Info | {enabled: boolean}>`, so a partial `{enabled:false}` is valid), method 2 (docs/config `mcp`), and a live check (`opencode mcp list` shows `playwright disabled`).
- **Default:** enabled (operator's global `opencode.json`). `--pure` does not affect it.
- **Recommended:** opencode-pickle, opencode-lightning, opencode-ultra: `{"mcp":{"playwright":{"enabled":false}}}`. Keep context7, because docs lookup helps accuracy and costs ~1.2K tokens.
- **Where:** teammate `env: {OPENCODE_CONFIG_CONTENT: '{"mcp":{"playwright":{"enabled":false}}}'}`. horch's `apply_env` reads the teammate's value first and merges `skills.paths` into it (`skills.rs:230-241`).
- **Token effect:** large for these workers, input side, ≈21K chars of tool schema (~5K tokens) per request.
- **Accuracy:** neutral for non-UI work. A UI/QA free worker would lose browser automation.
- **Verdict:** RECOMMEND.

### 3. OpenCode `effort` → `--variant` (existing horch lever)
- **Controls:** provider reasoning variant.
- **Verified by:** method 3 (`opencode --help` lists no `--variant` for the TUI; `opencode run --help` does), method 1 (TUI handler `args:{continue,sessionID,agent,model,prompt,fork,auto}`; big-pickle hard-coded `return {}`), and `opencode models --verbose` (`variants: {}` for all three). `opencode --variant zzz --version` exits 0, so the flag is silently swallowed.
- **Default:** no variant.
- **Recommended:** treat `effort:` on `opencode-*` teammates as having no effect. The comments in `opencode-lightning.md` ("Built for speed, so `effort: minimal`") and `effort: high` on pickle and ultra describe behaviour that does not happen. The orchestrator can either drop `effort` from those files or have horch reject or warn on `effort` for TUI-launched opencode. Agent-level `variant` exists in the schema (`agent.<name>.variant`, "applies only when using the agent's configured model"), but with no variants defined for these models it would also do nothing.
- **Where:** teammate files / horch roster check (orchestrator's call; no change made).
- **Token effect:** none today.
- **Accuracy:** neutral today. The risk is a false belief that lightning is tuned for speed and ultra for depth.
- **Verdict:** TUNE CAREFULLY (fix or document the lever).

### 4. `compaction.prune`
- **Controls:** pruning of old tool outputs. Outputs older than the most recent 40,000 tokens of tool output are cleared once at least 20,000 tokens are prunable (bundle constants `PRUNE_PROTECT = 40000`, `PRUNE_MINIMUM = 20000`, with `skill` outputs protected).
- **Verified by:** method 1 (schema: "Enable pruning of old tool outputs (default: false)"), method 2 (docs/config compaction: "`prune` - Remove old tool outputs to save tokens (default: `false`)"), and a live check (`opencode debug config` shows `compaction.prune: true` when set).
- **Default:** **`false`** in 1.18.2. `OPENCODE_DISABLE_PRUNE` only forces it false, so it is a no-op today.
- **Recommended:** trial `true` on opencode-ultra first. Its auto-compaction trigger is 968,000 tokens (B.4), so pruning is the only thing that keeps its context lean. Then consider it for pickle and lightning.
- **Where:** `OPENCODE_CONFIG_CONTENT` `{"compaction":{"prune":true}}`.
- **Token effect:** large on long sessions, input side. Nothing on short tasks.
- **Accuracy:** risk: the model loses old tool outputs and may re-read files. Mitigated by the 40K protected tail.
- **Verdict:** TUNE CAREFULLY.

### 5. `small_model` (per worker)
- **Controls:** the model used for title generation (and other small tasks). `Provider.getSmallModel` returns `cfg.small_model` first, and the title agent uses `agent.title.model ?? getSmallModel(...)` (bundle).
- **Verified by:** method 1 (bundle code above), method 2 (docs/config "Small Model"), and `opencode debug config` (the operator's global `small_model` is a paid Bedrock model).
- **Default:** the operator's global `small_model` (a paid Bedrock Qwen model). So each free-tier worker session makes at least one **paid** off-tier call carrying its first message, the horch briefing.
- **Recommended:** each opencode worker sets `small_model` to its own free model (e.g. `"opencode/nemotron-3.5-lightning-free"`) in its `OPENCODE_CONFIG_CONTENT`. `agent.title.model` is the narrower alternative. Don't touch the operator's global.
- **Where:** `OPENCODE_CONFIG_CONTENT`.
- **Token effect:** small (one call per session), but it removes paid tokens and keeps worker data on one provider.
- **Accuracy:** neutral (titles only).
- **Verdict:** RECOMMEND.

### 6. `permission.doom_loop = "deny"`
- **Controls:** OpenCode's loop guard. When the last **3** tool calls are the same tool with identical input (bundle constant `yi = 3`), it raises a `doom_loop` permission request. horch runs workers with `--auto` ("auto-approve permissions that are not explicitly denied"), and the operator's config sets `doom_loop: "ask"`, so the guard is auto-approved and loops continue.
- **Verified by:** method 1 (bundle `r.ask({permission:"doom_loop", …})` and `--auto` description), plus the operator's config key.
- **Default:** `ask`, which `--auto` approves.
- **Recommended:** trial `{"permission":{"doom_loop":"deny"}}` on the free workers. Free models are the ones most likely to loop.
- **Where:** `OPENCODE_CONFIG_CONTENT` (or `OPENCODE_PERMISSION`, docs/cli "Inlined json permissions config").
- **Token effect:** caps runaway input and output from repeated identical calls. The size depends on how often loops happen, which I did not measure.
- **Accuracy:** helps when stuck (forces a new approach). Risk: a legitimately repeated identical call, such as polling the same command three times, gets denied.
- **Verdict:** TUNE CAREFULLY.

### 7. `OPENCODE_DISABLE_AUTOUPDATE` (or `"autoupdate": false`)
- **Controls:** automatic download of new versions at startup.
- **Verified by:** method 2 (docs/cli and docs/config "Autoupdate") and method 1.
- **Default:** auto-update on. The operator's global config does not set `autoupdate`.
- **Recommended:** workers `1`. Upgrades should happen deliberately, outside a running fleet.
- **Where:** teammate `env:`.
- **Token effect:** none.
- **Accuracy:** neutral. Prevents mid-fleet version drift between panes.
- **Verdict:** RECOMMEND.

### 8. `OPENCODE_DISABLE_SHARE` (or pin `"share": "disabled"` in the fleet JSON)
- **Controls:** session sharing to opencode.ai.
- **Verified by:** method 1 (bundle: `OPENCODE_DISABLE_SHARE==="true"||==="1"`) and method 2 for `share` (docs/config: `"manual"` default, `"auto"`, `"disabled"`).
- **Default:** upstream `manual`. **The operator's global already has `share: "disabled"`**, but a project `opencode.json` (common in OSS repos, which are exactly where the free tier works) overrides the global.
- **Recommended:** workers `OPENCODE_DISABLE_SHARE=1` (env, beats any config).
- **Where:** teammate `env:`.
- **Token effect:** none.
- **Accuracy:** neutral. A data-leak guard.
- **Verdict:** RECOMMEND. This is belt-and-braces on top of the operator's setting, not a new behaviour.

### 9. `OPENCODE_DISABLE_EXTERNAL_SKILLS`
- **Controls:** skips the external skill scans under `~/.claude/` and `~/.agents/` (built-in skill text). Config `skills.paths` still load.
- **Verified by:** method 1 (binary and the built-in customize-opencode skill), plus measurement (5 skills left, fleet bundle intact). **Not in the docs env table.**
- **Default:** unset.
- **Recommended:** the operator decides. This matches horch's "phase skills only" intent, but it also removes the operator's own cross-harness `~/.agents/skills` (23 `dev-*` skills) for that worker. The token-economy memory says to keep the operator's own skills unless asked. #1 is the narrow default, and this is the bigger opt-in cut.
- **Where:** teammate `env:`.
- **Token effect:** medium, input side, ≈12.5K chars (~3.1K tokens) per request (includes #1's saving).
- **Accuracy:** neutral to helps (less noise), with some risk of losing a useful `dev-*` skill.
- **Verdict:** TUNE CAREFULLY.

### 10. `tool_output.max_bytes` / `tool_output.max_lines`
- **Controls:** the truncation threshold for a single tool output. Over the limit, the full text is written to disk and the model gets a preview (not silent).
- **Verified by:** method 1 (schema: "Maximum lines … (default: 2000)", "Maximum bytes … (default: 51200)", "the full text is written to the truncation directory and a preview is returned").
- **Default:** 2000 lines / 51,200 bytes (≈12.8K tokens per output).
- **Recommended:** default for now. Trial `max_bytes: 24000` on opencode-lightning (bulk mechanical edits) if logs dominate.
- **Where:** `OPENCODE_CONFIG_CONTENT` `{"tool_output":{"max_bytes":24000}}`.
- **Token effect:** small to medium, tool-output side.
- **Accuracy:** risk of the model working from a preview when the detail was further down. It can read the saved file.
- **Verdict:** TUNE CAREFULLY.

**Composite teammate `env:` for an opencode free worker (each key verified individually; the composite was not launch-tested):**
```yaml
env:
  OPENCODE_DISABLE_CLAUDE_CODE_SKILLS: "1"
  OPENCODE_DISABLE_AUTOUPDATE: "1"
  OPENCODE_DISABLE_SHARE: "1"
  OPENCODE_CONFIG_CONTENT: '{"mcp":{"playwright":{"enabled":false}},"small_model":"opencode/nemotron-3.5-lightning-free"}'
```
(Add `"compaction":{"prune":true}` and `"permission":{"doom_loop":"deny"}` after a trial. horch merges its `skills.paths` into this JSON.)

## B.4 OpenCode compaction defaults (derived from the bundle)

From `Is(e)` / `Dl(e)`: `maxOut = min(limit.output, OUTPUT_TOKEN_MAX)`, where `OUTPUT_TOKEN_MAX` defaults to 32,000 (`OPENCODE_EXPERIMENTAL_OUTPUT_TOKEN_MAX` overrides it). Compaction fires when total tokens reach:
- `limit.input − (compaction.reserved ?? min(20000, maxOut))` if the model declares `limit.input`, otherwise
- `limit.context − maxOut`.

| model | trigger (default) | does `compaction.reserved` apply? |
|---|---|---|
| nemotron-3.5-lightning-free | 262,144 − 32,000 = **230,144** | No (no `limit.input`) |
| big-pickle | 160,000 − 20,000 = **140,000** | Yes |
| nemotron-3-ultra-free | 1,000,000 − 32,000 = **968,000** | No |

Docs/config shows `reserved: 10000` in an example. Only big-pickle is affected by it.

## B.5 OpenCode: unverified / folklore

1. **The exact token size of the MCP tool definitions as OpenCode serializes them to the provider.** I measured raw `tools/list` JSON chars. OpenCode may transform schemas.
2. **Any trains-on-input opt-out for the free tier.** None is documented. docs/zen: Big Pickle "During its free period, collected data may be used to improve the model". NVIDIA free endpoints: "Your use is logged for security purposes and to improve NVIDIA products and services". Nothing to set. The existing `trains_on_input: true` restriction is the right control.
3. **Custom `variants` for these models via provider config.** The docs say you can add your own, but whether the Zen backend honours any reasoning parameter for these models is unknown.
4. **An AGENTS.md / instructions size cap.** I found none in the bundle, and the docs list none.
5. **`agent.<name>.steps`** (max agentic iterations, schema-verified). The default and whether it helps free workers were not measured, so it is not recommended.
6. **Free-tier rate limits.** None documented. The bundle has a "Free usage exceeded, subscribe to Go" message, so limits exist server-side.

## B.6 OpenCode: skip list

| item | reason |
|---|---|
| `OPENCODE_DISABLE_AUTOCOMPACT` / `compaction.auto=false` | Context overflow on long sessions (policy). |
| `OPENCODE_DISABLE_PRUNE` | Prune is already off by default, so no-op. It would also block #4. |
| `OPENCODE_DISABLE_CLAUDE_CODE` (broad) / `_PROMPT` | Also drops the project `CLAUDE.md` instruction fallback (verified in bundle). Use `_SKILLS` (#1). `~/.claude/CLAUDE.md` does not exist on this machine, so `_PROMPT` saves nothing today. |
| `provider.opencode.models.<id>.limit.context` lowered (e.g. ultra → 400,000) | Works (a partial override merges, verified: ultra shows `{context: 400000, output: 128000}` with other fields intact) and would move ultra's trigger to 368,000. But it defeats ultra's stated purpose ("hold a lot at once") and misstates the window. Prefer `prune` (#4). |
| `OPENCODE_EXPERIMENTAL_OUTPUT_TOKEN_MAX` | Changes the per-response cap and, inversely, the compaction trigger for lightning and ultra. Lowering it risks truncated reasoning responses. Experimental. |
| `OPENCODE_DISABLE_MODELS_FETCH` | Only saves a startup fetch. The free models are "limited time" and churn, so a stale catalog risks broken model IDs. |
| `OPENCODE_DISABLE_DEFAULT_PLUGINS` | Default plugins include provider auth. Launch-risk for no token gain. |
| `OPENCODE_DISABLE_LSP_DOWNLOAD` / `lsp: false` | LSP diagnostics after edits help accuracy. No meaningful token gain. |
| `snapshot: false` | No token effect. Loses in-session revert. |
| `mcp.context7.enabled=false` | ~1.2K tokens, and docs lookup helps accuracy. Keep it. |
| `--variant` / agent `variant` tuning | No variants exist for these three models (#3). |
| `OPENCODE_PURE`, `--session`, `OPENCODE_CONFIG_CONTENT` skills merge | Already set by horch. |
| `OPENCODE_DISABLE_PROJECT_CONFIG` | Removes the target repo's own config and instructions discovery wholesale. |
| `OPENCODE_ENABLE_EXA` / `OPENCODE_ENABLE_PARALLEL` (web search tools) | Adds tools (input tokens), and there is no evidence of an accuracy win for implementation workers. |

---

## Other observations for the orchestrator (outside the brief; no action taken)

- **Codex writes its `.system` skills into whatever `$CODEX_HOME/skills` resolves to, which is the horch skill bundle.** My own probe triggered this. A fake private home linked to *my* per-launch bundle, and at 07:40:14 codex created `<bundle>/skills/.system/` there (imagegen, openai-docs, plugin-creator, review-agent, skill-creator, skill-installer, plus a marker file). The bundle belongs to this Claude pane and is removed when it exits, so no other worker is affected. The same thing will happen in every real codex pane, because `attach_skills` points `skills` at the bundle. Those system skills then get listed under the `horch:` prefix instead of from `~/.codex/skills/.system`. That is roughly token-neutral (a relabel), but it means codex writes into the fleet bundle.
- `~/.local/state/horch/codex-home/` holds 72 private homes. A sampled one contained only `rules/`, so they look like homes left over from panes that were killed rather than cleaned up.
- All measurements depend on the operator's current installed plugins, skills, and MCP servers. Re-measure after changes with `codex debug prompt-input` (chars) and `codex exec --ephemeral --json` (tokens), and with `opencode debug skill --pure` and `opencode mcp list --pure`.
