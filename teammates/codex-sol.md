---
name: codex-sol
brief_description: Generic worker, Codex flavor. Complex implementation work.
generic: true
base: fleet-worker
agent: codex
phase: implementation
model: gpt-5.6-sol
# Set explicitly: unset, the pane inherits ~/.codex/config.toml. medium is the
# builder level cezaar#40 settled on; raise it per spawn with --effort.
effort: medium
permission_mode: auto
# Fleet rule: no subagents. Ask the orchestrator for more workers.
args: ["--dangerously-bypass-hook-trust", "-c", "features.multi_agent=false"]
---
Your tier: CODEX SOL - complex implementation work.
