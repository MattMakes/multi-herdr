---
name: opencode-lightning
brief_description: Free tier, fastest, huge output. TRAINS ON YOUR INPUT - public/OSS work only, never secrets.
generic: true
base: fleet-worker
agent: opencode
phase: implementation
model: opencode/nemotron-3.5-lightning-free
# No effort: the free models report `variants: {}`, so --variant is a no-op
# and `--check` refuses the field.
permission_mode: acceptEdits
trains_on_input: true

# 262k context and 262k output - the only one of the three that can emit as much
# as it reads. Built for speed: this is the free tier's
# grunt worker, for bulk mechanical edits rather than judgement.
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
Your tier: OPENCODE LIGHTNING - fast, high-volume, mechanical work on
public code. Execute exactly what is asked, do not redesign anything, and
say plainly when the task is not one a free tier should be doing.
