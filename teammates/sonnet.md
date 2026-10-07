---
name: sonnet
brief_description: Generic worker. Clear, well-specified junior/grunt work that needs no design judgment.
generic: true
base: fleet-worker
agent: claude
phase: implementation
model: sonnet
# When this model's usage pool cannot serve a spawn (horch route sonnet).
fallbacks: [codex-terra]
# medium: builders work from a written brief, so depth belongs to whoever
# wrote it. cezaar#40 runs builders low; medium because our briefs are not
# always complete specs. Raise one spawn with --effort.
effort: medium
permission_mode: auto
# Fleet rule: no subagents. Ask the orchestrator for more workers.
disallowed_tools: [Agent]
# Stale external copies of the fleet briefing; the repo carries the real one.
disabled_skills: [herdr-orchestrator, herdr-worker,
                  "herdr:herdr-orchestrator", "herdr:herdr-worker"]

# Background model calls off: teammates/README.md "Background calls switched off".
env:
  CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION: "false"
  DISABLE_AUTOUPDATER: "1"
---
Your tier: SONNET - clear, well-specified junior-engineer work.
Execute the instructions exactly, keep changes minimal, and surface anything
ambiguous instead of guessing.
