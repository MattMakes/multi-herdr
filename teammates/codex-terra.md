---
name: codex-terra
brief_description: Codex worker for grunt work, as asked. Network on, so its sandbox cannot stop git push. Commits via horch ledger commit.
generic: true
base: fleet-worker
agent: codex
phase: implementation
model: gpt-5.6-terra
# When this model's usage pool cannot serve a spawn (horch route codex-terra).
fallbacks: [sonnet]
# low: Terra executes exactly what it is told, so the thinking is in the brief
# (cezaar#40: builders on complete specs run low).
effort: low
compact_window: 150000
compact_at: 120000
permission_mode: auto
# Fleet rule: no subagents. Ask the orchestrator for more workers.
args: ["--dangerously-bypass-hook-trust", "-c", "features.multi_agent=false"]
---
Your tier: CODEX TERRA - straightforward grunt work. Execute
exactly what is asked, nothing speculative.
