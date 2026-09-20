# Side calls: evidence appendix for section 3.6

Date: 2026-09-18. Author: researcher-5. Brief: `ai_docs/plans/env-research/05-side-calls.md`.
Deliverable: section 3.6 of `ai_docs/plans_to_improve.md`. This file makes each row traceable.

Versions checked:

| harness | version | where |
|---|---|---|
| Claude Code | 2.1.276 (the one `claude` resolves to), also 2.1.274 | `~/.local/share/claude/versions/2.1.27{4,6}` (Bun single-file Mach-O) |
| Codex CLI | 0.154.0 | source tarball of tag `rust-v0.154.0` at `/tmp/codex-154-full/codex-rust-v0.154.0/codex-rs` |
| OpenCode | 1.18.2 | `~/.opencode/bin/opencode` (Bun single-file) |
| pi | 0.85.1 | `/opt/homebrew/lib/node_modules/@earendil-works/pi-coding-agent/` |
| Prime Agent | 0.9.4 | `/opt/homebrew/lib/node_modules/prime-agent/` (binary `prime-agent`) |

Method: static reading of the installed binaries and bundles (method 1 in the shared context), shipped docs, `--help`, and read-only commands (`codex features list`, `codex debug prompt-input`, `opencode debug agent`). No model calls were made. Server flag values come from `cachedGrowthBookFeatures` in `~/.claude.json` on 2026-09-18.

Claude Code offsets are byte offsets into the 2.1.276 binary. Minified names differ in 2.1.274; the same logic was found there by string match (listed per row).

---

## 1. Claude Code

### 1.0 Inventory of `querySource` literals

`grep -aoE 'querySource:"[a-z_0-9]+"' <bin> | sort -u` gives 32 names in 2.1.276 and 33 in 2.1.274 (2.1.274 also has `repl_sampling`):

`agent_classifier, agent_namer, agent_summary, artifact_comment_analyst, artifact_comment_fast_ack, artifact_comment_reply, artifact_comment_triage, auto_dream, auto_mode, auto_mode_critique, auto_mode_setup_propose, away_summary, compact, extract_memories, feedback, generate_session_title, hook_agent, hook_prompt, insights, mcp_datetime_parse, model_validation, narration, plugin_eval_judge, plugin_eval_mock, prompt_suggestion, rename_generate_name, sdk, side_question, teleport_generate_title, tool_use_summary_generation, web_fetch_apply, web_search_tool`.

One more source is set through a variable: `var KPe="memdir_relevance"` (memory recall selector, 1.8).

`rolling_compact` has **0 hits** in both binaries. It appears only as a name in the server's cache allowlist: `tengu_prompt_cache_1h_config.allowlist = ["repl_main_thread*","sdk","auto_mode","rolling_compact","memdir_relevance","agent_classifier","prompt_suggestion","away_summary","extract_memories"]` in `~/.claude.json`.

### 1.1 Env boolean parsing (applies to every env switch below)

@169945213: `function Oe(e){...return["1","true","yes","on"].includes(n)}` and `function To(e){...return["0","false","no","off"].includes(n)}`. Registry helpers @170401509: `bool:()=>...transform((n)=>Oe(n))`, `triBool:()=>...{if(Oe(n))return!0;if(To(n))return!1;return}`. `CLAUDE_CODE_FORK_SUBAGENT` is `triBool` (`es=O.triBool()`), so `false` and `0` both turn it off.

### 1.2 `agent_summary`: ON for background subagents

- Timer @183434638 (function `ks`): `var Ua=30000` (30 s), `[AgentSummary] Timer fired`, skips when fewer than 3 messages or the transcript is unchanged, then `Qb({promptMessages:[...ja(C)],cacheSafeParams:ie,...,querySource:"agent_summary",forkLabel:"agent_summary",maxTurns:1,skipTranscript:!0,skipCacheWrite:!0})`. Prompt: "Describe your most recent action in 3-5 words using present tense (-ing)". `cacheSafeParams` are the subagent's own, so it runs on the subagent's model with its full context.
- Started only by `YW({... enableSummarization:M ...})` @183471180: `oe=M?(le,xe)=>{let{stop:Pe}=ks(e,Vr(e),le,xe,h);G=Pe}:void 0` @183474725.
- Callers and their gates:
  - Agent tool, async path @183626655: `enableSummarization:Z_e()||(It||Rt)&&!Ce()`, with `It=si()` (coordinator mode) and `Rt=k5()&&!we` (fork-subagent gate on, caller is not an in-process teammate) @183615231.
  - Agent tool, sync path @183630307: `enableSummarization:Z_e()`.
  - Forked skill (`spawnedByForkedSkill:!0`) @183533445: `enableSummarization:!0`.
  - `fork` subagent @188148174: `enableSummarization:!0`.
  - Resume @192362293: `Z_e()||($4()||M||k5())&&!Ce()`.
  - Observer agent @200627495: `enableSummarization:!1`.
- `Z_e()` @170218054 is `sdkAgentProgressSummariesEnabled()` (SDK `agentProgressSummaries` option only). `Ce()` @170216326 is `!isInteractive()`.
- Fork gate @176965388: `function jbo(){if(a.CLAUDE_CODE_FORK_SUBAGENT===!0)return"env";if(Ce())return"disabled";return"default"}` and `function Wbo(){...if($4())return"disabled";if(a.CLAUDE_CODE_FORK_SUBAGENT===!1)return"disabled";...}`, `function k5(){return Wbo()!=="disabled"}`. So in an interactive pane the gate is on by default.
- Async decision @183603600: `g=n.isCoordinator&&!p||n.forceAsync||!p&&h!==!1`, `A=h===!0||e.background===!0||!Jb(e)&&g`, `shouldRunAsync:v||A&&!n.backgroundTasksDisabled`. Agent-tool subagents run in the background by default, so every Agent-tool subagent in a pane gets a summary fork every 30 s while it runs.
- Side effects of `CLAUDE_CODE_FORK_SUBAGENT=false` (all `k5()` call sites): the `fork` subagent type and its system-prompt paragraph go away (@176993019, @179813475, @183638368), the Agent tool schema shows `run_in_background` again (@183608076: `_c()||k5()?n.omit({run_in_background:!0}):n`), and the Explore hint is added to the prompt (@176994765). Subagents still run in the background by default (the `!p&&h!==!1` term).
- No horch phase skill uses `context: fork`: `grep -rl 'context: *fork'` over the skill bundle returns nothing. No operator plugin or user skill does either: `grep -rl --include=SKILL.md -E '^context: *fork' ~/.claude/plugins ~/.claude/skills` returns nothing.
- The forked-skill launcher `CDt()` (the function that holds the `enableSummarization:!0` @183533445) does not call `k5()` or `OGn()`. So `CLAUDE_CODE_FORK_SUBAGENT=false` does not stop summaries for a `context: fork` skill.
- Status: the operator decides. This research session itself spawned three `fork` agents, so fleet sessions do use the `fork` type that the switch removes.
- 2.1.274: `enableSummarization:Oye()||(Rt||Pt)&&!Te()`, `enableSummarization:!0` (twice), `enableSummarization:Oye()`, `enableSummarization:!1`. Same logic.

