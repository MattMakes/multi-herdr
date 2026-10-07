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
| claude | `DISABLE_AUTOUPDATER: "1"` | a mid-fleet update, so all panes in a fleet run 1 build |
| codex | `-c tui.auto_recap=false` | a hidden recap thread on the pane's model when the pane loses focus (0.160.0). It yields to `tui.auto_recap` in the operator's `config.toml` |
| codex, fleet workers | `-c notify=[]` | the operator's `notify` command after every worker turn. `force`: it applies even when `config.toml` sets `notify`. Orchestrators keep the operator's command |
| opencode | `{"agent":{"title":{"disable":true}}}` in `OPENCODE_CONFIG_CONTENT` | a title call that sends the briefing to the operator's paid `small_model` (1.18.34) |
| opencode | `OPENCODE_DISABLE_EXTERNAL_SKILLS: "1"` | the operator's personal skills from `~/.claude/` and `~/.agents/` going to a provider that trains on input. The phase skills still load |
| prime | `RLM_MAX_DEPTH: "1"` | recursive child sessions, each a fresh Opus context (default depth 2) |
| prime | `autoRefine.enabled: false` (`agent_settings`) | Prime auto-refine. It has no env var or flag; the fleet-owned Prime agent dir applies it (not yet built) |

A Claude `env` default needs no operator check: a value in the operator's
settings `env` beats the process env. A non-Claude `env` default yields to
the same variable in the operator's environment; the name must be in
`runtime::context::DEFAULT_ENV_YIELD`.

To turn a switch back on for 1 teammate, set the opposite value in its file
(`CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION: "true"`, `-c tui.auto_recap=true`).
To turn it off for all, edit this file or overlay it in
`~/.config/horch/teammates/_base/harness-defaults.md`. A teammate line that
only repeats a default fails `horch teammates --check`.
