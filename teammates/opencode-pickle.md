---
name: opencode-pickle
brief_description: Free tier, balanced. TRAINS ON YOUR INPUT - public/OSS work only, never proprietary code or secrets.
generic: true
base: fleet-worker
agent: opencode
phase: implementation
model: opencode/big-pickle
# No effort: the free models report `variants: {}`, so --variant is a no-op
# and `--check` refuses the field.
permission_mode: acceptEdits
trains_on_input: true

# 200k context. The default free worker: enough for ordinary, well-scoped
# implementation work on public code without reaching for the big context.
inherit_plugins: false

# Background calls off: teammates/README.md "Background calls switched off".
# The title agent would send the briefing to the operator's paid small_model.
# External skills are the operator's personal skills; this provider trains on
# input. horch merges the phase skills into OPENCODE_CONFIG_CONTENT, and they
# still load with OPENCODE_DISABLE_EXTERNAL_SKILLS=1 (opencode 1.18.34).
env:
  OPENCODE_CONFIG_CONTENT: '{"agent":{"title":{"disable":true}}}'
  OPENCODE_DISABLE_EXTERNAL_SKILLS: "1"
---
Your tier: OPENCODE BIG PICKLE - the everyday free worker. Well-scoped
implementation work on public code. Keep changes minimal, and say plainly
when the task is not one a free tier should be doing.