### 1.3 `agent_classifier` and `agent_namer`: OFF in a plain pane

- Classifier call @199720987 (chunk exports `classify`, `classifyAndPush`, `classifyAndPushDebounced`): `if(A&&!y)C="preclassify"...else if(u==="heuristic"){...}else{... ZP({querySource:"agent_classifier",model:oe,...})}`. The model is `Oe()` @199702548: `if(re()?.useSmallFastModel)return Dh();return PVt(nt())`, where the client default is `tengu_bg_classifier_config {useSmallFastModel:!0,disableThinking:!0,midTurnLlmDebounceMs:60000}`. `Dh()` is the small fast model (`ANTHROPIC_SMALL_FAST_MODEL` first, else the Haiku default).
- Namer @199710668: `ZP({querySource:"agent_namer",model:w,...,content:"2-4 word lowercase label for this job..."})`. It runs only from the classifier when `u==="llm"`.
- Surfaces @183766131: `function gQr(e){if(At())return new Set(["bg"]);...if((e??wbt())||CS()!==null)r.add("watched");if(gZt())r.add("ccr");if(...bridge...)r.add("bridge");...if(_Qr())r.add("cli");if(!Ce()&&Ma())r.add("repl");return r}`. Sinks: `{bg:["state"],watched:["state"],ccr:["summary"],bridge:["summary"],desktop:["summary"],cli:["summary"],repl:["headline"]}`. Engine @183767374: `e.has("state")?"llm":!e.has("summary")?"heuristic":CLAUDE_CODE_CLASSIFIER_SUMMARY...`. `_Qr()` returns `!1`.
- So a plain interactive pane has at most the `repl` surface, sink `headline`, engine `heuristic`, and no model call. The LLM engine starts only for `bg` sessions (`At()`, `Uq()==="bg"`) or `watched` ones.
- `watched` @172661910: `function wbt(){...Cue(xt(cj(),Zl))...s=n-g<PB}` with `Zl=".fleetview-heartbeat"`, `PB=5000`, and `function cj(){return Eo(Ee(),"sessions")}`. The session is watched when `~/.claude/sessions/.fleetview-heartbeat` was touched in the last 5 s, which happens while the agents view (FleetView) is open. The file does not exist on this host today.
- Main loop use @184063914: `if(Rn&&xr&&Tu&&Us(C)==="main"&&!M.agentId){...}` then `Rn().classifyAndPushDebounced(...)`.
- There is no client env var or setting that turns off the `state` engine. The server-side knobs are `tengu_classifier_disabled_surfaces` (cached `""`) and `tengu_cobalt_wren` (cached `false`). `CLAUDE_CODE_CLASSIFIER_SUMMARY` affects only the `summary` sink.
- 2.1.274: `engineFor` (5 hits) and `fleetview-heartbeat` (2 hits) are present.

### 1.4 `generate_session_title`: ON in every interactive pane

- Generator chunk @183680421: `function yAr(){return kt()||a.CLAUDE_CODE_DISABLE_TERMINAL_TITLE}`, `async function rJ(r,l,s){let n=r.trim();if(n.length<g)return null;...w({systemPrompt:y,content:n,...})}` with `g=10`. `w()` calls `Rk({...options:{querySource:"generate_session_title",...}})`.
- `Rk` @178698622: `Pse({...,thinkingConfig:{type:"disabled"},tools:[],options:{...,model:Dh(),enablePromptCaching:g.enablePromptCaching??!1,...}})`. So the small fast model is used, with no prompt caching.
- `Dh()` 2.1.276: `function Dh(){let e=a.ANTHROPIC_SMALL_FAST_MODEL;if(e!==void 0)return qv(e);...}`.
- Interactive trigger @196664583 (`_runImpl`): `let{disabled:In,sessionTitle:$n,aiSessionTitle:Ho,agentTitle:Mn}=Ct.getSnapshot();if(!In&&!$n&&!Ho&&!Mn&&!this._haikuTitleAttempted){...this._engine.generateSessionTitle(dr,...)`. It fires on the first real user prompt, which is the horch briefing.
- Store construction @196927000: `new Nte({session:Dc,store:ll,sessionController:ku,scope:Ml,disabled:a.CLAUDE_CODE_DISABLE_TERMINAL_TITLE})`. This is the switch. The same env var also stops the terminal-title updates.
- Headless and SDK path @192909116: `fh(yd, <has messages> || yAr())`, so `-p` sessions also honor the env var (and `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` through `kt()`, which is SKIP by policy in `claude-code.md` Headline 5).
- Unverified alternative: `claude --help` lists `-n, --name <name>` ("Set a display name for this session"). The trigger skips when the session already has a custom title (`sessionTitle`). I did not confirm that `--name` fills that field, so the table names only the env var.
- Other callers: remote sessions (@195638321, `useRemoteSession`), bridge / Remote Control (@202493285), SDK control request (@184282427). None applies to a horch pane.
- 2.1.274: `disabled:a.CLAUDE_CODE_DISABLE_TERMINAL_TITLE` (1 hit).

### 1.5 Away summary: default ON, OFF here only through the operator's setting

