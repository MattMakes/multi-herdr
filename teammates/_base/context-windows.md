---
name: context-windows
description: >
  The fleet's native auto-compact windows and in-place route as data. horch
  applies a window here only when the operator's own settings set none
  (design.md §5). An operator overlay of this file changes no message.
# Fleet default native auto-compact settings (plans_to_improve §2.3).
# Key: <harness>/<teammate name> first, then <harness>/<model>.
windows:
  claude/orchestrator: 300000
  claude/opus: 200000
  claude/sonnet: 150000
  codex/gpt-5.6-sol: 200000
  codex/gpt-5.6-terra: 150000
# Harnesses whose compaction live check passed (CTX-17). Others use the
# fresh-session route.
in_place: [claude, codex]
---
# Context windows

`windows` sets each harness's own auto-compact setting for a launch: the
Claude `CLAUDE_CODE_AUTO_COMPACT_WINDOW`, the Codex
`model_auto_compact_token_limit`, the Prime `contextWindow`. A teammate's
`compact_window` wins over this file. A value in the operator's own settings
wins over both, and horch then applies nothing.

`in_place` lists the harnesses that horch compacts in the same pane. Every
other harness takes the fresh-session route.

## Route per harness

| harness | command typed | keep-list as argument | busy behaviour | sends | route in slice 1 |
|---|---|---|---|---|---|
| claude | `/compact <instructions>` | yes | queued | 1 | in place |
| codex | `/compact` | no | rejected | up to 3 | in place |
| pi | `/compact <instructions>` | yes | aborts turn | 1 | fresh |
| prime | `/compact <instructions>` | yes | queued | 1 | fresh |
| opencode | `/compact` | no | ends turn | up to 3 | fresh |
| antigravity | - | - | - | 0 | not watched (`not-read`) |

## Threshold

horch watches `threshold = min(300000, floor(0.8 x native trigger))`.
When the native trigger is unknown, the threshold is 300000.
`horch teammates --check` fails a window that leaves less than 20000 tokens between threshold and native trigger.

## Override

To change a window or the `in_place` list on one machine, copy this file to
`~/.config/horch/teammates/_base/context-windows.md` and edit the copy. The
copy replaces this whole file, so keep both keys, `windows` and `in_place`.
The messages are not in this file: they are in `context-messages.md`, and the
copy does not change them.
