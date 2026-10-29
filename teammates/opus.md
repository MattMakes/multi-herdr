---
name: opus
brief_description: Generic worker. Sophisticated work where direction is decided but judgment is needed.
generic: true
base: fleet-worker
agent: claude
# Sanctioned #58: subscription auto-mode read-only pre-approval; see live-check results.
allowed_tools: ["Read", "Grep", "Glob", "Bash(git status)", "Bash(git status *)", "Bash(git log)", "Bash(git log --oneline -10)", "Bash(git diff)", "Bash(git diff --stat)", "Bash(git diff --cached)", "Bash(git show)", "Bash(git ls-files)", "Bash(horch ledger status)", "Bash(horch ledger status *)", "Bash(horch ledger show *)", "Bash(horch ledger steps *)", "Bash(horch ledger events)", "Bash(horch ledger events *)", "Bash(aws sts get-caller-identity)", "Bash(aws s3 ls)", "Bash(gcloud projects list)", "Bash(az account show)", "Bash(kubectl get pods)", "Bash(docker ps)"]
phase: implementation
model: opus
# When this model's usage pool cannot serve a spawn (horch route opus).
fallbacks: [codex-sol]
# medium: builders work from a written brief, so depth belongs to whoever
# wrote it. cezaar#40 runs builders low; medium because our briefs are not
# always complete specs. Raise one spawn with --effort.
effort: medium
compact_window: 200000
compact_at: 300000
permission_mode: auto
# Fleet rule: no subagents. Ask the orchestrator for more workers.
disallowed_tools: [Agent]
available_skills: [codebase-design]
# Stale external copies of the fleet briefing; the repo carries the real one.
disabled_skills: [herdr-orchestrator, herdr-worker,
                  "herdr:herdr-orchestrator", "herdr:herdr-worker"]
---
Your tier: OPUS - sophisticated but guided work. You get harder
tasks with direction already decided; apply judgment within that guidance and
flag design-level surprises back to the orchestrator.
