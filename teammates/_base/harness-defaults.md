---
name: harness-defaults
description: >
  Launch defaults per harness: env, args and settings that every teammate of
  a harness gets. A teammate's own value for the same key wins. A value the
  operator sets in their own config wins too, unless the entry has `force`
  with the reason (design.md §6A.2). teammates/README.md "Harness defaults".
defaults:
  - harness: claude
    env:
      CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION: "false"
      DISABLE_AUTOUPDATER: "1"
  - harness: claude
    settings: {"includeCoAuthoredBy": false, "attribution": {"commit": "", "pr": "", "sessionUrl": false}}
    force: "operator request 2026-11-04: no commit or PR attribution in any Claude pane horch launches"
  - harness: claude
    settings: {"hooks": {"PostToolBatch": [{"hooks": [{"type": "command", "command": "horch hook context", "timeout": 20}]}], "PreCompact": [{"hooks": [{"type": "command", "command": "horch hook pre-compact", "timeout": 20}]}], "PostCompact": [{"hooks": [{"type": "command", "command": "horch hook post-compact", "timeout": 20}]}], "SessionStart": [{"matcher": "compact", "hooks": [{"type": "command", "command": "horch hook session-start", "timeout": 20}]}]}}
    force: "operator request 2026-11-21: precompact, run whats-next, compact, postcompact, hydrate with that whats-next (docs/specs/context-policy.md §4.6)"
  - harness: codex
    args: ["-c", "tui.auto_recap=false"]
  - harness: codex
    base: fleet-worker
    args: ["-c", "notify=[]"]
    force: "00-decisions Q3: the operator's notify command after every worker turn is noise"
  - harness: opencode
    env:
      OPENCODE_DISABLE_EXTERNAL_SKILLS: "1"
    config: {"agent": {"title": {"disable": true}}}
  - harness: prime
    env:
      RLM_MAX_DEPTH: "1"
    agent_settings: {"autoRefine": {"enabled": false}}
---
# Harness defaults

Some CLIs make model calls that the worker did not ask for. Nobody reads
their output in a fleet pane, so these defaults turn them off. Each switch
was checked on the installed CLI on 2026-10-06.

The roster gives each teammate the entries whose `base` matches its own (no
`base`: every teammate). At launch, the builder uses only the entries of
the harness the teammate runs on. Merge order, first value wins:

1. The operator's own config for the key, unless the entry has `force`.
2. The teammate's own value (`env:`, `args:`, `settings:`, its own
   `OPENCODE_CONFIG_CONTENT` keys).
3. horch's fixed switches (plugins, skills, Remote Control, sandbox).
4. The entries below, in file order: a later entry wins.

| harness | default | what it stops |
|---|---|---|
| claude | `CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION: "false"` | a fork of the full conversation after each turn, to suggest the next prompt (2.1.292) |
| claude | `DISABLE_AUTOUPDATER: "1"` | a pane that updates its own CLI mid-fleet. `horch fleet` updates every agent CLI before the first pane starts (`horch update`); an updater left running, or a later fleet start, can still replace a CLI |
| claude | `includeCoAuthoredBy: false` and `attribution: {"commit": "", "pr": "", "sessionUrl": false}` (`settings`, `force`) | the `Co-Authored-By` commit trailer, the PR attribution text and the session link (operator request 2026-11-04). `includeCoAuthoredBy` is deprecated since 2.0.62, and Claude Code ignores it once `attribution.commit` or `attribution.pr` is set, so both are set. `attribution: false` needs 2.1.281 and makes an older CLI skip the whole settings file, so the object form is used. `force`: it wins over the operator's own `attribution`. A teammate key only drops it when the teammate sets the same key path, and a teammate settings file (not inline JSON) replaces the whole overlay |
| claude | `hooks.PreCompact`: `horch hook pre-compact` (`settings`, `force`) | a summary that is not a handoff. Before every compaction, Claude Code's own auto-compact included, the hook prints the `summary-format` line, and Claude Code uses it as instructions for the summary: the summary is then a whats-next handoff in 6 sections. For a compaction with no argument it prints the `instructions` keep-list first. It never blocks a compaction |
| claude | `hooks.PostCompact`: `horch hook post-compact` (`settings`, `force`) | a whats-next that exists only in 1 context. After every compaction the hook reads `compact_summary` from its input and saves it as `ai_docs/handoffs/<role>-whats-next.md`, with a first line that marks it as saved from the summary, and records `whats-next-saved` on the ledger. It does not replace a current handoff file that the session wrote itself. It prints nothing |
| claude | `hooks.SessionStart`, matcher `compact`: `horch hook session-start` (`settings`, `force`) | a compacted session that does not know where its whats-next is. After every compaction the hook prints 1 text that Claude Code adds to the context: `hydrate-summary` (the summary above is the whats-next, and where it is saved), or `hydrate` and the file when the session wrote its own handoff (at most 8,000 characters) |
| claude | `hooks.PostToolBatch`: `horch hook context` (`settings`, `force`) | a session that passes its threshold while Claude Code's own trigger is far above it (an operator window). After each batch of tool calls the hook reads the session's context. At or over the threshold it adds the `request` line to the conversation, once per compaction cycle (`context-warned` on the ledger). An orchestrator gets `warning-orchestrator`. `force`: these 4 run beside the operator's own hooks; Claude Code runs the hooks of every settings source. Checked live on Claude Code 2.1.330 (`docs/live-checks/context.md`, part D) |
| codex | `-c tui.auto_recap=false` | a hidden recap thread on the pane's model when the pane loses focus (0.160.0). It yields to `tui.auto_recap` in the operator's `config.toml` |
| codex, fleet workers | `-c notify=[]` | the operator's `notify` command after every worker turn. `force`: it applies even when `config.toml` sets `notify`. Orchestrators keep the operator's command |
| opencode | `{"agent":{"title":{"disable":true}}}` in `OPENCODE_CONFIG_CONTENT` | a title call that sends the briefing to the operator's paid `small_model` (1.18.34) |
| opencode | `OPENCODE_DISABLE_EXTERNAL_SKILLS: "1"` | the operator's personal skills from `~/.claude/` and `~/.agents/` going to a provider that trains on input. The phase skills still load |
| prime | `RLM_MAX_DEPTH: "1"` | recursive child sessions, each a fresh Opus context (default depth 2); the fleet orchestrator already delegates. A global `rlmMaxDepth` setting wins |
| prime | `autoRefine.enabled: false` (`agent_settings`) | Prime auto-refine. It has no env var or flag; the fleet-owned Prime agent dir applies it (built per launch, CTX-05) |

A Claude `env` default needs no operator check: a value in the operator's
settings `env` beats the process env. A non-Claude `env` default yields to
the same variable in the operator's environment; the name must be in
`runtime::context::DEFAULT_ENV_YIELD`.

To turn a switch back on for 1 teammate, set the opposite value in its file
(`CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION: "true"`, `-c tui.auto_recap=true`).
To turn it off for all, edit this file or overlay it in
`~/.config/horch/teammates/_base/harness-defaults.md`. A teammate line that
only repeats a default fails `horch teammates --check`.
