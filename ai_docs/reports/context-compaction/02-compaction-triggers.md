# 02: How the orchestrator triggers compaction on each harness

Brief: `ai_docs/plans/context-compaction/02-research-compaction-triggers.md`.
Author: researcher-5, 2026-09-28.

Versions checked (same as round 1): Claude Code 2.1.284, Codex 0.157.1,
OpenCode 1.18.2, pi 0.85.1, Prime Agent 0.9.4.

Evidence sources:
- Claude Code: binary strings (`/tmp/horch-compact-research/claude-2.1.284.txt`)
  and 1 live session (see "Live checks").
- Codex: source at tag `rust-v0.157.1` (commit `3665039`), cloned to
  `/tmp/horch-compact-research/codex/codex-rs`. Paths below are relative to that dir.
- OpenCode: source at tag `v1.18.2` (commit `70b56a0`), cloned to
  `/tmp/horch-compact-research/opencode`. Paths below are relative to that dir.
- pi: installed package `/opt/homebrew/lib/node_modules/@earendil-works/pi-coding-agent`
  (docs and `dist/`).
- Prime: installed package `/opt/homebrew/lib/node_modules/prime-agent` (docs and `dist/`).

## How `horch tell` delivers text (applies to every harness)

`horch tell <role> "<msg>"` calls `Herdr::send_line`
(`crates/horch-core/src/herdr.rs:680-688`):
1. `herdr pane send-text <pane> <msg>` (the raw text, with no prefix; `crates/horch/src/cmd/messaging.rs:18-36`).
2. Sleep 1 s (2 s if the text is over 1,500 bytes).
3. `send-keys enter`.
4. Sleep 1 s.
5. `send-keys enter` again.

So the orchestrator can type `/compact ...` exactly. The second Enter lands on
an empty input box. On every harness below, an Enter on an empty input does
nothing (Claude: observed live; Codex: empty `Submitted` text returns early,
`tui/src/chatwidget/input_flow.rs:52-58`; OpenCode: `if (!store.prompt.input) return false`,
`packages/tui/src/component/prompt/index.tsx:960`; pi/Prime: `if(text=text.trim(),!!text)` in `onSubmit`).

UNVERIFIED: whether `herdr pane send-text` uses bracketed paste. This matters for
Codex paste-burst detection and for OpenCode autocomplete. Brief 03 owns it.

---

## 1. Claude Code 2.1.284

### 1.1 Command
- `/compact <optional custom summarization instructions>`.
  Binary: `{type:"local",name:"compact",description:"Free up context by summarizing the conversation so far",isEnabled:()=>!Le(process.env.DISABLE_COMPACT),supportsNonInteractive:!0,argumentHint:"<optional custom summarization instructions>"}`.
- The command has no `immediate:!0` flag. `immediate` is "whether it runs mid-turn" (binary string
  `a changed immediate (read only: the command declares whether it runs mid-turn ...)`).
- Live check: `/compact keep only the word banana` produced a summary of exactly `banana`
  (transcript line 163, see Live checks).

### 1.2 Busy behavior: QUEUED until the turn ends (verified live)
- Live check, test B: I typed `/compact keep only the word banana` + Enter + Enter
  4 s into a 67 s generation turn. The input box showed `Press up to edit queued messages`.
  The queued line showed `ctrl+x ctrl+s to send now`. The turn continued to its end
  (`Crunched for 1m 7s`). Then `Compacting conversation… (9s)` started by itself.
