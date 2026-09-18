---
name: opencode-pickle
brief_description: Free tier, balanced. TRAINS ON YOUR INPUT - public/OSS work only, never proprietary code or secrets.
generic: true
base: fleet-worker
agent: opencode
phase: implementation
model: opencode/big-pickle
effort: high
permission_mode: acceptEdits
trains_on_input: true

# 200k context. The default free worker: enough for ordinary, well-scoped
# implementation work on public code without reaching for the big context.
inherit_plugins: false
---
Your tier: OPENCODE BIG PICKLE - the everyday free worker. Well-scoped
implementation work on public code. Keep changes minimal, and say plainly
when the task is not one a free tier should be doing.
