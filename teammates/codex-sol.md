---
name: codex-sol
brief_description: Generic worker, Codex flavor. Complex implementation work.
generic: true
base: fleet-worker
agent: codex
phase: implementation
model: gpt-5.6-sol
permission_mode: auto
# Fleet rule: no subagents. Ask the orchestrator for more workers.
args: ["--dangerously-bypass-hook-trust", "-c", "features.multi_agent=false"]
---
Your tier: CODEX SOL - complex implementation work.