- @183770416: `function gIe(){let e=process.env.CLAUDE_CODE_ENABLE_AWAY_SUMMARY;if(To(e))return!1;if(Oe(e))return!0;if(Ce())return!1;if(Ke()?.awaySummaryEnabled===!1)return!1;return!0}`. The env var is read first, so `CLAUDE_CODE_ENABLE_AWAY_SUMMARY=false` wins whatever settings are loaded.
- Call: `Qb({...querySource:"away_summary",forkLabel:"away_summary",maxTurns:1,skipCacheWrite:!0,...})`, a fork of the main conversation.
- No Claude teammate sets `setting_sources` today (`grep setting_sources teammates/*.md` finds only `_template.md`), so the operator's `awaySummaryEnabled:false` loads in every pane.
- `CLAUDE_CODE_ENABLE_REMOTE_RECAP` / `tengu_harbor_moth` feed a CCR-only recap (`Sot()` returns unless `gZt()`). Not reachable in a pane.
- 2.1.274: `CLAUDE_CODE_ENABLE_AWAY_SUMMARY;if(Ho(e))return!1;if(He(e))return!0;if(Te())return!1;if(qe()?.awaySummaryEnabled===!1)return!1;return!0`.

### 1.6 Background memory extraction: OFF, and no extraction-only switch

- Gate @172720329: `function ERe(){if(wRe()!==null)return!0;if(!P("tengu_passport_quail",!1))return!1;return!Ce()||P("tengu_slate_thimble",!1)}`.
- Per-turn path @182832847: `if(!M&&!P("tengu_passport_quail",!1))return;if(!M&&!Sa())return;if(!M&&x2())return;if(sr()!==null)return;...`. Cached `tengu_passport_quail=false`.
- Client switches: `Sa()` @172719476 is `uqt()`: `let e=process.env.CLAUDE_CODE_DISABLE_AUTO_MEMORY;if(Oe(e))return!1;if(To(e))return!0;...if(n.autoMemoryEnabled!==void 0)return n.autoMemoryEnabled;return!0`. `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1` equals `autoMemoryEnabled:false`: it turns off all auto memory, reads included. There is no switch that stops only extraction.
- `CLAUDE_CODE_POST_TURN_MEMORY` (+ `_CONFIG`, `_SYNC`) is an opt-in CCR memory-server path (`wRe()`), off unless set.

### 1.7 `auto_dream` (memory consolidation): OFF

- @auto_dream call: `Qb({...querySource:"auto_dream",forkLabel:"auto_dream",...})`. Gate `Wn()`: `if(sr()!==null)return!1;if(rO())return!1;if(!Sa())return!1;if(x2())return!1;return tHe()`.
- @182842244: `function en(){return P("tengu_onyx_plover",null)}`, `function Btt(){let e=en();return e?.enabled===!0||e?.available===!0}`, `function tHe(){if(!Btt())return!1;let e=Ke().autoDreamEnabled;if(e!==void 0)return e;return en()?.enabled===!0}`. Cached `tengu_onyx_plover={"enabled":false,"minHours":24,"minSessions":3,"remoteEnabled":false}`, so `Btt()` is false.

### 1.8 `memdir_relevance` (memory recall selector): OFF

- `QKn()` gate: `if(!s||n.agentId||!Sa()||!pj()||qxs.has(r))return;`, where `pj()` is `P("tengu_moth_copse",!1)||!!process.env.CLAUDE_MEMORY_STORES?.trim()`. Cached `tengu_moth_copse=false`; `CLAUDE_MEMORY_STORES` is unset in the operator shell.
- The LLM selector `txs()` calls `ZP({...querySource:KPe,...})` with "Select memories relevant to: ...", and runs only when the index path (`tengu_mill_orange`, cached `false`) is not taken.

### 1.9 Precomputed compaction and `rolling_compact`

- @178504070: `function j9(){if(!Hf())return!1;if(!LF())return!1;if(!P("tengu_sepia_moth",!1))return!1;return Po("precomputeCompactionEnabled",Eqe()).value}`. Cached `tengu_sepia_moth=false`, so precompute is OFF today. The `/config` row is shown only when that flag is on (@188526153).
- Settings schema @170867322: `precomputeCompactionEnabled:I().optional().describe("Precompute the compaction summary in the background before it is needed. Only applies when auto-compact is on.")`.
- `rolling_compact`: see 1.0.

### 1.10 Auto-mode permission classifier: ON wherever `permission_mode: auto`

- @179168697: `at={model:y,max_tokens:(B==="fast"?256:64)+lr,system:r,...,querySource:"auto_mode",...}`, called with `classifierModel:y`. The transcript is sent inside `<transcript>` tags.
- Subagent hand-back review strings: "Handoff classifier flagged sub-agent output", `classifierModel`, `classifierStage1Severity` (bytecode string table near offset 75.36M).
- `permission_mode: auto` is set in `architect-reviewer`, `backend-developer`, `designer`, `frontend-developer`, `opus`, `orchestration-worker`, `product-lead`, and the codex teammates (`grep -n permission_mode teammates/*.md`). The classifier model was not traced further.

### 1.11 Tool-use summaries: OFF (opt-in)

- @183994812: `gates:{emitToolUseSummaries:a.CLAUDE_CODE_EMIT_TOOL_USE_SUMMARIES,...}`. The generator @183925134 calls `Rk({...querySource:"tool_use_summary_generation",enablePromptCaching:!1,...})`. The env var is unset.

### 1.12 Sources that need a user command, a tool call or operator config

Context read before each `querySource` literal:

- `side_question`: `/btw`; its prompt says "Side questions cannot use tools".
- `rename_generate_name`: `/rename` with no argument. Prompt: "Generate a short kebab-case name...".
- `teleport_generate_title`: remote sessions and teleport.
- `insights`: `/insights` transcript-chunk summaries.
- `feedback`: `/feedback` title drafting.
- `model_validation`: `/model` check, `max_tokens:1`.
- `mcp_datetime_parse`: parses a date typed into an MCP elicitation form.
- `auto_mode_critique` and `auto_mode_setup_propose`: `claude auto-mode` subcommands.
- `plugin_eval_judge` and `plugin_eval_mock`: `claude plugin eval`.
- `artifact_comment_*`: artifact comments. The operator sets `disableArtifact: true`.
- `web_fetch_apply` and `web_search_tool`: run inside the WebFetch and WebSearch tools that the worker calls.
- `hook_prompt` and `hook_agent`: prompt-type and agent-type hooks. They run only if a hook of that type is configured.
- `compact`: auto-compaction and `/compact`.
- `sdk`: headless mode.

