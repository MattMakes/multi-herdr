---
name: context-windows
description: >
  The fleet's native auto-compact windows and in-place route as data. horch
  applies a window here only when the operator's own settings set none
  (docs/specs/context-policy.md). An operator overlay of this file changes
  no message.
# Fleet default native auto-compact settings (docs/specs/context-policy.md).
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

A substituted launch keeps the original teammate's name and takes the
fallback's harness and model. So a key `<fallback harness>/<teammate>` can
apply to it, and a `compact_window` comes from the fallback's file; the
recorded detail then names the fallback.

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
It checks every `windows` key by itself, whichever teammate it reaches.

## Check

`horch teammates --check` also fails:

- a key on a harness with no window setting (opencode, antigravity), or with
  no name after the `/`;
- a key `<harness>/<name>` where teammate `<name>` runs another model and
  `<name>` is also the model of a teammate on that harness: set
  `compact_window` in the teammate file instead;
- an `in_place` entry for a harness that horch has no compact command for;
- a copy that leaves out `windows` or `in_place`.

It warns about a key whose name is no teammate and no model of a teammate on
its harness.

## Override

To change a window or the `in_place` list on one machine, copy this file to
`~/.config/horch/teammates/_base/context-windows.md` and edit the copy. The
copy replaces this whole file, so keep both keys, `windows` and `in_place`.
The messages are not in this file: they are in `context-messages.md`, and the
copy does not change them.