- The second Enter did NOT force the queued command to run early.
- Limit: test B had no tool calls. I did not test a turn with a tool boundary. Claude
  delivers plain queued messages mid-turn as steering ("Send messages to Claude while it works
  to steer Claude in real-time"). A local slash command has no `immediate` flag, so it
  cannot join an API turn. Behavior at a tool boundary: UNVERIFIED, expected QUEUED.
- Consequence: the orchestrator does not have to wait for idle. The compaction then runs at
  the end of the current turn, not at a natural stopping point the orchestrator selects.

### 1.3 Self-compaction from inside the model's turn
- No model tool exists for compaction (binary: no `Compact` tool; `/compact` is `type:"local"`).
- Hooks cannot start a compaction. `PreCompact` can only block it (exit 2) or add instructions.
- Possible path: the session runs `horch tell <own-role> "/compact ..."` through Bash.
  The text lands while its own turn runs, so Claude queues it (1.2). It runs when the turn ends.
  Whether `horch tell` can address the orchestrator's own pane is a brief 03 question.
  Self-send: UNVERIFIED end to end; mechanism verified by test B.

### 1.4 Threshold lever to about 300,000 tokens
- `CLAUDE_CODE_AUTO_COMPACT_WINDOW=<tokens>` env var. Range 100,000-1,000,000, plain integer.
  It beats the setting and the flag. Trigger = window − min(max_output, 20,000) − 13,000
  (`ai_docs/reports/env-research/compaction-benchmarks.md` 2.2).
  - Trigger at about 300,000: set `CLAUDE_CODE_AUTO_COMPACT_WINDOW=333000` (trigger 300,000).
  - `333000` is my arithmetic from that formula; not measured live.
- `--autocompact <auto|tokens>` CLI flag, same range, one launch only.
- `autoCompactWindow` settings key (do not edit global settings; pass `--settings` JSON instead).
- `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE=<1-100>` lowers only. 31 on a 1M window gives about 303,800.
- Do not use `DISABLE_COMPACT`; it also disables manual `/compact` (binary schema string).

### 1.5 What survives, and how the model re-reads the handoff
- Survives (docs https://code.claude.com/docs/en/context-window#what-survives-compaction,
  quoted in `compaction-benchmarks.md` 3.2): system prompt; project-root CLAUDE.md re-injected
  from disk; auto memory; up to 5 recently touched files (files over 5,000 tokens come back
  as a path only); invoked skill bodies (5,000 tokens each, 25,000 total); SessionStart hooks
  with matcher `compact` re-run.
- Live check: a recent segment of messages is also kept verbatim
  (`compact_boundary.compactMetadata.preservedSegment`, 3-4 message uuids in each of my 3 compactions).
- The custom instructions argument works: summary was exactly `banana` (transcript line 163).
- Hooks (binary schema strings):
  - `PreCompact`, matcher `manual` / `auto`. Input `{trigger, custom_instructions}`.
    "Exit code 0 - stdout appended as custom compact instructions. Exit code 2 - block compaction."
  - `PostCompact`, matcher `manual` / `auto`. Input `{trigger, compact_summary}`. Stdout goes to the user only.
  - `SessionStart` with `source` = `compact` (enum `startup, resume, clear, compact, fork`).
    "Exit code 0 - stdout shown to Claude." This is the hook that can inject
    "Read ai_docs/handoffs/<role>-whats-next.md" into the new context.
- Compaction does NOT start a new turn. After `Compacted` the pane is idle (live check, all 3 runs).
  The model reads the handoff only when the next user message arrives.
- Follow-up pattern verified live (test C): I sent `/compact ...` and then, 1 s later,
  `Reply with the one word from your summary, then KIWI.` The second message showed as queued
  during `Compacting conversation…`. It ran after compaction and the model replied `banana` / `KIWI`.
  So the orchestrator can send `/compact <instructions>` and then, at once, a
  `horch tell` that says "read the handoff file and continue". Claude runs them in order.

### 1.6 Completion signal
- Transcript (`~/.claude/projects/<slug>/<sid>.jsonl`), verified live:
  - `type:"system", subtype:"compact_boundary"` with
    `compactMetadata {trigger:"manual"|"auto", preTokens, postTokens, durationMs, preservedSegment}`.
  - Next line: `type:"user"`, `isCompactSummary:true`, text starts
    `This session is being continued from a previous conversation ...`.
  - Then a `user` line with `<command-name>/compact</command-name>` and `<command-args>`.
- Screen text: `Compacting conversation… (Ns)` while running, then
  `⎿  Compacted (ctrl+o to see full summary)`.
- `PostCompact` hook fires after compaction (a hook could run `horch note` or signal horch).

---

## 2. Codex 0.157.1

### 2.1 Command
- `/compact`, description "summarize conversation to prevent hitting the context limit"
  (`tui/src/slash_command.rs:39,95`).
- NO argument. `Compact` is not in `supports_inline_args()` (`tui/src/slash_command.rs:166-189`).
- `/compact <text>` is NOT a compaction. `bare_command` returns `None` when args are present
  (`tui/src/bottom_pane/chat_composer/slash_input.rs:89-104`). `inline_command` returns `None`
  because the command takes no inline args (`slash_input.rs:106-121`). The text then goes to the
  model as a normal user prompt (`chat_composer.rs:3241-3270`; queued path:
  `tui/src/chatwidget/input_flow.rs:178-201`, `queued_slash_prompt`).
- Custom summary instructions: config only. `compact_prompt` (string) or
  `experimental_compact_prompt_file` (path) (`core/src/config/mod.rs:3945,4015-4022`), via `-c` at launch.
  - WARNING: with the OpenAI provider these keys have no effect on manual compaction. The manual task
    uses remote compaction V2 for OpenAI and Azure providers
    (`model-provider/src/provider.rs:411-417`; `core/src/tasks/compact.rs:41-48`).
    `compact_prompt` is read only on the local path (`core/src/tasks/compact.rs:50-66`).
    The fleet's Codex teammates use the OpenAI provider, so a custom compact prompt does not apply.

### 2.2 Busy behavior: REJECTED with Enter
- `/compact` is not `available_during_task()` (`tui/src/slash_command.rs:239-253`).
- With Enter while a task runs, the composer rejects it and prints
  `'/compact' is disabled while a task is in progress.` (`tui/src/bottom_pane/chat_composer.rs:3384-3398`).
- The Tab key queues input while a task runs (`queue_keys` default `Tab`, `chat_composer.rs:813`,
  `keymap.rs:1681`). A queued `/compact` is parsed on dequeue (`QueuedInputAction::ParseSlash`,
  `slash_input.rs:196-207`) and dispatched after the turn ends (`input_flow.rs:313-318`).
  `horch tell` sends Enter, not Tab, so this path needs a new send method. Tab path: source only, not run live.
- Plain text + Enter during a turn goes into the running turn as a steer message
  (`input_flow.rs:48-80`).
- Consequence: the orchestrator must wait until the Codex pane is idle, then send bare `/compact`.

### 2.3 Self-compaction from inside the model's turn
- No model tool for compaction in the default tool set. Not possible through `horch tell` to itself:
  the text arrives during its own turn, and bare `/compact` is rejected (2.2).
- Hooks cannot start compaction. `PreCompact` can stop it (`hooks/src/events/compact.rs:52-56`, `should_stop`).
- Possible workaround: a detached delayed send (for example `(sleep 60; horch tell <self> /compact) &`)
  that lands after the turn ends. UNVERIFIED.

### 2.4 Threshold lever to about 300,000 tokens
- `model_auto_compact_token_limit` (integer tokens), launch flag `-c model_auto_compact_token_limit=N`.
- Effective limit = `min(N, 90% of resolved context window)` (`protocol/src/openai_models.rs:525-536`).
- `gpt-5.6-sol` / `gpt-5.6-terra` catalog window is 272,000, so the default trigger is 244,800
  (`compaction-benchmarks.md` 2.3). That is already below 300,000. A value of 300,000 has no effect.
- To move the trigger to 300,000 you also need `-c model_context_window=N` with N ≥ 333,334
  (cap `max_context_window` 872,000). Not recommended: it raises the window above the catalog default.
- Companion key: `model_auto_compact_token_limit_scope = "total" | "body_after_prefix"` (default `total`).

### 2.5 What survives, and how the model re-reads the handoff
- Remote compaction V2 (OpenAI) keeps: real user messages and hook prompts, up to 64,000 tokens
  (`RETAINED_MESSAGE_TOKEN_BUDGET`, `core/src/compact_remote_v2.rs:75,556-600`); client developer messages
  when enabled; the opaque server compaction item.
- The initial context (developer instructions, AGENTS.md user instructions, environment context)
  is rebuilt and inserted again (`build_compaction_initial_context`, `core/src/compact.rs:92-111`;
  `compact_remote_v2.rs:324-327`).
- Assistant messages and tool output are not kept verbatim; only the server's compaction item carries them.
- Hooks (`hooks/src/schema.rs:102-125`): `PreCompact`, `PostCompact`, and `SessionStart` with
  source `compact` (`hooks/src/events/session_start.rs:23-40`). After compaction Codex queues a pending
  `SessionStart` source `Compact` (`core/src/session/mod.rs:4115-4118`). It runs at the next turn start.
  Its `additional_contexts` go to the model (`session_start.rs:87,199`).
  - Hooks are on by default (feature `hooks`, `Stable`, `default_enabled: true`, `features/src/lib.rs:1211-1216`).
  - Hook files: `hooks.json` in a config folder, or `[hooks]` in `config.toml`, per config layer
    (`hooks/src/engine/discovery.rs:129-175,339-345`). horch gives each Codex pane a private `CODEX_HOME`,
    so horch can write `hooks.json` there without touching `~/.codex`.
  - Hook trust: new hooks may need approval (`bypass_hook_trust: false`, `discovery.rs:138`). UNVERIFIED in a pane.
- A manual compaction does not start a new turn (the compact task returns `Ok(None)`,
  `core/src/tasks/compact.rs:69-75`). A follow-up `horch tell` is needed. It must arrive after compaction
  ends, or the TUI queues it. Enter during compaction: compaction sets the task-running state
  (`tui/src/chatwidget/slash_dispatch.rs:294-310`), so plain text + Enter goes the `Submitted` path
  and is queued when `user_turn_pending_start` is set (`input_flow.rs:60-80`). Order after compaction: UNVERIFIED live.

### 2.6 Completion signal
- Rollout file `~/.codex/sessions/YYYY/MM/DD/rollout-*-<sid>.jsonl` (or under the pane's `CODEX_HOME`),
  verified on `rollout-2026-09-27T11-38-40-01a0e429-....jsonl`:
  - line 356: top-level `"type":"compacted"` with `replacement_history`.
  - then `world_state`, `turn_context`, `event_msg thread_settings_applied`.
  - then `event_msg` `item_completed` with `item.type` `ContextCompaction`.
  - `token_count.last_token_usage.input_tokens`: 236,597 before (line 354); `total_tokens` 18,207 after (line 360).
- Protocol event `EventMsg::ContextCompacted` (`protocol/src/protocol.rs:1397,2143`).
- Screen text: `Compacting context` / `Making room to continue.` while running
  (`tui/src/chatwidget/compaction.rs:6-7`), then `Context compacted` (`compaction.rs:43`).

---

## 3. OpenCode 1.18.2

### 3.1 Command
- `/compact`, alias `/summarize`, keybind `ctrl+x c` (`packages/web/src/content/docs/tui.mdx:88-96`;
  `packages/tui/src/routes/session/index.tsx:554-578`).
- NO argument. The TUI command calls `sdk.client.session.summarize({sessionID, modelID, providerID})`
  with no text (`index.tsx:572-576`).
- `/compact <text>`: `compact` is a TUI command, not a server command, so `submitInner` sends the whole
  text as a normal prompt (`packages/tui/src/component/prompt/index.tsx:1070-1113`). Not a compaction.
- Bare `/compact` + Enter works through the autocomplete: `submitInner` returns early while the autocomplete
  is visible (`index.tsx:958`), and the autocomplete `select()` runs the command (`autocomplete.tsx:553-558`).
  UNVERIFIED live, and it depends on how herdr types the text (see the note at the top).
- Custom summary instructions: no CLI or TUI argument. Options:
  - Plugin hook `experimental.session.compacting` returns `{context, prompt}` to add context or replace the prompt
    (`packages/opencode/src/session/compaction.ts:342-348`).
  - The hidden `compaction` agent uses `agent/prompt/compaction.txt` (`packages/opencode/src/agent/agent.ts:219-231`).
    Overriding it through config `agent.compaction.prompt`: UNVERIFIED.

### 3.2 Busy behavior: RUNS at the next loop step, even mid-turn, and ENDS the turn
- The command has no busy check. `summarize` inserts a user message with a `compaction` part
  (`compaction.ts:513-534`), then calls `promptSvc.loop`
  (`packages/opencode/src/server/routes/instance/httpapi/handlers/session.ts:273-293`).
- `loop` joins the running loop (`state.ensureRunning`, `session/prompt.ts:1343-1346`).
- On the next loop step, `MessageV2.latest` collects `compaction` parts newer than the last finished assistant
  message (`session/message-v2.ts:585-600`). The loop runs `compaction.process` before the next model call
  (`prompt.ts:1143-1158`).
- A manual compaction (`auto:false`) adds no "continue" message. That is added only for `auto`
  (`compaction.ts:422-503`). After the summary, the last assistant message is newer than the last user message,
  so the loop exits (`prompt.ts:1111-1129`). The interrupted work does not resume by itself.
- This is a source reading. UNVERIFIED live.
- Consequence: the orchestrator must wait until the OpenCode session is idle. Else it cuts the turn at a
  step boundary and the worker stops.

### 3.3 Self-compaction from inside the model's turn
- No model tool. `horch tell` to itself would compact at the next step and end its own turn (3.2).
  The model's own Bash call is in flight at that time. UNVERIFIED.
- A plugin could call the summarize API. UNVERIFIED.

### 3.4 Threshold lever to about 300,000 tokens
- From `compaction-benchmarks.md` 2.4 (bundle formula, re-read here as `Is`/`Dl`):
  trigger = `limit.input − (compaction.reserved ?? min(20000, maxOut))` if the model has `limit.input`,
  else `limit.context − min(limit.output, 32000)`.
- Fleet models:
  - `nemotron-3.5-lightning-free`: default 230,144. Already below 300,000.
  - `big-pickle`: default 140,000. Already below 300,000.
  - `nemotron-3-ultra-free`: default 968,000. To get 300,000, override the model limit through
    `OPENCODE_CONFIG_CONTENT`:
    `{"provider":{"opencode":{"models":{"nemotron-3-ultra-free":{"limit":{"context":332000,"output":128000}}}}}}`
    gives 332,000 − 32,000 = 300,000. My arithmetic; not measured.
- `compaction.reserved` is ignored for models without `limit.input` (both Nemotron models).
- Do not set `OPENCODE_DISABLE_AUTOCOMPACT` or `compaction.auto=false`.

### 3.5 What survives, and how the model re-reads the handoff
- System prompt and AGENTS.md instructions are rebuilt on every loop step (`instruction.system()`,
  `session/prompt.ts:1257-1267`). They survive compaction.
- The last `compaction.tail_turns` (default 2) user turns stay verbatim (`compaction-benchmarks.md` 3.3).
- The summary is an assistant message with `summary: true` (`compaction.ts:364`).
- Re-read of the handoff: no SessionStart-style hook. Options: a follow-up `horch tell` after compaction ends,
  or the plugin hook `experimental.session.compacting` adds context that names the handoff path.
- Manual compaction leaves the session idle (3.2). A follow-up message is needed.

### 3.6 Completion signal
- Bus event `session.compacted` `{sessionID}` published after success (`compaction.ts:506-509`;
  listed in `packages/web/src/content/docs/plugins.mdx`).
- SQLite: an assistant message with `summary: true` after a user message that has a `compaction` part.
- Screen text: a divider titled ` Compaction ` above the compaction user message (`index.tsx:1442-1450`).
- Session status returns to `idle` (`session/run-state.ts:60-63`).

---

## 4. pi 0.85.1

### 4.1 Command
- `/compact [instructions]` ("Manually compact context, optionally with custom instructions",
  `docs/usage.md:53`; `docs/compaction.md:37`).
- Source (`dist/bundle/chunks/chunk-JVUZSMYM.js:1366`): `if(text==="/compact"||text.startsWith("/compact ")){let customInstructions=...text.slice(9).trim()...; await this.handleCompactCommand(customInstructions)}`.

### 4.2 Busy behavior: INTERRUPTS (aborts the running agent run)
- The slash check is at the top of `onSubmit`, before the steer/follow-up logic (same chunk, `onSubmit=async text=>{...}`).
- `AgentSession.compact()` starts with `await this.abort();` (`dist/core/agent-session.js`, `async compact(customInstructions)`).
- So `/compact` during a run stops the run, then compacts. Source only; not run live.
- Plain text + Enter during a run is a steering message, delivered after the current tool calls
  (`docs/usage.md:67`). Alt+Enter is a follow-up after all work.
- Consequence: the orchestrator must wait for idle. Else the worker's current step is lost.

### 4.3 Self-compaction from inside the model's turn
- An extension can call `ctx.compact({customInstructions, onComplete, onError})` (`docs/extensions.md:1077-1091`),
  or register a tool (`pi.registerTool`) that calls it. horch would load it with `-e <path>`.
- `horch tell` to itself: `/compact` aborts its own run (4.2). UNVERIFIED.

### 4.4 Threshold lever to about 300,000 tokens
- Trigger: `contextTokens > contextWindow − compaction.reserveTokens` (`docs/compaction.md:27-35`).
- `ollama/qwen3.8` has `contextWindow` 262,144, trigger 245,760 (`compaction-benchmarks.md` 2.5).
  Already below 300,000. No change needed.
- Levers: `compaction.reserveTokens` (default 16384) in `<project>/.pi/settings.json` or in the dir named
  by `PI_CODING_AGENT_DIR` (`docs/settings.md:114-124`; `docs/environment-variables.md:81`).
  No CLI flag. `pi --help` did not run on this machine (see "Outside scope").

### 4.5 What survives, and how the model re-reads the handoff
- The system prompt stays; the model sees `system, summary, kept messages` (`docs/compaction.md`, "What the LLM sees").
- AGENTS.md / CLAUDE.md load at startup into the system prompt (`docs/usage.md:99-103`), so they stay.
- The last `keepRecentTokens` (default 20,000) stay verbatim. Read and modified files are tracked in the summary.
- Custom instructions: `/compact <instructions>`, or extension event `session_before_compact`, which gets
  `customInstructions` and can cancel or supply the summary (`docs/compaction.md` "session_before_compact").
- Re-read of the handoff: extension event `session_compact` fires after success (`docs/extensions.md:477-482`).
  The extension can call `pi.sendUserMessage("Read ai_docs/handoffs/<role>-whats-next.md ...")`, which
  "always triggers a turn" (`docs/extensions.md:1439-1451`). Or the orchestrator sends a follow-up `horch tell`.
- Manual compaction does not continue the run by itself (the run was aborted). UNVERIFIED live.

### 4.6 Completion signal
- Session JSONL: an entry `{"type":"compaction", ..., "summary", "firstKeptEntryId", "tokensBefore"}`
  (`docs/session-format.md:234`).
- Extension events `session_compact` / `session_compact_failed` (`docs/extensions.md:452-489`).
- Screen text: `Compacting context... <cancel hint>` then `Compacted from <N> tokens` (bundle strings).

---

## 5. Prime Agent 0.9.4

### 5.1 Command
- `/compact [instructions]` ("Compact the session context; optional instructions focus the summary",
  `dist/core/slash-commands.js:100-104`; `docs/usage.md:54`).
- The instructions go into the summarization prompt with high priority and are stored on the
  `CompactionEntry` (`docs/compaction.md:37`).

### 5.2 Busy behavior: QUEUED as a session command, runs at the next turn boundary, no abort
- `compact` is a session slash command (`SESSION_SLASH_COMMAND_NAMES`, `dist/core/slash-commands.js:2`).
- While streaming, it is admitted with schedule `steer` (`dist/core/agent-session.js:3856-3862`), and
  `steer` maps to delivery `next_turn_boundary` (`agent-session.js:4242-4244`).
- The queue runs it with `compact(args, {skipAbort: true})` (`agent-session.js:4887-4891`).
  `compact()` throws `Cannot compact without aborting while the agent is running.` if it is still streaming
  (`agent-session.js:6047-6050`).
- Exact point where it runs (between tool batches or after the run): UNVERIFIED. It does not abort the run.

### 5.3 Self-compaction from inside the model's turn: YES, built in
- Built-in skill `compact` (`dist/skills/compact/SKILL.md`): Python REPL `await compact.run("instructions")`
  and `await compact.status()` (returns `tokens`, `context_window`, `percent`, `scheduled`).
- "A scheduled compaction runs when the current turn ends; the harness then resumes you automatically
  with the summary plus recent messages" (SKILL.md "Rules").
- Host side: `handleCompactHostRequest` (`agent-session.js:2076-2110`). `compact.run` only schedules when
  a turn is running. Enabled by `compaction.agentCallable` (default `true`, `dist/core/settings-manager.js:579-581`).
- WARNING: horch passes `--no-skills`, which removes built-in skills (`docs/skills.md:101`;
  `ai_docs/reports/env-research/pi-ollama-prime.md` line 204). `--skill <path>` is additive
  (`docs/skills.md:37`). horch can add `--skill /opt/homebrew/lib/node_modules/prime-agent/dist/skills/compact`.
  Whether the kernel `compact` object exists without the skill loaded: UNVERIFIED.

### 5.4 Threshold lever to about 300,000 tokens
- Same formula as pi. `claude-opus-5` catalog `contextWindow` 1,000,000, default trigger 983,616.
- Lever A: `modelOverrides` in `models.json`, `"claude-opus-5": {"contextWindow": 316384}` gives
  316,384 − 16,384 = 300,000 (`pi-ollama-prime.md` row 1 names the key; value is my arithmetic).
- Lever B: `compaction.reserveTokens` = 700000 in `<project>/.prime/agent/settings.json` or the dir named by
  `PRIME_AGENT_CODING_AGENT_DIR`. Allowed range: UNVERIFIED.
- No CLI flag (`prime-agent --help` lists only `--skill` among related flags).

### 5.5 What survives, and how the model re-reads the handoff
- Same model as pi: system prompt, summary, last 20,000 tokens verbatim.
- "The Python kernel persists through compaction, so variables, imports, helper functions, and task state
  remain available" (`docs/long-running-agents.md`).
- Prime schedules a post-compaction continuation (`_schedulePostCompactionContinue`,
  `agent-session.js:6366-6380`), so a compaction from `compact.run` resumes the work by itself.
  For a typed `/compact`: UNVERIFIED.
- Re-read of the handoff: put it in the instructions (`/compact read ai_docs/handoffs/<role>-whats-next.md next`),
  or send a follow-up `horch tell`. Extension events match pi (`session_compact`). UNVERIFIED for Prime.

### 5.6 Completion signal
- Session JSONL entry `"type":"compaction"` (same session format as pi).
- Runtime events `compaction_start` / `compaction_end` (bundle event list).
- Screen text: `Compacting context<focus>... <cancel hint>` then `Compacted from <N> tokens<focus>`
  (`dist/modes/interactive` strings).

---

## Live checks

1 live session in total (the brief allows 2). Codex live check: none; the source answers steps 2 and 6.

Claude Code, session `e454b7e0-8e2b-45c5-93a5-9a608088d589`, `--model haiku`, run in tmux
(`tmux -L hcr`) in `/tmp/horch-compact-research/claude`, launched with
`env -u ANTHROPIC_API_KEY -u HERDR_ENV -u HERDR_PANE_ID -u HERDR_SOCKET_PATH claude --model haiku --allowedTools 'Bash(sleep:*)'`.
The herdr vars were unset so the herdr SessionStart hook exits early. The account showed
"You're now using usage credits". Transcript:
`~/.claude/projects/-private-tmp-horch-compact-research-claude/e454b7e0-8e2b-45c5-93a5-9a608088d589.jsonl`.
- Test A (idle): Haiku sent `sleep 30` to the background, so the turn was already idle. `/compact` ran at once.
  `compact_boundary` line 53: `trigger manual, preTokens 34981, postTokens 6136, durationMs 12065`.
- Test B (busy): `/compact keep only the word banana` + Enter + Enter during a 67 s generation. Queued, ran after
  the turn. Line 162: `preTokens 39292, postTokens 8249, durationMs 12169`. Summary (line 163) is `banana`.
- Test C (follow-up): `/compact ...` then a second message 1 s later. The second message was queued during
  compaction and ran after it. Reply: `banana` / `KIWI`.
- Claude blocked a foreground `sleep 25`: "The tool blocks standalone foreground sleep commands."

---

## Summary table

| harness | command | busy behavior | self-compact possible | threshold lever (≈300k) | post-compact re-read hook | completion signal |
|---|---|---|---|---|---|---|
| Claude Code 2.1.284 | `/compact <instructions>` | QUEUED until turn end (verified live) | Indirect: `horch tell` self queues it (mechanism verified; self-address UNVERIFIED) | `CLAUDE_CODE_AUTO_COMPACT_WINDOW=333000` (range 100k-1M) or `--autocompact 333000` | `SessionStart` matcher `compact` (stdout to model); `PreCompact` stdout adds instructions; queued follow-up `horch tell` (verified) | JSONL `system/compact_boundary` + `isCompactSummary`; screen `Compacted (ctrl+o ...)`; `PostCompact` hook |
| Codex 0.157.1 | `/compact` (no args; `/compact x` is a prompt) | REJECTED with Enter ("disabled while a task is in progress"); Tab queues it | No (no tool; self-send is rejected) | Default 244,800 already < 300k; `-c model_auto_compact_token_limit` only lowers | `SessionStart` source `compact` via `hooks.json` in pane `CODEX_HOME`; follow-up `horch tell` | Rollout `"type":"compacted"` + `item_completed` `ContextCompaction`; screen `Context compacted` |
| OpenCode 1.18.2 | `/compact` or `/summarize` (no args) | RUNS at next loop step even mid-turn, then turn ENDS (source) | Not safely (would end own turn) | Default already < 300k for lightning and big-pickle; ultra: `limit.context` 332000 via `OPENCODE_CONFIG_CONTENT` | None built in; plugin `experimental.session.compacting`; follow-up `horch tell` | Bus `session.compacted`; SQLite assistant `summary:true`; status `idle` |
| pi 0.85.1 | `/compact [instructions]` | INTERRUPTS (aborts run, then compacts) (source) | Via extension `ctx.compact()` tool | Default 245,760 already < 300k; `compaction.reserveTokens` in project `.pi/settings.json` | Extension `session_compact` + `pi.sendUserMessage`; follow-up `horch tell` | JSONL `"type":"compaction"`; screen `Compacted from N tokens` |
| Prime 0.9.4 | `/compact [instructions]` | QUEUED to next turn boundary, no abort (source) | YES: built-in `compact` skill `compact.run()` (needs `--skill .../skills/compact` because horch passes `--no-skills`) | `modelOverrides` `contextWindow` 316384, or `compaction.reserveTokens` 700000 | Auto-continue after skill compaction; instructions arg; follow-up `horch tell` | JSONL `"type":"compaction"`; `compaction_end` event; screen `Compacted from N tokens` |

## Recommendation input

1. Wait for idle before every compaction on every harness. Claude and Prime queue, but Codex rejects,
   OpenCode cuts the turn, and pi aborts it. One rule ("send only when idle") is safe for all 5.
2. Use bare `/compact` for Codex and OpenCode. Use `/compact <instructions>` only for Claude, pi and Prime.
   On Codex and OpenCode, `/compact <text>` becomes a normal prompt to the model.
3. The flow "write handoff, compact, re-read handoff" needs a trigger after compaction on every harness,
   because no harness starts a new turn after a manual compaction (Claude verified; others source or UNVERIFIED;
   Prime's skill path is the exception). The simplest common method: after the completion signal, horch sends
   `horch tell <role> "Read ai_docs/handoffs/<role>-whats-next.md and continue."`
   On Claude this message can be sent at once; Claude queues it behind the compaction (verified).
4. Claude alone can do it without a follow-up: a `SessionStart` hook with matcher `compact` whose stdout names
   the handoff file. The model still acts only on the next user message.
5. Completion detection that horch can read without screen scraping: Claude transcript `compact_boundary`
   (usage.rs already reads this file); Codex rollout `compacted`; pi/Prime JSONL `compaction` entry;
   OpenCode SQLite `summary: true`. The screen text is a fallback.
6. Backstop thresholds: only Claude Code (1M window) and Prime (1M) and OpenCode ultra (1M) need a lever to
   stop near 300k. Codex (244,800), pi (245,760), lightning (230,144) and big-pickle (140,000) already
   auto-compact below 300k, so the orchestrator's 300k watch never fires first for them.
   The design must decide whether the watch threshold is per harness.
7. Orchestrator self-compaction: the orchestrator runs on Claude Code. Its `horch tell` to itself queues
   `/compact` until its turn ends (mechanism verified). The design must confirm that `horch tell` can target
   the orchestrator's own pane (brief 03).
8. Codex custom compact instructions do not work on the OpenAI provider (remote compaction ignores
   `compact_prompt`). Put what must survive into the handoff file and into the user message that asks for it;
   remote compaction keeps user messages up to 64,000 tokens.
9. Prime: consider adding `--skill .../dist/skills/compact` to the Prime teammate so the worker can compact
   itself at a natural boundary and auto-resume.

## Outside scope (not fixed)

- `pi --version` and `pi --help` exit with code 1 on this machine and print a large minified stack from
  `dist/bundle/chunks/chunk-JVUZSMYM.js:268` (an undici SQLite cache class). pi panes may fail to start. Not investigated.
- The Claude account showed "You're now using usage credits" during the live check.