### 1.13 Prompt suggestion and narration (rows unchanged)

- Prompt suggestion: see `claude-code.md` #2. Cached `tengu_chomp_inflection=true`, `tengu_prompt_suggestion=true`. Env var `CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION` is in the registry.
- New input: the Codex fork found that herdr forwards pane focus events (herdr binary strings `focus_reporting`, "failed to forward pane focus event"). So a pane that reports focus-out may already skip the suggestion. The env switch is still needed for focused panes.
- Narration @narration: `ZP({model:h.options.mainLoopModel,system:Cro,...})`, gated by `CLAUDE_CODE_ENABLE_NARRATION` and `tengu_pewter_kite_ms` (cached `0`).

### 1.14 horch landing facts

- `crates/horch-core/src/launch.rs:288-318`: the Claude `--settings` overlay is assembled in Rust from a fixed set of keys (`enabledPlugins` off-switches, `statusLine`). A teammate `settings:` file replaces the whole overlay, so it drops the plugin off-switches. `--settings` is not repeatable, so `args:` cannot add a second one. Any new settings key therefore needs a Rust change, or a per-teammate settings file that copies the plugin list.
- Teammate `env:` is exported before the CLI starts. Env switches need no Rust change.
- `crates/horch-core/src/skills.rs:279` (`opencode_config`): the teammate's `OPENCODE_CONFIG_CONTENT` JSON passes through and horch adds only `skills.paths`. No Rust change is needed.

---

## 2. Codex CLI 0.154.0


Source: full tarball of tag `rust-v0.154.0` extracted at
`/tmp/codex-154-full/codex-rust-v0.154.0/codex-rs` (the `/tmp/codex-src` checkout
is a blobless sparse clone holding only `core/src`, `config`, `protocol/src`,
`models-manager`; `git grep` on it fetches blobs one by one and stalled, and
`tui/`, `app-server*`, `features/`, `ext/` are not checked out there).
Paths below are relative to `codex-rs/` in the tarball.
`codex --version` -> `codex-cli 0.154.0`.

Fleet launch facts (horch): codex teammates `codex-sol`, `codex-terra`,
`orchestrator-codex` all set `permission_mode: auto`, which horch maps to
`-s workspace-write -a never` (`crates/horch-core/src/teammates.rs:313`).
Launch args: `crates/horch-core/src/launch.rs:337-373` (`-c model=...`,
`-c check_for_update_on_startup=false`, `-c tui.resume_cwd="current"`,
`--dangerously-bypass-hook-trust`, permission args, `-c model_reasoning_effort=...`,
then teammate `args:`). Panes run the interactive TUI, not `codex exec`.
The private `CODEX_HOME` symlinks every entry of the real home except `rules/`
and `skills/` (`crates/horch-core/src/codex.rs:13-16`), so the operator's
`~/.codex/config.toml` applies in fleet panes.



### 2.1 Automatic thread title (ON, no switch)
- `tui/src/app/thread_title.rs:1-4`: "Structured-output schema and normalization for generated TUI thread titles. Automatic titles are persisted only after generation..."
- `tui/src/app/thread_title.rs:25-28`: `THREAD_TITLE_MAX_CHARS = 36`, `THREAD_TITLE_MODEL = "gpt-5.6-luna"`, `THREAD_TITLE_PROMPT_MAX_BYTES = 960`.
- `tui/src/app/thread_title.rs:67-81`: luna if `model_provider_id == "openai" && has_chatgpt_account() && catalog contains luna`, else `current_model()`; effort `Low` only for luna.
- `tui/src/app/thread_routing.rs:1954-1987`: trigger. `if self.chat_widget.thread_name().is_none()` and the event is `ItemCompleted(UserMessage)`, then `generate_thread_title(..., ThreadTitleDestination::Automatic, thread_title_prompt(&user_message))`. No feature or config check.
- `tui/src/app/event_dispatch.rs:2713-2729`: result saved with `thread_set_name` only if the thread still has no name.
- `tui/src/temporary_structured_request.rs:42-96,130-148`: hidden thread is `ephemeral: true`, `approval_policy: Never`, read-only sandbox, all MCP servers disabled, features apps/hooks/memories/multi_agent/shell_tool/... false, `web_search = "disabled"`. `ThreadStartParams` leaves `base_instructions`/`developer_instructions` at None (default), so the normal Codex base prompt applies.
- Config search: `grep -i title config/src core/src/config*` finds only `terminal_title` (window title, no model call). `codex --help` has no name/title flag.
- Runtime: `~/.codex/auth.json` `auth_mode` = `chatgpt`; `~/.codex/models_cache.json` lists `gpt-5.6-luna`. Horch worker rollouts today (containing "herdr multi-agent fleet") map to names in `session_index.jsonl`: `01a0b4ea-...` "Plan fleet phase", `01a0b50d-...` "Implement fleet skill phase", `01a0b51a-...` "Implement map fixes".

### 2.2 Automatic recap (ON by default, `tui.auto_recap`)
- `tui/src/app/recap.rs:1-2`: "Determines when an unfocused conversation is ready for an automatic recap. The TUI opt-out suppresses automatic scheduling and requests, but not manual `/recap`."
- `tui/src/app/recap.rs:34-39`: `MIN_COMPLETED_TURNS = 3`, `MIN_TURNS_BETWEEN_RECAPS = 2`, `RECAP_DELAY = 3 min`, `RECAP_MAX_CHARS = 320`, retry 30 s.
- `tui/src/app/recap.rs:43-52`: prompt "Write a brief catch-up for a user returning to this Codex task. In at most 40 words ..."
- `tui/src/app/recap.rs:180-193`: `RecapTrigger::Automatic => self.local_settings.tui.auto_recap`; `schedule_recap_check` returns early when `!auto_recap`.
- `tui/src/app/recap.rs:~283`: `let model = self.chat_widget.current_model()` (pane's own model), effort None (thread default).
- `tui/src/app/recap.rs:609-627`: deadline = max(unfocused_since, last_turn_finished_at) + 3 min, only while unfocused.
- `tui/src/app.rs:889-901`: `TuiEvent::FocusLost` -> `note_focus_lost` + `schedule_recap_check`.
- `tui/src/tui.rs:244`: `execute!(stdout(), EnableFocusChange)`.
- `config/src/types.rs:744-747`: "Generate automatic conversation recaps when the terminal is unfocused. Defaults to `true`. Disabling this leaves `/recap` available on demand." `pub auto_recap: bool` with `default_true`.
- `core/src/config/mod.rs:4357`: `tui_auto_recap: cfg.tui...auto_recap.unwrap_or(true)`.
- herdr (`~/.local/bin/herdr`, `strings`): `...bracketed_pastefocus_reportingmouse_protocol_mode...`, `failed to forward pane focus event`.
- Parse check: `codex debug prompt-input -c tui.auto_recap=false -c 'approvals_reviewer="user"' --disable memories "x"` -> renders 5 input items, no error. Control: `-c tui.auto_recap=17` -> `Error: invalid type: integer 17, expected a boolean in tui.auto_recap`.

### 2.3 Memories (OFF)
- `features/src/lib.rs:1100-1105`: `id: Feature::MemoryTool, key: "memories", stage: Stable, default_enabled: false`.
- `memories/README.md` "When it runs": at root session start, not ephemeral, feature enabled, not a sub-agent, state DB available; Phase 1 sends each rollout to a model, Phase 2 consolidates.
- `core/src/responses_metadata.rs:153-158`: request kinds are exactly `Turn`, `Prewarm`, `Compaction`, `Memory` (the only non-turn kinds in core).
- `codex features list` -> `memories  stable  false`. Operator `[features]` has only `hooks = true`, `js_repl = false`.

### 2.4 Guardian (OFF for fleet codex teammates)
- `features/src/lib.rs:1547-1550`: `guardian_approval`, Stable, `default_enabled: true` (`codex features list` shows `guardian_approval stable true`, `guardianv2 under development false`).
- `core/src/guardian/review.rs:226-233`: routes only when `approval_policy` is `OnRequest | Granular(_)` AND `approvals_reviewer == AutoReview`.
- `protocol/src/config_types.rs:183-190`: `ApprovalsReviewer` default `User`; `auto_review` (alias `guardian_subagent`).
- `~/.codex/config.toml:5`: `approvals_reviewer = "auto_review"` (operator global).
- `crates/horch-core/src/teammates.rs:311-313`: Plan -> `-a on-request`, AcceptEdits -> `-a on-request`, Auto -> `-a never`. All three codex teammates use `permission_mode: auto`.
- `ext/guardian-v2/src/async_scorer/config.rs:74,83-87`: scoring disabled when feature `guardianv2` is off and the model has no `guardian` block. `jq` over `~/.codex/models_cache.json`: no model has a `guardian` field.
- `core/src/session_startup_prewarm.rs:275`: the guardian review session is also only initialized when `routes_approval_to_guardian` is true.

### 2.5 Compaction and prewarm (no action)
- `core/src/responses_metadata.rs:153-158`: `Compaction` kind; `codex features list` -> `remote_compaction_v2 stable true`. Key `model_auto_compact_token_limit` at `core/src/config/mod.rs:631`.
- `core/src/client.rs:15`: "WebSocket prewarm is a v2-only `response.create` with `generate=false`"; `client.rs:1784`: "`generate=false` prewarm is connection setup, not an inference request." `core/src/session_startup_prewarm.rs:198-207`: without websockets only auth is prewarmed.

### 2.6 Ambient suggestions (not in CLI)
- `grep -rli ambient.suggest codex-rs` -> only `tui/src/task_mentions.rs:141` (filter on `ThreadSource::Feature("ambient_suggestions")`) and a core test. No generator in the CLI.

### 2.7 notify (not a model call)
- `~/.codex/config.toml:4`: `notify = [".../SkyComputerUseClient", "turn-ended"]`.
- `codex debug prompt-input -c 'notify=[]' "x"` -> renders without error.

### 2.8 Checked and not a side call
- `tool_suggest` (stable true): adds a model-visible tool for app suggestions inside the main turn (`core/src/tools/spec_plan.rs:117`), no separate request.
- `goals` (stable true): `ext/goal` has no model-client code; goal turns are main turns.
- `ext/` crates with model-calling code: only `agent` (model-initiated sub-agents) and `guardian-v2` (above).
- Ghost commits / `undo` and `codex_git_commit`: `removed` in `codex features list`; not model calls.
- Operator `~/.codex/hooks.json`: only `type: "command"` hooks (no prompt or agent hooks, which would call a model).

---

## 3. OpenCode 1.18.2

Binary: `/Users/mascott/.opencode/bin/opencode` (Mach-O arm64, Bun single-file; `opencode --version` = 1.18.2). Offsets below are byte offsets from `grep -aob` in that file.


### 3.1 Env var list (`grep -aoE 'OPENCODE_[A-Z0-9_]+' | sort -u`)
No `OPENCODE_DISABLE_TITLE`, no `..._SMALL_MODEL`. Disable-type vars present: `OPENCODE_DISABLE_AUTOCOMPACT, _AUTOUPDATE, _CHANNEL_DB, _CLAUDE_CODE, _CLAUDE_CODE_PROMPT, _CLAUDE_CODE_SKILLS, _DEFAULT_PLUGINS, _EMBEDDED_WEB_UI, _EXTERNAL_SKILLS, _FFF, _LSP_DOWNLOAD, _MODELS_FETCH, _MOUSE, _PROJECT_CONFIG, _PRUNE, _SHARE, _TERMINAL_TITLE`.

### 3.2 `getSmallModel`: 4 hits, 2 callers
- 67063353 definition `Provider.getSmallModel`: `if(j.small_model){... return yield*H(q.providerID,q.modelID)}` (config `small_model` wins, whatever the provider). Else plugin hook `experimental.provider.small_model`, else family list: provider `opencode*` -> `["gpt-nano"]`, `github-copilot*` -> `["gpt-mini",...]`, others -> `DN=["gemini-flash","gpt-nano","claude-haiku"]`; returns undefined if none, and the caller falls back to the main model.
- 67065378 service export (`getSmallModel:w`).
- 65596574 caller 1, `SessionPrompt.ensureTitle`:
  `if(t.session.parentID)return;if(!R.isDefaultTitle(t.session.title))return; ... ie=yield*l.get("title");if(!ie)return;let Ge=ie.model?yield*a.getModel(ie.model.providerID,ie.model.modelID):(yield*a.getSmallModel(t.providerID))??(yield*a.getModel(t.providerID,t.modelID)) ... N.stream({agent:ie,...,small:!0,tools:{},model:Ge,...,messages:[{role:"user",content:"Generate a title for this conversation:\n"},...se]})`
  `l` is the Agent service: the SessionPrompt layer binds `,l=yield*io.Service`, and `io` is the Agent service (SessionCompaction binds `l=yield*io.Service` and calls `l.get("compaction")`).
- 65314929 caller 2, `ProjectCopyHttpApi.generateName`: `(yield*r.getSmallModel(y.providerID))??(yield*r.getModel(...))`, prompt "Generate a short 2-3 word name that describes this task". Route: `M.post("generateName","/experimental/project/:projectID/copy/generate-name")`. The TUI calls it only in the "Creating copy" handler (`client.experimental.projectCopy.generateName(...)` then `v2.projectCopy.create({... strategy:"git_worktr...`).
- Only two `stream({agent:` sites exist in the bundle (65315030, 65596755), and only two `small:!0` (65315191, 65596789). They are the two callers above.

### 3.3 Title trigger
In the session loop: `if(_++,_===1)yield*to({session:Q,modelID:...,providerID:...,history:C}).pipe(s.ignore,s.forkIn(A))`. `to` = `ensureTitle`. So it fires once per fresh top-level session, forked, errors ignored. A resumed session (`--session`) already has a non-default title, so no call.

### 3.4 The `disable` switch is honored
- Agent build (about 65763796): native agents `compaction`, `title` (temperature 0.5), `summary` are `hidden:!0`, then `for(let[n,s]of Object.entries(p.agent??{})){if(s.disable){delete r[n];continue} ...}`. `get` is `function*(n){return r[n]}`, so a disabled `title` returns undefined and `ensureTitle` returns at `if(!ie)return`.
- Schema: `disable:D.optional(D.Boolean)` in the agent config struct.
- Runtime check (no model call; run in /tmp with `OPENCODE_DISABLE_MODELS_FETCH=1 --pure`): `opencode debug agent title` prints the title agent JSON. With `OPENCODE_CONFIG_CONTENT='{"agent":{"title":{"disable":true}}}'` it prints `Agent title not found, run 'opencode agent list' to get an agent list`.
- `OPENCODE_CONFIG_CONTENT` is loaded after the global and project files: `if(process.env.OPENCODE_CONFIG_CONTENT){let U=yield*y(process.env.OPENCODE_CONFIG_CONTENT,{dir:Y.directory,source:"OPENCODE_CONFIG_CONTENT"});yield*v("OPENCODE_CONFIG_CONTENT",U,"local")...}`.
- Resolved config today (`opencode debug config --pure` in /tmp): `"model": "amazon-bedrock/qwen.qwen3-coder-next"`, `"small_model": "amazon-bedrock/qwen.qwen3-coder-30b-a3b-v1:0"` (from `~/.config/opencode/opencode.json`, line 4). `--pure` removes plugins only, not config.

### 3.5 horch landing place (no Rust change)
- `crates/horch-core/src/launch.rs:33-40` `apply_env` exports every teammate `env:` pair.
- `crates/horch-core/src/skills.rs:230-242` `Bundle::apply_env`: takes the teammate's `env.OPENCODE_CONFIG_CONTENT` (else the inherited process value) and passes it through `opencode_config`.
- `crates/horch-core/src/skills.rs:279-300` `opencode_config`: parses the JSON object and only inserts `skills.paths`. Every other key, `agent` included, passes through unchanged. So `agent.title.disable` (and a `small_model` override) is a teammate-file change only.

### 3.6 Summary agent has no caller
- `grep -aoE '\.get\("(title|summary|compaction|...)"\)'` finds only `.get("compaction")` (65568175) and `.get("title")`.
- `SessionSummary.summarize` (65404681): `setSummary({additions:0,...})`, then `diffFull` over snapshots. No LLM call.
- `SessionHttpApi.summarize` (65328020) creates a compaction request, so it is the explicit `/compact` path (user-triggered).
- The second agent registry at about 71112120 (`f.update(v.ID.make("title"|"summary"|"compaction"),...)`, v2 catalog) defines prompts only. The only "Generate a title" call text is at 65596881 (v1 `ensureTitle`).
- Docs (https://opencode.ai/docs/agents/) describe the Summary agent as "Hidden system agent that creates session summaries. It runs automatically". The 1.18.2 bundle has no call site that does that.

### 3.7 Compaction model
65568175 `SessionCompaction.process`: `let m=yield*l.get("compaction"),q=m.model?yield*r.getModel(m.model...):yield*r.getModel(i.model.providerID,i.model.modelID)`. It uses the main model of the user message and has no undefined guard on `m`, so disabling the compaction agent would throw.

### 3.8 Subagents
Task tool: `R=b.model??{modelID:U.info.modelID,providerID:U.info.providerID}`, so the subagent inherits the parent message's model. Docs (https://opencode.ai/docs/agents/): "subagents will use the model of the primary agent that invoked the subagent."

### 3.9 Other model calls, user-triggered
`Agent.generate` (65766183): "Create an agent configuration based on this request", for `opencode agent create`. `LLM.generate` in the native-LLM library has no other `.generate({` caller.

### 3.10 Docs
- https://opencode.ai/docs/config/: "The `small_model` option configures a separate model for lightweight tasks like title generation. By default, OpenCode tries to use a cheaper model if one is available from your provider, otherwise it falls back to your main model." `OPENCODE_CONFIG_CONTENT` is listed as a runtime override in the precedence order.
- https://opencode.ai/docs/agents/: `disable`: "Set to `true` to disable the agent." Title Agent: "Hidden system agent that generates short session titles. It runs automatically and is not selectable in the UI."

---

## 4. pi 0.85.1 and Prime Agent 0.9.4

Versions: pi 0.85.1 (`/opt/homebrew/lib/node_modules/@earendil-works/pi-coding-agent/package.json`; `pi --version` crashes with a JS stack, see section 4 bug), Prime Agent 0.9.4 (`prime-agent --version` prints `0.9.4`). horch launches Prime as `prime-agent` (`crates/horch-core/src/agent.rs:43-45`), not `prime`.
Method: read the unbundled `dist/core`/`dist/modes` modules (same code as the shipped bundle; bundle hits confirmed for every Prime row), shipped `docs/*.md`, `CHANGELOG.md`, `prime-agent --help`. No live model calls.


### 4.1 pi 0.85.1 (paths relative to `/opt/homebrew/lib/node_modules/@earendil-works/pi-coding-agent/`)

Model call sites (exhaustive grep of `completeSimple(|streamSimple(|.complete(|.stream(|streamFn` over `dist/core`, `dist/modes`, `dist/main.js`):
- `dist/core/sdk.js:185-194`: the main agent loop `streamFn` → `modelRuntime.streamSimple`.
- `dist/core/compaction/compaction.js:454-464` `completeSummarization` (the only `completeSimple` import in core, line 8), used by `generateSummaryWithUsage` (488-506), `compact` (609-629), `generateTurnPrefixSummary` (650-655). Compaction only.
- `dist/core/compaction/branch-summarization.js:187-226` `generateBranchSummary`, called only from `dist/core/agent-session.js:2542` inside `navigateTree()` when `options.summarize && entriesToSummarize.length > 0` (2539).
- `dist/core/model-registry.js:65-66` `complete()` is a pass-through; it has no callers in core or modes, so only extensions can use it.
- In the bundle (`dist/bundle/chunks/chunk-JVUZSMYM.js`, minified), the lane harness's `summaryKind(task)` returns only `"branch_summary"` or `"compaction"` (`task.boundary.kind==="commit_navigation"?"branch_summary":"compaction"`); `kind:"..."` literals include only `branch_summary` (4) and `compaction` (10). There are no `autoRefine`, `SUMMARY_MODEL_ID` or `side_question` strings (0 hits).

Branch summary trigger:
- `dist/modes/interactive/interactive-mode.js:4376-4391`: `let wantsSummary = false; if (!this.settingsManager.getBranchSummarySkipPrompt()) { … wantsSummary = summaryChoice !== "No summary"; }`, then `navigateTree(entryId, { summarize: wantsSummary })` (4421). Other callers pass `options?.summarize` through: `print-mode.js:63`, `rpc-mode.js:241`, and the extension action at `interactive-mode.js:1433`.
- `dist/core/settings-manager.js:572-579`: `branchSummary.skipPrompt ?? false`.
- `docs/settings.md:137`: "`branchSummary.skipPrompt` | boolean | `false` | Skip "Summarize branch?" prompt on `/tree` navigation (defaults to no summary)". `docs/compaction.md:21`: "Branch summarization | `/tree` navigation". `docs/sessions.md:129-137`: the user chooses no summary, the default prompt, or custom instructions.

Session naming:
- `setSessionName` callers: `interactive-mode.js:5185` (`handleNameCommand`, `/name <name>`), `rpc-mode.js:530`, extension API `core/extensions/loader.js:307`. `core/agent-session.js:2451-2453` appends a `session_info` entry. There is no model call anywhere on this path.
- `docs/sessions.md:28,52-66`: "`/name <name>` | Set the current session display name"; "Set the name at startup with `--name` or `-n`".

Extensions: horch `crates/horch-core/src/launch.rs:176-183` adds `--no-extensions --no-skills --no-prompt-templates --no-themes` when `!teammate.inherit_plugins`; `teammates/pi.md` sets `inherit_plugins: false`.

### 4.2 Prime Agent 0.9.4 (paths relative to `/opt/homebrew/lib/node_modules/prime-agent/`; bundle lines are in `dist/bundle/`)

Model call sites (`completeSimple`/`streamSimple`/`new Agent(` in `dist/core`, `dist/modes`, `dist/main.js`): main loop `core/sdk.js:178`; compaction `core/compaction/compaction.js:453,596`; branch summary `core/compaction/branch-summarization.js:197`; refinement `core/refinement/refinement.js:716` (`planRefinement`) and `:764` (`reviewAutoRefine`); daemon recap `modes/daemon/daemon-session-summarizer.js:142`; side question `core/side-question.js:49` (`new Agent`). There are no other sites.

Auto-refine:
- Defaults: `core/settings-manager.js:589-597` (bundle `chunk-AQLIARII.js:6738-6743`): `enabled: this.settings.autoRefine?.enabled ?? true`, `turnInterval … : 25`, `compact: … ?? true`, `cooldownMs … : 20 * 60_000`.
- Gate: `core/agent-session.js:6261-6263` (bundle `chunk-TOACIHN2.js:57237`): `_autoRefineAllowedForSession() { return this._rlmDepth === 0 && this._localHarnessStateDir() !== undefined; }`. `_localHarnessStateDir()` (6257) is defined whenever the session has an artifact dir, which a persisted `--session-dir` session has. Skills are not part of the gate.
- Triggers: the counter increments on every non-error, non-aborted assistant message (`agent-session.js:2658`). `_maybeAutoRefine` (6523-6589) checks `settings.enabled`, then the turn interval and cooldown, then calls `_reviewAutoRefine` (6665-6675 → `reviewAutoRefine(this.agent.state.messages, …, model = this.model, …)`). After compaction it runs through `_scheduleAutoRefineAfterCompaction` (6350-6364) and at disposal (3012-3053). In serialized (non-interactive, non-daemon) mode, `_maybeStartSerializedBackgroundPlan` (1699-1728) checks the same `settings.enabled`.
- Payload: `core/refinement/refinement.js:744-764`: `serializeConversation(...).slice(-40_000)` plus harness state plus history → `completeSimple(model, { systemPrompt: AUTO_REFINE_REVIEW_SYSTEM_PROMPT … })`. Plan: `681-716`, `.slice(-80_000)` → `completeSimple(model, { systemPrompt: REFINEMENT_SYSTEM_PROMPT … })`. Bundle `chunk-TOACIHN2.js:39978` (prompt), `:40505` (`planRefinement`), `:40566` (`reviewAutoRefine`).
- Global write target: `refinement.js:146-147` `getGlobalHarnessStateDir(agentDir = getAgentDir()) → join(agentDir, "harness")`; `config.js:383,404-407` `PRIME_AGENT_CODING_AGENT_DIR` overrides the agent dir. `~/.prime/agent/harness` does not exist today.
- `--no-skills` does not affect it: `agent-session.js:7660` only filters the `refine` skill out of the model-visible list when auto-refine is NOT allowed. The auto-refine path itself never checks skills.
- CHANGELOG.md:519: "Changed automatic harness refinement to be enabled by default while keeping `autoRefine.enabled: false` as the opt-out." Line 550: added "auto-refine review hook … after turn intervals or compaction checkpoints (#201)". `docs/settings.md` does not document `autoRefine` (0 hits). `prime-agent --help` has no refine flag, and the bundle's `PRIME_AGENT_*` env list has no refine variable.
- Settings files: `docs/settings.md:5-8`: `~/.prime/agent/settings.json` (global) and `.prime/agent/settings.json` (project); project overrides global. `prime-agent --help` has no `--settings`.
- Operator state: `~/.prime/agent/settings.json` keys = `['telemetry']`.

Daemon status recap:
- `modes/daemon/daemon-session-summarizer.js:3-15` (bundle `chunk-AQLIARII.js:59988-59993`): `SWEEP_INTERVAL_MS = 25_000`, `SETTLE_DEBOUNCE_MS = 2_000`, `SUMMARY_MODEL_PROVIDER = "prime-inference"`, `SUMMARY_MODEL_ID = "qwen/qwen3-30b-a3b-instruct-2507"`, `SUMMARY_CONTEXT_MESSAGES = 8`, `SUMMARY_MAX_CHARS_PER_MESSAGE = 600`, `SUMMARY_MAX_TOKENS = 400`.
- `:31-37` (bundle `:60011`) `resolveSummaryModel`: returns the model only `if (model && registry.hasConfiguredAuth(model))`; `:128-142` (bundle `:60097`) `generateAgentStatus` returns undefined when there is no model or no apiKey.
- Started unconditionally with the daemon: `modes/daemon/daemon-mode.js:266` constructs it and `:384` (bundle `chunk-AQLIARII.js:61569`) calls `this.summarizer.start();`. The class comment: "Background status summarization for daemon-hosted sessions, top-level and subagents alike."
- Credential sources for `prime-inference`: `core/auth-storage.js:380-388`: runtime, env (`PRIME_API_KEY`, per bundle `chunk-6GUVYK6P.js:78` `"prime-inference": "PRIME_API_KEY"`), Prime CLI config (`~/.prime/config.json` `api_key`, `core/prime-inference-auth.js:15-16,97`, enabled by default via `main.js:561`), stored auth.json, custom-provider fallback.
- Host state: `~/.prime/agent/auth.json` has no keys; `~/.prime/config.json` is absent; `PRIME_API_KEY` is unset in the fleet pane environment.
- horch gives each Prime pane its own daemon (`--daemon-socket`, `crates/horch-core/src/prime.rs:1-12`), so the summarizer would run once per pane.

Session naming: slash commands `core/slash-commands.js:61` (`name`) and `:153` (`rename` alias); `prime-agent --help` lists `rename    Rename an agent`. Agents-view title fallback, `modes/agents-view/agents-view-state.js:836-844`: `[summary.sessionName, summary.firstMessage, basename(summary.cwd), summary.sessionId, summary.id]`. A grep for `generate*Name|generate*Title|autoName|sessionTitle|titleModel` finds no generator.

Skills under `--no-skills`: `core/resource-loader.js:305-307`: `this.noSkills ? this.mergePaths(cliEnabledSkills, this.additionalSkillPaths) : …enabledSkills`. Bundled skills enter only through `enabledSkills`, from `core/package-manager.js:1792-1799` (`bundledSkillsDir && settingsManager.getEnableBuiltinSkills()`), so `--no-skills` keeps only the `--skill` paths horch passes. Bundled skills: `dist/skills/{agent-message,agent-observe,attach-image,compact,edit,goal,linear,notion,prime-intellect,refine,rlm-heartbeat,skill-creator,websearch}`. `--goal <objective>` is a CLI flag (`prime-agent --help`) that horch does not pass (`launch.rs:142-205`).

Heartbeats/cron: `core/cron-jobs.js:11` `DEFAULT_HEARTBEAT_SCHEDULE = "every 5m"`. The daemon starts the scheduler (`daemon-mode.js` bundle `chunk-AQLIARII.js:61572`), which fires only existing jobs. The `rlm_heartbeat.*` host requests come from "the bundled rlm-heartbeat skill" (`agent-session.js:2192-2194`).

Autonomous: `core/autonomous.js:21-22` `const enabled = config?.enabled === true`; config comes from CLI args (`main.js:471` `runtimeAutonomousConfigFromArgs(parsed)`) or `/autonomous` (`slash-commands.js:116`).

Side question: `core/slash-commands.js:55-58` `btw` "Ask a side question without adding it to the session", alias `side` (:154); `core/side-question.js:14-49` runs on `parent.state.model`.

Trace upload: `core/agent-session-services.js:125` installs the uploader for every session; `core/agent-traces.js:659-664` checks `getAgentTracesEnabled()`; `core/settings-manager.js:536-537` `agentTraces?.enabled ?? false` (bundle `chunk-AQLIARII.js:6685`). The credential resolution (`agent-traces.js:632-657`) finds none on this host.
